//! CesiumJS `BoundingSphere`.

use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::geographic_projection::GeographicProjection;
use super::intersect::Intersect;
use super::interval::Interval;
use super::map_projection::MapProjection;
use super::math::{js_max, PI};
use super::matrix3::Matrix3;
use super::matrix4::Matrix4;
use super::occluder::Occluder;
use super::oriented_bounding_box::OrientedBoundingBox;
use super::plane::Plane;
use super::rectangle::Rectangle;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoundingSphere {
    pub center: Cartesian3,
    pub radius: f64,
}

/// Cesium's sphere from points: the smaller of Ritter's sphere and the
/// sphere around the axis-aligned box center. Points are visited twice.
fn ritter<I: Iterator<Item = Cartesian3> + Clone>(points: I) -> BoundingSphere {
    let mut iter = points.clone();
    let Some(first) = iter.next() else {
        return BoundingSphere::default();
    };
    let (mut x_min, mut y_min, mut z_min) = (first, first, first);
    let (mut x_max, mut y_max, mut z_max) = (first, first, first);
    for p in iter {
        if p.x < x_min.x {
            x_min = p;
        }
        if p.x > x_max.x {
            x_max = p;
        }
        if p.y < y_min.y {
            y_min = p;
        }
        if p.y > y_max.y {
            y_max = p;
        }
        if p.z < z_min.z {
            z_min = p;
        }
        if p.z > z_max.z {
            z_max = p;
        }
    }
    let x_span = x_max.subtract(x_min).magnitude_squared();
    let y_span = y_max.subtract(y_min).magnitude_squared();
    let z_span = z_max.subtract(z_min).magnitude_squared();
    let (mut d1, mut d2, mut max_span) = (x_min, x_max, x_span);
    if y_span > max_span {
        max_span = y_span;
        d1 = y_min;
        d2 = y_max;
    }
    if z_span > max_span {
        d1 = z_min;
        d2 = z_max;
    }
    let mut ritter_center = Cartesian3::new((d1.x + d2.x) * 0.5, (d1.y + d2.y) * 0.5, (d1.z + d2.z) * 0.5);
    let mut radius_squared = d2.subtract(ritter_center).magnitude_squared();
    let mut ritter_radius = radius_squared.sqrt();
    let min_box = Cartesian3::new(x_min.x, y_min.y, z_min.z);
    let max_box = Cartesian3::new(x_max.x, y_max.y, z_max.z);
    let naive_center = min_box.midpoint(max_box);
    let mut naive_radius: f64 = 0.0;
    for p in points {
        let r = p.subtract(naive_center).magnitude();
        if r > naive_radius {
            naive_radius = r;
        }
        let old_center_to_point_squared = p.subtract(ritter_center).magnitude_squared();
        if old_center_to_point_squared > radius_squared {
            let old_center_to_point = old_center_to_point_squared.sqrt();
            ritter_radius = (ritter_radius + old_center_to_point) * 0.5;
            radius_squared = ritter_radius * ritter_radius;
            let old_to_new = old_center_to_point - ritter_radius;
            ritter_center = Cartesian3::new(
                (ritter_radius * ritter_center.x + old_to_new * p.x) / old_center_to_point,
                (ritter_radius * ritter_center.y + old_to_new * p.y) / old_center_to_point,
                (ritter_radius * ritter_center.z + old_to_new * p.z) / old_center_to_point,
            );
        }
    }
    if ritter_radius < naive_radius {
        BoundingSphere::new(ritter_center, ritter_radius)
    } else {
        BoundingSphere::new(naive_center, naive_radius)
    }
}

impl BoundingSphere {
    pub const PACKED_LENGTH: usize = 4;

    pub const fn new(center: Cartesian3, radius: f64) -> Self {
        Self { center, radius }
    }

    pub fn from_points(positions: &[Cartesian3]) -> Self {
        ritter(positions.iter().copied())
    }

    /// Bounds a cartographic rectangle in 2D map coordinates.
    pub fn from_rectangle_2d(rectangle: &Rectangle, projection: Option<&dyn MapProjection>) -> Self {
        Self::from_rectangle_with_heights_2d(rectangle, projection, 0.0, 0.0)
    }

    pub fn from_rectangle_with_heights_2d(
        rectangle: &Rectangle,
        projection: Option<&dyn MapProjection>,
        minimum_height: f64,
        maximum_height: f64,
    ) -> Self {
        let default = GeographicProjection::default();
        let projection = projection.unwrap_or(&default);
        let mut sw = rectangle.southwest();
        sw.height = minimum_height;
        let mut ne = rectangle.northeast();
        ne.height = maximum_height;
        let lower_left = projection.project(&sw);
        let upper_right = projection.project(&ne);
        let width = upper_right.x - lower_left.x;
        let height = upper_right.y - lower_left.y;
        let elevation = upper_right.z - lower_left.z;
        Self::new(
            Cartesian3::new(
                lower_left.x + width * 0.5,
                lower_left.y + height * 0.5,
                lower_left.z + elevation * 0.5,
            ),
            (width * width + height * height + elevation * elevation).sqrt() * 0.5,
        )
    }

    pub fn from_rectangle_3d(rectangle: &Rectangle, ellipsoid: &Ellipsoid, surface_height: f64) -> Self {
        Self::from_points(&rectangle.subsample(ellipsoid, surface_height))
    }

    /// Positions packed `x, y, z` every `stride` values (Cesium default 3),
    /// offset by `center`.
    pub fn from_vertices(positions: &[f64], center: Cartesian3, stride: usize) -> Self {
        if positions.is_empty() {
            return Self::default();
        }
        ritter((0..positions.len()).step_by(stride).map(move |i| {
            Cartesian3::new(positions[i] + center.x, positions[i + 1] + center.y, positions[i + 2] + center.z)
        }))
    }

    /// Positions split into high and low parts (see `EncodedCartesian3`).
    pub fn from_encoded_cartesian_vertices(positions_high: &[f64], positions_low: &[f64]) -> Self {
        if positions_high.len() != positions_low.len() || positions_high.is_empty() {
            return Self::default();
        }
        ritter((0..positions_high.len()).step_by(3).map(move |i| {
            Cartesian3::new(
                positions_high[i] + positions_low[i],
                positions_high[i + 1] + positions_low[i + 1],
                positions_high[i + 2] + positions_low[i + 2],
            )
        }))
    }

    pub fn from_corner_points(corner: Cartesian3, opposite_corner: Cartesian3) -> Self {
        let center = corner.midpoint(opposite_corner);
        Self::new(center, center.distance(opposite_corner))
    }

    pub fn from_ellipsoid(ellipsoid: &Ellipsoid) -> Self {
        Self::new(Cartesian3::ZERO, ellipsoid.maximum_radius)
    }

    pub fn from_bounding_spheres(spheres: &[Self]) -> Self {
        match spheres.len() {
            0 => Self::default(),
            1 => spheres[0],
            2 => spheres[0].union(&spheres[1]),
            _ => {
                let centers: Vec<Cartesian3> = spheres.iter().map(|s| s.center).collect();
                let mut result = Self::from_points(&centers);
                let mut radius = result.radius;
                for s in spheres {
                    radius = js_max(radius, result.center.distance(s.center) + s.radius);
                }
                result.radius = radius;
                result
            }
        }
    }

    /// Sphere around an oriented box given by its center and half axes.
    pub fn from_oriented_bounding_box_parts(center: Cartesian3, half_axes: &Matrix3) -> Self {
        let u = half_axes.get_column(0);
        let v = half_axes.get_column(1);
        let w = half_axes.get_column(2);
        Self::new(center, u.add(v).add(w).magnitude())
    }

    pub fn from_oriented_bounding_box(obb: &OrientedBoundingBox) -> Self {
        Self::from_oriented_bounding_box_parts(obb.center, &obb.half_axes)
    }

    pub fn is_occluded(&self, occluder: &Occluder) -> bool {
        !occluder.is_bounding_sphere_visible(self)
    }

    /// Sphere around the unit cube transformed by `transformation`.
    pub fn from_transformation(transformation: &Matrix4) -> Self {
        Self::new(transformation.get_translation(), 0.5 * transformation.get_scale().magnitude())
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.center.pack(array, starting_index);
        array[starting_index + 3] = self.radius;
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self::new(Cartesian3::unpack(array, starting_index), array[starting_index + 3])
    }

    pub fn union(&self, right: &Self) -> Self {
        let to_right_center = right.center.subtract(self.center);
        let center_separation = to_right_center.magnitude();
        if self.radius >= center_separation + right.radius {
            return *self;
        }
        if right.radius >= center_separation + self.radius {
            return *right;
        }
        let half_distance = (self.radius + center_separation + right.radius) * 0.5;
        let center = to_right_center
            .multiply_by_scalar((-self.radius + half_distance) / center_separation)
            .add(self.center);
        Self::new(center, half_distance)
    }

    pub fn expand(&self, point: Cartesian3) -> Self {
        let mut r = *self;
        let radius = point.subtract(r.center).magnitude();
        if radius > r.radius {
            r.radius = radius;
        }
        r
    }

    pub fn intersect_plane(&self, plane: &Plane) -> Intersect {
        let distance_to_plane = plane.normal.dot(self.center) + plane.distance;
        if distance_to_plane < -self.radius {
            Intersect::Outside
        } else if distance_to_plane < self.radius {
            Intersect::Intersecting
        } else {
            Intersect::Inside
        }
    }

    pub fn transform(&self, transform: &Matrix4) -> Self {
        Self::new(transform.multiply_by_point(self.center), transform.get_maximum_scale() * self.radius)
    }

    /// Squared distance from the sphere's surface; 0 inside.
    pub fn distance_squared_to(&self, cartesian: Cartesian3) -> f64 {
        let distance = self.center.subtract(cartesian).magnitude() - self.radius;
        if distance <= 0.0 {
            0.0
        } else {
            distance * distance
        }
    }

    pub fn transform_without_scale(&self, transform: &Matrix4) -> Self {
        Self::new(transform.multiply_by_point(self.center), self.radius)
    }

    /// Near and far distances along `direction` from `position`.
    pub fn compute_plane_distances(&self, position: Cartesian3, direction: Cartesian3) -> Interval {
        let mag = direction.dot(self.center.subtract(position));
        Interval::new(mag - self.radius, mag + self.radius)
    }

    /// Sphere in 2D map coordinates (`z` is the projected x); geographic
    /// projection on WGS84 when `projection` is `None`.
    pub fn project_to_2d(&self, projection: Option<&dyn MapProjection>) -> Self {
        let default = GeographicProjection::default();
        let projection = projection.unwrap_or(&default);
        let ellipsoid = projection.ellipsoid();
        let center = self.center;
        let radius = self.radius;
        let mut normal = if center == Cartesian3::ZERO {
            Cartesian3::UNIT_X
        } else {
            ellipsoid
                .geodetic_surface_normal(center)
                .unwrap_or(Cartesian3::new(f64::NAN, f64::NAN, f64::NAN))
        };
        let east = Cartesian3::UNIT_Z.cross(normal).normalize();
        let north = normal.cross(east).normalize();
        normal = normal.multiply_by_scalar(radius);
        let north = north.multiply_by_scalar(radius);
        let east = east.multiply_by_scalar(radius);
        let south = north.negate();
        let west = east.negate();
        let up = normal;
        let down = normal.negate();
        let corners = [
            up.add(north).add(east),
            up.add(north).add(west),
            up.add(south).add(west),
            up.add(south).add(east),
            down.add(north).add(east),
            down.add(north).add(west),
            down.add(south).add(west),
            down.add(south).add(east),
        ];
        let positions: Vec<Cartesian3> = corners
            .iter()
            .map(|corner| {
                let p = center.add(*corner);
                let c = ellipsoid
                    .cartesian_to_cartographic(p)
                    .unwrap_or(Cartographic::new(f64::NAN, f64::NAN, f64::NAN));
                projection.project(&c)
            })
            .collect();
        let mut result = Self::from_points(&positions);
        let c = result.center;
        result.center = Cartesian3::new(c.z, c.x, c.y);
        result
    }

    pub fn equals(&self, right: &Self) -> bool {
        self.center == right.center && self.radius == right.radius
    }

    pub fn volume(&self) -> f64 {
        (4.0 / 3.0) * PI * self.radius * self.radius * self.radius
    }
}
