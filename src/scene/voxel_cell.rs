//! CesiumJS `VoxelCell`: typed sample metadata and padding-aware box bounds.
//! The caller provides primitive identity and resolved node/content; scene picking
//! and cylinder/ellipsoid samples are not integrated yet.
use super::{
    metadata_type::MetadataType, spatial_node::SpatialNode, voxel_content::VoxelContent,
};
use crate::core::{Cartesian3, OrientedBoundingBox, RuntimeError, TypedArray};
#[derive(Debug)]
pub struct VoxelCell<P> {
    pub primitive: P,
    pub tile_index: usize,
    pub sample_index: usize,
    metadata: Vec<(String, TypedArray)>,
    obb: OrientedBoundingBox,
}
fn slice(a: &TypedArray, start: usize, end: usize) -> TypedArray {
    macro_rules! slice { ($($v:ident),*) => {match a {$(TypedArray::$v(v)=>TypedArray::$v(v[start.min(v.len())..end.min(v.len())].to_vec())),*}}; }
    slice!(Int8, Uint8, Int16, Uint16, Int32, Uint32, Float32, Float64)
}
impl<P> VoxelCell<P> {
    #[allow(clippy::too_many_arguments)]
    pub fn from_node(
        primitive: P,
        tile_index: usize,
        sample_index: usize,
        node: &SpatialNode,
        content: &VoxelContent,
        properties: &[(String, MetadataType)],
        shape: &dyn super::voxel_shape::VoxelShape,
        dimensions: Cartesian3,
        padding_before: Cartesian3,
    ) -> Result<Self, RuntimeError> {
        if properties.len() != content.metadata().len() {
            return Err(RuntimeError::new("Voxel property count mismatch"));
        }
        if dimensions.x <= 0.0
            || dimensions.y <= 0.0
            || dimensions.z <= 0.0
            || node.dimensions.x <= 0.0
            || node.dimensions.y <= 0.0
        {
            return Err(RuntimeError::new("Voxel dimensions must be positive"));
        }
        let mut metadata = vec![];
        for ((name, kind), data) in properties.iter().zip(content.metadata()) {
            let n = kind.get_component_count();
            let start = sample_index.checked_mul(n).ok_or_else(|| RuntimeError::new("Sample offset overflow"))?;
            let end = start.checked_add(n).ok_or_else(|| RuntimeError::new("Sample offset overflow"))?;
            metadata.push((name.clone(), slice(data, start, end)));
        }
        let index = sample_index as f64;
        let z = (index / (node.dimensions.x * node.dimensions.y)).floor();
        let rest = index - z * node.dimensions.x * node.dimensions.y;
        let y = (rest / node.dimensions.x).floor();
        let x = rest - y * node.dimensions.x;
        let uv = Cartesian3::new(
            (x - padding_before.x) / dimensions.x,
            (y - padding_before.y) / dimensions.y,
            (z - padding_before.z) / dimensions.z,
        );
        let obb = shape.sample_obb(node.level, node.coords, dimensions, uv);
        Ok(Self { primitive, tile_index, sample_index, metadata, obb })
    }
    pub fn has_property(&self, name: &str) -> bool {
        self.get_property(name).is_some()
    }
    pub fn get_names(&self) -> Vec<&str> {
        self.metadata.iter().map(|(n, _)| n.as_str()).collect()
    }
    pub fn get_property(&self, name: &str) -> Option<&TypedArray> {
        self.metadata.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn oriented_bounding_box(&self) -> OrientedBoundingBox {
        self.obb
    }
}
