//! CesiumJS `Particle`: one particle of a `ParticleSystem`, which moves by
//! its velocity and ages each update.

use super::FrameResource;
use crate::core::{Cartesian2, Cartesian3, Color};

/// `new Particle(options)`: unset fields take Cesium's defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleOptions {
    /// Kilograms; 1 by default.
    pub mass: Option<f64>,
    pub position: Option<Cartesian3>,
    pub velocity: Option<Cartesian3>,
    /// Seconds; `f64::MAX` (Cesium's `Number.MAX_VALUE`), so never dies, by default.
    pub life: Option<f64>,
    pub image: Option<FrameResource>,
    pub start_color: Option<Color>,
    pub end_color: Option<Color>,
    pub start_scale: Option<f64>,
    pub end_scale: Option<f64>,
    /// Pixels; 1×1 by default.
    pub image_size: Option<Cartesian2>,
}

/// A `ParticleSystem`'s per-particle update callback, given the time step.
pub type ParticleUpdateFunction<'a> = &'a mut dyn FnMut(&mut Particle, f64);

#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    pub mass: f64,
    pub position: Cartesian3,
    pub velocity: Cartesian3,
    pub life: f64,
    /// The billboard image: a URL, image or canvas the host owns.
    pub image: Option<FrameResource>,
    pub start_color: Color,
    pub end_color: Color,
    pub start_scale: f64,
    pub end_scale: f64,
    pub image_size: Cartesian2,
    age: f64,
    normalized_age: f64,
}

impl Particle {
    pub fn new(options: ParticleOptions) -> Self {
        Self {
            mass: options.mass.unwrap_or(1.0),
            position: options.position.unwrap_or(Cartesian3::ZERO),
            velocity: options.velocity.unwrap_or(Cartesian3::ZERO),
            life: options.life.unwrap_or(f64::MAX),
            image: options.image,
            start_color: options.start_color.unwrap_or(Color::WHITE),
            end_color: options.end_color.unwrap_or(Color::WHITE),
            start_scale: options.start_scale.unwrap_or(1.0),
            end_scale: options.end_scale.unwrap_or(1.0),
            image_size: options.image_size.unwrap_or(Cartesian2::new(1.0, 1.0)),
            age: 0.0,
            normalized_age: 0.0,
        }
    }

    /// Seconds since the particle was created.
    pub fn age(&self) -> f64 {
        self.age
    }

    /// Age over life, in [0, 1] while alive; 0 for a particle that never dies.
    pub fn normalized_age(&self) -> f64 {
        self.normalized_age
    }

    /// Moves the particle by `velocity * dt`, runs `update_function`, then
    /// ages it. Returns whether it is still alive.
    pub fn update(&mut self, dt: f64, update_function: Option<ParticleUpdateFunction<'_>>) -> bool {
        self.position += self.velocity * dt;
        if let Some(update) = update_function {
            update(self, dt);
        }
        self.age += dt;
        self.normalized_age = if self.life == f64::MAX { 0.0 } else { self.age / self.life };
        self.age <= self.life
    }
}

impl Default for Particle {
    fn default() -> Self {
        Self::new(ParticleOptions::default())
    }
}
