#![no_std]
#![no_main]

#![feature(generic_const_exprs)]
#![feature(step_trait)]

mod sand;
mod slots;
mod direction;

extern crate alloc;

use core::mem::MaybeUninit;
use defmt::*;
use defmt::export::display;
use embassy_executor::Executor;
use embassy_executor::Spawner;
use embassy_rp::{Peripherals, bind_interrupts};
use embassy_time::Timer;
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"Blinky Example"),
    embassy_rp::binary_info::rp_program_description!(
        c"This example tests the RP Pico on board LED, connected to gpio 25"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

use embassy_rp::gpio::Output;
use embassy_rp::i2c::I2c;
use embassy_rp::peripherals::I2C1;
use embassy_rp::spi::Spi;
use embedded_alloc::LlffHeap as Heap;
use embedded_graphics::Drawable;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::{DrawTarget, Point, RgbColor};
use embedded_graphics::text::Text;
use embedded_hal::digital::OutputPin;
use lcd_async::options::ColorInversion;
use lcd_async::raw_framebuf::RawFrameBuf;

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[embassy_executor::task(pool_size = 1)]
async fn main_task(spawner: Spawner, ps: Peripherals) -> ! {
    {
        embassy_rp::psram::Psram::new(
            embassy_rp::qmi_cs1::QmiCs1::new(ps.QMI_CS1, ps.PIN_0),
            embassy_rp::psram::Config::aps6404l(),
        )
        .expect("Failed to initialize PSRAM");

        unsafe extern "C" {
            static __psram_heap_start: u8;
            static __psram_heap_end: u8;
        }

        let start = unsafe { &__psram_heap_start as *const u8 as usize };
        let end = unsafe { &__psram_heap_end as *const u8 as usize };
        info!("Heap: start 0x{:x}, size 0x{:x}", start, end - start);
        unsafe { HEAP.init(start, end - start) }
    }

    {
        bind_interrupts!(struct I2CIrq {
            I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
        });

        let mut i2c = I2c::new_async(
            ps.I2C1,
            ps.PIN_39,
            ps.PIN_38,
            I2CIrq,
            embassy_rp::i2c::Config::default(),
        );
        const IO_EXP: u8 = 0x59;
        let mut result = [0u8; 1];
        // for addr in 1u8..=127 {
        //     info!("{}", addr);
        //     info!("{}: {}", addr, i2c.write(addr, &[0]));
        // }
        i2c.write_async(IO_EXP, [0x7F, 0x00]).await.unwrap(); // soft reset
        Timer::after_micros(10).await;
        info!(
            "IoExpander ID: {} {}",
            i2c.write_read_async(IO_EXP, [0x10], &mut result).await,
            result
        );
        i2c.write_async(IO_EXP, [0x06, 0xFF]).await.unwrap(); // disable p0 interrupts
        i2c.write_async(IO_EXP, [0x07, 0xFF]).await.unwrap(); // disable p1 interrupts
        i2c.write_async(IO_EXP, [0x04, 0b1111_1111]).await.unwrap(); // set all P0 to input
        i2c.write_async(IO_EXP, [0x03, 0b0000_0000]).await.unwrap(); // set all P1 to low
        i2c.write_async(IO_EXP, [0x13, 0b1111_0000]).await.unwrap(); // set P1_0 ~ P1_3 to LED mode
        i2c.write_async(IO_EXP, [0x05, 0b0000_0000]).await.unwrap(); // set all P1 to output
        i2c.write_async(IO_EXP, [0x20, 0xFF]).await.unwrap(); // set P1_0 current to max

        /// Noop impl of OutputPin.
        struct NoCs;

        impl OutputPin for NoCs {
            fn set_low(&mut self) -> Result<(), Self::Error> {
                Ok(())
            }

            fn set_high(&mut self) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        impl embedded_hal::digital::ErrorType for NoCs {
            type Error = core::convert::Infallible;
        }

        let mut lcd_cs = Output::new(ps.PIN_33, embassy_rp::gpio::Level::High);
        let lcd_clk = ps.PIN_34;
        let lcd_mosi = ps.PIN_35;
        let lcd_dc = Output::new(ps.PIN_36, embassy_rp::gpio::Level::Low);
        let mut lcd_spi_cfg = embassy_rp::spi::Config::default();
        lcd_spi_cfg.frequency = 64_000_000;
        lcd_spi_cfg.phase = embassy_rp::spi::Phase::CaptureOnSecondTransition;
        lcd_spi_cfg.polarity = embassy_rp::spi::Polarity::IdleHigh;
        let lcd_spi = Spi::new_txonly(ps.SPI0, lcd_clk, lcd_mosi, ps.DMA_CH3, lcd_spi_cfg);
        lcd_cs.set_low();
        // TODO: figure out why this doesn't work with proper CS in release mode
        let lcd_spi_device =
            embedded_hal_bus::spi::ExclusiveDevice::new(lcd_spi, NoCs, embassy_time::Delay)
                .unwrap();
        let display_interface = lcd_async::interface::SpiInterface::new(lcd_spi_device, lcd_dc);

        i2c.write_async(IO_EXP, [0x03, 0b0000_0000]).await.unwrap(); // set lcd rst low
        Timer::after_micros(10).await;
        i2c.write_async(IO_EXP, [0x03, 0b0001_0000]).await.unwrap(); // set lcd rst high

        // let mut display = mipidsi::Builder::new(mipidsi::models::ST7789, display_interface)
        //     .display_size(240, 240)
        //     .init(&mut embassy_time::Delay)
        //     .unwrap();

        const WIDTH: usize = 240;
        const HEIGHT: usize = 240;

        let mut display = lcd_async::Builder::new(lcd_async::models::ST7789, display_interface)
            .display_size(WIDTH as u16, HEIGHT as u16)
            .invert_colors(ColorInversion::Inverted)
            .init(&mut embassy_time::Delay).await
            .unwrap();

        static FRAME_BUFFER: StaticCell<[u8; WIDTH * HEIGHT * 2]> = StaticCell::new();
        let frame_buffer = FRAME_BUFFER.init_with(|| [0; WIDTH * HEIGHT * 2]);
        display.show_raw_data(0, 0, WIDTH as u16, HEIGHT as u16, frame_buffer).await.unwrap();

        loop {
            info!("Hello world!");
            {
                let mut fbuf = RawFrameBuf::<Rgb565, _>::new(frame_buffer.as_mut_slice(), WIDTH, HEIGHT);
                fbuf.clear(Rgb565::GREEN).unwrap();
            }
            display.show_raw_data(0, 0, WIDTH as u16, HEIGHT as u16, frame_buffer).await.unwrap();
            {
                let mut fbuf = RawFrameBuf::<Rgb565, _>::new(frame_buffer.as_mut_slice(), WIDTH, HEIGHT);
                fbuf.clear(Rgb565::RED).unwrap();
            }
            display.show_raw_data(0, 0, WIDTH as u16, HEIGHT as u16, frame_buffer).await.unwrap();
            // {
            //     let mut fbuf = RawFrameBuf::<Rgb565, _>::new(frame_buffer.as_mut_slice(), WIDTH, HEIGHT);
            //     lcd_async::TestImage::new().draw(&mut fbuf).unwrap();
            // }
            // display.show_raw_data(0, 0, WIDTH as u16, HEIGHT as u16, frame_buffer).await.unwrap();
        }

        loop {
            info!("Hello world!");

            i2c.write_read_async(IO_EXP, [0x00], &mut result)
                .await
                .unwrap();
            info!("P0: {:b}", result[0]);

            // i2c.write_async(IO_EXP, [0x20, 0xFF]).await.unwrap(); // set P1_0 current to max
            // Timer::after_millis(250).await;
            // i2c.write_async(IO_EXP, [0x20, 0x00]).await.unwrap(); // set P1_0 current to none
            // Timer::after_millis(250).await;

            Timer::after_millis(500).await;
        }
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let clock_config = embassy_rp::clocks::ClockConfig::crystal(12_000_000);
    let ps = embassy_rp::init(embassy_rp::config::Config::new(clock_config));

    static EXECUTOR: StaticCell<Executor> = StaticCell::new();
    let executor = EXECUTOR.init(Executor::new());
    executor.run(|spawner| spawner.spawn(main_task(spawner, ps).unwrap()));
}
