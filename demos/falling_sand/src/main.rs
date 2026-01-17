#![no_std]
#![no_main]
#![feature(generic_const_exprs)]
#![feature(step_trait)]
#![feature(core_float_math)]

extern crate alloc;
mod direction;
mod sand;
mod slots;
mod utils;

use core::fmt::Debug;
use core::mem::MaybeUninit;
use core::ops::Add;
use defmt::*;
use embassy_executor::Executor;
use embassy_executor::Spawner;
use embassy_rp::{Peripherals, bind_interrupts};
use embassy_time::{Duration, Instant, Ticker, Timer};
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

use crate::sand::{Dust, LocalWorld, Particle, World};
use embassy_rp::gpio::Output;
use embassy_rp::i2c::I2c;
use embassy_rp::peripherals::I2C1;
use embassy_rp::spi::Spi;
use embedded_alloc::LlffHeap as Heap;
use embedded_graphics::Drawable;
use embedded_graphics::pixelcolor::{Rgb565, Rgb888};
use embedded_graphics::prelude::{DrawTarget, Point, Primitive, RgbColor, Size, WebColors};
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_hal::digital::OutputPin;
use fixed::types::{I16F16, I8F8};
use lcd_async::options::ColorInversion;
use lcd_async::raw_framebuf::RawFrameBuf;
use nalgebra_glm as glm;
use nalgebra_glm::{I16Vec3, Vec2, Vec3, vec2};
use num_traits::FromPrimitive;

#[global_allocator]
static SRAM_HEAP: Heap = Heap::empty();

static PSRAM_HEAP: Heap = Heap::empty();

const SYS_CLOCK_HZ: u32 = 290_000_000;

#[embassy_executor::task(pool_size = 1)]
async fn main_task(spawner: Spawner, ps: Peripherals) -> ! {
    let mut power_latch_pin = Output::new(ps.PIN_23, embassy_rp::gpio::Level::High);

    {
        unsafe {
            embedded_alloc::init!(SRAM_HEAP, 390 * 1024);
        }
    }

    // {
    //     let mut cfg = embassy_rp::psram::Config::aps6404l();
    //     cfg.clock_hz = SYS_CLOCK_HZ;
    //     embassy_rp::psram::Psram::new(embassy_rp::qmi_cs1::QmiCs1::new(ps.QMI_CS1, ps.PIN_0), cfg)
    //         .expect("Failed to initialize PSRAM");
    //
    //     unsafe extern "C" {
    //         static __psram_heap_start: u8;
    //         static __psram_heap_end: u8;
    //     }
    //
    //     let start = unsafe { &__psram_heap_start as *const u8 as usize };
    //     let end = unsafe { &__psram_heap_end as *const u8 as usize };
    //     info!("Heap: start 0x{:x}, size 0x{:x}", start, end - start);
    //     unsafe { PSRAM_HEAP.init(start, end - start) }
    // }

    {
        bind_interrupts!(struct I2CIrq {
            I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
        });

        let mut i2c_cfg = embassy_rp::i2c::Config::default();
        i2c_cfg.frequency = 400_000;
        let mut i2c = I2c::new_async(ps.I2C1, ps.PIN_39, ps.PIN_38, I2CIrq, i2c_cfg);
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
        let lcd_spi = Spi::new_txonly(ps.SPI0, lcd_clk, lcd_mosi, ps.DMA_CH0, lcd_spi_cfg);
        lcd_cs.set_low();
        // TODO: figure out why this doesn't work with proper CS in release mode
        let lcd_spi_device = embedded_hal_bus::spi::ExclusiveDevice::new(lcd_spi, NoCs, embassy_time::Delay).unwrap();
        let display_interface = lcd_async::interface::SpiInterface::new(lcd_spi_device, lcd_dc);

        i2c.write_async(IO_EXP, [0x03, 0b0000_0000]).await.unwrap(); // set lcd rst low
        Timer::after_micros(10).await;
        i2c.write_async(IO_EXP, [0x03, 0b0001_0000]).await.unwrap(); // set lcd rst high

        // let mut display = mipidsi::Builder::new(mipidsi::models::ST7789, display_interface)
        //     .display_size(240, 240)
        //     .init(&mut embassy_time::Delay)
        //     .unwrap();

        const DISPLAY_WIDTH: usize = 240;
        const DISPLAY_HEIGHT: usize = 240;

        let mut display = lcd_async::Builder::new(lcd_async::models::ST7789, display_interface)
            .display_size(DISPLAY_WIDTH as u16, DISPLAY_HEIGHT as u16)
            .invert_colors(ColorInversion::Inverted)
            .init(&mut embassy_time::Delay)
            .await
            .unwrap();

        static FRAME_BUFFER: StaticCell<[u8; DISPLAY_WIDTH * DISPLAY_HEIGHT * 2]> = StaticCell::new();
        let frame_buffer_data = unsafe { FRAME_BUFFER.uninit().assume_init_mut() };
        frame_buffer_data.fill(0);

        display
            .show_raw_data(0, 0, DISPLAY_WIDTH as u16, DISPLAY_HEIGHT as u16, frame_buffer_data)
            .await
            .unwrap();

        const LSM6DS3TR: u8 = 0x6A;
        i2c.write_async(LSM6DS3TR, [0x12, 0b00000101]).await.unwrap(); // soft reset
        i2c.write_async(LSM6DS3TR, [0x10, 0b0100_01_1_0]).await.unwrap(); // CTRL1_XL
        i2c.write_async(LSM6DS3TR, [0x12, 0b0_1_0_0_0_1_0_0]).await.unwrap(); // CTRL3_C
        i2c.write_async(LSM6DS3TR, [0x17, 0b0_00_0_0_0_0_0]).await.unwrap(); // CTRL8_XL

        const WIDTH: u16 = 120;
        const HEIGHT: u16 = 120;
        const PIXEL_SIZE: u16 = 2;
        let mut world = LocalWorld::<WIDTH, HEIGHT, 10000>::new();

        const SW: i16 = 80;
        const SH: i16 = 80;
        for y in 0..SH {
            for x in 0..SW {
                world
                    .spawn_particle(Particle::Dust(Dust::new(
                        vec2(x, y),
                        Rgb888::new(
                            (x as f32 / SW as f32 * 255.0) as u8,
                            (y as f32 / SH as f32 * 255.0) as u8,
                            127,
                        ),
                    )))
                    .await
                    .unwrap();
            }
        }

        let mut ticker = Ticker::every(Duration::from_hz(30));
        let mut tick_counter = 0;

        loop {
            info!("Tick {}", tick_counter);

            i2c.write_read_async(IO_EXP, [0x00], &mut result).await.unwrap();
            let io_exp_inputs = result[0];
            // info!("P0: {:b}", io_exp_inputs);
            if io_exp_inputs & 0b1111_0000 != 0 {
                power_latch_pin.set_low();
            }

            let start_time = Instant::now();

            frame_buffer_data.fill(0);
            let mut frame_buffer = RawFrameBuf::new(frame_buffer_data.as_mut_slice(), DISPLAY_WIDTH, DISPLAY_HEIGHT);
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let color = match world.get_particle_at(vec2(x as i16, y as i16)).await {
                        Ok(Some(Particle::Dust(dust))) => dust.color,
                        _ => continue,
                    };
                    frame_buffer
                        .fill_solid(
                            &Rectangle::new(
                                Point::new(x as i32, y as i32) * PIXEL_SIZE as i32,
                                Size::new(PIXEL_SIZE as u32, PIXEL_SIZE as u32),
                            ),
                            Rgb565::from(color),
                        )
                        .unwrap();
                    // Rectangle::new(
                    //     Point::new(x as i32, y as i32) * PIXEL_SIZE as i32,
                    //     Size::new(PIXEL_SIZE as u32, PIXEL_SIZE as u32),
                    // )
                    // .into_styled(PrimitiveStyle::with_fill(Rgb565::from(color)))
                    // .draw(&mut frame_buffer)
                    // .unwrap();
                }
            }

            // display
            //     .show_raw_data(0, 0, DISPLAY_WIDTH as u16, DISPLAY_HEIGHT as u16, frame_buffer_data)
            //     .await
            //     .unwrap();

            let acceleration = {
                let mut buf = [0u8; 6];
                i2c.write_read_async(LSM6DS3TR, [0x28], &mut buf).await.unwrap();
                let acceleration_raw =
                    I16Vec3::from_fn(|i, _| i16::from_le_bytes(*buf[i * 2..(i + 1) * 2].as_array().unwrap()));
                let acceleration = glm::convert::<_, Vec3>(acceleration_raw) * (0.488 / 1e3 * 9.81);
                vec2(-acceleration.y, acceleration.x)
            };
            info!("accel: {:?}", Debug2Format(&acceleration));
            world.set_global_gravity(acceleration);

            // world.tick().await;

            embassy_futures::join::join(
                display.show_raw_data(0, 0, DISPLAY_WIDTH as u16, DISPLAY_HEIGHT as u16, frame_buffer_data),
                world.tick(),
            )
            .await
            .0
            .unwrap();

            // ticker.next().await;

            let fps = 1.0 / ((Instant::now() - start_time).as_micros() as f32 / 1e6);
            info!("FPS: {}", fps);
            tick_counter += 1;
        }

        loop {
            info!("Hello world!");

            i2c.write_read_async(IO_EXP, [0x00], &mut result).await.unwrap();
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
    // unsafe {
    //     embassy_rp::rom_data::flash_select_xip_read_mode(3, 3);
    // }
    let mut clock_config = embassy_rp::clocks::ClockConfig::system_freq(SYS_CLOCK_HZ).unwrap();
    // let clock_config = embassy_rp::clocks::ClockConfig::crystal(12_000_000);
    clock_config.core_voltage = embassy_rp::clocks::CoreVoltage::V1_30;
    let ps = embassy_rp::init(embassy_rp::config::Config::new(clock_config));

    // { // tmp
    //     fn bm<T: Debug>(text: &str, f: &impl Fn() -> T) {
    //         let st = Instant::now();
    //         let result = f();
    //         info!("{} {:?}: {}t", text, Debug2Format(&result), (Instant::now() - st).as_ticks());
    //     }
    //
    //     let n = 100_000_000;
    //     bm(
    //         "f32 add",
    //         &|| {
    //             let mut x = 0f32;
    //             for _ in 0..n {
    //                 x = core::hint::black_box(x + 1.2);
    //             }
    //             x
    //         }
    //     );
    //
    //     bm(
    //         "i32 add",
    //         &|| {
    //             let mut x = 0i32;
    //             for _ in 0..n {
    //                 x = core::hint::black_box(x + 2);
    //             }
    //             x
    //         }
    //     );
    //
    //     bm(
    //         "I16F16 add",
    //         &|| {
    //             let mut x = I16F16::from_f32(0.0).unwrap();
    //             let a = I16F16::from_f32(1.5f32).unwrap();
    //             for _ in 0..n {
    //                 x = core::hint::black_box(x + a);
    //             }
    //             x
    //         }
    //     );
    //
    //     bm(
    //         "I8F8 add",
    //         &|| {
    //             let mut x = I8F8::from_f32(0.0).unwrap();
    //             let a = I8F8::from_f32(1.5f32).unwrap();
    //             for _ in 0..n {
    //                 x = core::hint::black_box(x + a);
    //             }
    //             x
    //         }
    //     );
    //
    //     bm(
    //         "f32 div",
    //         &|| {
    //             let mut x = 0f32;
    //             for _ in 0..n {
    //                 x = core::hint::black_box(x / 0.9);
    //             }
    //             x
    //         }
    //     );
    //
    //     bm(
    //         "i32 div",
    //         &|| {
    //             let mut x = 1000000000i32;
    //             for _ in 0..n {
    //                 x = core::hint::black_box(x / 2);
    //             }
    //             x
    //         }
    //     );
    //
    //     bm(
    //         "I16F16 div",
    //         &|| {
    //             let mut x = I16F16::from_f32(0.0).unwrap();
    //             let a = I16F16::from_f32(0.9f32).unwrap();
    //             for _ in 0..n {
    //                 x = x / core::hint::black_box(a);
    //             }
    //             x
    //         }
    //     );
    //
    //     bm(
    //         "I8F8 div",
    //         &|| {
    //             let mut x = I8F8::from_f32(0.0).unwrap();
    //             let a = I8F8::from_f32(0.9f32).unwrap();
    //             for _ in 0..n {
    //                 x = core::hint::black_box(x / a);
    //             }
    //             x
    //         }
    //     );
    // }

    static EXECUTOR: StaticCell<Executor> = StaticCell::new();
    let executor = EXECUTOR.init(Executor::new());
    executor.run(|spawner| spawner.spawn(main_task(spawner, ps).unwrap()));
}
