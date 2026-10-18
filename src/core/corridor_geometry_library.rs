//! CesiumJS `CorridorGeometryLibrary`: the left and right edges of a
//! corridor along a polyline, with its corners and rounded end caps.

use super::cartesian3::Cartesian3;
use super::corner_type::CornerType;
use super::ellipsoid::Ellipsoid;
use super::math::{equals_epsilon, js_max, to_radians, EPSILON7};
use super::matrix3::Matrix3;
use super::polyline_pipeline::{generate_arc, ArcOptions};
use super::polyline_volume_geometry_library::angle_is_greater_than_pi;
use super::quaternion::Quaternion;

/// The outside points of one corner; Cesium sets exactly one side.
#[derive(Clone, Debug, PartialEq)]
pub struct CorridorCorner {
    pub left_positions: Option<Vec<f64>>,
    pub right_positions: Option<Vec<f64>>,
}

/// What `compute_positions` returns.
#[derive(Clone, Debug, PartialEq)]
pub struct CorridorPositions {
    /// Right then left edge (left in reverse order) of each straight run.
    pub positions: Vec<Vec<f64>>,
    pub corners: Vec<CorridorCorner>,
    /// One left and surface normal per run end, when attributes are saved.
    pub lefts: Option<Vec<f64>>,
    pub normals: Option<Vec<f64>>,
    /// The first and last caps, for rounded corners.
    pub end_positions: Option<[Vec<f64>; 2]>,
}

/// Writes `value` forward at `front` and backward (z, y, x) ending at
/// `back`, ignoring writes outside the array as a typed array does.
pub fn add_attribute<T: Copy>(attribute: &mut [T], value: [f64; 3], front: Option<i64>, back: Option<i64>, convert: impl Fn(f64) -> T) {
    let mut set = |i: i64, v: f64| {
        if let Some(slot) = usize::try_from(i).ok().and_then(|i| attribute.get_mut(i)) {
            *slot = convert(v);
        }
    };
    if let Some(f) = front {
        set(f, value[0]);
        set(f + 1, value[1]);
        set(f + 2, value[2]);
    }
    if let Some(b) = back {
        set(b, value[2]);
        set(b - 1, value[1]);
        set(b - 2, value[0]);
    }
}

/// `count` points rotating `start_point` about `corner_point`, the last
/// landing on the end point; `None` where Cesium throws (a NaN angle).
fn compute_round_corner(corner_point: Cartesian3, start_point: Cartesian3, end_point: Cartesian3, corner_type: CornerType, left_is_outside: bool) -> Option<Vec<f64>> {
    let angle = start_point.subtract(corner_point).angle_between(end_point.subtract(corner_point));
    let granularity = if corner_type == CornerType::Beveled { 1.0 } else { (angle / to_radians(5.0)).ceil() + 1.0 };
    if !granularity.is_finite() {
        return None;
    }
    let axis = if left_is_outside { corner_point.negate() } else { corner_point };
    let m = Matrix3::from_quaternion(&Quaternion::from_axis_angle(axis, angle / granularity));
    let mut array = Vec::with_capacity(granularity as usize * 3);
    let mut start = start_point;
    for _ in 0..granularity as usize {
        start = m.multiply_by_vector(start);
        array.extend([start.x, start.y, start.z]);
    }
    Some(array)
}

fn last_point(array: &[f64]) -> Cartesian3 {
    Cartesian3::from_array(array, array.len() - 3)
}

fn add_end_caps(calculated_positions: &[Vec<f64>]) -> Option<[Vec<f64>; 2]> {
    let start = last_point(&calculated_positions[1]);
    let end = Cartesian3::from_array(&calculated_positions[0], 0);
    let first = compute_round_corner(start.midpoint(end), start, end, CornerType::Rounded, false)?;
    let length = calculated_positions.len() - 1;
    let start = last_point(&calculated_positions[length - 1]);
    let end = Cartesian3::from_array(&calculated_positions[length], 0);
    let last = compute_round_corner(start.midpoint(end), start, end, CornerType::Rounded, false)?;
    Some([first, last])
}

fn compute_mitered_corner(position: Cartesian3, left_corner_direction: Cartesian3, last_point: Cartesian3, left_is_outside: bool) -> Vec<f64> {
    let corner = position.add(if left_is_outside { left_corner_direction } else { left_corner_direction.negate() });
    vec![corner.x, corner.y, corner.z, last_point.x, last_point.y, last_point.z]
}

/// Pushes the arc shifted right and left by `scalar`.
fn add_shifted_positions(positions: &[f64], left: Cartesian3, scalar: f64, calculated_positions: &mut Vec<Vec<f64>>) {
    let n = positions.len();
    let mut right_positions = vec![f64::NAN; n];
    let mut left_positions = vec![f64::NAN; n];
    let scaled_left = left.multiply_by_scalar(scalar);
    let scaled_right = scaled_left.negate();
    let (mut ri, mut li) = (0usize, n as i64 - 1);
    for i in (0..n).step_by(3) {
        let pos = Cartesian3::new(positions[i], positions.get(i + 1).copied().unwrap_or(f64::NAN), positions.get(i + 2).copied().unwrap_or(f64::NAN));
        let r = pos.add(scaled_right);
        for v in [r.x, r.y, r.z] {
            if let Some(slot) = right_positions.get_mut(ri) {
                *slot = v;
            }
            ri += 1;
        }
        let l = pos.add(scaled_left);
        for v in [l.z, l.y, l.x] {
            if let Some(slot) = usize::try_from(li).ok().and_then(|i| left_positions.get_mut(i)) {
                *slot = v;
            }
            li -= 1;
        }
    }
    calculated_positions.push(right_positions);
    calculated_positions.push(left_positions);
}

/// The corridor's edges for `positions` (already on the surface, without
/// duplicates), `width` wide. `None` where Cesium throws (a position at the
/// ellipsoid's center).
pub fn compute_positions(
    positions: &[Cartesian3],
    ellipsoid: &Ellipsoid,
    width: f64,
    corner_type: CornerType,
    granularity: f64,
    save_attributes: bool,
) -> Option<CorridorPositions> {
    let width = width / 2.0;
    let mut calculated_positions = Vec::new();
    let mut lefts = Vec::new();
    let mut normals = Vec::new();
    let arc = |p0: Cartesian3, p1: Cartesian3| {
        let mut o = ArcOptions::new(vec![p0, p1]);
        o.granularity = Some(granularity);
        o.ellipsoid = *ellipsoid;
        generate_arc(&o)
    };
    let mut save = |left: Cartesian3, normal: Cartesian3| {
        if save_attributes {
            lefts.extend([left.x, left.y, left.z]);
            normals.extend([normal.x, normal.y, normal.z]);
        }
    };

    let mut position = positions[0];
    let mut next_position = positions[1];
    let mut forward = next_position.subtract(position).normalize();
    let mut normal = ellipsoid.geodetic_surface_normal(position)?;
    let mut left = normal.cross(forward).normalize();
    save(left, normal);
    let mut previous_pos = position;
    position = next_position;
    let mut backward = forward.negate();
    let mut corners = Vec::new();
    for i in 1..positions.len() - 1 {
        normal = ellipsoid.geodetic_surface_normal(position)?;
        next_position = positions[i + 1];
        forward = next_position.subtract(position).normalize();
        let forward_projection = forward.subtract(normal.multiply_by_scalar(forward.dot(normal))).normalize();
        let backward_projection = backward.subtract(normal.multiply_by_scalar(backward.dot(normal))).normalize();
        let do_corner = !equals_epsilon(forward_projection.dot(backward_projection).abs(), 1.0, EPSILON7, EPSILON7);
        if do_corner {
            let mut corner_direction = forward.add(backward).normalize();
            corner_direction = corner_direction.cross(normal);
            corner_direction = normal.cross(corner_direction).normalize();
            let scalar = width / js_max(0.25, corner_direction.cross(backward).magnitude());
            let left_is_outside = angle_is_greater_than_pi(forward, backward, position, ellipsoid)?;
            corner_direction = corner_direction.multiply_by_scalar(scalar);
            if left_is_outside {
                let right_pos = position.add(corner_direction);
                let center = right_pos.add(left.multiply_by_scalar(width));
                let left_pos = right_pos.add(left.multiply_by_scalar(width * 2.0));
                add_shifted_positions(&arc(previous_pos, center)?, left, width, &mut calculated_positions);
                save(left, normal);
                let start_point = left_pos;
                left = normal.cross(forward).normalize();
                let left_pos = right_pos.add(left.multiply_by_scalar(width * 2.0));
                previous_pos = right_pos.add(left.multiply_by_scalar(width));
                let left_positions = if corner_type == CornerType::Rounded || corner_type == CornerType::Beveled {
                    compute_round_corner(right_pos, start_point, left_pos, corner_type, true)?
                } else {
                    compute_mitered_corner(position, corner_direction.negate(), left_pos, true)
                };
                corners.push(CorridorCorner { left_positions: Some(left_positions), right_positions: None });
            } else {
                let left_pos = position.add(corner_direction);
                let center = left_pos.add(left.multiply_by_scalar(width).negate());
                let right_pos = left_pos.add(left.multiply_by_scalar(width * 2.0).negate());
                add_shifted_positions(&arc(previous_pos, center)?, left, width, &mut calculated_positions);
                save(left, normal);
                let start_point = right_pos;
                left = normal.cross(forward).normalize();
                let right_pos = left_pos.add(left.multiply_by_scalar(width * 2.0).negate());
                previous_pos = left_pos.add(left.multiply_by_scalar(width).negate());
                let right_positions = if corner_type == CornerType::Rounded || corner_type == CornerType::Beveled {
                    compute_round_corner(left_pos, start_point, right_pos, corner_type, false)?
                } else {
                    compute_mitered_corner(position, corner_direction, right_pos, false)
                };
                corners.push(CorridorCorner { left_positions: None, right_positions: Some(right_positions) });
            }
            backward = forward.negate();
        }
        position = next_position;
    }
    normal = ellipsoid.geodetic_surface_normal(position)?;
    add_shifted_positions(&arc(previous_pos, position)?, left, width, &mut calculated_positions);
    save(left, normal);
    let end_positions = if corner_type == CornerType::Rounded { Some(add_end_caps(&calculated_positions)?) } else { None };
    Some(CorridorPositions {
        positions: calculated_positions,
        corners,
        lefts: save_attributes.then_some(lefts),
        normals: save_attributes.then_some(normals),
        end_positions,
    })
}
