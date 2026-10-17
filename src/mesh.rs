//! Globe, graticule, geodesic, and tile meshes in ECEF.

use crate::ellipsoid::{cartographic_to_cartesian, geodetic_surface_normal_cartographic, Ellipsoid};
use crate::geodesic::Geodesic;
use crate::math::{to_radians, PI, PI_OVER_TWO, TAU};
use crate::projection::{
    geographic_tile_count, geographic_tile_of, geographic_tile_rectangle, web_mercator_tile_count,
    web_mercator_tile_rectangle,
};

#[derive(Clone, Debug)]
pub struct GlobeMesh {
    /// Interleaved `px, py, pz, nx, ny, nz, u, v`.
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
}

pub fn build_globe(ellipsoid: &Ellipsoid, lat_segments: u32, lon_segments: u32) -> GlobeMesh {
    let lat_segments = lat_segments.max(2);
    let lon_segments = lon_segments.max(3);
    let rows = lat_segments + 1;
    let cols = lon_segments + 1;
    let mut vertices = Vec::with_capacity((rows * cols) as usize * 8);
    for row in 0..rows {
        let v = f64::from(row) / f64::from(lat_segments);
        let lat = PI_OVER_TWO - v * PI;
        for col in 0..cols {
            let u = f64::from(col) / f64::from(lon_segments);
            let lon = -PI + u * TAU;
            let p = cartographic_to_cartesian(ellipsoid, lon, lat, 0.0);
            let n = geodetic_surface_normal_cartographic(lon, lat);
            vertices.extend_from_slice(&[
                p.x as f32,
                p.y as f32,
                p.z as f32,
                n.x as f32,
                n.y as f32,
                n.z as f32,
                u as f32,
                v as f32,
            ]);
        }
    }
    let mut indices = Vec::with_capacity((lat_segments * lon_segments * 6) as usize);
    for row in 0..lat_segments {
        for col in 0..lon_segments {
            let i = row * cols + col;
            indices.extend_from_slice(&[i, i + cols, i + 1, i + 1, i + cols, i + cols + 1]);
        }
    }
    GlobeMesh { vertices, indices }
}

/// Line segments on the ellipsoid, packed `x, y, z` pairs for `GL_LINES`.
pub fn build_graticule(ellipsoid: &Ellipsoid, step_deg: f64, segments: u32) -> Vec<f32> {
    let step = to_radians(step_deg.max(1.0));
    let segments = segments.max(4);
    let mut out = Vec::new();
    let mut lon = -PI;
    while lon < PI - 1e-9 {
        push_meridian(&mut out, ellipsoid, lon, segments);
        lon += step;
    }
    let mut lat = -PI_OVER_TWO + step;
    while lat < PI_OVER_TWO - 1e-8 {
        push_parallel(&mut out, ellipsoid, lat, segments * 2);
        lat += step;
    }
    out
}

fn push_meridian(out: &mut Vec<f32>, ellipsoid: &Ellipsoid, lon: f64, segments: u32) {
    let mut prev = cartographic_to_cartesian(ellipsoid, lon, -PI_OVER_TWO, 0.0);
    for i in 1..=segments {
        let lat = -PI_OVER_TWO + PI * f64::from(i) / f64::from(segments);
        let p = cartographic_to_cartesian(ellipsoid, lon, lat, 0.0);
        push_seg(out, prev, p);
        prev = p;
    }
}

fn push_parallel(out: &mut Vec<f32>, ellipsoid: &Ellipsoid, lat: f64, segments: u32) {
    let mut prev = cartographic_to_cartesian(ellipsoid, -PI, lat, 0.0);
    for i in 1..=segments {
        let lon = -PI + TAU * f64::from(i) / f64::from(segments);
        let p = cartographic_to_cartesian(ellipsoid, lon, lat, 0.0);
        push_seg(out, prev, p);
        prev = p;
    }
}

fn push_seg(out: &mut Vec<f32>, a: crate::math::Vec3, b: crate::math::Vec3) {
    out.extend_from_slice(&[
        a.x as f32, a.y as f32, a.z as f32, b.x as f32, b.y as f32, b.z as f32,
    ]);
}

pub fn build_geodesic_arc(
    ellipsoid: &Ellipsoid,
    lon1: f64,
    lat1: f64,
    lon2: f64,
    lat2: f64,
    segments: u32,
) -> Vec<f32> {
    let segments = segments.max(2);
    let geo = Geodesic::new(ellipsoid, lon1, lat1, lon2, lat2);
    let mut out = Vec::with_capacity(segments as usize * 6);
    let mut prev = cartographic_to_cartesian(ellipsoid, lon1, lat1, 0.0);
    for i in 1..=segments {
        let (lon, lat) = geo.interpolate(f64::from(i) / f64::from(segments));
        let p = cartographic_to_cartesian(ellipsoid, lon, lat, 0.0);
        push_seg(&mut out, prev, p);
        prev = p;
    }
    out
}

/// `scheme` 0 = geographic, 1 = web mercator.
/// `window` 0 draws every tile; otherwise a `(2*window+1)` neighborhood of the focus.
pub fn build_tile_grid(
    ellipsoid: &Ellipsoid,
    level: u32,
    scheme: u32,
    focus_lon: f64,
    focus_lat: f64,
    window: u32,
    edge_segments: u32,
) -> Vec<f32> {
    let level = level.min(6);
    let edge_segments = edge_segments.max(1);
    let (nx, ny) = if scheme == 1 {
        web_mercator_tile_count(level)
    } else {
        geographic_tile_count(level)
    };
    let (cx, cy) = if scheme == 0 {
        geographic_tile_of(focus_lon, focus_lat, level)
    } else {
        let n = f64::from(nx);
        let x = ((focus_lon + PI) / TAU * n).floor() as u32;
        let max_lat = crate::projection::maximum_latitude();
        let lat = focus_lat.clamp(-max_lat, max_lat);
        let y_m = crate::projection::geodetic_latitude_to_mercator_angle(lat) * ellipsoid.maximum_radius;
        let north = PI * ellipsoid.maximum_radius;
        let span = TAU * ellipsoid.maximum_radius;
        let y = ((north - y_m) / (span / n)).floor() as u32;
        (x.min(nx - 1), y.min(ny - 1))
    };

    let mut out = Vec::new();
    let (x0, _x1, y0, y1) = if window == 0 || nx * ny <= 64 {
        (0, nx, 0, ny)
    } else {
        let w = window;
        let y_start = cy.saturating_sub(w);
        let y_end = (cy + w + 1).min(ny);
        (cx.wrapping_sub(w), cx.wrapping_add(w + 1), y_start, y_end)
    };
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        let count = if window == 0 || nx * ny <= 64 {
            nx
        } else {
            window * 2 + 1
        };
        for _ in 0..count {
            let tx = x % nx;
            let rect = if scheme == 1 {
                web_mercator_tile_rectangle(tx, y, level, ellipsoid.maximum_radius)
            } else {
                geographic_tile_rectangle(tx, y, level)
            };
            push_rect(&mut out, ellipsoid, rect, edge_segments);
            x = x.wrapping_add(1);
        }
        y += 1;
    }
    out
}

fn push_rect(
    out: &mut Vec<f32>,
    ellipsoid: &Ellipsoid,
    rect: crate::projection::Rectangle,
    segments: u32,
) {
    push_edge(out, ellipsoid, rect.west, rect.south, rect.east, rect.south, segments);
    push_edge(out, ellipsoid, rect.east, rect.south, rect.east, rect.north, segments);
    push_edge(out, ellipsoid, rect.east, rect.north, rect.west, rect.north, segments);
    push_edge(out, ellipsoid, rect.west, rect.north, rect.west, rect.south, segments);
}

fn push_edge(
    out: &mut Vec<f32>,
    ellipsoid: &Ellipsoid,
    lon0: f64,
    lat0: f64,
    lon1: f64,
    lat1: f64,
    segments: u32,
) {
    let mut prev = cartographic_to_cartesian(ellipsoid, lon0, lat0, 0.0);
    for i in 1..=segments {
        let t = f64::from(i) / f64::from(segments);
        let p = cartographic_to_cartesian(
            ellipsoid,
            lon0 + (lon1 - lon0) * t,
            lat0 + (lat1 - lat0) * t,
            0.0,
        );
        push_seg(out, prev, p);
        prev = p;
    }
}

pub fn build_stars(count: u32, seed: u32) -> Vec<f32> {
    let mut state = seed | 1;
    let mut next = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state >> 8) as f32 / 16_777_216.0
    };
    let mut out = Vec::with_capacity(count as usize * 4);
    for _ in 0..count {
        let u = next() as f64;
        let v = next() as f64;
        let theta = TAU * u;
        let z = 1.0 - 2.0 * v;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let brightness = 0.35 + next() as f64 * 0.65;
        out.extend_from_slice(&[
            (r * theta.cos()) as f32,
            (r * theta.sin()) as f32,
            z as f32,
            brightness as f32,
        ]);
    }
    out
}
