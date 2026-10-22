//! CesiumJS `decodeI3S` worker.
//!
//! Decodes an I3S binary geometry buffer or a Draco mesh whose payload starts
//! with the `DRACO` magic, shifts orthometric heights, converts vertices into
//! the tile's local frame, and returns the glTF document the worker builds.
//! Blob URLs are owned byte buffers. The worker's WebAssembly initialization
//! message has no module to compile and resolves `true`.
//!
//! Draco decoding uses `draco-core` inside this module. It is not a shared
//! `decodeDraco` helper: I3S reads draco3d attribute values and `i3s-*`
//! metadata directly.

mod components;
mod mesh;
mod source;

use crate::core::{Cartesian3, Cartographic, Matrix3, RuntimeError};

/// glTF buffer view. `target` is 34962 (`ARRAY_BUFFER`) or 34963
/// (`ELEMENT_ARRAY_BUFFER`).
#[derive(Clone, Debug)]
pub struct I3sBufferView {
    pub buffer: usize,
    pub byte_offset: u32,
    pub byte_length: usize,
    pub target: u32,
}

/// glTF accessor. Position `min` and `max` are stored swapped, as Cesium does.
#[derive(Clone, Debug)]
pub struct I3sAccessor {
    pub buffer_view: usize,
    pub byte_offset: u32,
    pub component_type: u32,
    pub count: usize,
    pub type_name: &'static str,
    pub min: Option<[f64; 3]>,
    pub max: Option<[f64; 3]>,
}

/// `EXT_mesh_features` feature count and `CESIUM_primitive_outline` accessor.
#[derive(Clone, Debug, Default)]
pub struct I3sPrimitiveExtensions {
    pub feature_count: Option<f64>,
    pub outline: Option<usize>,
}

/// One mesh primitive. `transparent` sets `extra.isTransparent`.
#[derive(Clone, Debug)]
pub struct I3sPrimitive {
    pub attributes: Vec<(&'static str, usize)>,
    pub indices: usize,
    pub material: usize,
    pub extensions: I3sPrimitiveExtensions,
    pub transparent: bool,
}

/// Mesh of primitives.
#[derive(Clone, Debug)]
pub struct I3sMesh {
    pub primitives: Vec<I3sPrimitive>,
}

/// Node pointing at a mesh.
#[derive(Clone, Debug)]
pub struct I3sNode {
    pub mesh: u32,
}

/// Property table values buffer view used by `EXT_structural_metadata`.
#[derive(Clone, Debug)]
pub struct I3sPropertyTable {
    pub count: f64,
    pub values: usize,
}

/// Document-level extensions. Absent on the empty early return.
#[derive(Clone, Debug, Default)]
pub struct I3sRootExtensions {
    pub property_table: Option<I3sPropertyTable>,
}

/// Worker result. Buffers own the bytes Cesium would put behind blob URLs.
#[derive(Clone, Debug)]
pub struct I3sMeshData {
    pub buffers: Vec<Vec<u8>>,
    pub buffer_views: Vec<I3sBufferView>,
    pub accessors: Vec<I3sAccessor>,
    pub meshes: Vec<I3sMesh>,
    pub nodes: Vec<I3sNode>,
    pub nodes_in_scene: Vec<u32>,
    pub root_extensions: Option<I3sRootExtensions>,
    pub extensions_used: Option<Vec<String>>,
    pub positions: Option<Vec<f64>>,
    pub indices: Option<Vec<u32>>,
    pub feature_index: Option<Vec<Option<f64>>>,
    pub source_url: Option<String>,
    pub cartesian_center: Option<Cartesian3>,
    pub parent_rotation: Option<Matrix3>,
}

impl I3sMeshData {
    pub(super) fn empty() -> Self {
        Self {
            buffers: Vec::new(),
            buffer_views: Vec::new(),
            accessors: Vec::new(),
            meshes: Vec::new(),
            nodes: Vec::new(),
            nodes_in_scene: Vec::new(),
            root_extensions: None,
            extensions_used: None,
            positions: None,
            indices: None,
            feature_index: None,
            source_url: None,
            cartesian_center: None,
            parent_rotation: None,
        }
    }
}

/// One geoid heightmap the worker samples before the local-frame transform.
///
/// `buffer` is indexed like a JavaScript array: a non-integer address is
/// missing, and a missing sample becomes NaN. Web Mercator tiles supply the
/// ellipsoid radii the worker reads from `projection._ellipsoid._radii`.
#[derive(Clone, Debug)]
pub struct I3sGeoid {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
    pub width: f64,
    pub height: f64,
    pub buffer: Vec<f64>,
    pub scale: f64,
    pub offset: f64,
    pub web_mercator: bool,
    pub web_mercator_radii: Option<Cartesian3>,
}

/// Symbology properties in insertion order. `true` means `edges` is defined.
#[derive(Clone, Debug, Default)]
pub struct I3sSymbology {
    pub entries: Vec<(String, bool)>,
}

/// Owned inputs of one `decodeI3S` message.
///
/// `ordering` and `feature_attribute_order` are the schema lists, already
/// expanded from `featureData` key order when that override is present.
/// `buffer_attributes` is `bufferInfo.attributes` and, when set, replaces the
/// schema walk.
#[derive(Clone, Debug)]
pub struct I3sDecodeParameters {
    pub binary_data: Vec<u8>,
    pub ordering: Option<Vec<String>>,
    pub feature_attribute_order: Option<Vec<String>>,
    pub buffer_attributes: Option<Vec<String>>,
    pub geoid_data_list: Vec<I3sGeoid>,
    pub cartographic_center: Option<Cartographic>,
    pub cartesian_center: Option<Cartesian3>,
    pub parent_rotation: Option<Matrix3>,
    pub ellipsoid_radii_square: Option<Cartesian3>,
    pub url: Option<String>,
    pub enable_features: bool,
    pub split_geometry_by_color_transparency: bool,
    pub calculate_normals: bool,
    pub symbology: Option<I3sSymbology>,
}

/// The initialization message. There is no Draco WebAssembly module to compile.
pub fn init_decode_i3s() -> bool {
    true
}

/// Decode one I3S payload into the worker's mesh document.
pub fn decode_i3s(parameters: &I3sDecodeParameters) -> Result<I3sMeshData, RuntimeError> {
    let geometry = source::decode_geometry(
        &parameters.binary_data,
        parameters.ordering.as_deref(),
        parameters.feature_attribute_order.as_deref(),
        parameters.buffer_attributes.as_deref(),
    )
    .map_err(|()| RuntimeError::new("Invalid I3S payload"))?;
    mesh::build(geometry, parameters)
}
