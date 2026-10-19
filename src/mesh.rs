//! Globe, graticule, geodesic, and tile meshes in ECEF, built on `core`:
//! positions and normals from `Ellipsoid`, tile rectangles and lookup from
//! the `GeographicTilingScheme` / `WebMercatorTilingScheme` implementations
//! of Core's `TilingScheme` trait.

use crate::core::math::to_radians;
use crate::core::{
    Cartesian3, Cartographic, Ellipsoid, GeographicTilingScheme, Rectangle, TilingScheme, WebMercatorProjection,
    WebMercatorTilingScheme,
};
use crate::geodesic::Geodesic;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn to_cartesian(ellipsoid: &Ellipsoid, lon: f64, lat: f64) -> Cartesian3 {
    ellipsoid.cartographic_to_cartesian(&Cartographic::new(lon, lat, 0.0))
}

#[derive(Clone, Debug)]
pub struct GlobeMesh {
    /// Interleaved `px, py, pz, nx, ny, nz, u, v`.
    ///
    /// `u = 0` is −180° and `v = 0` is the north pole, matching an
    /// equirectangular image whose first row is the north edge. Upload that
    /// image with `UNPACK_FLIP_Y_WEBGL` off; turning it on (or using `1 - v`)
    /// mirrors the globe north–south.
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
        let lat = FRAC_PI_2 - v * PI;
        for col in 0..cols {
            let u = f64::from(col) / f64::from(lon_segments);
            let lon = -PI + u * TAU;
            let p = to_cartesian(ellipsoid, lon, lat);
            let n = ellipsoid.geodetic_surface_normal_cartographic(&Cartographic::new(lon, lat, 0.0));
            vertices.extend_from_slice(&[
                p.x as f32, p.y as f32, p.z as f32, n.x as f32, n.y as f32, n.z as f32, u as f32, v as f32,
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
    let mut lat = -FRAC_PI_2 + step;
    while lat < FRAC_PI_2 - 1e-8 {
        push_parallel(&mut out, ellipsoid, lat, segments * 2);
        lat += step;
    }
    out
}

fn push_meridian(out: &mut Vec<f32>, ellipsoid: &Ellipsoid, lon: f64, segments: u32) {
    let mut prev = to_cartesian(ellipsoid, lon, -FRAC_PI_2);
    for i in 1..=segments {
        let lat = -FRAC_PI_2 + PI * f64::from(i) / f64::from(segments);
        let p = to_cartesian(ellipsoid, lon, lat);
        push_seg(out, prev, p);
        prev = p;
    }
}

fn push_parallel(out: &mut Vec<f32>, ellipsoid: &Ellipsoid, lat: f64, segments: u32) {
    let mut prev = to_cartesian(ellipsoid, -PI, lat);
    for i in 1..=segments {
        let lon = -PI + TAU * f64::from(i) / f64::from(segments);
        let p = to_cartesian(ellipsoid, lon, lat);
        push_seg(out, prev, p);
        prev = p;
    }
}

fn push_seg(out: &mut Vec<f32>, a: Cartesian3, b: Cartesian3) {
    out.extend_from_slice(&[a.x as f32, a.y as f32, a.z as f32, b.x as f32, b.y as f32, b.z as f32]);
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
    let mut prev = to_cartesian(ellipsoid, lon1, lat1);
    for i in 1..=segments {
        let (lon, lat) = geo.interpolate(f64::from(i) / f64::from(segments));
        let p = to_cartesian(ellipsoid, lon, lat);
        push_seg(&mut out, prev, p);
        prev = p;
    }
    out
}

/// Coarser tile level when the camera is far. Capped so a globe view stays
/// readable. A viewer heuristic; Core has no equivalent.
pub fn suggested_tile_level(range: f64, semimajor: f64) -> u32 {
    let ratio = (semimajor / range.max(1.0)).max(0.25);
    let level = ratio.log2().floor().max(0.0) as u32;
    level.min(5)
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
    let geographic = GeographicTilingScheme::new(*ellipsoid, Rectangle::MAX_VALUE, 2, 1);
    let mercator = WebMercatorTilingScheme::new(*ellipsoid, 1, 1, None);
    let tiling: &dyn TilingScheme = if scheme == 1 { &mercator } else { &geographic };
    let nx = tiling.get_number_of_x_tiles_at_level(level).max(0) as u32;
    let ny = tiling.get_number_of_y_tiles_at_level(level).max(0) as u32;

    // The Web Mercator scheme has no tiles beyond its maximum latitude;
    // clamp the focus into it, as the viewer always draws around the focus.
    let focus_lat = if scheme == 1 {
        let max_lat = WebMercatorProjection::maximum_latitude();
        focus_lat.clamp(-max_lat, max_lat)
    } else {
        focus_lat.clamp(-FRAC_PI_2, FRAC_PI_2)
    };
    let focus = Cartographic::new(focus_lon, focus_lat, 0.0);
    let (cx, cy) = match tiling.position_to_tile_xy(&focus, level) {
        Some(xy) => (xy.x.max(0.0) as u32, xy.y.max(0.0) as u32),
        None => (0, 0),
    };

    let mut out = Vec::new();
    let (x0, y0, y1) = if window == 0 || nx * ny <= 64 {
        (0, 0, ny)
    } else {
        let w = window;
        let y_start = cy.saturating_sub(w);
        let y_end = (cy + w + 1).min(ny);
        (cx.wrapping_sub(w), y_start, y_end)
    };
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        let count = if window == 0 || nx * ny <= 64 { nx } else { window * 2 + 1 };
        for _ in 0..count {
            let tx = x % nx;
            let rect = tiling.tile_xy_to_rectangle(f64::from(tx), f64::from(y), level);
            push_rect(&mut out, ellipsoid, rect, edge_segments);
            x = x.wrapping_add(1);
        }
        y += 1;
    }
    out
}

fn push_rect(out: &mut Vec<f32>, ellipsoid: &Ellipsoid, rect: Rectangle, segments: u32) {
    push_edge(out, ellipsoid, rect.west, rect.south, rect.east, rect.south, segments);
    push_edge(out, ellipsoid, rect.east, rect.south, rect.east, rect.north, segments);
    push_edge(out, ellipsoid, rect.east, rect.north, rect.west, rect.north, segments);
    push_edge(out, ellipsoid, rect.west, rect.north, rect.west, rect.south, segments);
}

fn push_edge(out: &mut Vec<f32>, ellipsoid: &Ellipsoid, lon0: f64, lat0: f64, lon1: f64, lat1: f64, segments: u32) {
    let mut prev = to_cartesian(ellipsoid, lon0, lat0);
    for i in 1..=segments {
        let t = f64::from(i) / f64::from(segments);
        let p = to_cartesian(ellipsoid, lon0 + (lon1 - lon0) * t, lat0 + (lat1 - lat0) * t);
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
        out.extend_from_slice(&[(r * theta.cos()) as f32, (r * theta.sin()) as f32, z as f32, brightness as f32]);
    }
    out
}
