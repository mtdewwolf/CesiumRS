//! Rust port of the CesiumJS 1.139 geospatial core.
//!
//! The formulas follow CesiumJS (`Ellipsoid`, `EllipsoidGeodesic`,
//! `WebMercatorProjection`, `IntersectionTests.rayEllipsoid`,
//! `Transforms.eastNorthUpToFixedFrame`, and `Camera` heading/pitch/range),
//! which is Apache-2.0. This is not a binding to the JavaScript library and
//! it is not affiliated with Cesium GS, Inc.

pub mod api;
pub mod camera;
pub mod ellipsoid;
pub mod geodesic;
pub mod intersect;
pub mod math;
pub mod mesh;
pub mod projection;
pub mod sun;

pub use camera::look_at;
pub use ellipsoid::{
    cartesian_to_cartographic, cartographic_to_cartesian, Ellipsoid,
};
pub use geodesic::Geodesic;
pub use intersect::{pick_ray, ray_ellipsoid};
pub use projection::{project, unproject};
pub use sun::sun_direction_ecef;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{enu_axes, offset_from_heading_pitch_range};
    use crate::math::{to_radians, PI, PI_OVER_TWO};
    use crate::projection::{
        geographic_tile_rectangle, maximum_latitude, mercator_angle_to_geodetic_latitude,
    };

    fn wgs84() -> Ellipsoid {
        Ellipsoid::wgs84()
    }

    #[test]
    fn equator_prime_meridian_is_semimajor_axis() {
        let p = cartographic_to_cartesian(&wgs84(), 0.0, 0.0, 0.0);
        assert!((p.x - 6_378_137.0).abs() < 1e-6);
        assert!(p.y.abs() < 1e-6);
        assert!(p.z.abs() < 1e-6);
    }

    #[test]
    fn north_pole_is_semiminor_axis() {
        let p = cartographic_to_cartesian(&wgs84(), 0.0, PI_OVER_TWO, 0.0);
        assert!(p.x.abs() < 1e-4);
        assert!(p.y.abs() < 1e-4);
        assert!((p.z - 6_356_752.314_245_179_3).abs() < 1e-4);
    }

    #[test]
    fn cartographic_roundtrip() {
        let samples = [
            (to_radians(-105.0844), to_radians(40.5853), 1525.0),
            (to_radians(139.6917), to_radians(35.6895), 40.0),
            (to_radians(-43.1729), to_radians(-22.9068), 10.0),
            (0.4, -0.7, 250_000.0),
        ];
        for (lon, lat, h) in samples {
            let p = cartographic_to_cartesian(&wgs84(), lon, lat, h);
            let c = cartesian_to_cartographic(&wgs84(), p).unwrap();
            assert!((c.longitude - lon).abs() < 1e-10, "lon {lon}");
            assert!((c.latitude - lat).abs() < 1e-10, "lat {lat}");
            assert!((c.height - h).abs() < 1e-3, "height {}", c.height - h);
        }
    }

    #[test]
    fn enu_at_equator() {
        let origin = cartographic_to_cartesian(&wgs84(), 0.0, 0.0, 0.0);
        let (east, north, up) = enu_axes(origin, &wgs84());
        assert!(east.y > 0.999);
        assert!(north.z > 0.999);
        assert!(up.x > 0.999);
    }

    #[test]
    fn enu_at_north_pole() {
        let origin = cartographic_to_cartesian(&wgs84(), 0.0, PI_OVER_TWO, 0.0);
        let (east, north, up) = enu_axes(origin, &wgs84());
        assert!((east.y - 1.0).abs() < 1e-8);
        assert!((north.x + 1.0).abs() < 1e-6);
        assert!((up.z - 1.0).abs() < 1e-8);
    }

    #[test]
    fn nadir_camera_sits_above_focus() {
        let e = wgs84();
        let frame = look_at(
            &e,
            0.0,
            0.0,
            0.0,
            0.0,
            -PI_OVER_TWO,
            1_000.0,
            to_radians(60.0),
            1.0,
            1.0,
            10_000_000.0,
        );
        let focus = cartographic_to_cartesian(&e, 0.0, 0.0, 0.0);
        let offset = frame.eye - focus;
        assert!((offset.magnitude() - 1_000.0).abs() < 1e-4);
        assert!(offset.x > 999.0);
        let toward = (focus - frame.eye).normalize();
        assert!(frame.direction.dot(toward) > 0.999);
    }

    #[test]
    fn hpr_offset_distance_matches_range() {
        let offset = offset_from_heading_pitch_range(0.4, -0.5, 12_345.0);
        assert!((offset.magnitude() - 12_345.0).abs() < 1e-6);
    }

    #[test]
    fn ray_from_outside_hits_equator() {
        let e = wgs84();
        let origin = cartographic_to_cartesian(&e, 0.0, 0.0, 2_000_000.0);
        let hit = pick_ray(origin, crate::math::Vec3::new(-1.0, 0.0, 0.0), &e).unwrap();
        assert!(hit.longitude.abs() < 1e-6);
        assert!(hit.latitude.abs() < 1e-6);
        assert!(hit.height.abs() < 1e-2);
    }

    #[test]
    fn equator_quarter_geodesic_is_quarter_circumference() {
        let g = Geodesic::new(&wgs84(), 0.0, 0.0, PI / 2.0, 0.0);
        let expected = wgs84().maximum_radius * PI / 2.0;
        assert!((g.distance - expected).abs() < 0.05, "{}", g.distance);
        let (lon, lat) = g.interpolate(1.0);
        assert!((lon - PI / 2.0).abs() < 1e-6, "{lon}");
        assert!(lat.abs() < 1e-6, "{lat}");
        let (lon0, lat0) = g.interpolate(0.0);
        assert!(lon0.abs() < 1e-6 && lat0.abs() < 1e-6);
    }

    #[test]
    fn new_york_to_london_is_about_5570_km() {
        let g = Geodesic::new(
            &wgs84(),
            to_radians(-74.006),
            to_radians(40.7128),
            to_radians(-0.1276),
            to_radians(51.5074),
        );
        assert!(
            (g.distance - 5_570_000.0).abs() < 30_000.0,
            "{}",
            g.distance
        );
    }

    #[test]
    fn web_mercator_roundtrip_and_max_latitude() {
        let max_lat = maximum_latitude();
        assert!((max_lat - mercator_angle_to_geodetic_latitude(PI)).abs() < 1e-15);
        assert!((max_lat.to_degrees() - 85.051_128_78).abs() < 1e-6);
        let a = wgs84().maximum_radius;
        let (x, y, z) = project(0.5, 0.4, 12.0, a);
        let (lon, lat, h) = unproject(x, y, z, a);
        assert!((lon - 0.5).abs() < 1e-12);
        assert!((lat - 0.4).abs() < 1e-12);
        assert!((h - 12.0).abs() < 1e-9);
        let (ox, oy, _) = project(0.0, 0.0, 0.0, a);
        assert!(ox.abs() < 1e-6 && oy.abs() < 1e-6);
    }

    #[test]
    fn geographic_level_zero_northwest_tile() {
        let r = geographic_tile_rectangle(0, 0, 0);
        assert!((r.west + PI).abs() < 1e-12);
        assert!(r.east.abs() < 1e-12);
        assert!((r.north - PI_OVER_TWO).abs() < 1e-12);
        assert!((r.south + PI_OVER_TWO).abs() < 1e-12);
    }

    #[test]
    fn sun_direction_is_unit_and_seasonal() {
        let d = sun_direction_ecef(2_460_230.5);
        assert!((d.magnitude() - 1.0).abs() < 1e-9);
        let p = cartesian_to_cartographic(&wgs84(), d * wgs84().maximum_radius).unwrap();
        assert!(p.latitude.abs() < 0.45);
    }
}
