//! CesiumJS `TileBoundingSphere`: a tile bounding volume specified as a
//! sphere.

use crate::core::math::EPSILON7;
use crate::core::{BoundingSphere, Cartesian3, Color, Intersect, Matrix4, Plane, SphereOutlineGeometry};

use super::tile_bounding_volume::{DebugVolume, TileBoundingVolume, TileBoundingVolumeFrame};

#[derive(Clone, Debug, PartialEq)]
pub struct TileBoundingSphere {
    bounding_sphere: BoundingSphere,
}

impl TileBoundingSphere {
    /// A radius of exactly 0 becomes `EPSILON7`.
    pub fn new(center: Cartesian3, radius: f64) -> Self {
        let radius = if radius == 0.0 { EPSILON7 } else { radius };
        Self { bounding_sphere: BoundingSphere::new(center, radius) }
    }

    pub fn center(&self) -> Cartesian3 {
        self.bounding_sphere.center
    }

    pub fn radius(&self) -> f64 {
        self.bounding_sphere.radius
    }

    /// The underlying volume, the sphere itself.
    pub fn bounding_volume(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    /// Moves the sphere after the tile is transformed. Unlike the
    /// constructor, a zero radius is kept.
    pub fn update(&mut self, center: Cartesian3, radius: f64) {
        self.bounding_sphere.center = center;
        self.bounding_sphere.radius = radius;
    }
}

impl TileBoundingVolume for TileBoundingSphere {
    fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    fn distance_to_camera(&self, frame: &TileBoundingVolumeFrame<'_>) -> f64 {
        let sphere = &self.bounding_sphere;
        (sphere.center.distance(frame.camera_position_wc) - sphere.radius).max(0.0)
    }

    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        self.bounding_sphere.intersect_plane(plane)
    }

    fn create_debug_volume(&self, color: &Color) -> DebugVolume {
        let geometry = SphereOutlineGeometry::new(self.radius(), 10.0, 8.0, 128.0).and_then(|g| g.create_geometry());
        DebugVolume::outlines(vec![geometry], Matrix4::from_translation(self.center()), color)
    }
}
