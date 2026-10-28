//! CesiumJS `Moon`: draws the Moon in 3D at its astronomical position and
//! orientation based on the Simon 1994 planetary ephemeris and IAU orientation parameters.

use super::ellipsoid_primitive::{EllipsoidPrimitive, EllipsoidPrimitiveOptions};
use super::frame_state::FrameState;
use super::material::{Material, MaterialImage, MaterialValue, IMAGE_TYPE};
use super::scene::{Result, ScenePrimitive};
use super::scene_mode::SceneMode;
use crate::core::simon1994_planetary_positions;
use crate::core::{
    Ellipsoid, IauOrientationAxes, Matrix3, Matrix4, RuntimeError, Transforms,
};

/// Default texture URL path for the Moon in CesiumJS.
pub const DEFAULT_MOON_TEXTURE_URL: &str = "Assets/Textures/moonSmall.jpg";

/// Options for configuring a [`Moon`] primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct MoonOptions {
    pub show: Option<bool>,
    pub texture_url: Option<String>,
    pub ellipsoid: Option<Ellipsoid>,
    pub only_sun_lighting: Option<bool>,
}

impl Default for MoonOptions {
    fn default() -> Self {
        Self {
            show: Some(true),
            texture_url: Some(DEFAULT_MOON_TEXTURE_URL.to_string()),
            ellipsoid: Some(Ellipsoid::MOON),
            only_sun_lighting: Some(true),
        }
    }
}

/// Draws the Moon in 3D using an astronomical coordinate frame on an `EllipsoidPrimitive`.
pub struct Moon {
    pub show: bool,
    pub texture_url: String,
    pub ellipsoid: Ellipsoid,
    pub only_sun_lighting: bool,
    pub ellipsoid_primitive: EllipsoidPrimitive,
    axes: IauOrientationAxes,
    destroyed: bool,
}

impl Default for Moon {
    fn default() -> Self {
        Self::new(MoonOptions::default())
    }
}

impl Moon {
    /// Creates a new `Moon` primitive with the given options.
    pub fn new(options: MoonOptions) -> Self {
        let show = options.show.unwrap_or(true);
        let texture_url = options
            .texture_url
            .unwrap_or_else(|| DEFAULT_MOON_TEXTURE_URL.to_string());
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::MOON);
        let only_sun_lighting = options.only_sun_lighting.unwrap_or(true);

        let mut ellipsoid_primitive = EllipsoidPrimitive::new(EllipsoidPrimitiveOptions {
            radii: Some(ellipsoid.radii),
            only_sun_lighting: Some(only_sun_lighting),
            depth_test_enabled: Some(false),
            ..Default::default()
        });

        let mut material = Material::from_type(IMAGE_TYPE);
        let _ = material.set_uniform(
            "image",
            MaterialValue::Sampler2D(MaterialImage::Url(texture_url.clone())),
        );
        ellipsoid_primitive.material = material;

        Self {
            show,
            texture_url,
            ellipsoid,
            only_sun_lighting,
            ellipsoid_primitive,
            axes: IauOrientationAxes::default(),
            destroyed: false,
        }
    }

    /// Evaluates the transformation matrix for the Moon at the given Julian date.
    pub fn compute_moon_transform(&self, time: &crate::core::JulianDate) -> Matrix4 {
        let icrf_to_fixed = Transforms::compute_teme_to_pseudo_fixed_matrix(time);
        let rotation_axes = self.axes.evaluate(time);
        let rotation = Matrix3::multiply(&icrf_to_fixed, &rotation_axes.transpose());

        let translation_inertial =
            simon1994_planetary_positions::compute_moon_position_in_earth_inertial_frame(time);
        let translation = icrf_to_fixed.multiply_by_vector(translation_inertial);

        Matrix4::from_rotation_translation(&rotation, translation)
    }

    /// Returns `true` if this primitive has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

impl ScenePrimitive for Moon {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("Moon primitive is destroyed"));
        }
        if !self.show || frame.mode != SceneMode::Scene3D {
            return Ok(());
        }

        let model_matrix = self.compute_moon_transform(&frame.time);
        self.ellipsoid_primitive.model_matrix = model_matrix;
        self.ellipsoid_primitive.only_sun_lighting = self.only_sun_lighting;
        self.ellipsoid_primitive.update(frame)
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, pass: super::Cesium3DTilePass) -> Result<()> {
        self.ellipsoid_primitive.update_for_pass(frame, pass)
    }

    fn destroy(&mut self) -> Result<()> {
        self.destroyed = true;
        self.ellipsoid_primitive.destroy()
    }
}
