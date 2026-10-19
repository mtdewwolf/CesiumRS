//! A small stable-Rust benchmark harness (no external dependencies, in
//! keeping with the crate's two-dependency budget).
//!
//! Run it with the native-speed bench profile — the default release
//! profile is size-tuned for the wasm viewer and would understate the
//! numbers:
//!
//! `cargo run --profile bench --example benchmark`
//!
//! Each workload runs for at least ~200 ms and reports median throughput.
//! Treat the output as a regression smoke signal, not a metrology lab:
//! compare runs on the same machine before and after a change.

use cesium_rs::core::{Cartographic, Ellipsoid, EllipsoidGeodesic, JulianDate};
use cesium_rs::mesh::build_globe;
use std::hint::black_box;
use std::time::{Duration, Instant};

fn bench(name: &str, mut workload: impl FnMut() -> f64) -> f64 {
    // Warm up, then take the best of several timed batches.
    let mut sink = workload();
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let mut iterations: u64 = 1;
        let elapsed = loop {
            let start = Instant::now();
            for _ in 0..iterations {
                sink += workload();
            }
            let e = start.elapsed();
            if e >= Duration::from_millis(200) || iterations > (1 << 26) {
                break e;
            }
            iterations *= 4;
        };
        let per_op = elapsed.as_secs_f64() / iterations as f64;
        best = best.min(per_op);
    }
    println!("{name:<44} {:>10.0} ops/s  ({:.2} µs/op)", 1.0 / best, best * 1e6);
    sink
}

fn main() {
    let ellipsoid = Ellipsoid::WGS84;
    let a = Cartographic::from_degrees(-74.006, 40.7128, 0.0);
    let b = Cartographic::from_degrees(-0.1276, 51.5074, 0.0);
    let geodesic = EllipsoidGeodesic::new(&a, &b, &ellipsoid).unwrap();
    let mut sink = 0.0;

    sink += bench("EllipsoidGeodesic::new (NY-London)", || {
        black_box(EllipsoidGeodesic::new(black_box(&a), black_box(&b), &ellipsoid).unwrap().surface_distance)
    });
    sink += bench("geodesic interpolate_using_fraction", || {
        black_box(geodesic.interpolate_using_fraction(black_box(0.37)).longitude)
    });
    sink += bench("cartographic_to_cartesian", || black_box(ellipsoid.cartographic_to_cartesian(black_box(&a)).x));
    sink += bench("cartesian_to_cartographic", || {
        let p = ellipsoid.cartographic_to_cartesian(&a);
        black_box(ellipsoid.cartesian_to_cartographic(black_box(p)).unwrap().height)
    });
    sink += bench("JulianDate UTC -> TAI round trip", || {
        let d = JulianDate::from_iso8601(black_box("2026-10-18T12:34:56Z")).unwrap();
        black_box(d.compute_tai_minus_utc())
    });
    sink +=
        bench("build_globe 64x128 (viewer mesh)", || black_box(build_globe(&ellipsoid, 64, 128).vertices.len() as f64));

    println!("(sink: {sink:.3})");
}
