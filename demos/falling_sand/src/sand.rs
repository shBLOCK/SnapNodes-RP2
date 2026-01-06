use crate::direction::Direction;
use crate::slots::{MappedSlots, MappedSlotsOutOfBoundsError, MappedSlotsPopAtError, MappedSlotsPushError};
use alloc::boxed::Box;
use core::f32::math::{floor, fract};
use defmt::*;
use embedded_graphics::pixelcolor::Rgb888;
use nalgebra_glm as glm;
use nalgebra_glm::{I16Vec2, U16Vec2, Vec2, vec2};
use num_traits::Zero;

pub trait ParticleBehavior: Copy {
    fn slot_pos(&self) -> I16Vec2;
    fn set_slot_pos(&mut self, pos: I16Vec2);

    async fn tick(&mut self, world: &mut impl World) {}
}

#[derive(Clone, Copy)]
pub struct Dust {
    pos: Vec2,
    velocity: Vec2,
    pub color: Rgb888,
}

impl Dust {
    pub fn new(pos: I16Vec2, color: Rgb888) -> Dust {
        Self {
            pos: glm::convert::<_, Vec2>(pos).add_scalar(0.5),
            velocity: Vec2::zeros(),
            color,
        }
    }

    async fn could_move_to(&self, world: &mut impl World, pos: I16Vec2) -> bool {
        match world.get_particle_at(pos).await {
            Ok(Some(_)) => false,
            Err(WorldOutOfBoundsError) => false,
            _ => true,
        }
    }

    /// Use grid raymarching to move, handling collisions, etc.
    /// Returns: (number of steps (grids) moved)
    async fn do_move(&mut self, world: &mut impl World, mut delta: Vec2) -> (u8,) {
        const RESTITUTION: f32 = 0.0;
        let mut step_count = 0u8;
        let mut pos = self.pos.map(|it| if fract(it) == 0.0 { it.next_up() } else { it }); // avoid being at edge of cell
        loop {
            let old_slot_pos = glm::try_convert::<_, I16Vec2>(glm::floor(&pos)).unwrap();

            let simple_move_pos = pos + delta;
            let simple_move_slot_pos = glm::try_convert::<_, I16Vec2>(glm::floor(&simple_move_pos)).unwrap();
            if simple_move_slot_pos != old_slot_pos {
                // march through the grid

                // use this to flip the space so that delta has positive elements (_pe)
                // let flip = delta.map(|it| if it >= 0.0 { 1.0 } else { -1.0 });
                let flip = delta.map(f32::signum);
                let flip_i16 = glm::convert_unchecked::<_, I16Vec2>(flip);
                let mut pos_pe = pos.component_mul(&flip);
                let mut delta_pe = delta.component_mul(&flip);
                let ceil_pos_pe = glm::ceil(&pos_pe);
                let pos_to_ceil_pos_pe = ceil_pos_pe - pos_pe;
                // core::assert!(pos_to_ceil_pos_pe.x != 0.0 && pos_to_ceil_pos_pe.y != 0.0);
                // core::assert!(!delta_pe.is_zero(), "delta_pe: {:?}, old: {:?}, simple: {:?}", delta_pe, old_slot_pos, simple_move_slot_pos);
                let step_len_xy_pe = pos_to_ceil_pos_pe.component_div(&delta_pe);
                // core::assert!(!step_len_xy_pe.iter().all(|it| !it.is_finite()));
                if step_len_xy_pe.x <= step_len_xy_pe.y {
                    // should step in x direction
                    let step_y_pe = delta_pe.y * step_len_xy_pe.x;
                    // core::assert!(step_y_pe.is_finite(), "{:?} {:?} {:?}", step_len_xy_pe, pos_to_ceil_pos_pe, delta_pe);
                    pos_pe.y += step_y_pe;
                    delta_pe.y -= step_y_pe;
                    delta_pe.x -= pos_to_ceil_pos_pe.x;
                    if self
                        .could_move_to(world, vec2(old_slot_pos.x + flip_i16.x, old_slot_pos.y))
                        .await
                    {
                        let new_x_pe = ceil_pos_pe.x.next_up();
                        pos_pe.x = new_x_pe;
                    } else {
                        // if !allow_bounce {return;}
                        pos_pe.x = ceil_pos_pe.x.next_down();
                        delta_pe.x *= -RESTITUTION;
                        self.velocity.x *= -RESTITUTION;
                    }
                } else {
                    // should step in y direction
                    let step_x_pe = delta_pe.x * step_len_xy_pe.y;
                    // core::assert!(step_x_pe.is_finite());
                    pos_pe.x += step_x_pe;
                    delta_pe.x -= step_x_pe;
                    delta_pe.y -= pos_to_ceil_pos_pe.y;
                    if self
                        .could_move_to(world, vec2(old_slot_pos.x, old_slot_pos.y + flip_i16.y))
                        .await
                    {
                        pos_pe.y = ceil_pos_pe.y.next_up();
                    } else {
                        pos_pe.y = ceil_pos_pe.y.next_down();
                        delta_pe.y *= -RESTITUTION;
                        self.velocity.y *= -RESTITUTION;
                    }
                }

                pos = pos_pe.component_mul(&flip);
                delta = delta_pe.component_mul(&flip);

                let new_slot_pos_f = glm::floor(&pos);
                let new_slot_pos = glm::try_convert::<_, I16Vec2>(new_slot_pos_f).unwrap();
                if new_slot_pos != old_slot_pos {
                    // move one step
                    step_count += 1;
                    if let Err(_) = world.swap_particles(old_slot_pos, new_slot_pos).await {
                        error!("swap_particles failed: {:?}", Debug2Format(&pos));
                    }
                }
            } else {
                // we are at final cell
                pos = simple_move_pos;

                fn fmod1(x: f32) -> f32 {
                    x - floor(x)
                }

                // check x collision
                let x_neighbor_direction = if fmod1(pos.x) < 0.5 {
                    Direction::NegX
                } else {
                    Direction::PosX
                };
                if !self
                    .could_move_to(world, simple_move_slot_pos + x_neighbor_direction.i16vec2())
                    .await
                {
                    pos.x = floor(pos.x) + 0.5;
                    self.velocity.x *= -RESTITUTION;
                }

                // check y collision
                let y_neighbor_direction = if fmod1(pos.y) < 0.5 {
                    Direction::NegY
                } else {
                    Direction::PosY
                };
                if !self
                    .could_move_to(world, simple_move_slot_pos + y_neighbor_direction.i16vec2())
                    .await
                {
                    pos.y = floor(pos.y) + 0.5;
                    self.velocity.y *= -RESTITUTION;
                }

                self.pos = pos;

                break;
            }
        }

        (step_count,)
    }
}

impl ParticleBehavior for Dust {
    fn slot_pos(&self) -> I16Vec2 {
        glm::try_convert(glm::floor(&self.pos)).unwrap()
    }

    fn set_slot_pos(&mut self, pos: I16Vec2) {
        self.pos = glm::convert::<_, Vec2>(pos).add_scalar(0.5);
    }

    async fn tick(&mut self, world: &mut impl World) {
        self.velocity *= 0.9;

        let gravity = world.get_gravity(self.pos);
        self.velocity += gravity * 0.1;

        // debug!("Velocity: {}", Debug2Format(&self.velocity));
        // debug!("Position: {}", Debug2Format(&self.pos));

        {
            let pre_move_vel = self.velocity;
            let (steps_moved,) = self.do_move(world, self.velocity).await;
            // if steps_moved == 0 {
            //     // do diagonal move
            //     let v = vec2(pre_move_vel.x - pre_move_vel.y, pre_move_vel.x + pre_move_vel.y).normalize();
            //     if self.do_move(world, v).await.0 != 0 {
            //
            //     } else {
            //         let v = vec2(v.y, -v.x);
            //         self.do_move(world, v).await;
            //     }
            // }
        }
    }
}

#[derive(Clone, Copy)]
pub enum Particle {
    Dust(Dust),
}
impl ParticleBehavior for Particle {
    fn slot_pos(&self) -> I16Vec2 {
        match self {
            Particle::Dust(particle) => particle.slot_pos(),
        }
    }

    fn set_slot_pos(&mut self, pos: I16Vec2) {
        match self {
            Particle::Dust(particle) => particle.set_slot_pos(pos),
        }
    }

    async fn tick(&mut self, world: &mut impl World) {
        match self {
            Particle::Dust(particle) => particle.tick(world).await,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorldOutOfBoundsError;

impl From<MappedSlotsOutOfBoundsError> for WorldOutOfBoundsError {
    fn from(_value: MappedSlotsOutOfBoundsError) -> Self {
        Self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpawnParticleError {
    OutOfBounds,
    Conflict,
    Other,
}

impl From<MappedSlotsPushError> for SpawnParticleError {
    fn from(value: MappedSlotsPushError) -> Self {
        match value {
            MappedSlotsPushError::PosOutOfBounds => Self::OutOfBounds,
            MappedSlotsPushError::AlreadyOccupied => Self::Conflict,
            MappedSlotsPushError::Full => Self::Other,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KillParticleError {
    OutOfBounds,
    NothingThere,
}

impl From<MappedSlotsPopAtError> for KillParticleError {
    fn from(value: MappedSlotsPopAtError) -> Self {
        match value {
            MappedSlotsPopAtError::PosOutOfBounds => Self::OutOfBounds,
            MappedSlotsPopAtError::NotOccupied => Self::NothingThere,
        }
    }
}

pub trait World {
    fn get_gravity(&self, pos: Vec2) -> Vec2 {
        vec2(0.0, 0.0)
    }

    async fn get_particle_at(&self, pos: I16Vec2) -> Result<Option<Particle>, WorldOutOfBoundsError>;
    async fn spawn_particle(&mut self, particle: Particle) -> Result<(), SpawnParticleError>;
    async fn kill_particle_at(&mut self, pos: I16Vec2) -> Result<Particle, KillParticleError>;

    async fn swap_particles(&mut self, pos1: I16Vec2, pos2: I16Vec2) -> Result<(), WorldOutOfBoundsError>;

    async fn tick(&mut self) {}
}

pub struct LocalWorld<const WIDTH: u16, const HEIGHT: u16, const MAX_PARTICLES: u16>
where
    [(); MAX_PARTICLES as usize]:,
    [[(); WIDTH as usize]; HEIGHT as usize]:,
{
    particles: Box<MappedSlots<Particle, u16, { WIDTH as usize }, { HEIGHT as usize }, { MAX_PARTICLES as usize }>>,

    global_gravity: Vec2,
}

impl<const WIDTH: u16, const HEIGHT: u16, const MAX_PARTICLES: u16> LocalWorld<WIDTH, HEIGHT, MAX_PARTICLES>
where
    [(); MAX_PARTICLES as usize]:,
    [[(); WIDTH as usize]; HEIGHT as usize]:,
{
    pub fn new() -> Self {
        Self {
            particles: unsafe {
                let mut it = Box::<
                    MappedSlots<Particle, u16, { WIDTH as usize }, { HEIGHT as usize }, { MAX_PARTICLES as usize }>,
                >::new_uninit()
                .assume_init();
                it.init();
                it
            },
            global_gravity: vec2(0.0, 0.0),
        }
    }

    pub fn set_global_gravity(&mut self, value: Vec2) {
        self.global_gravity = value;
    }
}

impl<const WIDTH: u16, const HEIGHT: u16, const MAX_PARTICLES: u16> World for LocalWorld<WIDTH, HEIGHT, MAX_PARTICLES>
where
    [(); MAX_PARTICLES as usize]:,
    [[(); WIDTH as usize]; HEIGHT as usize]:,
{
    fn get_gravity(&self, _pos: Vec2) -> Vec2 {
        self.global_gravity
    }

    async fn get_particle_at(&self, pos: I16Vec2) -> Result<Option<Particle>, WorldOutOfBoundsError> {
        Ok(self
            .particles
            .get_at(glm::convert_unchecked::<_, U16Vec2>(pos))?
            .map(|it| it.item)
            .copied())
    }

    async fn spawn_particle(&mut self, particle: Particle) -> Result<(), SpawnParticleError> {
        self.particles
            .push_at(glm::convert_unchecked::<_, U16Vec2>(particle.slot_pos()), particle)
            .map(|_| ())
            .map_err(SpawnParticleError::from)
    }

    async fn kill_particle_at(&mut self, pos: I16Vec2) -> Result<Particle, KillParticleError> {
        Ok(self.particles.pop_at(glm::convert_unchecked::<_, U16Vec2>(pos))?)
    }

    async fn swap_particles(&mut self, pos1: I16Vec2, pos2: I16Vec2) -> Result<(), WorldOutOfBoundsError> {
        let upos1 = glm::try_convert::<_, U16Vec2>(pos1).ok_or(WorldOutOfBoundsError)?;
        let upos2 = glm::try_convert::<_, U16Vec2>(pos2).ok_or(WorldOutOfBoundsError)?;
        let slot1 = self.particles.get_slot_at(upos1)?;
        let slot2 = self.particles.get_slot_at(upos2)?;
        slot1
            .and_then(|it| self.particles.get_mut(it))
            .map(|it| it.set_slot_pos(pos2));
        slot2
            .and_then(|it| self.particles.get_mut(it))
            .map(|it| it.set_slot_pos(pos1));
        unsafe {
            self.particles.set_slot_at(upos1, slot2).unwrap();
            self.particles.set_slot_at(upos2, slot1).unwrap();
        }
        Ok(())
    }

    async fn tick(&mut self) {
        core::assert_eq!(MAX_PARTICLES, 10000);
        for slot in 0..MAX_PARTICLES {
            let slot = ((slot as u32 * 2573) % 10000) as u16; // tmp
            if let Some(particle) = self.particles.get_mut(slot) {
                unsafe { &mut *(particle as *mut Particle) }.tick(self).await;
            }
        }
    }
}
