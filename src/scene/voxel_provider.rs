//! CesiumJS abstract voxel provider. `None` means scheduling was deferred.
use super::{voxel_content::VoxelContent, MetadataComponentType, MetadataType, VoxelShapeType};
use crate::core::{Cartesian3, Matrix4, RuntimeError, TimeIntervalCollection};
use std::{future::Future, pin::Pin};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoxelRequest {
    pub tile_level: u32,
    pub tile_x: u32,
    pub tile_y: u32,
    pub tile_z: u32,
    pub keyframe: u32,
}
pub type VoxelFuture<'a> = Pin<Box<dyn Future<Output = Result<VoxelContent, RuntimeError>> + 'a>>;
pub trait VoxelProvider {
    fn global_transform(&self) -> &Matrix4;
    fn shape_transform(&self) -> &Matrix4;
    fn shape(&self) -> VoxelShapeType;
    fn min_bounds(&self) -> Option<Cartesian3>;
    fn max_bounds(&self) -> Option<Cartesian3>;
    fn dimensions(&self) -> Cartesian3;
    fn padding_before(&self) -> Cartesian3;
    fn padding_after(&self) -> Cartesian3;
    fn names(&self) -> &[String];
    fn types(&self) -> &[MetadataType];
    fn component_types(&self) -> &[MetadataComponentType];
    fn minimum_values(&self) -> Option<&[Vec<f64>]>;
    fn maximum_values(&self) -> Option<&[Vec<f64>]>;
    fn maximum_tile_count(&self) -> Option<usize>;
    fn available_levels(&self) -> Option<u32>;
    fn keyframe_count(&self) -> Option<u32>;
    fn time_interval_collection(&self) -> Option<&TimeIntervalCollection<u32>>;
    fn request_data(&mut self, options: VoxelRequest) -> Option<VoxelFuture<'_>>;
}
