//! CesiumJS `ParticleBurst`: a burst of particles a `ParticleSystem` emits at
//! a set time.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleBurst {
    /// Seconds after the start of the particle system's lifetime.
    pub time: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub(crate) complete: bool,
}

/// `new ParticleBurst(options)`: time 0, minimum 0, maximum 50 by default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParticleBurstOptions {
    pub time: Option<f64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

impl ParticleBurst {
    pub fn new(options: ParticleBurstOptions) -> Self {
        Self {
            time: options.time.unwrap_or(0.0),
            minimum: options.minimum.unwrap_or(0.0),
            maximum: options.maximum.unwrap_or(50.0),
            complete: false,
        }
    }

    /// Whether the burst has fired in the current cycle.
    pub fn complete(&self) -> bool {
        self.complete
    }
}

impl Default for ParticleBurst {
    fn default() -> Self {
        Self::new(ParticleBurstOptions::default())
    }
}
