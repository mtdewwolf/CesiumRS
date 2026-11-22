//! CesiumJS `Moon`: draws the Moon in 3D at its astronomical position and
//! orientation based on the Simon 1994 planetary ephemeris and IAU orientation
//! parameters, on top of an [`EllipsoidPrimitive`].

use super::ellipsoid_primitive::{EllipsoidPrimitive, EllipsoidPrimitiveOptions};
use super::frame_state::FrameState;
use super::material::{Material, MaterialImage, MaterialValue, IMAGE_TYPE};
use super::scene::{Result, SceneCommand, ScenePrimitive};
use crate::core::simon1994_planetary_positions;
use crate::core::{
    EarthOrientationParameters, Ellipsoid, IauOrientationAxes, Iau2006XysData, Matrix3, Matrix4, RuntimeError, Transforms,
};
use crate::renderer::Gl;

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

/// Draws the Moon in 3D using an astronomical coordinate frame on an
/// `EllipsoidPrimitive`.
pub struct Moon<G: Gl + 'static> {
    pub show: bool,
    pub texture_url: String,
    pub ellipsoid: Ellipsoid,
    pub only_sun_lighting: bool,
    pub ellipsoid_primitive: EllipsoidPrimitive<G>,
    axes: IauOrientationAxes,
    destroyed: bool,
}

impl<G: Gl + 'static> Default for Moon<G> {
    fn default() -> Self {
        Self::new(MoonOptions::default())
    }
}

impl<G: Gl + 'static> Moon<G> {
    /// Creates a new `Moon` primitive with the given options.
    pub fn new(options: MoonOptions) -> Self {
        let show = options.show.unwrap_or(true);
        let texture_url = options.texture_url.unwrap_or_else(|| DEFAULT_MOON_TEXTURE_URL.to_string());
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::MOON);
        let only_sun_lighting = options.only_sun_lighting.unwrap_or(true);

        let mut ellipsoid_primitive = EllipsoidPrimitive::new(EllipsoidPrimitiveOptions {
            radii: Some(ellipsoid.radii),
            material: Some(Material::from_type(IMAGE_TYPE)),
            only_sun_lighting: Some(only_sun_lighting),
            depth_test_enabled: Some(false),
            ..Default::default()
        });
        ellipsoid_primitive.material.translucent = Some(false);

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
        // `Transforms.computeIcrfToFixedMatrix` with a TEME fallback, exactly
        // as Moon.js. Without loaded EOP/XYS data the ICRF step returns none and
        // the pseudo-fixed rotation is used.
        let icrf_to_fixed = Transforms::compute_icrf_to_central_body_fixed_matrix(
            time,
            &EarthOrientationParameters::none(),
            &Iau2006XysData::default(),
        );
        let rotation_axes = self.axes.evaluate(time);
        let rotation = Matrix3::multiply(&icrf_to_fixed, &rotation_axes.transpose());

        let translation_inertial = simon1994_planetary_positions::compute_moon_position_in_earth_inertial_frame(time);
        let translation = icrf_to_fixed.multiply_by_vector(translation_inertial);

        Matrix4::from_rotation_translation(&rotation, translation)
    }

    /// Cesium's `Moon.prototype.update`.
    pub fn update(&mut self, frame: &mut FrameState) -> Result<Option<SceneCommand>> {
        self.check()?;
        if !self.show {
            self.ellipsoid_primitive.clear_commands();
            return Ok(None);
        }

        let _ = self.ellipsoid_primitive.material.set_uniform(
            "image",
            MaterialValue::Sampler2D(MaterialImage::Url(self.texture_url.clone())),
        );
        self.ellipsoid_primitive.only_sun_lighting = self.only_sun_lighting;
        self.ellipsoid_primitive.model_matrix = self.compute_moon_transform(&frame.time);

        // The Moon captures its single command into a scratch list and returns
        // it, leaving the frame's command list untouched.
        let saved = std::mem::take(&mut frame.command_list);
        let result = self.ellipsoid_primitive.update(frame);
        let mut scratch = std::mem::replace(&mut frame.command_list, saved);
        result?;
        Ok(if scratch.len() == 1 { scratch.pop() } else { None })
    }

    /// Returns `true` if this primitive has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn check(&self) -> Result<()> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }

    /// Cesium's `Moon.prototype.destroy`.
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        self.ellipsoid_primitive.destroy()?;
        self.destroyed = true;
        Ok(())
    }
}

impl<G: Gl + 'static> ScenePrimitive for Moon<G> {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if let Some(command) = Moon::update(self, frame)? {
            frame.command_list.push(command);
        }
        Ok(())
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, _pass: super::Cesium3DTilePass) -> Result<()> {
        if let Some(command) = Moon::update(self, frame)? {
            frame.command_list.push(command);
        }
        Ok(())
    }

    fn destroy(&mut self) -> Result<()> {
        Moon::destroy(self)
    }
}
