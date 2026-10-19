//! CesiumJS CustomHeightmapTerrainProvider. Native callbacks return optional
//! futures; typed credits are supplied by the caller's host/UI layer.
use super::ellipsoid::Ellipsoid;
use super::event::Event;
use super::heightmap_terrain_data::{HeightmapTerrainData, HeightmapTerrainDataOptions};
use super::runtime_error::RuntimeError;
use super::terrain_provider::{self, TerrainProvider};
use super::tile_availability::TileAvailability;
use super::tiling_scheme::{GeographicTilingScheme, TilingScheme};
use super::typed_array::TypedArray;
use std::cell::RefCell;
use std::future::{Future, Ready};
use std::pin::Pin;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum HeightmapSamples {
    Typed(TypedArray),
    Numbers(Vec<f64>),
}
impl From<TypedArray> for HeightmapSamples {
    fn from(buffer: TypedArray) -> Self {
        Self::Typed(buffer)
    }
}
impl From<Vec<f64>> for HeightmapSamples {
    fn from(values: Vec<f64>) -> Self {
        Self::Numbers(values)
    }
}

pub trait HeightmapCallback {
    type Future<'a>: Future<Output = Result<HeightmapSamples, RuntimeError>>
    where
        Self: 'a;
    fn request(&self, x: u32, y: u32, level: u32) -> Option<Self::Future<'_>>;
}
impl<F, Fut> HeightmapCallback for F
where
    F: Fn(u32, u32, u32) -> Option<Fut>,
    Fut: Future<Output = Result<HeightmapSamples, RuntimeError>> + 'static,
{
    type Future<'a>
        = Fut
    where
        Self: 'a;
    fn request(&self, x: u32, y: u32, level: u32) -> Option<Fut> {
        self(x, y, level)
    }
}

pub struct CustomHeightmapTerrainProviderOptions<C = String> {
    pub width: usize,
    pub height: usize,
    pub tiling_scheme: Option<Box<dyn TilingScheme>>,
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<C>,
    pub quality: Option<f64>,
}
impl<C> CustomHeightmapTerrainProviderOptions<C> {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, tiling_scheme: None, ellipsoid: None, credit: None, quality: None }
    }
}
pub struct CustomHeightmapTerrainProvider<F, C = String> {
    callback: F,
    width: usize,
    height: usize,
    tiling_scheme: Box<dyn TilingScheme>,
    credit: Option<C>,
    level_zero_maximum_geometric_error: f64,
    errors: Rc<RefCell<Event<RuntimeError>>>,
}
impl<F: HeightmapCallback, C> CustomHeightmapTerrainProvider<F, C> {
    pub fn new(callback: F, options: CustomHeightmapTerrainProviderOptions<C>) -> Option<Self> {
        if options.width < 2 || options.height < 2 {
            return None;
        }
        options.width.checked_mul(options.height)?;
        let tiling_scheme = options.tiling_scheme.unwrap_or_else(|| {
            Box::new(GeographicTilingScheme::new(
                options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
                super::rectangle::Rectangle::MAX_VALUE,
                2,
                1,
            ))
        });
        let error = terrain_provider::get_estimated_level_zero_geometric_error_for_a_heightmap(
            tiling_scheme.ellipsoid(),
            options.width.max(options.height) as f64,
            tiling_scheme.get_number_of_x_tiles_at_level(0) as f64,
            options.quality,
        );
        Some(Self {
            callback,
            width: options.width,
            height: options.height,
            tiling_scheme,
            credit: options.credit,
            level_zero_maximum_geometric_error: error,
            errors: Rc::new(RefCell::new(Event::new())),
        })
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
}
impl<F: HeightmapCallback, C> TerrainProvider for CustomHeightmapTerrainProvider<F, C> {
    type Error = RuntimeError;
    type Credit = C;
    type TerrainData = HeightmapTerrainData;
    type Request = ();
    type GeometryFuture<'a>
        = Pin<Box<dyn Future<Output = Result<HeightmapTerrainData, RuntimeError>> + 'a>>
    where
        Self: 'a;
    type AvailabilityFuture<'a>
        = Ready<Result<(), RuntimeError>>
    where
        Self: 'a;
    fn error_event(&self) -> Rc<RefCell<Event<RuntimeError>>> {
        self.errors.clone()
    }
    fn credit(&self) -> Option<&C> {
        self.credit.as_ref()
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
        x: u32,
        y: u32,
        level: u32,
        _request: Option<()>,
    ) -> Option<Self::GeometryFuture<'_>> {
        let future = self.callback.request(x, y, level)?;
        let (width, height) = (self.width, self.height);
        Some(Box::pin(async move {
            let buffer = match future.await? {
                HeightmapSamples::Typed(buffer) => buffer,
                HeightmapSamples::Numbers(values) => TypedArray::Float64(values),
            };
            HeightmapTerrainData::new(HeightmapTerrainDataOptions::new(buffer, width, height))
                .ok_or_else(|| RuntimeError::new("Invalid heightmap dimensions"))
        }))
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
