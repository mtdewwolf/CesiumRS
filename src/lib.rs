//! Rust port of the CesiumJS 1.139 geospatial core, renderer and worker calculations,
//! plus the Lakebed viewer that runs on Core.
//!
//! The [`renderer`] module provides executable WebGL1/WebGL2 rendering with
//! explicit host, resource-lifetime and validation contracts.
//!
//! The ported CesiumJS Core modules live in [`core`] — one Rust module per
//! Cesium module, checked against CesiumJS fixtures. The viewer layer
//! ([`camera`], [`geodesic`], [`mesh`], [`sun`], and the C ABI in [`api`])
//! is built entirely on `core`: there are no separate first-generation
//! implementations of the ellipsoid, geodesic, projection, or intersection
//! math. Viewer-only behaviour that Core does not have — the
//! heading/pitch/range camera offset, the near-antipodal geodesic fallback,
//! tile-level heuristics, and the no-EOP sun rotation fallback — is kept in
//! those viewer modules and documented there.
//!
//! ## Global state and threading
//!
//! CesiumJS is single-threaded and keeps several pieces of process-wide
//! state; the port keeps the same *semantics* with explicit threading
//! contracts instead of pretending the state is not there:
//!
//! - **Leap seconds** (`core::JulianDate`): one process-wide table behind an
//!   `RwLock`, mirroring Cesium's mutable `JulianDate.leapSeconds`. Inserts
//!   are date-ordered and de-duplicated; `reset_leap_seconds` restores the
//!   defaults, and tests that add a second must reset it.
//! - **Random numbers** (`core::math`): a per-thread Mersenne Twister behind
//!   `next_random_number`, seeded per thread via `set_random_number_seed`
//!   (Cesium's `Math.setRandomNumberSeed` seeds one global generator; here
//!   each thread has its own, so parallel tests do not interleave draws).
//! - **Terrain grid caches** (`core::TerrainProvider` helpers): per-thread
//!   caches of immutable `Arc` index buffers keyed by grid size.
//! - **Events** (`core::Event`): listeners must be `Send`; shared events
//!   travel as `Arc<Mutex<Event<_>>>` (see `TerrainProvider::error_event`
//!   and `core::EventHelper`), so providers work across threads.
//! - Small items: the GUID counter is an `AtomicU64`, `core::Clock`'s start
//!   instant is a per-thread monotonic anchor, and the C ABI scratch buffers
//!   in [`api`] are per-thread (the wasm host is single-threaded).
//!
//! This is not a binding to the JavaScript library and it is not affiliated
//! with Cesium GS, Inc. CesiumJS is Apache-2.0; see `LICENSE` and `NOTICE`.

pub mod api;
pub mod camera;
pub mod core;
pub mod geodesic;
pub mod mesh;
pub mod renderer;
pub mod sun;
pub mod workers;

pub use camera::{look_at, pick_ray, CameraFrame, Pick};
pub use geodesic::Geodesic;
pub use sun::sun_direction_ecef;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{offset_from_heading_pitch_range, ray_from_ndc};
    use crate::core::math::{to_radians, PI, PI_OVER_TWO};
    use crate::core::{
        Cartesian3, Cartographic, Ellipsoid, GeographicTilingScheme, MapProjection, TilingScheme, Transforms,
        WebMercatorProjection,
    };

    fn wgs84() -> Ellipsoid {
        Ellipsoid::WGS84
    }

    fn to_cartesian(e: &Ellipsoid, lon: f64, lat: f64, h: f64) -> Cartesian3 {
        e.cartographic_to_cartesian(&Cartographic::new(lon, lat, h))
    }

    #[test]
    fn equator_prime_meridian_is_semimajor_axis() {
        let p = to_cartesian(&wgs84(), 0.0, 0.0, 0.0);
        assert!((p.x - 6_378_137.0).abs() < 1e-6);
        assert!(p.y.abs() < 1e-6);
        assert!(p.z.abs() < 1e-6);
    }

    #[test]
    fn north_pole_is_semiminor_axis() {
        let e = wgs84();
        let p = to_cartesian(&e, 0.0, PI_OVER_TWO, 0.0);
        assert!(p.x.abs() < 1e-4);
        assert!(p.y.abs() < 1e-4);
        assert!((p.z - e.radii.z).abs() < 1e-4);
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
            let e = wgs84();
            let p = to_cartesian(&e, lon, lat, h);
            let c = e.cartesian_to_cartographic(p).unwrap();
            assert!((c.longitude - lon).abs() < 1e-10, "lon {lon}");
            assert!((c.latitude - lat).abs() < 1e-10, "lat {lat}");
            assert!((c.height - h).abs() < 1e-3, "height {}", c.height - h);
        }
    }

    fn enu_columns(origin: Cartesian3, e: &Ellipsoid) -> (Cartesian3, Cartesian3, Cartesian3) {
        let m = Transforms::east_north_up_to_fixed_frame(origin, e);
        (
            Cartesian3::new(m.0[0], m.0[1], m.0[2]),
            Cartesian3::new(m.0[4], m.0[5], m.0[6]),
            Cartesian3::new(m.0[8], m.0[9], m.0[10]),
        )
    }

    #[test]
    fn enu_at_equator() {
        let e = wgs84();
        let origin = to_cartesian(&e, 0.0, 0.0, 0.0);
        let (east, north, up) = enu_columns(origin, &e);
        assert!(east.y > 0.999);
        assert!(north.z > 0.999);
        assert!(up.x > 0.999);
    }

    #[test]
    fn enu_at_north_pole() {
        let e = wgs84();
        let origin = to_cartesian(&e, 0.0, PI_OVER_TWO, 0.0);
        let (east, north, up) = enu_columns(origin, &e);
        assert!((east.y - 1.0).abs() < 1e-8);
        assert!((north.x + 1.0).abs() < 1e-6);
        assert!((up.z - 1.0).abs() < 1e-8);
    }

    #[test]
    fn nadir_camera_sits_above_focus() {
        let e = wgs84();
        let frame = look_at(&e, 0.0, 0.0, 0.0, 0.0, -PI_OVER_TWO, 1_000.0, to_radians(60.0), 1.0, 1.0, 10_000_000.0);
        let focus = to_cartesian(&e, 0.0, 0.0, 0.0);
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
        let origin = to_cartesian(&e, 0.0, 0.0, 2_000_000.0);
        let hit = pick_ray(origin, Cartesian3::new(-1.0, 0.0, 0.0), &e).unwrap();
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
        let g =
            Geodesic::new(&wgs84(), to_radians(-74.006), to_radians(40.7128), to_radians(-0.1276), to_radians(51.5074));
        assert!((g.distance - 5_570_000.0).abs() < 30_000.0, "{}", g.distance);
    }

    #[test]
    fn web_mercator_roundtrip_and_max_latitude() {
        let max_lat = WebMercatorProjection::maximum_latitude();
        assert!((max_lat - WebMercatorProjection::mercator_angle_to_geodetic_latitude(PI)).abs() < 1e-15);
        assert!((max_lat.to_degrees() - 85.051_128_78).abs() < 1e-6);
        let projection = WebMercatorProjection::new(wgs84());
        let p = projection.project(&Cartographic::new(0.5, 0.4, 12.0));
        let c = projection.unproject(p);
        assert!((c.longitude - 0.5).abs() < 1e-12);
        assert!((c.latitude - 0.4).abs() < 1e-12);
        assert!((c.height - 12.0).abs() < 1e-9);
        let o = projection.project(&Cartographic::new(0.0, 0.0, 0.0));
        assert!(o.x.abs() < 1e-6 && o.y.abs() < 1e-6);
    }

    #[test]
    fn geographic_level_zero_northwest_tile() {
        let scheme = GeographicTilingScheme::new(wgs84(), crate::core::Rectangle::MAX_VALUE, 2, 1);
        let r = scheme.tile_xy_to_rectangle(0.0, 0.0, 0);
        assert!((r.west + PI).abs() < 1e-12);
        assert!(r.east.abs() < 1e-12);
        assert!((r.north - PI_OVER_TWO).abs() < 1e-12);
        assert!((r.south + PI_OVER_TWO).abs() < 1e-12);
    }

    #[test]
    fn sun_direction_is_unit_and_seasonal() {
        let e = wgs84();
        let d = sun_direction_ecef(2_460_230.5);
        assert!((d.magnitude() - 1.0).abs() < 1e-9);
        let p = e.cartesian_to_cartographic(d.multiply_by_scalar(e.maximum_radius)).unwrap();
        assert!(p.latitude.abs() < 0.45, "subsolar latitude {}", p.latitude);
    }

    fn to_ndc(m: &[f64; 16], p: Cartesian3) -> (f64, f64) {
        let x = m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12];
        let y = m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13];
        let w = m[3] * p.x + m[7] * p.y + m[11] * p.z + m[15];
        (x / w, y / w)
    }

    #[test]
    fn heading_zero_puts_north_up_and_east_right() {
        let e = wgs84();
        for &(lon, lat) in &[(-86.71, 30.05), (-105.08, 40.59), (151.2, -33.87), (0.0, 0.0)] {
            let (lon, lat) = (to_radians(lon), to_radians(lat));
            for &pitch in &[-PI_OVER_TWO, to_radians(-45.0)] {
                let f = look_at(&e, lon, lat, 0.0, 0.0, pitch, 2.0e7, 0.8, 1.5, 1.0, 1.0e9);
                let (cx, cy) = to_ndc(&f.view_projection.0, f.focus);
                let d = to_radians(5.0);
                let (nx, ny) = to_ndc(&f.view_projection.0, to_cartesian(&e, lon, lat + d, 0.0));
                let (ex, _) = to_ndc(&f.view_projection.0, to_cartesian(&e, lon + d, lat, 0.0));
                assert!(cx.abs() < 1e-6 && cy.abs() < 1e-6);
                assert!(ny > cy && (nx - cx).abs() < 1e-6, "north must be up");
                assert!(ex > cx, "east must be right");
            }
        }
    }

    #[test]
    fn globe_uv_matches_equirectangular_image_rows() {
        // Row 0 of an equirectangular image is the north edge and column 0 is
        // the antimeridian, so v = 0 is +90° and u = 0 is −180°.
        let mesh = crate::mesh::build_globe(&wgs84(), 6, 12);
        let cols = 13;
        let vert = |row: usize, col: usize| &mesh.vertices[(row * cols + col) * 8..][..8];
        let north = vert(0, 0);
        assert!(north[2] > 6.3e6 && north[7] == 0.0);
        let south = vert(6, 0);
        assert!(south[2] < -6.3e6 && south[7] == 1.0);
        let greenwich_equator = vert(3, 6);
        assert!(greenwich_equator[0] > 6.3e6 && greenwich_equator[6] == 0.5);
        let east_90 = vert(3, 9);
        assert!(east_90[1] > 6.3e6 && east_90[6] == 0.75);
    }

    #[test]
    fn globe_triangles_face_outward() {
        let mesh = crate::mesh::build_globe(&wgs84(), 8, 16);
        let p = |i: u32| {
            let v = &mesh.vertices[i as usize * 8..][..3];
            Cartesian3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
        };
        for t in mesh.indices.chunks(3) {
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let n = (b - a).cross(c - a);
            if n.magnitude() < 1.0 {
                continue; // collapsed triangle at a pole
            }
            assert!(n.dot(a + b + c) > 0.0, "triangle winds inward");
        }
    }

    #[test]
    fn pick_above_center_lands_north() {
        let e = wgs84();
        let (lon, lat) = (to_radians(-86.71), to_radians(30.05));
        let f = look_at(&e, lon, lat, 0.0, 0.0, -PI_OVER_TWO, 2.0e7, 0.8, 1.5, 1.0, 1.0e9);
        let (o, d) = ray_from_ndc(&f, 0.0, 0.2);
        let up = pick_ray(o, d, &e).unwrap();
        let (o, d) = ray_from_ndc(&f, 0.2, 0.0);
        let right = pick_ray(o, d, &e).unwrap();
        assert!(up.latitude > lat);
        assert!(right.longitude > lon);
    }
    #[test]
    fn tile_grids_are_finite_and_clamp_mercator_focus() {
        let e = wgs84();
        // Geographic level 1 is 4 x 2 tiles; each tile is 4 edges of 2
        // segments, 6 floats per segment.
        let geo = crate::mesh::build_tile_grid(&e, 1, 0, 0.3, 0.2, 0, 2);
        assert_eq!(geo.len(), 8 * 4 * 2 * 6);
        assert!(geo.iter().all(|v| v.is_finite()));
        // Web Mercator level 1 is 2 x 2 tiles; a focus beyond the maximum
        // latitude is clamped into the scheme instead of drawing nothing.
        let wm = crate::mesh::build_tile_grid(&e, 1, 1, 0.3, 1.5, 0, 2);
        assert_eq!(wm.len(), 4 * 4 * 2 * 6);
        assert!(wm.iter().all(|v| v.is_finite()));
        // A windowed grid around the focus draws (2w+1)^2 tiles at most.
        let win = crate::mesh::build_tile_grid(&e, 5, 0, 0.3, 0.2, 1, 2);
        assert_eq!(win.len(), 9 * 4 * 2 * 6);
        assert!(win.iter().all(|v| v.is_finite()));
        // A focus just past the antimeridian wraps instead of falling back to
        // tile (0, 0).
        let past = crate::mesh::build_tile_grid(&e, 5, 0, std::f64::consts::PI + 0.05, 0.2, 1, 2);
        let wrapped = crate::mesh::build_tile_grid(&e, 5, 0, 0.05 - std::f64::consts::PI, 0.2, 1, 2);
        assert_eq!(past, wrapped);
    }

    #[test]
    fn graticule_and_stars_are_finite_and_deterministic() {
        let e = wgs84();
        let graticule = crate::mesh::build_graticule(&e, 30.0, 8);
        assert!(!graticule.is_empty() && graticule.iter().all(|v| v.is_finite()));
        let stars = crate::mesh::build_stars(16, 42);
        assert_eq!(stars.len(), 16 * 4);
        assert_eq!(stars, crate::mesh::build_stars(16, 42));
    }
}
