//! Web Mercator and the two Cesium tiling schemes.

use crate::math::{clamp, PI, PI_OVER_TWO, TAU};

/// Cesium `WebMercatorProjection.mercatorAngleToGeodeticLatitude`.
pub fn mercator_angle_to_geodetic_latitude(mercator_angle: f64) -> f64 {
    PI_OVER_TWO - 2.0 * (-mercator_angle).exp().atan()
}

/// Latitude where the Web Mercator square is cut off. Cesium `MaximumLatitude`.
pub fn maximum_latitude() -> f64 {
    mercator_angle_to_geodetic_latitude(PI)
}

/// Cesium `WebMercatorProjection.geodeticLatitudeToMercatorAngle`.
pub fn geodetic_latitude_to_mercator_angle(latitude: f64) -> f64 {
    let max = maximum_latitude();
    let latitude = clamp(latitude, -max, max);
    let s = latitude.sin();
    0.5 * ((1.0 + s) / (1.0 - s)).ln()
}

pub fn project(longitude: f64, latitude: f64, height: f64, semimajor: f64) -> (f64, f64, f64) {
    (longitude * semimajor, geodetic_latitude_to_mercator_angle(latitude) * semimajor, height)
}

pub fn unproject(x: f64, y: f64, z: f64, semimajor: f64) -> (f64, f64, f64) {
    let inv = 1.0 / semimajor;
    (x * inv, mercator_angle_to_geodetic_latitude(y * inv), z)
}

#[derive(Clone, Copy, Debug)]
pub struct Rectangle {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

/// Cesium `GeographicTilingScheme`: 2×1 level-zero tiles over the whole ellipsoid.
pub fn geographic_tile_count(level: u32) -> (u32, u32) {
    (2 << level, 1 << level)
}

pub fn geographic_tile_rectangle(x: u32, y: u32, level: u32) -> Rectangle {
    let (nx, ny) = geographic_tile_count(level);
    let x_width = TAU / f64::from(nx);
    let y_height = PI / f64::from(ny);
    let west = -PI + f64::from(x) * x_width;
    let north = PI_OVER_TWO - f64::from(y) * y_height;
    Rectangle { west, south: north - y_height, east: west + x_width, north }
}

pub fn geographic_tile_of(longitude: f64, latitude: f64, level: u32) -> (u32, u32) {
    let (nx, ny) = geographic_tile_count(level);
    let mut lon = longitude;
    if lon < -PI {
        lon = -PI;
    }
    if lon >= PI {
        lon = PI - f64::EPSILON;
    }
    let mut lat = latitude.clamp(-PI_OVER_TWO, PI_OVER_TWO);
    if lat >= PI_OVER_TWO {
        lat = PI_OVER_TWO - f64::EPSILON;
    }
    let x = ((lon + PI) / TAU * f64::from(nx)).floor() as u32;
    let y = ((PI_OVER_TWO - lat) / PI * f64::from(ny)).floor() as u32;
    (x.min(nx - 1), y.min(ny - 1))
}

/// Cesium `WebMercatorTilingScheme`: `2^level` square tiles in Mercator meters.
pub fn web_mercator_tile_count(level: u32) -> (u32, u32) {
    let n = 1 << level;
    (n, n)
}

pub fn web_mercator_tile_rectangle(x: u32, y: u32, level: u32, semimajor: f64) -> Rectangle {
    let n = f64::from(1u32 << level);
    let span = TAU * semimajor;
    let tile = span / n;
    let west_m = -PI * semimajor + f64::from(x) * tile;
    let north_m = PI * semimajor - f64::from(y) * tile;
    let (west, north, _) = unproject(west_m, north_m, 0.0, semimajor);
    let (east, south, _) = unproject(west_m + tile, north_m - tile, 0.0, semimajor);
    Rectangle { west, south, east, north }
}

/// Coarser when the camera is far. Capped so a globe view stays readable.
pub fn suggested_tile_level(range: f64, semimajor: f64) -> u32 {
    let ratio = (semimajor / range.max(1.0)).max(0.25);
    let level = ratio.log2().floor().max(0.0) as u32;
    level.min(5)
}
