//! Shortest paths on the WGS84 ellipsoid with Core's `EllipsoidGeodesic`,
//! plus the viewer's fallback for a near-antipodal route Core rejects.
//!
//! Run with: `cargo run --example geodesic_route`

use cesium_rs::core::math::to_degrees;
use cesium_rs::core::{Cartographic, Ellipsoid, EllipsoidGeodesic};
use cesium_rs::Geodesic;

fn main() {
    let ellipsoid = Ellipsoid::WGS84;

    let new_york = Cartographic::from_degrees(-74.006, 40.7128, 0.0);
    let london = Cartographic::from_degrees(-0.1276, 51.5074, 0.0);
    let route = EllipsoidGeodesic::new(&new_york, &london, &ellipsoid).expect("not near-antipodal");
    println!(
        "New York -> London: {:.1} km, start heading {:.1}°, end heading {:.1}°",
        route.surface_distance / 1000.0,
        to_degrees(route.start_heading),
        to_degrees(route.end_heading)
    );
    for i in 0..=4 {
        let p = route.interpolate_using_fraction(f64::from(i) / 4.0);
        println!(
            "  {:>4.0}%: {:.3}° lon, {:.3}° lat",
            f64::from(i) * 25.0,
            to_degrees(p.longitude),
            to_degrees(p.latitude)
        );
    }

    // A route to within 0.0125 rad of Quito's antipode is nearly antipodal:
    // Vincenty's method may not converge there, Core returns None, and the
    // viewer's great-circle fallback still draws the route.
    let quito = Cartographic::from_degrees(-78.6167, -0.15, 0.0);
    let near_antipode = Cartographic::from_degrees(101.5, 0.3, 0.0);
    assert!(EllipsoidGeodesic::new(&quito, &near_antipode, &ellipsoid).is_none());
    let fallback =
        Geodesic::new(&ellipsoid, quito.longitude, quito.latitude, near_antipode.longitude, near_antipode.latitude);
    let mid = fallback.interpolate(0.5);
    println!(
        "Quito -> near-antipode (fallback): {:.1} km, midpoint {:.3}° lon, {:.3}° lat",
        fallback.distance / 1000.0,
        to_degrees(mid.0),
        to_degrees(mid.1)
    );
}
