//! CesiumJS `CorridorGeometryLibrary`: offset geodesic edges, corners and
//! rounded end caps shared by the filled and outline corridor geometries.

use super::cartesian3::Cartesian3;
use super::corner_type::CornerType;
use super::ellipsoid::Ellipsoid;
use super::math::{equals_epsilon, js_max, to_radians, EPSILON7, RADIANS_PER_DEGREE};
use super::matrix3::Matrix3;
use super::polyline_pipeline::{generate_arc, ArcOptions};
use super::polyline_volume_geometry_library::angle_is_greater_than_pi;
use super::quaternion::Quaternion;

#[derive(Clone, Debug, PartialEq)]
pub enum CorridorCorner {
    Left(Vec<f64>),
    Right(Vec<f64>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorridorPositions {
    /// Alternating right (forward) and left (reverse) edges, in xyz triples.
    pub positions: Vec<Vec<f64>>,
    pub corners: Vec<CorridorCorner>,
    pub lefts: Option<Vec<f64>>,
    pub normals: Option<Vec<f64>>,
    pub end_positions: Option<[Vec<f64>; 2]>,
}

#[derive(Clone, Debug)]
pub struct CorridorPositionOptions<'a> {
    /// Distinct consecutive points already projected onto the ellipsoid.
    pub positions: &'a [Cartesian3],
    pub ellipsoid: Ellipsoid,
    pub width: f64,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub save_attributes: bool,
}

impl<'a> CorridorPositionOptions<'a> {
    pub fn new(positions: &'a [Cartesian3], width: f64) -> Self {
        Self {
            positions,
            width,
            ellipsoid: Ellipsoid::WGS84,
            corner_type: CornerType::Rounded,
            granularity: RADIANS_PER_DEGREE,
            save_attributes: false,
        }
    }
}

/// Writes xyz at `front`, and/or zyx ending at `back`. Invalid indices leave
/// the buffer untouched.
pub fn add_attribute(
    attribute: &mut [f64],
    value: Cartesian3,
    front: Option<usize>,
    back: Option<usize>,
) -> Option<()> {
    if let Some(i) = front {
        attribute.get(i..i.checked_add(3)?)?;
    }
    if let Some(i) = back {
        attribute.get(i.checked_sub(2)?..i.checked_add(1)?)?;
    }
    for start in [front, back.map(|i| i - 2)].into_iter().flatten() {
        attribute[start..start + 3].copy_from_slice(&[value.x, value.y, value.z]);
    }
    Some(())
}

fn round_corner(
    pivot: Cartesian3,
    mut start: Cartesian3,
    end: Cartesian3,
    corner_type: CornerType,
    left_outside: bool,
) -> Vec<f64> {
    let angle = start.subtract(pivot).angle_between(end.subtract(pivot));
    let count = if corner_type == CornerType::Beveled { 1 } else { (angle / to_radians(5.0)).ceil() as usize + 1 };
    let axis = if left_outside { pivot.negate() } else { pivot };
    let rotation = Matrix3::from_quaternion(&Quaternion::from_axis_angle(axis, angle / count as f64));
    let mut out = Vec::with_capacity(count * 3);
    // Cesium rotates even the last point rather than copying `end`.
    for _ in 0..count {
        start = rotation.multiply_by_vector(start);
        out.extend([start.x, start.y, start.z]);
    }
    out
}

fn end_caps(positions: &[Vec<f64>]) -> [Vec<f64>; 2] {
    let first_left = &positions[1];
    let start = Cartesian3::from_array(first_left, first_left.len() - 3);
    let end = Cartesian3::from_array(&positions[0], 0);
    let first = round_corner(start.midpoint(end), start, end, CornerType::Rounded, false);
    let right = &positions[positions.len() - 2];
    let start = Cartesian3::from_array(right, right.len() - 3);
    let end = Cartesian3::from_array(&positions[positions.len() - 1], 0);
    [first, round_corner(start.midpoint(end), start, end, CornerType::Rounded, false)]
}

fn shifted_positions(positions: &[f64], left: Cartesian3, width: f64, calculated: &mut Vec<Vec<f64>>) {
    let offset = left.multiply_by_scalar(width);
    let mut right_positions = Vec::with_capacity(positions.len());
    let mut left_positions = vec![0.0; positions.len()];
    for i in (0..positions.len()).step_by(3) {
        let p = Cartesian3::from_array(positions, i);
        let r = p.add(offset.negate());
        let l = p.add(offset);
        right_positions.extend([r.x, r.y, r.z]);
        let j = positions.len() - i - 3;
        left_positions[j..j + 3].copy_from_slice(&[l.x, l.y, l.z]);
    }
    calculated.extend([right_positions, left_positions]);
}

fn append_attribute(values: &mut Option<Vec<f64>>, p: Cartesian3) {
    if let Some(values) = values {
        values.extend([p.x, p.y, p.z]);
    }
}

/// Computes the unscaled edge and corner positions. Returns `None` for
/// invalid spacing, too few positions, or an unsupported geodesic.
pub fn compute_positions(options: &CorridorPositionOptions<'_>) -> Option<CorridorPositions> {
    let positions = options.positions;
    if positions.len() < 2
        || !options.granularity.is_finite()
        || options.granularity <= 0.0
        || !options.width.is_finite()
    {
        return None;
    }
    let ellipsoid = &options.ellipsoid;
    let width = options.width / 2.0;
    let mut calculated = Vec::new();
    let mut corners = Vec::new();
    let mut lefts = options.save_attributes.then(Vec::new);
    let mut normals = options.save_attributes.then(Vec::new);
    let arc = |start: Cartesian3, end: Cartesian3| {
        let mut o = ArcOptions::new(vec![start, end]);
        o.granularity = Some(options.granularity);
        o.ellipsoid = *ellipsoid;
        generate_arc(&o)
    };
    let mut position = positions[0];
    let mut forward = positions[1].subtract(position).normalize();
    let mut normal = ellipsoid.geodetic_surface_normal(position)?;
    let mut left = normal.cross(forward).normalize();
    append_attribute(&mut lefts, left);
    append_attribute(&mut normals, normal);
    let mut previous = position;
    position = positions[1];
    let mut backward = forward.negate();
    for next in &positions[2..] {
        normal = ellipsoid.geodetic_surface_normal(position)?;
        forward = next.subtract(position).normalize();
        let forward_projection = forward.subtract(normal.multiply_by_scalar(forward.dot(normal))).normalize();
        let backward_projection = backward.subtract(normal.multiply_by_scalar(backward.dot(normal))).normalize();
        let do_corner = !equals_epsilon(forward_projection.dot(backward_projection).abs(), 1.0, EPSILON7, EPSILON7);
        if do_corner {
            let mut corner_direction = forward.add(backward).normalize();
            corner_direction = corner_direction.cross(normal);
            corner_direction = normal.cross(corner_direction).normalize();
            let scalar = width / js_max(0.25, corner_direction.cross(backward).magnitude());
            let left_outside = angle_is_greater_than_pi(forward, backward, position, ellipsoid)?;
            corner_direction = corner_direction.multiply_by_scalar(scalar);
            let pivot = position.add(corner_direction);
            let shift = |left: Cartesian3, width: f64| {
                let p = left.multiply_by_scalar(width);
                if left_outside {
                    p
                } else {
                    p.negate()
                }
            };
            let center = pivot.add(shift(left, width));
            let start = pivot.add(shift(left, width * 2.0));
            shifted_positions(&arc(previous, center)?, left, width, &mut calculated);
            append_attribute(&mut lefts, left);
            append_attribute(&mut normals, normal);
            left = normal.cross(forward).normalize();
            let end = pivot.add(shift(left, width * 2.0));
            previous = pivot.add(shift(left, width));
            let points = if options.corner_type == CornerType::Mitered {
                let corner = position.subtract(corner_direction);
                vec![corner.x, corner.y, corner.z, end.x, end.y, end.z]
            } else {
                round_corner(pivot, start, end, options.corner_type, left_outside)
            };
            corners.push(if left_outside { CorridorCorner::Left(points) } else { CorridorCorner::Right(points) });
            backward = forward.negate();
        }
        position = *next;
    }
    normal = ellipsoid.geodetic_surface_normal(position)?;
    shifted_positions(&arc(previous, position)?, left, width, &mut calculated);
    append_attribute(&mut lefts, left);
    append_attribute(&mut normals, normal);
    let end_positions = (options.corner_type == CornerType::Rounded).then(|| end_caps(&calculated));
    Some(CorridorPositions { positions: calculated, corners, lefts, normals, end_positions })
}
