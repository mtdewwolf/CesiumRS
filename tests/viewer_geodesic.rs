use cesium_rs::mesh::build_geodesic_arc;
use cesium_rs::{Ellipsoid, Geodesic};

#[test]
fn spherical_fallback_preserves_endpoints_and_draws_antipodal_routes() {
    let e = Ellipsoid::wgs84();
    for (lon, lat, end_lon, end_lat) in [
        (0.0, 0.0, std::f64::consts::PI, 0.0),
        (0.3, 0.7, 0.3 + std::f64::consts::PI, -0.7),
        (0.3, 0.7, 0.3 + std::f64::consts::PI - 0.001, -0.7),
        (0.3, 0.7, 0.3, 0.7),
    ] {
        let g = Geodesic::new(&e, lon, lat, end_lon, end_lat);
        for (fraction, expected_lon, expected_lat) in [(0.0, lon, lat), (1.0, end_lon, end_lat)] {
            let (actual_lon, actual_lat) = g.interpolate(fraction);
            assert!((actual_lon - expected_lon).sin().abs() < 1e-12);
            assert!((actual_lon - expected_lon).cos() > 0.999999999999);
            assert!((actual_lat - expected_lat).abs() < 1e-12);
        }
        let arc = build_geodesic_arc(&e, lon, lat, end_lon, end_lat, 32);
        assert!(arc.iter().all(|v| v.is_finite()));
        let expected = cesium_rs::cartographic_to_cartesian(&e, end_lon, end_lat, 0.0);
        for (actual, expected) in arc[arc.len() - 3..].iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*actual) - expected).abs() < 0.5);
        }
        if g.distance > 1.0 {
            let middle = &arc[arc.len() / 2..][..3];
            assert!(middle.iter().zip(&arc[..3]).any(|(a, b)| (a - b).abs() > 1e6));
        } else {
            for point in arc.as_chunks::<3>().0 {
                assert_eq!(point, &arc[..3]);
            }
        }
    }
}
