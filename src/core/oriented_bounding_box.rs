//! CesiumJS `OrientedBoundingBox`: a center and three half-axis vectors.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::culling_volume::BoundingVolume;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_tangent_plane::EllipsoidTangentPlane;
use super::intersect::Intersect;
use super::interval::Interval;
use super::math::{js_max, js_min, EPSILON10, EPSILON3, PI_OVER_TWO};
use super::matrix3::Matrix3;
use super::matrix4::Matrix4;
use super::occluder::Occluder;
use super::plane::Plane;
use super::rectangle::Rectangle;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OrientedBoundingBox {
    pub center: Cartesian3,
    /// Columns are the half-axis vectors.
    pub half_axes: Matrix3,
}

#[allow(clippy::too_many_arguments)]
fn from_plane_extents(
    plane_origin: Cartesian3,
    plane_x_axis: Cartesian3,
    plane_y_axis: Cartesian3,
    plane_z_axis: Cartesian3,
    minimum_x: f64,
    maximum_x: f64,
    minimum_y: f64,
    maximum_y: f64,
    minimum_z: f64,
    maximum_z: f64,
) -> OrientedBoundingBox {
    let half_axes = Matrix3::ZERO.set_column(0, plane_x_axis).set_column(1, plane_y_axis).set_column(2, plane_z_axis);
    let center_offset =
        Cartesian3::new((minimum_x + maximum_x) / 2.0, (minimum_y + maximum_y) / 2.0, (minimum_z + maximum_z) / 2.0);
    let scale =
        Cartesian3::new((maximum_x - minimum_x) / 2.0, (maximum_y - minimum_y) / 2.0, (maximum_z - minimum_z) / 2.0);
    let center = plane_origin.add(half_axes.multiply_by_vector(center_offset));
    OrientedBoundingBox { center, half_axes: half_axes.multiply_by_scale(scale) }
}

impl OrientedBoundingBox {
    pub const PACKED_LENGTH: usize = 12;

    pub const fn new(center: Cartesian3, half_axes: Matrix3) -> Self {
        Self { center, half_axes }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.center.pack(array, starting_index);
        self.half_axes.pack(array, starting_index + 3);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self::new(Cartesian3::unpack(array, starting_index), Matrix3::unpack(array, starting_index + 3))
    }

    /// Box aligned with the principal axes of the points' covariance.
    pub fn from_points(positions: &[Cartesian3]) -> Self {
        if positions.is_empty() {
            return Self::default();
        }
        let length = positions.len();
        let mut mean_point = positions[0];
        for p in &positions[1..] {
            mean_point = mean_point.add(*p);
        }
        let inv_length = 1.0 / length as f64;
        mean_point = mean_point.multiply_by_scalar(inv_length);
        let (mut exx, mut exy, mut exz, mut eyy, mut eyz, mut ezz) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for p in positions {
            let p = p.subtract(mean_point);
            exx += p.x * p.x;
            exy += p.x * p.y;
            exz += p.x * p.z;
            eyy += p.y * p.y;
            eyz += p.y * p.z;
            ezz += p.z * p.z;
        }
        exx *= inv_length;
        exy *= inv_length;
        exz *= inv_length;
        eyy *= inv_length;
        eyz *= inv_length;
        ezz *= inv_length;
        let covariance = Matrix3([exx, exy, exz, exy, eyy, eyz, exz, eyz, ezz]);
        let rotation = covariance.compute_eigen_decomposition().unitary;
        let v1 = rotation.get_column(0);
        let v2 = rotation.get_column(1);
        let v3 = rotation.get_column(2);
        let (mut u1, mut u2, mut u3) = (-f64::MAX, -f64::MAX, -f64::MAX);
        let (mut l1, mut l2, mut l3) = (f64::MAX, f64::MAX, f64::MAX);
        for p in positions {
            u1 = js_max(v1.dot(*p), u1);
            u2 = js_max(v2.dot(*p), u2);
            u3 = js_max(v3.dot(*p), u3);
            l1 = js_min(v1.dot(*p), l1);
            l2 = js_min(v2.dot(*p), l2);
            l3 = js_min(v3.dot(*p), l3);
        }
        let c1 = v1.multiply_by_scalar(0.5 * (l1 + u1));
        let c2 = v2.multiply_by_scalar(0.5 * (l2 + u2));
        let c3 = v3.multiply_by_scalar(0.5 * (l3 + u3));
        let center = c1.add(c2).add(c3);
        let scale = Cartesian3::new(u1 - l1, u2 - l2, u3 - l3).multiply_by_scalar(0.5);
        Self::new(center, rotation.multiply_by_scale(scale))
    }

    /// Tight box around a cartographic rectangle between two heights.
    /// `None` for rectangles wider than 2π or taller than π, or an ellipsoid
    /// that is not one of revolution, where Cesium throws.
    pub fn from_rectangle(
        rectangle: &Rectangle,
        minimum_height: f64,
        maximum_height: f64,
        ellipsoid: &Ellipsoid,
    ) -> Option<Self> {
        use super::math::{equals_epsilon, EPSILON15, PI, TWO_PI};
        let width = rectangle.width();
        let height = rectangle.height();
        if !(0.0..=TWO_PI).contains(&width)
            || !(0.0..=PI).contains(&height)
            || !equals_epsilon(ellipsoid.radii.x, ellipsoid.radii.y, EPSILON15, EPSILON15)
        {
            return None;
        }
        if width <= PI {
            let tangent_point_cartographic = rectangle.center();
            let tangent_point = ellipsoid.cartographic_to_cartesian(&tangent_point_cartographic);
            let tangent_plane = EllipsoidTangentPlane::new(tangent_point, ellipsoid)?;
            let plane = tangent_plane.plane;
            let lon_center = tangent_point_cartographic.longitude;
            let lat_center =
                if rectangle.south < 0.0 && rectangle.north > 0.0 { 0.0 } else { tangent_point_cartographic.latitude };
            let nc = Cartographic::new(lon_center, rectangle.north, maximum_height);
            let mut nw = Cartographic::new(rectangle.west, rectangle.north, maximum_height);
            let cw = Cartographic::new(rectangle.west, lat_center, maximum_height);
            let mut sw = Cartographic::new(rectangle.west, rectangle.south, maximum_height);
            let sc = Cartographic::new(lon_center, rectangle.south, maximum_height);
            let project = |c: &Cartographic| {
                tangent_plane.project_point_to_nearest_on_plane(ellipsoid.cartographic_to_cartesian(c))
            };
            let p_nc = project(&nc);
            let p_nw = project(&nw);
            let p_cw = project(&cw);
            let p_sw = project(&sw);
            let p_sc = project(&sc);
            let min_x = js_min(js_min(p_nw.x, p_cw.x), p_sw.x);
            let max_x = -min_x;
            let max_y = js_max(p_nw.y, p_nc.y);
            let min_y = js_min(p_sw.y, p_sc.y);
            nw.height = minimum_height;
            sw.height = minimum_height;
            let cartesian_nw = ellipsoid.cartographic_to_cartesian(&nw);
            let cartesian_sw = ellipsoid.cartographic_to_cartesian(&sw);
            let min_z = js_min(plane.get_point_distance(cartesian_nw), plane.get_point_distance(cartesian_sw));
            let max_z = maximum_height;
            return Some(from_plane_extents(
                tangent_plane.origin,
                tangent_plane.x_axis,
                tangent_plane.y_axis,
                tangent_plane.z_axis(),
                min_x,
                max_x,
                min_y,
                max_y,
                min_z,
                max_z,
            ));
        }

        let fully_above_equator = rectangle.south > 0.0;
        let fully_below_equator = rectangle.north < 0.0;
        let latitude_nearest_to_equator = if fully_above_equator {
            rectangle.south
        } else if fully_below_equator {
            rectangle.north
        } else {
            0.0
        };
        let center_longitude = rectangle.center().longitude;
        let mut plane_origin =
            Cartesian3::from_radians(center_longitude, latitude_nearest_to_equator, maximum_height, ellipsoid);
        plane_origin.z = 0.0;
        let is_pole = plane_origin.x.abs() < EPSILON10 && plane_origin.y.abs() < EPSILON10;
        let plane_normal = if !is_pole { plane_origin.normalize() } else { Cartesian3::UNIT_X };
        let plane_y_axis = Cartesian3::UNIT_Z;
        let plane_x_axis = plane_normal.cross(plane_y_axis);
        let plane = Plane::from_point_normal(plane_origin, plane_normal);
        let horizon = Cartesian3::from_radians(
            center_longitude + PI_OVER_TWO,
            latitude_nearest_to_equator,
            maximum_height,
            ellipsoid,
        );
        let max_x = plane.project_point_onto_plane(horizon).dot(plane_x_axis);
        let min_x = -max_x;
        let max_y = Cartesian3::from_radians(
            0.0,
            rectangle.north,
            if fully_below_equator { minimum_height } else { maximum_height },
            ellipsoid,
        )
        .z;
        let min_y = Cartesian3::from_radians(
            0.0,
            rectangle.south,
            if fully_above_equator { minimum_height } else { maximum_height },
            ellipsoid,
        )
        .z;
        let far_z = Cartesian3::from_radians(rectangle.east, latitude_nearest_to_equator, maximum_height, ellipsoid);
        let min_z = plane.get_point_distance(far_z);
        Some(from_plane_extents(
            plane_origin,
            plane_x_axis,
            plane_y_axis,
            plane_normal,
            min_x,
            max_x,
            min_y,
            max_y,
            min_z,
            0.0,
        ))
    }

    /// Box of the unit cube transformed by `transformation`.
    pub fn from_transformation(transformation: &Matrix4) -> Self {
        Self::new(transformation.get_translation(), transformation.get_matrix3().multiply_by_scalar(0.5))
    }

    pub fn intersect_plane(&self, plane: &Plane) -> Intersect {
        let n = plane.normal;
        let h = &self.half_axes.0;
        let rad_effective =
            (n.x * h[Matrix3::COLUMN0ROW0] + n.y * h[Matrix3::COLUMN0ROW1] + n.z * h[Matrix3::COLUMN0ROW2]).abs()
                + (n.x * h[Matrix3::COLUMN1ROW0] + n.y * h[Matrix3::COLUMN1ROW1] + n.z * h[Matrix3::COLUMN1ROW2]).abs()
                + (n.x * h[Matrix3::COLUMN2ROW0] + n.y * h[Matrix3::COLUMN2ROW1] + n.z * h[Matrix3::COLUMN2ROW2]).abs();
        let distance_to_plane = n.dot(self.center) + plane.distance;
        if distance_to_plane <= -rad_effective {
            Intersect::Outside
        } else if distance_to_plane >= rad_effective {
            Intersect::Inside
        } else {
            Intersect::Intersecting
        }
    }

    /// Squared distance from the box to a point; 0 inside. Degenerate
    /// (zero-length) axes are replaced by perpendicular ones.
    pub fn distance_squared_to(&self, cartesian: Cartesian3) -> f64 {
        let offset = cartesian.subtract(self.center);
        let mut u = self.half_axes.get_column(0);
        let mut v = self.half_axes.get_column(1);
        let mut w = self.half_axes.get_column(2);
        let u_half = u.magnitude();
        let v_half = v.magnitude();
        let w_half = w.magnitude();
        let (mut u_valid, mut v_valid, mut w_valid) = (true, true, true);
        if u_half > 0.0 {
            u = u.divide_by_scalar(u_half);
        } else {
            u_valid = false;
        }
        if v_half > 0.0 {
            v = v.divide_by_scalar(v_half);
        } else {
            v_valid = false;
        }
        if w_half > 0.0 {
            w = w.divide_by_scalar(w_half);
        } else {
            w_valid = false;
        }
        let degenerate = u32::from(!u_valid) + u32::from(!v_valid) + u32::from(!w_valid);
        match degenerate {
            1 => {
                if !v_valid {
                    v = u.cross(w);
                } else if !w_valid {
                    w = v.cross(u);
                } else {
                    u = v.cross(w);
                }
            }
            2 => {
                let valid_axis1 = if v_valid {
                    v
                } else if w_valid {
                    w
                } else {
                    u
                };
                let mut cross_vector = Cartesian3::UNIT_Y;
                if cross_vector.equals_epsilon(valid_axis1, EPSILON3) {
                    cross_vector = Cartesian3::UNIT_X;
                }
                let valid_axis2 = valid_axis1.cross(cross_vector).normalize();
                let valid_axis3 = valid_axis1.cross(valid_axis2).normalize();
                if u_valid {
                    v = valid_axis2;
                    w = valid_axis3;
                } else if v_valid {
                    w = valid_axis2;
                    u = valid_axis3;
                } else {
                    u = valid_axis2;
                    v = valid_axis3;
                }
            }
            3 => {
                u = Cartesian3::UNIT_X;
                v = Cartesian3::UNIT_Y;
                w = Cartesian3::UNIT_Z;
            }
            _ => {}
        }
        let p = Cartesian3::new(offset.dot(u), offset.dot(v), offset.dot(w));
        let mut distance_squared = 0.0;
        for (coord, half) in [(p.x, u_half), (p.y, v_half), (p.z, w_half)] {
            if coord < -half {
                let d = coord + half;
                distance_squared += d * d;
            } else if coord > half {
                let d = coord - half;
                distance_squared += d * d;
            }
        }
        distance_squared
    }

    /// Nearest and farthest corner distances along `direction` from `position`.
    pub fn compute_plane_distances(&self, position: Cartesian3, direction: Cartesian3) -> Interval {
        let mut min_dist = f64::INFINITY;
        let mut max_dist = f64::NEG_INFINITY;
        let center = self.center;
        let u = self.half_axes.get_column(0);
        let v = self.half_axes.get_column(1);
        let w = self.half_axes.get_column(2);
        let mut take = |corner: Cartesian3| {
            let mag = direction.dot(corner.subtract(position));
            min_dist = js_min(mag, min_dist);
            max_dist = js_max(mag, max_dist);
        };
        take(u.add(v).add(w).add(center));
        take(center.add(u).add(v).subtract(w));
        take(center.add(u).subtract(v).add(w));
        take(center.add(u).subtract(v).subtract(w));
        take(center.subtract(u).add(v).add(w));
        take(center.subtract(u).add(v).subtract(w));
        take(center.subtract(u).subtract(v).add(w));
        take(center.subtract(u).subtract(v).subtract(w));
        Interval::new(min_dist, max_dist)
    }

    /// The eight corners, in Cesium's order (x, then y, then z flipping fastest).
    pub fn compute_corners(&self) -> [Cartesian3; 8] {
        let c = self.center;
        let x = self.half_axes.get_column(0);
        let y = self.half_axes.get_column(1);
        let z = self.half_axes.get_column(2);
        [
            c.subtract(x).subtract(y).subtract(z),
            c.subtract(x).subtract(y).add(z),
            c.subtract(x).add(y).subtract(z),
            c.subtract(x).add(y).add(z),
            c.add(x).subtract(y).subtract(z),
            c.add(x).subtract(y).add(z),
            c.add(x).add(y).subtract(z),
            c.add(x).add(y).add(z),
        ]
    }

    /// Transform taking the unit cube to this box.
    pub fn compute_transformation(&self) -> Matrix4 {
        Matrix4::from_rotation_translation(&self.half_axes.multiply_by_uniform_scale(2.0), self.center)
    }

    pub fn is_occluded(&self, occluder: &Occluder) -> bool {
        !occluder.is_bounding_sphere_visible(&BoundingSphere::from_oriented_bounding_box(self))
    }

    pub fn equals(&self, right: &Self) -> bool {
        self == right
    }
}

impl BoundingVolume for OrientedBoundingBox {
    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        OrientedBoundingBox::intersect_plane(self, plane)
    }
}
