//! CesiumJS TerrainData contract. Native associated types accommodate credits
//! and image/byte water masks; futures preserve deferred/retry semantics.
use super::rectangle::Rectangle;
use super::runtime_error::RuntimeError;
use super::terrain_mesh::TerrainMesh;
use super::tiling_scheme::TilingScheme;
use std::future::Future;
use std::sync::Arc;

pub const MAXIMUM_ASYNCHRONOUS_TASKS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainTile {
    pub x: u32,
    pub y: u32,
    pub level: u32,
}

#[derive(Clone, Copy)]
pub struct TerrainMeshRequest<'a> {
    pub tiling_scheme: &'a dyn TilingScheme,
    pub tile: TerrainTile,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
    /// Replaces TerrainProvider's mutable global quality for this request.
    pub heightmap_quality: Option<f64>,
    /// A host scheduler may defer a throttled request. The synchronous
    /// heightmap implementation has no concurrent jobs and is always ready.
    pub throttle: bool,
}
impl<'a> TerrainMeshRequest<'a> {
    pub fn new(tiling_scheme: &'a dyn TilingScheme, tile: TerrainTile) -> Self {
        Self {
            tiling_scheme,
            tile,
            exaggeration: 1.0,
            exaggeration_relative_height: 0.0,
            heightmap_quality: None,
            throttle: true,
        }
    }
}

pub trait TerrainData: Sized {
    type Credit;
    type WaterMask: ?Sized;
    type MeshFuture<'a>: Future<Output = Result<Arc<TerrainMesh>, RuntimeError>>
    where
        Self: 'a;
    type UpsampleFuture<'a>: Future<Output = Result<Self, RuntimeError>>
    where
        Self: 'a;
    fn credits(&self) -> Option<&[Self::Credit]>;
    fn water_mask(&self) -> Option<&Self::WaterMask>;
    fn interpolate_height(&self, rectangle: &Rectangle, longitude: f64, latitude: f64) -> Option<f64>;
    fn is_child_available(&self, x: u32, y: u32, child_x: u32, child_y: u32) -> bool;
    /// None means deferred by the implementation's scheduler; retry later.
    fn create_mesh<'a>(&'a mut self, request: TerrainMeshRequest<'a>) -> Option<Self::MeshFuture<'a>>;
    fn upsample<'a>(
        &'a self,
        scheme: &'a dyn TilingScheme,
        source: TerrainTile,
        destination: TerrainTile,
    ) -> Option<Self::UpsampleFuture<'a>>;
    fn was_created_by_upsampling(&self) -> bool;
}
