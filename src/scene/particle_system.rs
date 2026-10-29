//! ParticleSystem CPU simulation and billboard snapshots.
//! GPU billboard collection submission and pool memory estimation remain unported.
use super::{particle::ParticleOptions, particle_emitter::CircleEmitter, Particle, ParticleBurst, ParticleEmitter};
use crate::core::{math::random_between, Cartesian2, Color, Event, Matrix4, RuntimeError};
pub type ParticleUpdateCallback<'a> = dyn FnMut(&mut Particle, f64) + 'a;
#[derive(Clone, Debug)]
pub struct ParticleBillboard {
    pub particle: Particle,
    pub color: Color,
    pub scale: f64,
}
pub struct ParticleSystem {
    pub show: bool,
    pub loop_animation: bool,
    pub lifetime: f64,
    pub emission_rate: f64,
    pub model_matrix: Matrix4,
    pub emitter_model_matrix: Matrix4,
    pub emitter: Option<Box<dyn ParticleEmitter>>,
    pub bursts: Vec<ParticleBurst>,
    pub particle_options: ParticleOptions,
    pub particle_life: [f64; 2],
    pub mass: [f64; 2],
    pub speed: [f64; 2],
    pub image_size: [Cartesian2; 2],
    pub complete: Event<()>,
    particles: Vec<Particle>,
    current_time: f64,
    previous_time: Option<f64>,
    carry_over: f64,
    is_complete: bool,
    destroyed: bool,
}
impl Default for ParticleSystem {
    fn default() -> Self {
        Self {
            show: true,
            loop_animation: true,
            lifetime: f64::MAX,
            emission_rate: 5.,
            model_matrix: Matrix4::IDENTITY,
            emitter_model_matrix: Matrix4::IDENTITY,
            emitter: Some(Box::new(CircleEmitter::new(Some(0.5)))),
            bursts: vec![],
            particle_options: ParticleOptions::default(),
            particle_life: [5., 5.],
            mass: [1., 1.],
            speed: [1., 1.],
            image_size: [Cartesian2::new(1., 1.); 2],
            complete: Event::new(),
            particles: vec![],
            current_time: 0.,
            previous_time: None,
            carry_over: 0.,
            is_complete: false,
            destroyed: false,
        }
    }
}
impl ParticleSystem {
    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }
    pub fn current_time(&self) -> f64 {
        self.current_time
    }
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }
    pub fn billboards(&self) -> Vec<ParticleBillboard> {
        self.particles
            .iter()
            .map(|p| {
                let t = p.normalized_age();
                let a = p.start_color;
                let b = p.end_color;
                ParticleBillboard {
                    particle: p.clone(),
                    color: Color::new(
                        a.red + (b.red - a.red) * t,
                        a.green + (b.green - a.green) * t,
                        a.blue + (b.blue - a.blue) * t,
                        a.alpha + (b.alpha - a.alpha) * t,
                    ),
                    scale: p.start_scale + (p.end_scale - p.start_scale) * t,
                }
            })
            .collect()
    }
    pub fn update(
        &mut self,
        time_seconds: f64,
        mut callback: Option<&mut ParticleUpdateCallback<'_>>,
    ) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("ParticleSystem was destroyed"));
        }
        if !self.show {
            return Ok(());
        }
        if !time_seconds.is_finite()
            || self.lifetime <= 0.
            || !self.lifetime.is_finite()
            || self.emission_rate < 0.
            || !self.emission_rate.is_finite()
        {
            return Err(RuntimeError::new(
                "Particle time, lifetime and emission rate must be finite, lifetime positive and rate nonnegative",
            ));
        }
        let dt = self.previous_time.map_or(0., |t| (time_seconds - t).max(0.));
        let mut i = 0;
        while i < self.particles.len() {
            let alive = match callback.as_mut() {
                Some(f) => self.particles[i].update(dt, Some(&mut **f)),
                None => self.particles[i].update(dt, None),
            };
            if alive {
                i += 1;
            } else {
                self.particles.swap_remove(i);
            }
        }
        let mut count = 0.;
        if !self.is_complete {
            let v = dt.rem_euclid(self.lifetime) * self.emission_rate;
            count = v.floor();
            self.carry_over += v - count;
            if self.carry_over > 1. {
                count += 1.;
                self.carry_over -= 1.;
            }
            for b in &mut self.bursts {
                if !b.complete && self.current_time > b.time {
                    count += random_between(b.minimum, b.maximum);
                    b.complete = true;
                }
            }
        }
        if let Some(emitter) = &self.emitter {
            let matrix = self.model_matrix.multiply(&self.emitter_model_matrix);
            for _ in 0..count.ceil() as usize {
                let mut p = Particle::new(self.particle_options.clone());
                emitter.emit(&mut p);
                let endpoint = matrix.multiply_by_point(p.position.add(p.velocity));
                p.position = matrix.multiply_by_point(p.position);
                p.velocity = endpoint.subtract(p.position).normalize();
                p.life = random_between(self.particle_life[0], self.particle_life[1]);
                p.mass = random_between(self.mass[0], self.mass[1]);
                p.image_size = Cartesian2::new(
                    random_between(self.image_size[0].x, self.image_size[1].x),
                    random_between(self.image_size[0].y, self.image_size[1].y),
                );
                p.velocity = p.velocity.multiply_by_scalar(random_between(self.speed[0], self.speed[1]));
                self.particles.push(p);
            }
        }
        self.previous_time = Some(time_seconds);
        self.current_time += dt;
        if self.lifetime != f64::MAX && self.current_time > self.lifetime {
            if self.loop_animation {
                self.current_time = self.current_time.rem_euclid(self.lifetime);
                for b in &mut self.bursts {
                    b.complete = false;
                }
            } else {
                self.is_complete = true;
                self.complete.raise_event(&());
            }
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) {
        self.particles.clear();
        self.destroyed = true;
    }
}
