//! CesiumJS `PolylineVolumeGeometryLibrary`: a 2D shape swept along a
//! polyline, with mitered, rounded or beveled corners.

use super::bounding_rectangle::BoundingRectangle;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::corner_type::CornerType;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_tangent_plane::EllipsoidTangentPlane;
use super::math::{equals_epsilon, js_max, to_radians, EPSILON7};
use super::matrix3::Matrix3;
use super::matrix4::Matrix4;
use super::polyline_pipeline::{generate_arc, ArcOptions};
use super::quaternion::Quaternion;
use super::transforms::Transforms;

/// Scales `positions` to the surface in place and returns their heights.
fn scale_to_surface(positions: &mut [Cartesian3], ellipsoid: &Ellipsoid) -> Option<Vec<f64>> {
    let mut heights = Vec::with_capacity(positions.len());
    for p in positions.iter_mut() {
        heights.push(ellipsoid.cartesian_to_cartographic(*p)?.height);
        *p = ellipsoid.scale_to_geodetic_surface(*p)?;
    }
    Some(heights)
}

/// Heights from `h0` to `h1` at Cesium's angular spacing, plus `h1`.
fn subdivide_heights(p0: Cartesian3, p1: Cartesian3, h0: f64, h1: f64, granularity: f64) -> Vec<f64> {
    let n = (p0.angle_between(p1) / granularity).ceil();
    let num_points = if n > 0.0 { n as usize } else { 0 };
    let mut heights = vec![f64::NAN; num_points];
    if h0 == h1 {
        heights.fill(h0);
        heights.push(h1);
        return heights;
    }
    let per_vertex = (h1 - h0) / n;
    for (i, h) in heights.iter_mut().enumerate().skip(1) {
        *h = h0 + i as f64 * per_vertex;
    }
    if heights.is_empty() {
        heights.push(h0);
    } else {
        heights[0] = h0;
    }
    heights.push(h1);
    heights
}

fn tangent_plane_projections(start: Cartesian3, end: Cartesian3, position: Cartesian3, ellipsoid: &Ellipsoid) -> Option<(Cartesian2, Cartesian2)> {
    let plane = EllipsoidTangentPlane::new(position, ellipsoid)?;
    let next = plane.project_point_onto_plane(position.add(start))?;
    let prev = plane.project_point_onto_plane(position.add(end))?;
    Some((next, prev))
}

fn compute_rotation_angle(start: Cartesian3, end: Cartesian3, position: Cartesian3, ellipsoid: &Ellipsoid) -> Option<f64> {
    let (next, prev) = tangent_plane_projections(start, end, position, ellipsoid)?;
    let angle = next.angle_between(prev);
    Some(if prev.x * next.y - prev.y * next.x >= 0.0 { -angle } else { angle })
}

/// Places the shape at `center`, turned to face along `left` and raised to
/// `height`, `repeat` times.
#[allow(clippy::too_many_arguments)]
fn add_position(
    center: Cartesian3,
    left: Cartesian3,
    shape: &[f64],
    final_positions: &mut Vec<f64>,
    ellipsoid: &Ellipsoid,
    height: f64,
    x_scalar: f64,
    repeat: usize,
) -> Option<()> {
    let enu = Transforms::east_north_up_to_fixed_frame(center, ellipsoid);
    let west = enu.multiply_by_point_as_vector(Cartesian3::new(-1.0, 0.0, 0.0)).normalize();
    let angle = compute_rotation_angle(west, left, center, ellipsoid)?;
    let rotation_z = Matrix3::from_rotation_z(angle);
    let transform = enu.multiply_transformation(&Matrix4::from_rotation_translation(&rotation_z, Cartesian3::new(0.0, 0.0, height)));
    let mut scale = Matrix3::IDENTITY;
    scale.0[0] = x_scalar;
    for _ in 0..repeat {
        for i in (0..shape.len()).step_by(3) {
            let p = transform.multiply_by_point(scale.multiply_by_vector(Cartesian3::from_array(shape, i)));
            final_positions.extend([p.x, p.y, p.z]);
        }
    }
    Some(())
}

fn add_positions(
    centers: &[f64],
    left: Cartesian3,
    shape: &[f64],
    final_positions: &mut Vec<f64>,
    ellipsoid: &Ellipsoid,
    heights: &[f64],
) -> Option<()> {
    for i in (0..centers.len()).step_by(3) {
        let height = heights.get(i / 3).copied().unwrap_or(f64::NAN);
        add_position(Cartesian3::from_array(centers, i), left, shape, final_positions, ellipsoid, height, 1.0, 1)?;
    }
    Some(())
}

fn offsets(br: &BoundingRectangle) -> (f64, f64) {
    (br.x + br.width / 2.0, br.y + br.height / 2.0)
}

/// The shape in the xz-plane, centered, with each interior point twice
/// (so sides get their own normals).
fn convert_shape_to_3d_duplicate(shape_2d: &[Cartesian2], br: &BoundingRectangle) -> Vec<f64> {
    let (x_offset, y_offset) = offsets(br);
    let mut shape = Vec::with_capacity(shape_2d.len() * 6);
    let p = shape_2d[0];
    shape.extend([p.x - x_offset, 0.0, p.y - y_offset]);
    for p in &shape_2d[1..] {
        let (x, z) = (p.x - x_offset, p.y - y_offset);
        shape.extend([x, 0.0, z, x, 0.0, z]);
    }
    shape.extend([p.x - x_offset, 0.0, p.y - y_offset]);
    shape
}

fn convert_shape_to_3d(shape_2d: &[Cartesian2], br: &BoundingRectangle) -> Vec<f64> {
    let (x_offset, y_offset) = offsets(br);
    shape_2d.iter().flat_map(|p| [p.x - x_offset, 0.0, p.y - y_offset]).collect()
}

#[allow(clippy::too_many_arguments)]
fn compute_round_corner(
    pivot: Cartesian3,
    start_point: Cartesian3,
    end_point: Cartesian3,
    corner_type: CornerType,
    left_is_outside: bool,
    ellipsoid: &Ellipsoid,
    final_positions: &mut Vec<f64>,
    shape: &[f64],
    height: f64,
    duplicate_points: bool,
) -> Option<()> {
    let angle = start_point.subtract(pivot).angle_between(end_point.subtract(pivot));
    let granularity = if corner_type == CornerType::Beveled { 0.0 } else { (angle / to_radians(5.0)).ceil() };
    let axis = if left_is_outside { pivot.negate() } else { pivot };
    let m = Matrix3::from_quaternion(&Quaternion::from_axis_angle(axis, angle / (granularity + 1.0)));
    let left_of = |p: Cartesian3| {
        let left = p.subtract(pivot).normalize();
        if left_is_outside { left } else { left.negate() }
    };
    if granularity > 0.0 {
        let repeat = if duplicate_points { 2 } else { 1 };
        let mut start = start_point;
        for _ in 0..granularity as usize {
            start = m.multiply_by_vector(start);
            let surface = ellipsoid.scale_to_geodetic_surface(start)?;
            add_position(surface, left_of(start), shape, final_positions, ellipsoid, height, 1.0, repeat)?;
        }
    } else {
        for p in [start_point, end_point] {
            let surface = ellipsoid.scale_to_geodetic_surface(p)?;
            add_position(surface, left_of(p), shape, final_positions, ellipsoid, height, 1.0, 1)?;
        }
    }
    Some(())
}

/// The shape with consecutive equal points (cyclically) removed.
pub fn remove_duplicates_from_shape(shape_positions: &[Cartesian2]) -> Vec<Cartesian2> {
    let length = shape_positions.len();
    (0..length)
        .filter(|&i1| shape_positions[(i1 + length - 1) % length] != shape_positions[i1])
        .map(|i1| shape_positions[i1])
        .collect()
}

/// Whether the turn from `backward` to `forward` at `position` bends
/// right (so the left side is outside the corner).
pub fn angle_is_greater_than_pi(forward: Cartesian3, backward: Cartesian3, position: Cartesian3, ellipsoid: &Ellipsoid) -> Option<bool> {
    let (next, prev) = tangent_plane_projections(forward, backward, position, ellipsoid)?;
    Some(prev.x * next.y - prev.y * next.x >= 0.0)
}

/// Positions of the swept shape: each side ring (points duplicated when
/// `duplicate_points`), then the two end caps. Scales `positions` to the
/// surface in place, as Cesium does.
#[allow(clippy::too_many_arguments)]
pub fn compute_positions(
    positions: &mut [Cartesian3],
    shape_2d: &[Cartesian2],
    bounding_rectangle: &BoundingRectangle,
    ellipsoid: &Ellipsoid,
    granularity: f64,
    corner_type: CornerType,
    duplicate_points: bool,
) -> Option<Vec<f64>> {
    let heights = scale_to_surface(positions, ellipsoid)?;
    let shape_for_sides =
        if duplicate_points { convert_shape_to_3d_duplicate(shape_2d, bounding_rectangle) } else { convert_shape_to_3d(shape_2d, bounding_rectangle) };
    let shape_for_ends = convert_shape_to_3d(shape_2d, bounding_rectangle);
    let height_offset = bounding_rectangle.height / 2.0;
    let width = bounding_rectangle.width / 2.0;
    let length = positions.len();
    let mut final_positions = Vec::new();
    let mut ends = Vec::new();
    let arc = |p0: Cartesian3, p1: Cartesian3| {
        let mut o = ArcOptions::new(vec![p0, p1]);
        o.granularity = Some(granularity);
        o.ellipsoid = *ellipsoid;
        generate_arc(&o)
    };

    let mut position = positions[0];
    let mut next_position = positions[1];
    let surface_normal0 = ellipsoid.geodetic_surface_normal(position)?;
    let mut forward = next_position.subtract(position).normalize();
    let mut left = surface_normal0.cross(forward).normalize();
    let mut h0 = heights[0];
    let mut h1 = heights[1];
    if duplicate_points {
        add_position(position, left, &shape_for_ends, &mut ends, ellipsoid, h0 + height_offset, 1.0, 1)?;
    }
    let mut previous_position = position;
    position = next_position;
    let mut backward = forward.negate();
    for i in 1..length - 1 {
        let repeat = if duplicate_points { 2 } else { 1 };
        next_position = positions[i + 1];
        if position == next_position {
            // Cesium warns once and skips the point.
            continue;
        }
        forward = next_position.subtract(position).normalize();
        let surface_normal = ellipsoid.geodetic_surface_normal(position)?;
        let forward_projection = forward.subtract(surface_normal.multiply_by_scalar(forward.dot(surface_normal))).normalize();
        let backward_projection = backward.subtract(surface_normal.multiply_by_scalar(backward.dot(surface_normal))).normalize();
        let do_corner = !equals_epsilon(forward_projection.dot(backward_projection).abs(), 1.0, EPSILON7, EPSILON7);
        if do_corner {
            let mut corner_direction = forward.add(backward).normalize();
            corner_direction = corner_direction.cross(surface_normal);
            corner_direction = surface_normal.cross(corner_direction).normalize();
            let scalar = 1.0 / js_max(0.25, corner_direction.cross(backward).magnitude());
            let left_is_outside = angle_is_greater_than_pi(forward, backward, position, ellipsoid)?;
            corner_direction = corner_direction.multiply_by_scalar(scalar * width);
            let pivot = position.add(corner_direction);
            let side = if left_is_outside { width } else { -width };
            let start = pivot.add(left.multiply_by_scalar(side));
            let subdivided_heights = subdivide_heights(previous_position, start, h0 + height_offset, h1 + height_offset, granularity);
            let subdivided_positions = arc(previous_position, start)?;
            add_positions(&subdivided_positions, left, &shape_for_sides, &mut final_positions, ellipsoid, &subdivided_heights)?;
            left = surface_normal.cross(forward).normalize();
            let end = pivot.add(left.multiply_by_scalar(side));
            if corner_type == CornerType::Rounded || corner_type == CornerType::Beveled {
                compute_round_corner(
                    pivot,
                    start,
                    end,
                    corner_type,
                    left_is_outside,
                    ellipsoid,
                    &mut final_positions,
                    &shape_for_sides,
                    h1 + height_offset,
                    duplicate_points,
                )?;
            } else {
                let direction = if left_is_outside { corner_direction.negate() } else { corner_direction };
                add_position(position, direction, &shape_for_sides, &mut final_positions, ellipsoid, h1 + height_offset, scalar, repeat)?;
            }
            previous_position = end;
            backward = forward.negate();
        } else {
            add_position(previous_position, left, &shape_for_sides, &mut final_positions, ellipsoid, h0 + height_offset, 1.0, 1)?;
            previous_position = position;
        }
        h0 = h1;
        h1 = heights.get(i + 1).copied().unwrap_or(f64::NAN);
        position = next_position;
    }
    let subdivided_heights = subdivide_heights(previous_position, position, h0 + height_offset, h1 + height_offset, granularity);
    let subdivided_positions = arc(previous_position, position)?;
    add_positions(&subdivided_positions, left, &shape_for_sides, &mut final_positions, ellipsoid, &subdivided_heights)?;
    if duplicate_points {
        add_position(position, left, &shape_for_ends, &mut ends, ellipsoid, h1 + height_offset, 1.0, 1)?;
        final_positions.extend(ends);
    }
    Some(final_positions)
}
