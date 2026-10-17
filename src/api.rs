//! C ABI used by the browser globe. Pointers are valid until the next call
//! that writes the same buffer.

use crate::camera::{look_at, ray_from_ndc, CameraFrame};
use crate::ellipsoid::{cartesian_to_cartographic, cartographic_to_cartesian, Ellipsoid};
use crate::geodesic::Geodesic;
use crate::intersect::pick_ray;
use crate::math::{to_radians, Vec3};
use crate::mesh::{build_geodesic_arc, build_globe, build_graticule, build_stars, build_tile_grid};
use crate::projection::suggested_tile_level;
use crate::sun::sun_direction_ecef;
use std::cell::RefCell;

struct Scratch {
    f64s: Vec<f64>,
    f32s: Vec<f32>,
    u32s: Vec<u32>,
    camera: Option<CameraFrame>,
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch {
        f64s: Vec::new(),
        f32s: Vec::new(),
        u32s: Vec::new(),
        camera: None,
    });
}

fn ellipsoid() -> Ellipsoid {
    Ellipsoid::wgs84()
}

fn write_f64(values: &[f64]) {
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        s.f64s.clear();
        s.f64s.extend_from_slice(values);
    });
}

#[no_mangle]
pub extern "C" fn f64_ptr() -> *const f64 {
    SCRATCH.with(|s| s.borrow().f64s.as_ptr())
}

#[no_mangle]
pub extern "C" fn f64_len() -> u32 {
    SCRATCH.with(|s| s.borrow().f64s.len() as u32)
}

#[no_mangle]
pub extern "C" fn f32_ptr() -> *const f32 {
    SCRATCH.with(|s| s.borrow().f32s.as_ptr())
}

#[no_mangle]
pub extern "C" fn f32_len() -> u32 {
    SCRATCH.with(|s| s.borrow().f32s.len() as u32)
}

#[no_mangle]
pub extern "C" fn u32_ptr() -> *const u32 {
    SCRATCH.with(|s| s.borrow().u32s.as_ptr())
}

#[no_mangle]
pub extern "C" fn u32_len() -> u32 {
    SCRATCH.with(|s| s.borrow().u32s.len() as u32)
}

fn pack_mat(out: &mut Vec<f64>, m: &[f64; 16]) {
    out.extend_from_slice(m);
}

#[no_mangle]
pub extern "C" fn camera_look_at(
    focus_lon: f64,
    focus_lat: f64,
    focus_height: f64,
    heading: f64,
    pitch: f64,
    range: f64,
    fov_y: f64,
    aspect: f64,
    near: f64,
    far: f64,
) {
    let frame =
        look_at(&ellipsoid(), focus_lon, focus_lat, focus_height, heading, pitch, range, fov_y, aspect, near, far);
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        s.f64s.clear();
        pack_mat(&mut s.f64s, &frame.view);
        pack_mat(&mut s.f64s, &frame.projection);
        pack_mat(&mut s.f64s, &frame.view_projection);
        s.f64s.extend_from_slice(&[
            frame.eye.x,
            frame.eye.y,
            frame.eye.z,
            frame.direction.x,
            frame.direction.y,
            frame.direction.z,
            frame.up.x,
            frame.up.y,
            frame.up.z,
            frame.right.x,
            frame.right.y,
            frame.right.z,
            frame.focus.x,
            frame.focus.y,
            frame.focus.z,
        ]);
        s.camera = Some(frame);
    });
}

#[no_mangle]
pub extern "C" fn pick_ndc(ndc_x: f64, ndc_y: f64) {
    let frame = SCRATCH.with(|s| s.borrow().camera);
    let Some(frame) = frame else {
        write_f64(&[0.0]);
        return;
    };
    let (origin, direction) = ray_from_ndc(&frame, ndc_x, ndc_y);
    match pick_ray(origin, direction, &ellipsoid()) {
        Some(hit) => write_f64(&[
            1.0,
            hit.t,
            hit.longitude,
            hit.latitude,
            hit.height,
            hit.position.x,
            hit.position.y,
            hit.position.z,
        ]),
        None => write_f64(&[0.0]),
    }
}

#[no_mangle]
pub extern "C" fn llh_to_ecef(lon: f64, lat: f64, height: f64) {
    let p = cartographic_to_cartesian(&ellipsoid(), lon, lat, height);
    write_f64(&[p.x, p.y, p.z]);
}

#[no_mangle]
pub extern "C" fn ecef_to_llh(x: f64, y: f64, z: f64) {
    match cartesian_to_cartographic(&ellipsoid(), Vec3::new(x, y, z)) {
        Some(c) => write_f64(&[1.0, c.longitude, c.latitude, c.height]),
        None => write_f64(&[0.0]),
    }
}

#[no_mangle]
pub extern "C" fn geodesic_info(lon1: f64, lat1: f64, lon2: f64, lat2: f64) {
    let g = Geodesic::new(&ellipsoid(), lon1, lat1, lon2, lat2);
    write_f64(&[g.distance, g.start_heading, g.end_heading]);
}

#[no_mangle]
pub extern "C" fn sun_direction(jd: f64) {
    let d = sun_direction_ecef(jd);
    let p = d * ellipsoid().maximum_radius;
    match cartesian_to_cartographic(&ellipsoid(), p) {
        Some(c) => write_f64(&[d.x, d.y, d.z, c.longitude, c.latitude]),
        None => write_f64(&[d.x, d.y, d.z, 0.0, 0.0]),
    }
}

#[no_mangle]
pub extern "C" fn suggested_level(range: f64) -> u32 {
    suggested_tile_level(range, ellipsoid().maximum_radius)
}

#[no_mangle]
pub extern "C" fn build_globe_mesh(lat_segments: u32, lon_segments: u32) {
    let mesh = build_globe(&ellipsoid(), lat_segments, lon_segments);
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        s.f32s = mesh.vertices;
        s.u32s = mesh.indices;
    });
}

#[no_mangle]
pub extern "C" fn build_graticule_mesh(step_deg: f64) {
    let lines = build_graticule(&ellipsoid(), step_deg, 48);
    SCRATCH.with(|s| {
        s.borrow_mut().f32s = lines;
    });
}

#[no_mangle]
pub extern "C" fn build_geodesic_mesh(lon1: f64, lat1: f64, lon2: f64, lat2: f64, segments: u32) {
    let lines = build_geodesic_arc(&ellipsoid(), lon1, lat1, lon2, lat2, segments);
    SCRATCH.with(|s| {
        s.borrow_mut().f32s = lines;
    });
}

#[no_mangle]
pub extern "C" fn build_tile_mesh(level: u32, scheme: u32, focus_lon: f64, focus_lat: f64, window: u32) {
    // Keep each edge chord under ~3° so outlines do not sink into the globe.
    let edge_segments = (64u32 >> level.min(6)).max(4);
    let lines = build_tile_grid(&ellipsoid(), level, scheme, focus_lon, focus_lat, window, edge_segments);
    SCRATCH.with(|s| {
        s.borrow_mut().f32s = lines;
    });
}

#[no_mangle]
pub extern "C" fn build_star_mesh(count: u32, seed: u32) {
    let stars = build_stars(count, seed);
    SCRATCH.with(|s| {
        s.borrow_mut().f32s = stars;
    });
}

/// Degrees helper so the JS host can stay in radians. Not required, but handy.
#[no_mangle]
pub extern "C" fn radians(degrees: f64) -> f64 {
    to_radians(degrees)
}
