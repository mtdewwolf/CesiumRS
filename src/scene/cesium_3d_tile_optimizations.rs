//! CesiumJS `Cesium3DTileOptimizations`: utility functions for computing optimization
//! hints for a 3D tileset.

use super::cesium_3d_tile_optimization_hint::Cesium3DTileOptimizationHint;
use crate::core::{Cartesian3, Matrix3, OrientedBoundingBox};

/// Projects the half-axes matrix of an oriented bounding box onto a unit ray axis.
/// Matches CesiumJS `Cesium3DTileOptimizations.js` projection formula.
pub fn project_half_axes_onto_axis(half_axes: &Matrix3, axis: Cartesian3) -> f64 {
    let m = &half_axes.0;
    (m[0] * axis.x).abs()
        + (m[1] * axis.y).abs()
        + (m[2] * axis.z).abs()
        + (m[3] * axis.x).abs()
        + (m[4] * axis.y).abs()
        + (m[5] * axis.z).abs()
        + (m[6] * axis.x).abs()
        + (m[7] * axis.y).abs()
        + (m[8] * axis.z).abs()
}

/// Evaluates support for the childrenWithinParent optimization for parent and child
/// oriented bounding boxes.
pub fn check_oriented_bounding_boxes_within_parent(
    parent_obb: &OrientedBoundingBox,
    children_obbs: &[Option<&OrientedBoundingBox>],
) -> Cesium3DTileOptimizationHint {
    for child in children_obbs {
        let Some(child_obb) = child else {
            return Cesium3DTileOptimizationHint::SkipOptimization;
        };

        let axis = child_obb.center.subtract(parent_obb.center);
        let axis_length = axis.magnitude();
        if axis_length <= crate::core::math::EPSILON7 {
            continue;
        }

        let unit_axis = axis.divide_by_scalar(axis_length);
        let proj1 = project_half_axes_onto_axis(&parent_obb.half_axes, unit_axis);
        let proj2 = project_half_axes_onto_axis(&child_obb.half_axes, unit_axis);

        if proj1 <= proj2 + axis_length {
            return Cesium3DTileOptimizationHint::SkipOptimization;
        }
    }

    Cesium3DTileOptimizationHint::UseOptimization
}

/// Namespace struct matching CesiumJS `Cesium3DTileOptimizations`.
pub struct Cesium3DTileOptimizations;

impl Cesium3DTileOptimizations {
    pub fn check_children_within_parent(
        parent_obb: Option<&OrientedBoundingBox>,
        children_obbs: &[Option<&OrientedBoundingBox>],
    ) -> (bool, Cesium3DTileOptimizationHint) {
        let Some(parent_obb) = parent_obb else {
            return (false, Cesium3DTileOptimizationHint::NotComputed);
        };

        let hint = check_oriented_bounding_boxes_within_parent(parent_obb, children_obbs);
        (hint == Cesium3DTileOptimizationHint::UseOptimization, hint)
    }
}
