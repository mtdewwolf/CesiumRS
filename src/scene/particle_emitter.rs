//! CesiumJS `ParticleEmitter`, the abstract base class of the particle
//! emitters, as a trait; and the four emitters: `BoxEmitter`,
//! `CircleEmitter`, `ConeEmitter` and `SphereEmitter`.
//!
//! Emitters draw from Cesium's random number generator
//! ([`crate::core::math::random_between`]), so seeding it with
//! `set_random_number_seed` reproduces CesiumJS's particles. Their argument
//! checks are debug-build `DeveloperError`s and are not ported.

use super::Particle;
use crate::core::js_math::JsMath;
use crate::core::math::{random_between, to_radians, PI, TWO_PI};
use crate::core::Cartesian3;

/// Sets a new particle's position and velocity.
pub trait ParticleEmitter {
    fn emit(&self, particle: &mut Particle);
}

/// Emits from random points in an axis-aligned box centered at the origin,
/// moving away from the center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxEmitter {
    dimensions: Cartesian3,
}

impl BoxEmitter {
    /// The box's width, height and depth; 1×1×1 by default.
    pub fn new(dimensions: Option<Cartesian3>) -> Self {
        Self { dimensions: dimensions.unwrap_or(Cartesian3::new(1.0, 1.0, 1.0)) }
    }

    pub fn dimensions(&self) -> Cartesian3 {
        self.dimensions
    }

    pub fn set_dimensions(&mut self, value: Cartesian3) {
        self.dimensions = value;
    }
}

impl Default for BoxEmitter {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ParticleEmitter for BoxEmitter {
    fn emit(&self, particle: &mut Particle) {
        let half = self.dimensions * 0.5;
        let x = random_between(-half.x, half.x);
        let y = random_between(-half.y, half.y);
        let z = random_between(-half.z, half.z);
        particle.position = Cartesian3::new(x, y, z);
        particle.velocity = particle.position.normalize();
    }
}

/// Emits from random points in a disk in the XY plane, moving along +Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleEmitter {
    radius: f64,
}

impl CircleEmitter {
    /// 1 by default.
    pub fn new(radius: Option<f64>) -> Self {
        Self { radius: radius.unwrap_or(1.0) }
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub fn set_radius(&mut self, value: f64) {
        self.radius = value;
    }
}

impl Default for CircleEmitter {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ParticleEmitter for CircleEmitter {
    fn emit(&self, particle: &mut Particle) {
        let theta = random_between(0.0, TWO_PI);
        let rad = random_between(0.0, self.radius);
        particle.position = Cartesian3::new(rad * theta.js_cos(), rad * theta.js_sin(), 0.0);
        particle.velocity = Cartesian3::UNIT_Z;
    }
}

/// Emits from the origin in random directions within a cone around +Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConeEmitter {
    angle: f64,
}

impl ConeEmitter {
    /// The cone's half angle in radians; 30° by default.
    pub fn new(angle: Option<f64>) -> Self {
        Self { angle: angle.unwrap_or(to_radians(30.0)) }
    }

    pub fn angle(&self) -> f64 {
        self.angle
    }

    pub fn set_angle(&mut self, value: f64) {
        self.angle = value;
    }
}

impl Default for ConeEmitter {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ParticleEmitter for ConeEmitter {
    fn emit(&self, particle: &mut Particle) {
        let radius = self.angle.js_tan();
        let theta = random_between(0.0, TWO_PI);
        let rad = random_between(0.0, radius);
        particle.velocity = Cartesian3::new(rad * theta.js_cos(), rad * theta.js_sin(), 1.0).normalize();
        particle.position = Cartesian3::ZERO;
    }
}

/// Emits from random points in a ball centered at the origin, moving away
/// from the center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SphereEmitter {
    radius: f64,
}

impl SphereEmitter {
    /// 1 by default.
    pub fn new(radius: Option<f64>) -> Self {
        Self { radius: radius.unwrap_or(1.0) }
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub fn set_radius(&mut self, value: f64) {
        self.radius = value;
    }
}

impl Default for SphereEmitter {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ParticleEmitter for SphereEmitter {
    fn emit(&self, particle: &mut Particle) {
        let theta = random_between(0.0, TWO_PI);
        let phi = random_between(0.0, PI);
        let rad = random_between(0.0, self.radius);
        let (sin_phi, cos_phi) = (phi.js_sin(), phi.js_cos());
        particle.position =
            Cartesian3::new(rad * theta.js_cos() * sin_phi, rad * theta.js_sin() * sin_phi, rad * cos_phi);
        particle.velocity = particle.position.normalize();
    }
}
