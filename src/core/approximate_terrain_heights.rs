//! CesiumJS ApproximateTerrainHeights, with a bundled lookup table replacing
//! the browser resource fetch. Owned initialization also accepts supplied JSON.
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::math::{js_max, js_min};
use super::rectangle::Rectangle;
use super::runtime_error::RuntimeError;
use super::tiling_scheme::{GeographicTilingScheme, TilingScheme};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub const DEFAULT_MIN_TERRAIN_HEIGHT: f64 = -100000.0;
pub const DEFAULT_MAX_TERRAIN_HEIGHT: f64 = 9000.0;
pub const TERRAIN_HEIGHTS_MAX_LEVEL: u32 = 6;

#[derive(Clone, Debug)]
pub struct ApproximateTerrainHeights {
    terrain_heights: Option<Arc<HashMap<String, [f64; 2]>>>,
    // Out-of-scheme corners leave Cesium's scratch tile coordinate unchanged.
    // Keep that state explicitly; the mutex also supports shared native use.
    scratch_tile: Arc<Mutex<Cartesian2>>,
}
impl Default for ApproximateTerrainHeights {
    fn default() -> Self {
        Self::new()
    }
}
impl ApproximateTerrainHeights {
    pub fn new() -> Self {
        Self { terrain_heights: None, scratch_tile: Arc::new(Mutex::new(Cartesian2::ZERO)) }
    }
    pub fn initialized(&self) -> bool {
        self.terrain_heights.is_some()
    }
    /// Repeat initialization reuses the first successful table, as Cesium does.
    pub fn initialize(&mut self) -> Result<(), RuntimeError> {
        self.initialize_from_json(include_str!("../../assets/approximateTerrainHeights.json"))
    }
    pub fn initialize_from_json(&mut self, json: &str) -> Result<(), RuntimeError> {
        if self.initialized() {
            return Ok(());
        }
        let table = serde_json::from_str(json).map_err(|e| RuntimeError::new(e.to_string()))?;
        self.terrain_heights = Some(Arc::new(table));
        Ok(())
    }
    pub fn bundled() -> &'static Self {
        static HEIGHTS: OnceLock<ApproximateTerrainHeights> = OnceLock::new();
        HEIGHTS.get_or_init(|| {
            let mut heights = Self::new();
            heights.initialize().expect("bundled Cesium terrain heights");
            heights
        })
    }
    pub fn get_minimum_maximum_heights(&self, rectangle: &Rectangle, ellipsoid: &Ellipsoid) -> Option<(f64, f64)> {
        let table = self.terrain_heights.as_ref()?;
        let (mut min, mut max) = (DEFAULT_MIN_TERRAIN_HEIGHT, DEFAULT_MAX_TERRAIN_HEIGHT);
        if let Some(key) = self.tile_key(rectangle) {
            if let Some(heights) = table.get(&key) {
                min = heights[0];
                max = heights[1];
            }
            let ne = ellipsoid.cartographic_to_cartesian(&Cartographic::new(rectangle.east, rectangle.north, 0.0));
            let sw = ellipsoid.cartographic_to_cartesian(&Cartographic::new(rectangle.west, rectangle.south, 0.0));
            let center = sw.midpoint(ne);
            min = if let Some(surface) = ellipsoid.scale_to_geodetic_surface(center) {
                js_min(min, -center.distance(surface))
            } else {
                DEFAULT_MIN_TERRAIN_HEIGHT
            };
        }
        Some((js_max(DEFAULT_MIN_TERRAIN_HEIGHT, min), max))
    }
    pub fn get_bounding_sphere(&self, rectangle: &Rectangle, ellipsoid: &Ellipsoid) -> Option<BoundingSphere> {
        let table = self.terrain_heights.as_ref()?;
        let max = self.tile_key(rectangle).and_then(|key| table.get(&key)).map_or(DEFAULT_MAX_TERRAIN_HEIGHT, |h| h[1]);
        Some(
            BoundingSphere::from_rectangle_3d(rectangle, ellipsoid, 0.0)
                .union(&BoundingSphere::from_rectangle_3d(rectangle, ellipsoid, max)),
        )
    }
    fn tile_key(&self, r: &Rectangle) -> Option<String> {
        let scheme = GeographicTilingScheme::default();
        let corners = [
            Cartographic::new(r.east, r.north, 0.0),
            Cartographic::new(r.west, r.north, 0.0),
            Cartographic::new(r.east, r.south, 0.0),
            Cartographic::new(r.west, r.south, 0.0),
        ];
        let mut scratch = self.scratch_tile.lock().unwrap();
        let mut last = None;
        for level in 0..=TERRAIN_HEIGHTS_MAX_LEVEL {
            let mut current = Cartesian2::ZERO;
            for (j, corner) in corners.iter().enumerate() {
                if let Some(tile) = scheme.position_to_tile_xy(corner, level) {
                    *scratch = tile;
                }
                if j == 0 {
                    current = *scratch;
                } else if current != *scratch {
                    return last;
                }
            }
            last = Some(format!("{level}-{}-{}", current.x, current.y));
        }
        last
    }
}
