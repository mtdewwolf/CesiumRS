//! CesiumJS `TileOrientedBoundingBox`: a tile bounding volume specified as
//! an oriented bounding box.

use crate::core::math::{EPSILON6, EPSILON7};
use crate::core::{BoundingSphere, BoxOutlineGeometry, Cartesian3, Color, Intersect, Matrix3, Matrix4};
use crate::core::{OrientedBoundingBox, Plane};

use super::tile_bounding_volume::{DebugVolume, TileBoundingVolume, TileBoundingVolumeFrame};

#[derive(Clone, Debug, PartialEq)]
pub struct TileOrientedBoundingBox {
    oriented_bounding_box: OrientedBoundingBox,
    bounding_sphere: BoundingSphere,
}

/// A vector `EPSILON7` long, perpendicular to `a` and `b`.
fn compute_missing_vector(a: Cartesian3, b: Cartesian3) -> Cartesian3 {
    let result = a.cross(b);
    let magnitude = result.magnitude();
    result.multiply_by_scalar(EPSILON7 / magnitude)
}

fn find_orthogonal_vector(a: Cartesian3) -> Cartesian3 {
    let temp = a.normalize();
    let b = if temp.equals_epsilon(Cartesian3::UNIT_X, EPSILON6) { Cartesian3::UNIT_Y } else { Cartesian3::UNIT_X };
    compute_missing_vector(a, b)
}

/// Replaces zero half-axes with tiny perpendicular ones, so a flat or
/// degenerate box still has volume.
fn check_half_axes(half_axes: Matrix3) -> Matrix3 {
    let (mut u, mut v, mut w) = (half_axes.get_column(0), half_axes.get_column(1), half_axes.get_column(2));
    let u_zero = u == Cartesian3::ZERO;
    let v_zero = v == Cartesian3::ZERO;
    let w_zero = w == Cartesian3::ZERO;
    if !u_zero && !v_zero && !w_zero {
        return half_axes;
    }
    if u_zero && v_zero && w_zero {
        let mut m = half_axes;
        m.0[0] = EPSILON7;
        m.0[4] = EPSILON7;
        m.0[8] = EPSILON7;
        return m;
    }
    if u_zero && !v_zero && !w_zero {
        u = compute_missing_vector(v, w);
    } else if !u_zero && v_zero && !w_zero {
        v = compute_missing_vector(u, w);
    } else if !u_zero && !v_zero && w_zero {
        w = compute_missing_vector(v, u);
    } else if !u_zero {
        v = find_orthogonal_vector(u);
        w = compute_missing_vector(v, u);
    } else if !v_zero {
        u = find_orthogonal_vector(v);
        w = compute_missing_vector(v, u);
    } else if !w_zero {
        u = find_orthogonal_vector(w);
        v = compute_missing_vector(w, u);
    }
    half_axes.set_column(0, u).set_column(1, v).set_column(2, w)
}

impl TileOrientedBoundingBox {
    pub fn new(center: Cartesian3, half_axes: Matrix3) -> Self {
        let oriented_bounding_box = OrientedBoundingBox::new(center, check_half_axes(half_axes));
        let bounding_sphere = BoundingSphere::from_oriented_bounding_box(&oriented_bounding_box);
        Self { oriented_bounding_box, bounding_sphere }
    }

    /// The underlying volume, the oriented bounding box.
    pub fn bounding_volume(&self) -> &OrientedBoundingBox {
        &self.oriented_bounding_box
    }

    /// Moves the box after the tile is transformed.
    pub fn update(&mut self, center: Cartesian3, half_axes: Matrix3) {
        self.oriented_bounding_box = OrientedBoundingBox::new(center, check_half_axes(half_axes));
        self.bounding_sphere = BoundingSphere::from_oriented_bounding_box(&self.oriented_bounding_box);
    }
}

impl TileBoundingVolume for TileOrientedBoundingBox {
    fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    fn distance_to_camera(&self, frame: &TileBoundingVolumeFrame<'_>) -> f64 {
        self.oriented_bounding_box.distance_squared_to(frame.camera_position_wc).sqrt()
    }

    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        self.oriented_bounding_box.intersect_plane(plane)
    }

    /// A 2×2×2 cube outline scaled and rotated by the half-axes.
    fn create_debug_volume(&self, color: &Color) -> DebugVolume {
        let geometry = BoxOutlineGeometry::new(Cartesian3::new(-1.0, -1.0, -1.0), Cartesian3::ONE, None)
            .and_then(|g| g.create_geometry());
        let obb = &self.oriented_bounding_box;
        DebugVolume::outlines(vec![geometry], Matrix4::from_rotation_translation(&obb.half_axes, obb.center), color)
    }
}
