//! CesiumJS `EllipsoidTangentPlane`: the plane tangent to the ellipsoid at a
//! point, with east and north as its x and y axes.

use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::intersection_tests::ray_plane;
use super::plane::Plane;
use super::ray::Ray;
use super::transforms::Transforms;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidTangentPlane {
    pub ellipsoid: Ellipsoid,
    /// The tangent point, on the ellipsoid's surface.
    pub origin: Cartesian3,
    pub x_axis: Cartesian3,
    pub y_axis: Cartesian3,
    pub plane: Plane,
}

impl EllipsoidTangentPlane {
    /// Tangent plane below `origin`; `None` if `origin` is the ellipsoid's center.
    pub fn new(origin: Cartesian3, ellipsoid: &Ellipsoid) -> Option<Self> {
        let origin = ellipsoid.scale_to_geodetic_surface(origin)?;
        let enu = Transforms::east_north_up_to_fixed_frame(origin, ellipsoid);
        let x_axis = Cartesian3::from_cartesian4(enu.get_column(0));
        let y_axis = Cartesian3::from_cartesian4(enu.get_column(1));
        let normal = Cartesian3::from_cartesian4(enu.get_column(2));
        Some(Self { ellipsoid: *ellipsoid, origin, x_axis, y_axis, plane: Plane::from_point_normal(origin, normal) })
    }

    pub fn z_axis(&self) -> Cartesian3 {
        self.plane.normal
    }

    /// Tangent plane at the center of the points' bounding box.
    pub fn from_points(cartesians: &[Cartesian3], ellipsoid: &Ellipsoid) -> Option<Self> {
        Self::new(AxisAlignedBoundingBox::from_points(cartesians).center, ellipsoid)
    }

    /// Projects along the ray from the ellipsoid's center (either direction).
    pub fn project_point_onto_plane(&self, cartesian: Cartesian3) -> Option<Cartesian2> {
        let mut ray = Ray { origin: cartesian, direction: cartesian.normalize() };
        let mut hit = ray_plane(&ray, &self.plane);
        if hit.is_none() {
            ray.direction = ray.direction.negate();
            hit = ray_plane(&ray, &self.plane);
        }
        let v = hit?.subtract(self.origin);
        Some(Cartesian2::new(self.x_axis.dot(v), self.y_axis.dot(v)))
    }

    /// Projects every point that can be projected, dropping the rest.
    pub fn project_points_onto_plane(&self, cartesians: &[Cartesian3]) -> Vec<Cartesian2> {
        cartesians.iter().filter_map(|c| self.project_point_onto_plane(*c)).collect()
    }

    /// Projects along the plane normal.
    pub fn project_point_to_nearest_on_plane(&self, cartesian: Cartesian3) -> Cartesian2 {
        let mut ray = Ray { origin: cartesian, direction: self.plane.normal };
        let mut hit = ray_plane(&ray, &self.plane);
        if hit.is_none() {
            ray.direction = ray.direction.negate();
            hit = ray_plane(&ray, &self.plane);
        }
        let v = hit.unwrap_or(Cartesian3::new(f64::NAN, f64::NAN, f64::NAN)).subtract(self.origin);
        Cartesian2::new(self.x_axis.dot(v), self.y_axis.dot(v))
    }

    pub fn project_points_to_nearest_on_plane(&self, cartesians: &[Cartesian3]) -> Vec<Cartesian2> {
        cartesians.iter().map(|c| self.project_point_to_nearest_on_plane(*c)).collect()
    }

    /// Plane coordinates back to the surface, along the ray from the center.
    pub fn project_point_onto_ellipsoid(&self, cartesian: Cartesian2) -> Cartesian3 {
        let p = self
            .origin
            .add(self.x_axis.multiply_by_scalar(cartesian.x))
            .add(self.y_axis.multiply_by_scalar(cartesian.y));
        self.ellipsoid.scale_to_geocentric_surface(p)
    }

    pub fn project_points_onto_ellipsoid(&self, cartesians: &[Cartesian2]) -> Vec<Cartesian3> {
        cartesians.iter().map(|c| self.project_point_onto_ellipsoid(*c)).collect()
    }
}
