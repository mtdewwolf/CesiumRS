//! The viewer camera: frame a place with a heading/pitch/range look-at,
//! then pick the ellipsoid through the center of the frame and above it.
//!
//! Run with: `cargo run --example camera_and_picking`

use cesium_rs::camera::ray_from_ndc;
use cesium_rs::core::math::{to_degrees, to_radians};
use cesium_rs::core::Ellipsoid;
use cesium_rs::{look_at, pick_ray};

fn main() {
    let ellipsoid = Ellipsoid::WGS84;
    // Fort Walton Beach, Florida — the viewer's default focus.
    let (lon, lat) = (to_radians(-86.71), to_radians(30.05));
    let frame = look_at(
        &ellipsoid,
        lon,
        lat,
        0.0,
        0.0,
        to_radians(-90.0),
        20_000_000.0,
        to_radians(60.0),
        16.0 / 9.0,
        1.0,
        1.0e9,
    );
    println!("eye: {:.0} m ECEF ({:.0}, {:.0}, {:.0})", frame.eye.magnitude(), frame.eye.x, frame.eye.y, frame.eye.z);

    for (name, ndc) in [("center", (0.0, 0.0)), ("above center", (0.0, 0.2)), ("right of center", (0.2, 0.0))] {
        let (origin, direction) = ray_from_ndc(&frame, ndc.0, ndc.1);
        match pick_ray(origin, direction, &ellipsoid) {
            Some(hit) => println!(
                "{name:<16} -> {:.3}° lat, {:.3}° lon at {:.0} km along the ray",
                to_degrees(hit.latitude),
                to_degrees(hit.longitude),
                hit.t / 1000.0
            ),
            None => println!("{name:<16} -> misses the ellipsoid"),
        }
    }
}
