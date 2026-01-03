use crate::slots::{MappedSlots, MappedSlotsOutOfBoundsError, MappedSlotsPopAtError, MappedSlotsPushError};
use alloc::boxed::Box;
use embedded_graphics::pixelcolor::Rgb888;
use nalgebra_glm as glm;
use nalgebra_glm::{I16Vec2, U16Vec2, Vec2, vec2};
use num_traits::Zero;

pub trait ParticleBehavior: Copy {
    fn slot_pos(&self) -> I16Vec2;

    async fn tick(&mut self, world: &mut impl World) {}
}

#[derive(Clone, Copy)]
pub struct Dust {
    pos: Vec2,
    velocity: Vec2,
    color: Rgb888,
}

impl Dust {
    async fn should_bounce(&self, world: &mut impl World, pos: I16Vec2) -> bool {
        match world.get_particle_at(pos).await {
            Ok(Some(_)) => true,
            Err(WorldOutOfBoundsError) => true,
            _ => false,
        }
    }
}

impl ParticleBehavior for Dust {
    fn slot_pos(&self) -> I16Vec2 {
        glm::try_convert(glm::floor(&self.pos)).unwrap()
    }

    async fn tick(&mut self, world: &mut impl World) {
        self.velocity *= 0.8;
        self.velocity += world.get_gravity(self.pos) * 0.1;

        {
            // use this to flip the space so that delta has positive elements (_pe)
            let flip = self.velocity.map(|it| if it >= 0.0 { 1.0 } else { -1.0 });
            let flip_i16 = glm::try_convert::<_, I16Vec2>(flip).unwrap();
            let mut delta_pe = self.velocity.component_mul(&flip);
            let mut pos_pe = self.pos;
            while !pos_pe.is_zero() {
                let old_slot_pos = glm::try_convert::<_, I16Vec2>(glm::floor(&pos_pe))
                    .unwrap()
                    .component_mul(&flip_i16);

                // let delta_slope_pe = delta_pe.y / delta_pe.x;
                //
                // let ceil_pos_pe = glm::ceil(&pos_pe);

                let new_slot_pos_f = glm::floor(&pos_pe).component_mul(&flip);
                let new_slot_pos = glm::try_convert::<_, I16Vec2>(new_slot_pos_f).unwrap();

                if new_slot_pos != old_slot_pos {
                    // move one step
                    self.pos = new_slot_pos_f + 0.5;
                    world.swap_particle(old_slot_pos, new_slot_pos);
                } else {
                    // reached final pos
                    self.pos = pos_pe.component_mul(&flip);
                    break;
                }
            }
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

    async fn tick(&mut self, world: &mut impl World) {
        match self {
            Particle::Dust(particle) => particle.tick(world).await,
        }
    }
}

pub struct WorldOutOfBoundsError;

impl From<MappedSlotsOutOfBoundsError> for WorldOutOfBoundsError {
    fn from(_value: MappedSlotsOutOfBoundsError) -> Self {
        Self
    }
}

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
            particles: Box::new(MappedSlots::new()),
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
        Some(
            *self
                .particles
                .get_at(glm::convert_unchecked::<_, U16Vec2>(pos))?
                .map(|it| it.item),
        )
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

    async fn tick(&mut self) {
        for slot in 0..MAX_PARTICLES {
            if let Some(particle) = self.particles.get_mut(slot) {
                unsafe { *(particle as *mut Particle) }.tick(self).await;
            }
        }
    }
}
