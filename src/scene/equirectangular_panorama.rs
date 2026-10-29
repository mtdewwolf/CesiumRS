//! CesiumJS `EquirectangularPanorama`: displays 360-degree equirectangular
//! imagery projected onto an enclosing sphere.

use super::frame_state::FrameState;
use super::material::{Material, MaterialImage, MaterialValue, IMAGE_TYPE};
use super::scene::{Result, ScenePrimitive};
use super::scene_mode::SceneMode;
use crate::core::{BoundingSphere, Cartesian2, Cartesian3, Credit, Matrix4, RuntimeError};

const DEFAULT_RADIUS: f64 = 100_000.0;

/// Options for configuring an [`EquirectangularPanorama`].
#[derive(Clone, Debug, PartialEq)]
pub struct EquirectangularPanoramaOptions {
    pub image: String,
    pub transform: Option<Matrix4>,
    pub radius: Option<f64>,
    pub repeat_horizontal: Option<f64>,
    pub repeat_vertical: Option<f64>,
    pub credit: Option<Credit>,
    pub show: Option<bool>,
}

impl EquirectangularPanoramaOptions {
    /// Creates options with the required image URL or path.
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            transform: Some(Matrix4::IDENTITY),
            radius: Some(DEFAULT_RADIUS),
            repeat_horizontal: Some(1.0),
            repeat_vertical: Some(1.0),
            credit: None,
            show: Some(true),
        }
    }
}

/// A 360-degree panorama primitive displaying equirectangular imagery mapped
/// inside a sphere.
pub struct EquirectangularPanorama {
    pub image: String,
    pub transform: Matrix4,
    pub radius: f64,
    pub repeat_horizontal: f64,
    pub repeat_vertical: f64,
    pub credit: Option<Credit>,
    pub show: bool,
    pub material: Material,
    bounding_sphere: BoundingSphere,
    destroyed: bool,
}

impl EquirectangularPanorama {
    /// Creates a new `EquirectangularPanorama` from options.
    pub fn new(options: EquirectangularPanoramaOptions) -> Self {
        let radius = options.radius.unwrap_or(DEFAULT_RADIUS);
        let transform = options.transform.unwrap_or(Matrix4::IDENTITY);
        let repeat_h = options.repeat_horizontal.unwrap_or(1.0);
        let repeat_v = options.repeat_vertical.unwrap_or(1.0);
        let show = options.show.unwrap_or(true);

        let mut material = Material::from_type(IMAGE_TYPE);
        let _ = material.set_uniform(
            "image",
            MaterialValue::Sampler2D(MaterialImage::Url(options.image.clone())),
        );
        let _ = material.set_uniform(
            "repeat",
            MaterialValue::Vec2(Cartesian2::new(-repeat_h, repeat_v)),
        );

        let base_sphere = BoundingSphere::new(Cartesian3::ZERO, radius);
        let bounding_sphere = BoundingSphere::transform(&base_sphere, &transform);

        Self {
            image: options.image,
            transform,
            radius,
            repeat_horizontal: repeat_h,
            repeat_vertical: repeat_v,
            credit: options.credit,
            show,
            material,
            bounding_sphere,
            destroyed: false,
        }
    }

    /// The enclosing bounding sphere in world coordinates.
    pub fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    /// Returns `true` if this primitive has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

impl ScenePrimitive for EquirectangularPanorama {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("EquirectangularPanorama is destroyed"));
        }
        if !self.show || frame.mode != SceneMode::Scene3D {
            return Ok(());
        }

        let base_sphere = BoundingSphere::new(Cartesian3::ZERO, self.radius);
        self.bounding_sphere = BoundingSphere::transform(&base_sphere, &self.transform);

        Ok(())
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, _pass: super::Cesium3DTilePass) -> Result<()> {
        self.update(frame)
    }

    fn destroy(&mut self) -> Result<()> {
        self.destroyed = true;
        Ok(())
    }
}

impl super::panorama::Panorama for EquirectangularPanorama {
    fn show(&self) -> bool { self.show }
    fn set_show(&mut self, show: bool) { self.show = show; }
    fn transform(&self) -> &Matrix4 { &self.transform }
    fn credit(&self) -> Option<&Credit> { self.credit.as_ref() }
}
