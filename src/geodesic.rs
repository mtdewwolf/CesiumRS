//! Viewer geodesic: Core's `EllipsoidGeodesic` (Vincenty) for ordinary
//! routes, plus the viewer's deterministic great-circle fallback for the
//! cases Core rejects or where Vincenty degenerates.
//!
//! Core's `EllipsoidGeodesic::new` returns `None` within 0.0125 rad of
//! antipodal, where Vincenty's inverse method may not converge. The viewer
//! still has to draw those routes, so near-antipodal and (near-)repeated
//! endpoints use a spherical approximation instead: the distance is the
//! great-circle angle times the semimajor axis, and interpolation slerps
//! between the endpoints' geodetic directions. These fallback distances are
//! spherical approximations; interpolation preserves the geodetic endpoints,
//! including repeated points (a normalized ECEF direction would encode
//! geocentric latitude and move even a repeated endpoint).

use crate::core::{Cartesian3, Cartographic, Ellipsoid, EllipsoidGeodesic};

#[derive(Clone, Copy, Debug)]
pub struct Geodesic {
    pub distance: f64,
    pub start_heading: f64,
    pub end_heading: f64,
    inner: Inner,
}

// `Geodesic` is `Copy` and created per route, so the size difference
// between the Vincenty state and the small spherical fallback is fine.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug)]
enum Inner {
    Vincenty(EllipsoidGeodesic),
    Spherical { start_dir: Cartesian3, end_dir: Cartesian3, start_lon: f64 },
}

fn direction(lon: f64, lat: f64) -> Cartesian3 {
    Cartesian3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin())
}

fn slerp(a: Cartesian3, b: Cartesian3, t: f64) -> Cartesian3 {
    let a = a.normalize();
    let b = b.normalize();
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let d = a.dot(b).clamp(-1.0, 1.0);
    if d > 0.9995 {
        return a.lerp(b, t).normalize();
    }
    let tangent = b - a.multiply_by_scalar(d);
    let length = tangent.magnitude();
    let tangent = if length > 1e-12 {
        tangent.divide_by_scalar(length)
    } else {
        // Antipodes have infinitely many great circles. Choose a stable
        // perpendicular axis, rather than collapsing the path to its start.
        let axis = if a.x.abs() <= a.y.abs() && a.x.abs() <= a.z.abs() {
            Cartesian3::UNIT_X
        } else if a.y.abs() <= a.z.abs() {
            Cartesian3::UNIT_Y
        } else {
            Cartesian3::UNIT_Z
        };
        axis.subtract(a.multiply_by_scalar(a.dot(axis))).normalize()
    };
    let angle = length.atan2(d) * t;
    a.multiply_by_scalar(angle.cos()).add(tangent.multiply_by_scalar(angle.sin())).normalize()
}

impl Geodesic {
    pub fn new(ellipsoid: &Ellipsoid, start_lon: f64, start_lat: f64, end_lon: f64, end_lat: f64) -> Self {
        let start_dir = direction(start_lon, start_lat);
        let end_dir = direction(end_lon, end_lat);
        let angle = start_dir.cross(end_dir).magnitude().atan2(start_dir.dot(end_dir));

        let start = Cartographic::new(start_lon, start_lat, 0.0);
        let end = Cartographic::new(end_lon, end_lat, 0.0);
        if angle >= 1e-12 {
            if let Some(g) = EllipsoidGeodesic::new(&start, &end, ellipsoid) {
                return Self {
                    distance: g.surface_distance,
                    start_heading: g.start_heading,
                    end_heading: g.end_heading,
                    inner: Inner::Vincenty(g),
                };
            }
        }

        // Repeated points, and the near-antipodal routes Core rejects.
        Self {
            distance: angle * ellipsoid.maximum_radius,
            start_heading: 0.0,
            end_heading: 0.0,
            inner: Inner::Spherical { start_dir, end_dir, start_lon },
        }
    }

    pub fn interpolate(&self, fraction: f64) -> (f64, f64) {
        self.interpolate_distance(self.distance * fraction)
    }

    pub fn interpolate_distance(&self, distance: f64) -> (f64, f64) {
        match &self.inner {
            Inner::Vincenty(g) => {
                let c = g.interpolate_using_surface_distance(distance);
                (c.longitude, c.latitude)
            }
            Inner::Spherical { start_dir, end_dir, start_lon } => {
                let t = if self.distance.abs() < 1e-6 { 0.0 } else { distance / self.distance };
                let dir = slerp(*start_dir, *end_dir, t.clamp(0.0, 1.0));
                let longitude = dir.y.atan2(dir.x);
                // For a repeated point the slerp returns the start direction
                // exactly; keep the input longitude instead of atan2's
                // equivalent angle so the endpoint is preserved bit for bit.
                let longitude = if self.distance.abs() < 1e-6 { *start_lon } else { longitude };
                let latitude = dir.z.clamp(-1.0, 1.0).asin();
                (longitude, latitude)
            }
        }
    }
}
