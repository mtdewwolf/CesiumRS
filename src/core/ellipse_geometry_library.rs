//! CesiumJS `EllipseGeometryLibrary`: points on an ellipse draped over the
//! ellipsoid, and raising them to a height.

use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::math::PI_OVER_TWO;
use super::matrix3::Matrix3;
use super::quaternion::Quaternion;
use std::f64::consts::PI;

/// The ellipse to sample: its center (on the surface), axes in meters,
/// rotation from north in radians, and angular granularity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseOptions {
    pub center: Cartesian3,
    pub semi_major_axis: f64,
    pub semi_minor_axis: f64,
    pub rotation: f64,
    pub granularity: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EllipsePositions {
    /// Rows of fill points from top to bottom (with `add_fill_positions`).
    pub positions: Option<Vec<f64>>,
    /// Points per quarter of the boundary (with `add_fill_positions`).
    pub num_pts: Option<usize>,
    /// The boundary, counter-clockwise (with `add_edge_positions`).
    pub outer_positions: Option<Vec<f64>>,
}

#[allow(clippy::too_many_arguments)]
fn point_on_ellipsoid(
    theta: f64,
    rotation: f64,
    north_vec: Cartesian3,
    east_vec: Cartesian3,
    a_sqr: f64,
    ab: f64,
    b_sqr: f64,
    mag: f64,
    unit_pos: Cartesian3,
) -> Cartesian3 {
    let azimuth = theta + rotation;
    let rot_axis = east_vec.multiply_by_scalar(azimuth.cos()).add(north_vec.multiply_by_scalar(azimuth.sin()));
    let cos_theta_squared = theta.cos() * theta.cos();
    let sin_theta_squared = theta.sin() * theta.sin();
    let radius = ab / (b_sqr * cos_theta_squared + a_sqr * sin_theta_squared).sqrt();
    let angle = radius / mag;
    let rot_mtx = Matrix3::from_quaternion(&Quaternion::from_axis_angle(rot_axis, angle));
    rot_mtx.multiply_by_vector(unit_pos).normalize().multiply_by_scalar(mag)
}

/// Moves surface positions up along the normal to `height`; with `extrude`,
/// appends a second copy at `extruded_height`.
pub fn raise_positions_to_height(
    positions: &[f64],
    ellipsoid: &Ellipsoid,
    height: f64,
    extruded_height: f64,
    extrude: bool,
) -> Vec<f64> {
    let length = positions.len();
    let size = if extrude { (length / 3) * 2 } else { length / 3 };
    let mut final_positions = vec![0.0; size * 3];
    let bottom_offset = if extrude { length } else { 0 };
    for i in (0..length).step_by(3) {
        let p = Cartesian3::from_array(positions, i);
        let position = ellipsoid.scale_to_geodetic_surface(p).unwrap_or(p);
        let normal = ellipsoid.geodetic_surface_normal(position).unwrap_or_default();
        if extrude {
            let extruded = position.add(normal.multiply_by_scalar(extruded_height));
            final_positions[i + bottom_offset..i + bottom_offset + 3]
                .copy_from_slice(&[extruded.x, extruded.y, extruded.z]);
        }
        let raised = position.add(normal.multiply_by_scalar(height));
        final_positions[i..i + 3].copy_from_slice(&[raised.x, raised.y, raised.z]);
    }
    final_positions
}

/// Samples the ellipse: interior rows for filling and/or the boundary.
pub fn compute_ellipse_positions(
    options: &EllipseOptions,
    add_fill_positions: bool,
    add_edge_positions: bool,
) -> EllipsePositions {
    let semi_minor_axis = options.semi_minor_axis;
    let semi_major_axis = options.semi_major_axis;
    let rotation = options.rotation;
    let center = options.center;
    let granularity = options.granularity * 8.0;

    let a_sqr = semi_minor_axis * semi_minor_axis;
    let b_sqr = semi_major_axis * semi_major_axis;
    let ab = semi_major_axis * semi_minor_axis;
    let mag = center.magnitude();
    let unit_pos = center.normalize();
    let east_vec = Cartesian3::UNIT_Z.cross(center).normalize();
    let north_vec = unit_pos.cross(east_vec);
    let point = |theta: f64| point_on_ellipsoid(theta, rotation, north_vec, east_vec, a_sqr, ab, b_sqr, mag, unit_pos);

    let mut num_pts = 1 + (PI_OVER_TWO / granularity).ceil() as usize;
    let delta_theta = PI_OVER_TWO / (num_pts as f64 - 1.0);
    let theta = PI_OVER_TWO - num_pts as f64 * delta_theta;
    if theta < 0.0 {
        num_pts -= (theta.abs() / delta_theta).ceil() as usize;
    }

    let mut positions = Vec::with_capacity(if add_fill_positions { 6 * num_pts * (num_pts + 2) } else { 0 });
    let outer_positions_length = num_pts * 4 * 3;
    let mut outer_positions = vec![0.0; if add_edge_positions { outer_positions_length } else { 0 }];
    let mut outer_right_index = outer_positions_length;
    let mut outer_left_index = 0;
    let mut push_right = |outer: &mut Vec<f64>, p: Cartesian3| {
        outer[outer_right_index - 3..outer_right_index].copy_from_slice(&[p.x, p.y, p.z]);
        outer_right_index -= 3;
    };
    let mut push_left = |outer: &mut Vec<f64>, p: Cartesian3| {
        outer[outer_left_index..outer_left_index + 3].copy_from_slice(&[p.x, p.y, p.z]);
        outer_left_index += 3;
    };
    let row = |positions: &mut Vec<f64>, position: Cartesian3, reflected: Cartesian3, num_interior: usize| {
        positions.extend([position.x, position.y, position.z]);
        for j in 1..num_interior - 1 {
            let t = j as f64 / (num_interior as f64 - 1.0);
            let p = position.lerp(reflected, t);
            positions.extend([p.x, p.y, p.z]);
        }
        positions.extend([reflected.x, reflected.y, reflected.z]);
    };

    let position = point(PI_OVER_TWO);
    if add_fill_positions {
        positions.extend([position.x, position.y, position.z]);
    }
    if add_edge_positions {
        push_right(&mut outer_positions, position);
    }

    let mut theta = PI_OVER_TWO - delta_theta;
    for i in 1..num_pts + 1 {
        let position = point(theta);
        let reflected = point(PI - theta);
        if add_fill_positions {
            row(&mut positions, position, reflected, 2 * i + 2);
        }
        if add_edge_positions {
            push_right(&mut outer_positions, position);
            push_left(&mut outer_positions, reflected);
        }
        theta = PI_OVER_TWO - (i + 1) as f64 * delta_theta;
    }

    for i in (2..=num_pts).rev() {
        let theta = PI_OVER_TWO - (i - 1) as f64 * delta_theta;
        let position = point(-theta);
        let reflected = point(theta + PI);
        if add_fill_positions {
            row(&mut positions, position, reflected, 2 * (i - 1) + 2);
        }
        if add_edge_positions {
            push_right(&mut outer_positions, position);
            push_left(&mut outer_positions, reflected);
        }
    }

    let position = point(-PI_OVER_TWO);
    let mut result = EllipsePositions::default();
    if add_fill_positions {
        positions.extend([position.x, position.y, position.z]);
        result.positions = Some(positions);
        result.num_pts = Some(num_pts);
    }
    if add_edge_positions {
        push_right(&mut outer_positions, position);
        result.outer_positions = Some(outer_positions);
    }
    result
}
