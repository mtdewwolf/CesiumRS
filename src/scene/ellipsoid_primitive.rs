//! CesiumJS `EllipsoidPrimitive`: a renderable 3D ellipsoid primitive
//! evaluated via ray casting against its bounding box geometry.

use super::frame_state::FrameState;
use super::material::{Material, COLOR_TYPE};
use super::scene::{Result, ScenePrimitive};
use super::scene_mode::SceneMode;
use crate::core::{BoundingSphere, Cartesian3, Matrix4, RuntimeError};

/// Options for configuring an [`EllipsoidPrimitive`].
#[derive(Clone, Debug, PartialEq)]
pub struct EllipsoidPrimitiveOptions {
    pub center: Option<Cartesian3>,
    pub radii: Option<Cartesian3>,
    pub model_matrix: Option<Matrix4>,
    pub show: Option<bool>,
    pub id: Option<String>,
    pub debug_show_bounding_volume: Option<bool>,
    pub only_sun_lighting: Option<bool>,
    pub depth_test_enabled: Option<bool>,
}

impl Default for EllipsoidPrimitiveOptions {
    fn default() -> Self {
        Self {
            center: Some(Cartesian3::ZERO),
            radii: None,
            model_matrix: Some(Matrix4::IDENTITY),
            show: Some(true),
            id: None,
            debug_show_bounding_volume: Some(false),
            only_sun_lighting: Some(true),
            depth_test_enabled: Some(true),
        }
    }
}

/// A renderable 3D ellipsoid primitive that casts rays against an enclosing box.
pub struct EllipsoidPrimitive {
    pub center: Cartesian3,
    pub radii: Cartesian3,
    pub model_matrix: Matrix4,
    pub show: bool,
    pub material: Material,
    pub id: Option<String>,
    pub debug_show_bounding_volume: bool,
    pub only_sun_lighting: bool,
    pub depth_test_enabled: bool,

    computed_model_matrix: Matrix4,
    one_over_ellipsoid_radii_squared: Cartesian3,
    bounding_sphere: BoundingSphere,
    destroyed: bool,
}

impl EllipsoidPrimitive {
    /// Creates a new `EllipsoidPrimitive` from the given options.
    pub fn new(options: EllipsoidPrimitiveOptions) -> Self {
        let center = options.center.unwrap_or(Cartesian3::ZERO);
        let radii = options.radii.unwrap_or(Cartesian3::ONE);
        let model_matrix = options.model_matrix.unwrap_or(Matrix4::IDENTITY);
        let show = options.show.unwrap_or(true);
        let id = options.id;
        let debug_show_bounding_volume = options.debug_show_bounding_volume.unwrap_or(false);
        let only_sun_lighting = options.only_sun_lighting.unwrap_or(true);
        let depth_test_enabled = options.depth_test_enabled.unwrap_or(true);
        let material = Material::from_type(COLOR_TYPE);

        let mut primitive = Self {
            center,
            radii,
            model_matrix,
            show,
            material,
            id,
            debug_show_bounding_volume,
            only_sun_lighting,
            depth_test_enabled,
            computed_model_matrix: Matrix4::IDENTITY,
            one_over_ellipsoid_radii_squared: Cartesian3::ZERO,
            bounding_sphere: BoundingSphere::default(),
            destroyed: false,
        };
        primitive.recompute_geometry();
        primitive
    }

    /// Recomputes cached spatial matrices and bounding spheres.
    pub fn recompute_geometry(&mut self) {
        // Translation from model coordinates by center
        let translation = Matrix4::from_translation(self.center);
        self.computed_model_matrix = Matrix4::multiply(&self.model_matrix, &translation);

        let rx = self.radii.x;
        let ry = self.radii.y;
        let rz = self.radii.z;
        self.one_over_ellipsoid_radii_squared = Cartesian3::new(
            if rx != 0.0 { 1.0 / (rx * rx) } else { 0.0 },
            if ry != 0.0 { 1.0 / (ry * ry) } else { 0.0 },
            if rz != 0.0 { 1.0 / (rz * rz) } else { 0.0 },
        );

        let max_radius = rx.max(ry).max(rz);
        let base_sphere = BoundingSphere::new(Cartesian3::ZERO, max_radius);
        self.bounding_sphere = BoundingSphere::transform(&base_sphere, &self.computed_model_matrix);
    }

    /// The computed 4x4 matrix including the center offset.
    pub fn computed_model_matrix(&self) -> &Matrix4 {
        &self.computed_model_matrix
    }

    /// Vector containing 1 / (radii^2) per component.
    pub fn one_over_ellipsoid_radii_squared(&self) -> Cartesian3 {
        self.one_over_ellipsoid_radii_squared
    }

    /// The bounding sphere enclosing the primitive in world coordinates.
    pub fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    /// Returns `true` if this primitive has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

impl ScenePrimitive for EllipsoidPrimitive {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("EllipsoidPrimitive is destroyed"));
        }
        if !self.show || frame.mode != SceneMode::Scene3D {
            return Ok(());
        }

        self.recompute_geometry();
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
