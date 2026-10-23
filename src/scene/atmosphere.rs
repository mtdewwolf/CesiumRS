//! CesiumJS atmosphere parameters shared by models, fog and sky rendering.
use super::DynamicAtmosphereLightingType;
use crate::{
    core::{
        math::{equals_epsilon, EPSILON7},
        Cartesian3,
    },
    renderer::UniformAtmosphere,
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    pub light_intensity: f64,
    pub rayleigh_coefficient: Cartesian3,
    pub mie_coefficient: Cartesian3,
    pub rayleigh_scale_height: f64,
    pub mie_scale_height: f64,
    pub mie_anisotropy: f64,
    pub hue_shift: f64,
    pub saturation_shift: f64,
    pub brightness_shift: f64,
    pub dynamic_lighting: DynamicAtmosphereLightingType,
}
impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            light_intensity: 10.,
            rayleigh_coefficient: Cartesian3::new(5.5e-6, 13e-6, 28.4e-6),
            mie_coefficient: Cartesian3::new(21e-6, 21e-6, 21e-6),
            rayleigh_scale_height: 10000.,
            mie_scale_height: 3200.,
            mie_anisotropy: 0.9,
            hue_shift: 0.,
            saturation_shift: 0.,
            brightness_shift: 0.,
            dynamic_lighting: DynamicAtmosphereLightingType::None,
        }
    }
}
impl Atmosphere {
    pub fn requires_color_correct(&self) -> bool {
        ![self.hue_shift, self.saturation_shift, self.brightness_shift]
            .into_iter()
            .all(|v| equals_epsilon(v, 0., EPSILON7, EPSILON7))
    }
    pub fn uniform(&self) -> UniformAtmosphere {
        UniformAtmosphere {
            hsb_shift: Cartesian3::new(self.hue_shift, self.saturation_shift, self.brightness_shift),
            light_intensity: self.light_intensity,
            rayleigh_coefficient: self.rayleigh_coefficient,
            mie_coefficient: self.mie_coefficient,
            rayleigh_scale_height: self.rayleigh_scale_height,
            mie_scale_height: self.mie_scale_height,
            mie_anisotropy: self.mie_anisotropy,
            dynamic_lighting: self.dynamic_lighting.value() as f64,
        }
    }
}
