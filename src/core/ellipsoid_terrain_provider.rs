//! CesiumJS EllipsoidTerrainProvider: zero-height 16x16 tiles and smooth
//! ellipsoid error estimates. An explicit quality replaces the JS global.
use super::ellipsoid::Ellipsoid;
use super::event::Event;
use super::heightmap_terrain_data::{HeightmapTerrainData, HeightmapTerrainDataOptions};
use super::runtime_error::RuntimeError;
use super::terrain_provider::{self, TerrainProvider};
use super::tile_availability::TileAvailability;
use super::tiling_scheme::{GeographicTilingScheme, TilingScheme};
use super::typed_array::TypedArray;
use std::cell::RefCell;
use std::future::{ready, Ready};
use std::rc::Rc;

pub struct EllipsoidTerrainProvider {
    tiling_scheme: Box<dyn TilingScheme>,
    level_zero_maximum_geometric_error: f64,
    errors: Rc<RefCell<Event<RuntimeError>>>,
}
impl Default for EllipsoidTerrainProvider {
    fn default() -> Self {
        Self::new(None, None, None)
    }
}
impl EllipsoidTerrainProvider {
    pub fn new(
        tiling_scheme: Option<Box<dyn TilingScheme>>,
        ellipsoid: Option<Ellipsoid>,
        quality: Option<f64>,
    ) -> Self {
        let tiling_scheme = tiling_scheme.unwrap_or_else(|| {
            Box::new(GeographicTilingScheme::new(
                ellipsoid.unwrap_or(Ellipsoid::WGS84),
                super::rectangle::Rectangle::MAX_VALUE,
                2,
                1,
            ))
        });
        let error = terrain_provider::get_estimated_level_zero_geometric_error_for_a_heightmap(
            tiling_scheme.ellipsoid(),
            64.0,
            tiling_scheme.get_number_of_x_tiles_at_level(0) as f64,
            quality,
        );
        Self { tiling_scheme, level_zero_maximum_geometric_error: error, errors: Rc::new(RefCell::new(Event::new())) }
    }
}
impl TerrainProvider for EllipsoidTerrainProvider {
    type Error = RuntimeError;
    type Credit = ();
    type TerrainData = HeightmapTerrainData;
    type Request = ();
    type GeometryFuture<'a> = Ready<Result<HeightmapTerrainData, RuntimeError>>;
    type AvailabilityFuture<'a> = Ready<Result<(), RuntimeError>>;
    fn error_event(&self) -> Rc<RefCell<Event<RuntimeError>>> {
        self.errors.clone()
    }
    fn credit(&self) -> Option<&()> {
        None
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.tiling_scheme.as_ref()
    }
    fn has_water_mask(&self) -> bool {
        false
    }
    fn has_vertex_normals(&self) -> bool {
        false
    }
    fn availability(&self) -> Option<&TileAvailability> {
        None
    }
    fn request_tile_geometry(
        &self,
        _x: u32,
        _y: u32,
        _level: u32,
        _request: Option<()>,
    ) -> Option<Self::GeometryFuture<'_>> {
        Some(ready(Ok(HeightmapTerrainData::new(HeightmapTerrainDataOptions::new(
            TypedArray::Uint8(vec![0; 256]),
            16,
            16,
        ))
        .unwrap())))
    }
    fn get_level_maximum_geometric_error(&self, level: u32) -> f64 {
        self.level_zero_maximum_geometric_error / 1i32.wrapping_shl(level) as f64
    }
    fn get_tile_data_available(&self, _x: u32, _y: u32, _level: u32) -> Option<bool> {
        None
    }
    fn load_tile_data_availability(&self, _x: u32, _y: u32, _level: u32) -> Option<Self::AvailabilityFuture<'_>> {
        None
    }
}
