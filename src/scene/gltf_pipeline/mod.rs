//! CesiumJS `Scene/GltfPipeline`: the glTF JSON utilities Cesium's loaders run
//! before building a model. They upgrade glTF 0.8 and 1.0 to 2.0, fill in
//! defaults, parse binary glTF and drop unused elements.
//!
//! A [`Gltf`] is the JSON as `serde_json::Value` (with `preserve_order`, so
//! object keys keep JavaScript's insertion order) plus the binary sources
//! Cesium attaches to buffers as `buffer.extras._pipeline.source`. In the JSON
//! that property holds a small marker naming an entry of the side table, so
//! sources stay with their buffers as indices shift.
//!
//! The functions follow Cesium's JavaScript semantics on valid glTF, including
//! its `for...in` order, `defined` and `??` checks and `DataView` bounds.
//! Where Cesium reads a property of a missing object, which throws a
//! `TypeError`, the port returns [`PipelineError::Type`].

pub(crate) mod js;

pub mod add_buffer;
pub mod add_defaults;
pub mod add_extensions_required;
pub mod add_extensions_used;
pub mod add_pipeline_extras;
pub mod add_to_array;
pub mod find_accessor_min_max;
pub mod for_each;
pub mod for_each_texture_in_material;
pub mod get_accessor_byte_stride;
pub mod get_component_reader;
pub mod move_technique_render_states;
pub mod move_techniques_to_extension;
pub mod number_of_components_for_type;
pub mod parse_glb;
pub mod read_accessor_packed;
pub mod remove_extension;
pub mod remove_extensions_required;
pub mod remove_extensions_used;
pub mod remove_pipeline_extras;
pub mod remove_unused_elements;
pub mod update_accessor_component_types;
pub mod update_version;
pub mod uses_extension;

pub use add_buffer::add_buffer;
pub use add_defaults::add_defaults;
pub use add_extensions_required::add_extensions_required;
pub use add_extensions_used::add_extensions_used;
pub use add_pipeline_extras::add_pipeline_extras;
pub use add_to_array::add_to_array;
pub use find_accessor_min_max::{find_accessor_min_max, MinMax};
pub use for_each_texture_in_material::for_each_texture_in_material;
pub use get_accessor_byte_stride::get_accessor_byte_stride;
pub use get_component_reader::get_component_reader;
pub use move_technique_render_states::move_technique_render_states;
pub use move_techniques_to_extension::move_techniques_to_extension;
pub use number_of_components_for_type::number_of_components_for_type;
pub use parse_glb::parse_glb;
pub use read_accessor_packed::read_accessor_packed;
pub use remove_extension::remove_extension;
pub use remove_extensions_required::remove_extensions_required;
pub use remove_extensions_used::remove_extensions_used;
pub use remove_pipeline_extras::remove_pipeline_extras;
pub use remove_unused_elements::{remove_unused_elements, ElementType};
pub use update_accessor_component_types::update_accessor_component_types;
pub use update_version::{update_version, UpdateVersionOptions};
pub use uses_extension::uses_extension;

use serde_json::{json, Value};
use std::sync::Arc;

/// A `Uint8Array` over part of an `ArrayBuffer`. Cesium reads through
/// `new DataView(source.buffer)` at `source.byteOffset`, so reads see the
/// whole underlying buffer, not just the view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferSource {
    pub buffer: Arc<Vec<u8>>,
    pub byte_offset: usize,
    pub byte_length: usize,
}

impl BufferSource {
    /// A view of all of `bytes`.
    pub fn new(bytes: Vec<u8>) -> Self {
        let byte_length = bytes.len();
        Self { buffer: Arc::new(bytes), byte_offset: 0, byte_length }
    }

    /// `subarray`: a view of `buffer[byte_offset..byte_offset + byte_length]`,
    /// clamped to the buffer.
    pub fn view(buffer: Arc<Vec<u8>>, byte_offset: usize, byte_length: usize) -> Self {
        let byte_offset = byte_offset.min(buffer.len());
        let byte_length = byte_length.min(buffer.len() - byte_offset);
        Self { buffer, byte_offset, byte_length }
    }

    /// The bytes of the view.
    pub fn bytes(&self) -> &[u8] {
        &self.buffer[self.byte_offset..self.byte_offset + self.byte_length]
    }
}

/// What Cesium throws. `Runtime` is a `RuntimeError` and `Developer` a
/// debug-build `DeveloperError`; the others are the JavaScript errors invalid
/// input runs into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PipelineError {
    Runtime(String),
    Developer(String),
    Type(String),
    Range(String),
    Syntax(String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, message) = match self {
            Self::Runtime(m) => ("RuntimeError", m),
            Self::Developer(m) => ("DeveloperError", m),
            Self::Type(m) => ("TypeError", m),
            Self::Range(m) => ("RangeError", m),
            Self::Syntax(m) => ("SyntaxError", m),
        };
        write!(f, "{kind}: {message}")
    }
}

impl std::error::Error for PipelineError {}

pub type Result<T> = std::result::Result<T, PipelineError>;

/// The property of `extras._pipeline.source` markers.
const SOURCE_MARKER: &str = "$pipelineSource";

/// A glTF document and the binary sources its buffers carry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gltf {
    pub json: Value,
    sources: Vec<BufferSource>,
}

impl Gltf {
    pub fn new(json: Value) -> Self {
        Self { json, sources: Vec::new() }
    }

    /// Stores `source` and returns the value to put in a buffer's
    /// `extras._pipeline.source`.
    pub fn add_source(&mut self, source: BufferSource) -> Value {
        self.sources.push(source);
        json!({ SOURCE_MARKER: self.sources.len() - 1 })
    }

    /// The source a marker names.
    pub fn source(&self, marker: &Value) -> Option<&BufferSource> {
        self.sources.get(marker.get(SOURCE_MARKER)?.as_u64()? as usize)
    }

    pub(crate) fn source_mut(&mut self, marker: &Value) -> Option<&mut BufferSource> {
        let id = marker.get(SOURCE_MARKER)?.as_u64()? as usize;
        self.sources.get_mut(id)
    }

    /// Whether `value` is a source marker.
    pub fn is_source(value: &Value) -> bool {
        value.get(SOURCE_MARKER).is_some()
    }

    /// `gltf.buffers[buffer].extras._pipeline.source`.
    pub fn buffer_source(&self, buffer: &Value) -> Option<&BufferSource> {
        let buffer = js::index(self.json.get("buffers"), buffer)?;
        self.source(buffer.get("extras")?.get("_pipeline")?.get("source")?)
    }
}

/// An element id as `ForEach` passes it: an array index, or the key of a
/// glTF 1.0 object.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Id {
    Index(usize),
    Key(String),
}

impl Id {
    /// The JavaScript property key.
    pub fn key(&self) -> String {
        match self {
            Self::Index(i) => i.to_string(),
            Self::Key(k) => k.clone(),
        }
    }

    /// The id as a JSON value: a number for an index, a string for a key.
    pub fn value(&self) -> Value {
        match self {
            Self::Index(i) => Value::from(*i),
            Self::Key(k) => Value::from(k.clone()),
        }
    }
}
