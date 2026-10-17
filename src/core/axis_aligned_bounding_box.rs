//! CesiumJS `AxisAlignedBoundingBox`.

use super::cartesian3::Cartesian3;
use super::culling_volume::BoundingVolume;
use super::intersect::Intersect;
use super::math::{js_max, js_min};
use super::plane::Plane;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxisAlignedBoundingBox {
    pub minimum: Cartesian3,
    pub maximum: Cartesian3,
    pub center: Cartesian3,
}

impl AxisAlignedBoundingBox {
    /// `center` defaults to the midpoint of the corners.
    pub fn new(minimum: Cartesian3, maximum: Cartesian3, center: Option<Cartesian3>) -> Self {
        Self { minimum, maximum, center: center.unwrap_or_else(|| minimum.midpoint(maximum)) }
    }

    pub fn from_corners(minimum: Cartesian3, maximum: Cartesian3) -> Self {
        Self::new(minimum, maximum, None)
    }

    /// Box around the points; all zero for none.
    pub fn from_points(positions: &[Cartesian3]) -> Self {
        let Some(first) = positions.first() else {
            return Self::default();
        };
        let (mut min_x, mut min_y, mut min_z) = (first.x, first.y, first.z);
        let (mut max_x, mut max_y, mut max_z) = (first.x, first.y, first.z);
        for p in &positions[1..] {
            min_x = js_min(p.x, min_x);
            max_x = js_max(p.x, max_x);
            min_y = js_min(p.y, min_y);
            max_y = js_max(p.y, max_y);
            min_z = js_min(p.z, min_z);
            max_z = js_max(p.z, max_z);
        }
        Self::from_corners(Cartesian3::new(min_x, min_y, min_z), Cartesian3::new(max_x, max_y, max_z))
    }

    pub fn equals(&self, right: &Self) -> bool {
        self == right
    }

    pub fn intersect_plane(&self, plane: &Plane) -> Intersect {
        let h = self.maximum.subtract(self.minimum).multiply_by_scalar(0.5);
        let normal = plane.normal;
        let e = h.x * normal.x.abs() + h.y * normal.y.abs() + h.z * normal.z.abs();
        let s = self.center.dot(normal) + plane.distance;
        if s - e > 0.0 {
            Intersect::Inside
        } else if s + e < 0.0 {
            Intersect::Outside
        } else {
            Intersect::Intersecting
        }
    }

    pub fn intersect_axis_aligned_bounding_box(&self, other: &Self) -> bool {
        self.minimum.x <= other.maximum.x
            && self.maximum.x >= other.minimum.x
            && self.minimum.y <= other.maximum.y
            && self.maximum.y >= other.minimum.y
            && self.minimum.z <= other.maximum.z
            && self.maximum.z >= other.minimum.z
    }
}

impl BoundingVolume for AxisAlignedBoundingBox {
    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        AxisAlignedBoundingBox::intersect_plane(self, plane)
    }
}
