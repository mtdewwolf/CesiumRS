//! CesiumJS `CullingVolume`: a set of clipping planes, usually a frustum.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::intersect::Intersect;
use super::plane::Plane;

/// A volume that can be classified against a plane: Cesium's duck-typed
/// `boundingVolume.intersectPlane`.
pub trait BoundingVolume {
    fn intersect_plane(&self, plane: &Plane) -> Intersect;
}

impl BoundingVolume for BoundingSphere {
    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        BoundingSphere::intersect_plane(self, plane)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CullingVolume {
    /// Planes as `(normal, distance)`, inside on the normal's side.
    pub planes: Vec<Cartesian4>,
}

/// Plane bits for [`CullingVolume::compute_visibility_with_plane_mask`].
pub const MASK_OUTSIDE: u32 = 0xffff_ffff;
pub const MASK_INSIDE: u32 = 0x0000_0000;
pub const MASK_INDETERMINATE: u32 = 0x7fff_ffff;

impl CullingVolume {
    pub const MASK_OUTSIDE: u32 = MASK_OUTSIDE;
    pub const MASK_INSIDE: u32 = MASK_INSIDE;
    pub const MASK_INDETERMINATE: u32 = MASK_INDETERMINATE;

    pub fn new(planes: Vec<Cartesian4>) -> Self {
        Self { planes }
    }

    /// Six axis-aligned planes tangent to the sphere.
    pub fn from_bounding_sphere(sphere: &BoundingSphere) -> Self {
        let mut planes = Vec::with_capacity(6);
        for face in [Cartesian3::UNIT_X, Cartesian3::UNIT_Y, Cartesian3::UNIT_Z] {
            let near = sphere.center.add(face.multiply_by_scalar(-sphere.radius));
            planes.push(Cartesian4::new(face.x, face.y, face.z, -face.dot(near)));
            let far = sphere.center.add(face.multiply_by_scalar(sphere.radius));
            let neg = face.negate();
            planes.push(Cartesian4::new(-face.x, -face.y, -face.z, -neg.dot(far)));
        }
        Self { planes }
    }

    pub fn compute_visibility(&self, volume: &dyn BoundingVolume) -> Intersect {
        let mut intersecting = false;
        for p in &self.planes {
            match volume.intersect_plane(&Plane::from_cartesian4(*p)) {
                Intersect::Outside => return Intersect::Outside,
                Intersect::Intersecting => intersecting = true,
                Intersect::Inside => {}
            }
        }
        if intersecting {
            Intersect::Intersecting
        } else {
            Intersect::Inside
        }
    }

    /// Like [`Self::compute_visibility`], skipping planes the parent was
    /// already inside. Bit `k` of the result is set if the volume crosses plane `k`.
    pub fn compute_visibility_with_plane_mask(&self, volume: &dyn BoundingVolume, parent_plane_mask: u32) -> u32 {
        if parent_plane_mask == MASK_OUTSIDE || parent_plane_mask == MASK_INSIDE {
            return parent_plane_mask;
        }
        let mut mask = MASK_INSIDE;
        for (k, p) in self.planes.iter().enumerate() {
            let flag = if k < 31 { 1u32 << k } else { 0 };
            if k < 31 && (parent_plane_mask & flag) == 0 {
                continue;
            }
            match volume.intersect_plane(&Plane::from_cartesian4(*p)) {
                Intersect::Outside => return MASK_OUTSIDE,
                Intersect::Intersecting => mask |= flag,
                Intersect::Inside => {}
            }
        }
        mask
    }
}
