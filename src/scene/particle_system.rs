//! CesiumJS `ParticleSystem`: emits, simulates and draws particles as
//! billboards of its own [`BillboardCollection`].
//!
//! The update follows Cesium step for step: top up the particle pool (each
//! pooled particle owns a hidden billboard), age every live particle and
//! write it into its billboard, return dead particles to the pool, emit by
//! rate, carry-over and bursts through the emitter and the combined model
//! matrix, update the billboards, advance time (looping or completing) and,
//! every 120th frame, trim the pool. It is checked against a SwiftShader
//! capture of the pinned CesiumJS engine with a seeded random number
//! generator in `tests/scene_particle_system.rs`.
use std::rc::Rc;

use super::{
    billboard::{Billboard, BillboardImage, BillboardOptions},
    billboard_collection::BillboardCollection,
    particle_emitter::CircleEmitter,
    scene::{Result, ScenePrimitive},
    Cesium3DTilePass, FrameState, Particle, ParticleBurst, ParticleEmitter,
};
use crate::{
    core::{
        math::{self, random_between},
        Cartesian2, Color, Event, JulianDate, Matrix4, RuntimeError,
    },
    renderer::Gl,
};

/// Makes a particle billboard's image. Called once per billboard, as Cesium
/// passes `image` to every billboard it adds: an image with a URI shares one
/// atlas entry, one without gets a new id each time.
pub type ParticleImage<G> = Rc<dyn Fn() -> BillboardImage<G>>;

/// Called for every live particle each update with the time step.
pub type ParticleSystemCallback = Box<dyn FnMut(&mut Particle, f64)>;

/// `new ParticleSystem(options)`. As in Cesium, `color`, `scale`, `speed`,
/// `particle_life`, `mass` and `image_size` set both their minimum/start and
/// maximum/end values and take precedence over them.
pub struct ParticleSystemOptions<G: Gl + 'static> {
    pub show: Option<bool>,
    pub update_callback: Option<ParticleSystemCallback>,
    pub loop_animation: Option<bool>,
    pub image: Option<ParticleImage<G>>,
    pub emitter: Option<Box<dyn ParticleEmitter>>,
    pub bursts: Option<Vec<ParticleBurst>>,
    pub model_matrix: Option<Matrix4>,
    pub emitter_model_matrix: Option<Matrix4>,
    pub color: Option<Color>,
    pub start_color: Option<Color>,
    pub end_color: Option<Color>,
    pub scale: Option<f64>,
    pub start_scale: Option<f64>,
    pub end_scale: Option<f64>,
    pub emission_rate: Option<f64>,
    pub speed: Option<f64>,
    pub minimum_speed: Option<f64>,
    pub maximum_speed: Option<f64>,
    pub particle_life: Option<f64>,
    pub minimum_particle_life: Option<f64>,
    pub maximum_particle_life: Option<f64>,
    pub mass: Option<f64>,
    pub minimum_mass: Option<f64>,
    pub maximum_mass: Option<f64>,
    pub image_size: Option<Cartesian2>,
    pub minimum_image_size: Option<Cartesian2>,
    pub maximum_image_size: Option<Cartesian2>,
    pub size_in_meters: Option<bool>,
    pub lifetime: Option<f64>,
}

impl<G: Gl + 'static> Default for ParticleSystemOptions<G> {
    fn default() -> Self {
        Self {
            show: None,
            update_callback: None,
            loop_animation: None,
            image: None,
            emitter: None,
            bursts: None,
            model_matrix: None,
            emitter_model_matrix: None,
            color: None,
            start_color: None,
            end_color: None,
            scale: None,
            start_scale: None,
            end_scale: None,
            emission_rate: None,
            speed: None,
            minimum_speed: None,
            maximum_speed: None,
            particle_life: None,
            minimum_particle_life: None,
            maximum_particle_life: None,
            mass: None,
            minimum_mass: None,
            maximum_mass: None,
            image_size: None,
            minimum_image_size: None,
            maximum_image_size: None,
            size_in_meters: None,
            lifetime: None,
        }
    }
}

/// A particle and the billboard it draws with.
struct Slot<G: Gl + 'static> {
    particle: Particle,
    billboard: Option<Billboard<G>>,
}

/// `Check.typeOf.number.greaterThanOrEquals(name, value, 0)`.
fn check_non_negative(name: &str, value: f64) -> Result<()> {
    if value >= 0.0 {
        Ok(())
    } else {
        Err(RuntimeError::new(format!(
            "Expected {name} to be greater than or equal to 0, actual value was {}",
            math::js_number_to_string(value)
        )))
    }
}

pub struct ParticleSystem<G: Gl + 'static> {
    pub show: bool,
    pub update_callback: Option<ParticleSystemCallback>,
    pub loop_animation: bool,
    pub image: Option<ParticleImage<G>>,
    /// Raised each update after a non-looping system's lifetime has passed.
    pub complete: Event<()>,
    emitter: Box<dyn ParticleEmitter>,
    bursts: Option<Vec<ParticleBurst>>,
    model_matrix: Matrix4,
    emitter_model_matrix: Matrix4,
    matrix_dirty: bool,
    combined_matrix: Matrix4,
    start_color: Color,
    end_color: Color,
    start_scale: f64,
    end_scale: f64,
    emission_rate: f64,
    minimum_speed: f64,
    maximum_speed: f64,
    minimum_particle_life: f64,
    maximum_particle_life: f64,
    minimum_mass: f64,
    maximum_mass: f64,
    minimum_image_size: Cartesian2,
    maximum_image_size: Cartesian2,
    size_in_meters: bool,
    lifetime: f64,
    billboards: Option<BillboardCollection<G>>,
    particles: Vec<Slot<G>>,
    pool: Vec<Slot<G>>,
    previous_time: Option<JulianDate>,
    current_time: f64,
    carry_over: f64,
    is_complete: bool,
    update_pool: bool,
    particle_estimate: f64,
    destroyed: bool,
}

impl<G: Gl + 'static> Default for ParticleSystem<G> {
    fn default() -> Self {
        Self::new(ParticleSystemOptions::default())
    }
}

macro_rules! non_negative {
    ($get:ident, $set:ident, $field:ident, $name:literal $(, $pool:ident)?) => {
        pub fn $get(&self) -> f64 {
            self.$field
        }
        pub fn $set(&mut self, value: f64) -> Result<()> {
            check_non_negative($name, value)?;
            self.$field = value;
            $(self.$pool = true;)?
            Ok(())
        }
    };
}

impl<G: Gl + 'static> ParticleSystem<G> {
    pub fn new(o: ParticleSystemOptions<G>) -> Self {
        let one = Cartesian2::new(1.0, 1.0);
        Self {
            show: o.show.unwrap_or(true),
            update_callback: o.update_callback,
            loop_animation: o.loop_animation.unwrap_or(true),
            image: o.image,
            complete: Event::new(),
            emitter: o.emitter.unwrap_or_else(|| Box::new(CircleEmitter::new(Some(0.5)))),
            bursts: o.bursts,
            model_matrix: o.model_matrix.unwrap_or(Matrix4::IDENTITY),
            emitter_model_matrix: o.emitter_model_matrix.unwrap_or(Matrix4::IDENTITY),
            matrix_dirty: true,
            combined_matrix: Matrix4::default(),
            start_color: o.color.or(o.start_color).unwrap_or(Color::WHITE),
            end_color: o.color.or(o.end_color).unwrap_or(Color::WHITE),
            start_scale: o.scale.or(o.start_scale).unwrap_or(1.0),
            end_scale: o.scale.or(o.end_scale).unwrap_or(1.0),
            emission_rate: o.emission_rate.unwrap_or(5.0),
            minimum_speed: o.speed.or(o.minimum_speed).unwrap_or(1.0),
            maximum_speed: o.speed.or(o.maximum_speed).unwrap_or(1.0),
            minimum_particle_life: o.particle_life.or(o.minimum_particle_life).unwrap_or(5.0),
            maximum_particle_life: o.particle_life.or(o.maximum_particle_life).unwrap_or(5.0),
            minimum_mass: o.mass.or(o.minimum_mass).unwrap_or(1.0),
            maximum_mass: o.mass.or(o.maximum_mass).unwrap_or(1.0),
            minimum_image_size: o.image_size.or(o.minimum_image_size).unwrap_or(one),
            maximum_image_size: o.image_size.or(o.maximum_image_size).unwrap_or(one),
            size_in_meters: o.size_in_meters.unwrap_or(false),
            lifetime: o.lifetime.unwrap_or(f64::MAX),
            billboards: None,
            particles: Vec::new(),
            pool: Vec::new(),
            previous_time: None,
            current_time: 0.0,
            carry_over: 0.0,
            is_complete: false,
            update_pool: true,
            particle_estimate: 0.0,
            destroyed: false,
        }
    }

    pub fn emitter(&self) -> &dyn ParticleEmitter {
        self.emitter.as_ref()
    }
    pub fn set_emitter(&mut self, emitter: Box<dyn ParticleEmitter>) {
        self.emitter = emitter;
    }
    pub fn bursts(&self) -> Option<&[ParticleBurst]> {
        self.bursts.as_deref()
    }
    /// Replacing the bursts resizes the pool on the next update.
    pub fn set_bursts(&mut self, bursts: Option<Vec<ParticleBurst>>) {
        self.bursts = bursts;
        self.update_pool = true;
    }
    pub fn model_matrix(&self) -> Matrix4 {
        self.model_matrix
    }
    pub fn set_model_matrix(&mut self, value: Matrix4) {
        self.matrix_dirty |= self.model_matrix != value;
        self.model_matrix = value;
    }
    pub fn emitter_model_matrix(&self) -> Matrix4 {
        self.emitter_model_matrix
    }
    pub fn set_emitter_model_matrix(&mut self, value: Matrix4) {
        self.matrix_dirty |= self.emitter_model_matrix != value;
        self.emitter_model_matrix = value;
    }
    pub fn start_color(&self) -> Color {
        self.start_color
    }
    pub fn set_start_color(&mut self, value: Color) {
        self.start_color = value;
    }
    pub fn end_color(&self) -> Color {
        self.end_color
    }
    pub fn set_end_color(&mut self, value: Color) {
        self.end_color = value;
    }
    non_negative!(start_scale, set_start_scale, start_scale, "value");
    non_negative!(end_scale, set_end_scale, end_scale, "value");
    non_negative!(emission_rate, set_emission_rate, emission_rate, "value", update_pool);
    non_negative!(minimum_speed, set_minimum_speed, minimum_speed, "value");
    non_negative!(maximum_speed, set_maximum_speed, maximum_speed, "value");
    non_negative!(minimum_particle_life, set_minimum_particle_life, minimum_particle_life, "value");
    non_negative!(maximum_particle_life, set_maximum_particle_life, maximum_particle_life, "value", update_pool);
    non_negative!(minimum_mass, set_minimum_mass, minimum_mass, "value");
    non_negative!(maximum_mass, set_maximum_mass, maximum_mass, "value");
    non_negative!(lifetime, set_lifetime, lifetime, "value");

    pub fn minimum_image_size(&self) -> Cartesian2 {
        self.minimum_image_size
    }
    pub fn set_minimum_image_size(&mut self, value: Cartesian2) -> Result<()> {
        check_non_negative("value.x", value.x)?;
        check_non_negative("value.y", value.y)?;
        self.minimum_image_size = value;
        Ok(())
    }
    pub fn maximum_image_size(&self) -> Cartesian2 {
        self.maximum_image_size
    }
    pub fn set_maximum_image_size(&mut self, value: Cartesian2) -> Result<()> {
        check_non_negative("value.x", value.x)?;
        check_non_negative("value.y", value.y)?;
        self.maximum_image_size = value;
        Ok(())
    }
    pub fn size_in_meters(&self) -> bool {
        self.size_in_meters
    }
    pub fn set_size_in_meters(&mut self, value: bool) {
        self.size_in_meters = value;
    }

    pub fn is_complete(&self) -> bool {
        self.is_complete
    }
    /// Seconds into the current cycle.
    pub fn current_time(&self) -> f64 {
        self.current_time
    }
    /// The fractional emission carried to the next update.
    pub fn carry_over(&self) -> f64 {
        self.carry_over
    }
    /// `_particleEstimate`: the pool size the last pool update aimed for.
    pub fn particle_estimate(&self) -> f64 {
        self.particle_estimate
    }
    /// The live particles, in Cesium's order.
    pub fn particles(&self) -> impl Iterator<Item = &Particle> {
        self.particles.iter().map(|s| &s.particle)
    }
    /// The billboard each live particle draws with.
    pub fn particle_billboards(&self) -> impl Iterator<Item = Option<&Billboard<G>>> {
        self.particles.iter().map(|s| s.billboard.as_ref())
    }
    /// The billboards of the pooled particles, in pool order.
    pub fn pool_billboards(&self) -> impl Iterator<Item = Option<&Billboard<G>>> {
        self.pool.iter().map(|s| s.billboard.as_ref())
    }
    /// The billboard collection, created by the first shown update.
    pub fn billboard_collection(&self) -> Option<&BillboardCollection<G>> {
        self.billboards.as_ref()
    }
    pub fn billboard_collection_mut(&mut self) -> Option<&mut BillboardCollection<G>> {
        self.billboards.as_mut()
    }

    fn image(&self) -> Option<BillboardImage<G>> {
        self.image.as_ref().map(|f| f())
    }

    /// `updateParticlePool`: pre-create hidden billboards for the particles
    /// the rate, life and bursts can keep alive.
    fn update_particle_pool(&mut self) -> Result<()> {
        let burst_amount: f64 = self.bursts.iter().flatten().map(|b| b.maximum).sum();
        let estimate = (self.emission_rate * self.maximum_particle_life + burst_amount).ceil();
        let to_add = (estimate - self.particles.len() as f64 - self.pool.len() as f64).max(0.0) as usize;
        for _ in 0..to_add {
            let image = self.image();
            let collection = self.billboards.as_mut().expect("collection exists");
            let billboard = collection.add(BillboardOptions { show: false, ..Default::default() }, image)?;
            self.pool.push(Slot { particle: Particle::default(), billboard: Some(billboard) });
        }
        self.particle_estimate = estimate;
        Ok(())
    }

    /// `freeParticlePool`. Cesium removes the pool's tail beyond
    /// `estimate - particles - pool`, which the pool update keeps from being
    /// positive, so this removes billboards only if the pool fell short.
    fn free_particle_pool(&mut self) -> Result<()> {
        let pool = self.pool.len() as f64;
        let shortfall = (self.particle_estimate - self.particles.len() as f64 - pool).max(0.0);
        let start = (pool - shortfall).max(0.0) as usize;
        let collection = self.billboards.as_mut().expect("collection exists");
        for slot in &self.pool[start..] {
            if let Some(billboard) = &slot.billboard {
                collection.remove(billboard)?;
            }
        }
        self.pool.truncate(start);
        Ok(())
    }

    /// `updateBillboard`: draw the particle at its position, size, color
    /// and scale for its age.
    fn update_billboard(&mut self, slot: &mut Slot<G>) -> Result<()> {
        if slot.billboard.is_none() {
            let image = self.image();
            let collection = self.billboards.as_mut().expect("collection exists");
            slot.billboard = Some(collection.add(BillboardOptions::default(), image)?);
        }
        let billboard = slot.billboard.as_ref().unwrap();
        let p = &slot.particle;
        billboard.set_width(Some(p.image_size.x));
        billboard.set_height(Some(p.image_size.y));
        billboard.set_position(p.position);
        billboard.set_size_in_meters(self.size_in_meters);
        billboard.set_show(true);
        let t = p.normalized_age();
        let (a, b) = (p.start_color, p.end_color);
        billboard.set_color(Color::new(
            math::lerp(a.red, b.red, t),
            math::lerp(a.green, b.green, t),
            math::lerp(a.blue, b.blue, t),
            math::lerp(a.alpha, b.alpha, t),
        ));
        billboard.set_scale(math::lerp(p.start_scale, p.end_scale, t));
        Ok(())
    }

    /// `addParticle`: give a just-emitted particle the system's look, a
    /// random life, mass, size and speed.
    fn add_particle(&mut self, mut slot: Slot<G>) -> Slot<G> {
        let p = &mut slot.particle;
        p.start_color = self.start_color;
        p.end_color = self.end_color;
        p.start_scale = self.start_scale;
        p.end_scale = self.end_scale;
        p.life = random_between(self.minimum_particle_life, self.maximum_particle_life);
        p.mass = random_between(self.minimum_mass, self.maximum_mass);
        p.image_size.x = random_between(self.minimum_image_size.x, self.maximum_image_size.x);
        p.image_size.y = random_between(self.minimum_image_size.y, self.maximum_image_size.y);
        p.reset_age();
        let speed = random_between(self.minimum_speed, self.maximum_speed);
        p.velocity = p.velocity.multiply_by_scalar(speed);
        slot
    }

    /// `calculateNumberToEmit`: whole emissions for `dt`, the carried
    /// fraction, and every burst whose time has passed.
    fn number_to_emit(&mut self, dt: f64) -> Result<f64> {
        if self.is_complete {
            return Ok(0.0);
        }
        if self.lifetime == 0.0 {
            // `CesiumMath.mod`'s check.
            return Err(RuntimeError::new("divisor cannot be 0."));
        }
        let v = math::modulo(dt, self.lifetime) * self.emission_rate;
        let mut count = v.floor();
        self.carry_over += v - count;
        if self.carry_over > 1.0 {
            count += 1.0;
            self.carry_over -= 1.0;
        }
        let current_time = self.current_time;
        for burst in self.bursts.iter_mut().flatten() {
            if !burst.complete && current_time > burst.time {
                count += random_between(burst.minimum, burst.maximum);
                burst.complete = true;
            }
        }
        Ok(count)
    }

    fn update_frame(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.show {
            return Ok(());
        }
        if self.billboards.is_none() {
            self.billboards = Some(BillboardCollection::default());
        }
        if self.update_pool {
            self.update_particle_pool()?;
            self.update_pool = false;
        }
        let dt = self
            .previous_time
            .as_ref()
            .map_or(0.0, |previous| JulianDate::seconds_difference(&frame.time, previous).max(0.0));

        let mut callback = self.update_callback.take();
        let mut slots = std::mem::take(&mut self.particles);
        let mut i = 0;
        let mut result = Ok(());
        while i < slots.len() {
            let alive = match callback.as_mut() {
                Some(f) => slots[i].particle.update(dt, Some(&mut **f)),
                None => slots[i].particle.update(dt, None),
            };
            if alive {
                if let Err(e) = self.update_billboard(&mut slots[i]) {
                    result = Err(e);
                    break;
                }
                i += 1;
            } else {
                let dead = slots.swap_remove(i);
                if let Some(billboard) = &dead.billboard {
                    billboard.set_show(false);
                }
                self.pool.push(dead);
            }
        }
        self.particles = slots;
        self.update_callback = callback;
        result?;

        let count = self.number_to_emit(dt)?;
        if count > 0.0 {
            if self.matrix_dirty {
                self.combined_matrix = self.model_matrix.multiply(&self.emitter_model_matrix);
                self.matrix_dirty = false;
            }
            let matrix = self.combined_matrix;
            let mut emitted = 0.0;
            while emitted < count {
                let mut slot = self.pool.pop().unwrap_or(Slot { particle: Particle::default(), billboard: None });
                self.emitter.emit(&mut slot.particle);
                let p = &mut slot.particle;
                let rotated = matrix.multiply_by_point(p.position + p.velocity);
                p.position = matrix.multiply_by_point(p.position);
                p.velocity = (rotated - p.position).normalize();
                let mut slot = self.add_particle(slot);
                self.update_billboard(&mut slot)?;
                self.particles.push(slot);
                emitted += 1.0;
            }
        }

        self.billboards.as_mut().expect("collection exists").update(frame)?;
        self.previous_time = Some(frame.time);
        self.current_time += dt;
        if self.lifetime != f64::MAX && self.current_time > self.lifetime {
            if self.loop_animation {
                self.current_time = math::modulo(self.current_time, self.lifetime);
                for burst in self.bursts.iter_mut().flatten() {
                    burst.complete = false;
                }
            } else {
                self.is_complete = true;
                self.complete.raise_event(&());
            }
        }
        if frame.frame_number.is_multiple_of(120) {
            self.free_particle_pool()?;
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

impl<G: Gl + 'static> ScenePrimitive for ParticleSystem<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.update_frame(frame)
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<()> {
        Ok(())
    }
    /// Destroys the billboard collection.
    fn destroy(&mut self) -> Result<()> {
        if let Some(mut billboards) = self.billboards.take() {
            billboards.destroy()?;
        }
        self.particles.clear();
        self.pool.clear();
        self.destroyed = true;
        Ok(())
    }
}
