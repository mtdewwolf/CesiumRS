//! CesiumJS `Cesium3DTileFeatureTable`: per-feature and global properties of a
//! b3dm/i3dm/pnts feature table, read from its JSON or its binary body.
//!
//! As upstream, a property's typed array is created once per semantic and
//! cached: a later request with another component type returns the first
//! array. Binary views follow JavaScript's typed-array rules: the byte offset
//! is relative to the whole tile buffer, must be a multiple of the element
//! size and must fit, or the view is the `RangeError` JavaScript throws. A JSON
//! property is converted with `ComponentDatatype.createTypedArray`, so a JSON
//! number makes that many zeros and an object makes an empty array.

use super::gltf_pipeline::{js, BufferSource, PipelineError};
use crate::core::{ComponentDatatype, TypedArray};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// What `getGlobalProperty` returns: the JSON value itself, or a one-element
/// typed array when the JSON points into the binary body.
#[derive(Clone, Debug, PartialEq)]
pub enum GlobalProperty {
    Json(Value),
    Binary(Rc<TypedArray>),
}

impl GlobalProperty {
    /// Element `index`, as `value[index]` reads it (`None` for undefined).
    pub fn element(&self, index: usize) -> Option<f64> {
        match self {
            Self::Json(v) => js::get(Some(v), &index.to_string()).map(|e| js::to_number(Some(e))),
            Self::Binary(a) => (index < a.len()).then(|| a.to_f64_vec()[index]),
        }
    }

    /// JavaScript `ToNumber`: a one-element typed array reads as its element.
    pub fn to_number(&self) -> f64 {
        match self {
            Self::Json(v) => js::to_number(Some(v)),
            Self::Binary(a) if a.len() == 1 => a.to_f64_vec()[0],
            Self::Binary(a) if a.is_empty() => 0.0,
            Self::Binary(_) => f64::NAN,
        }
    }
}

pub struct Cesium3DTileFeatureTable {
    pub json: Value,
    pub buffer: BufferSource,
    /// Per-feature property arrays have this many elements. Upstream assigns
    /// whatever POINTS_LENGTH or INSTANCES_LENGTH read as; arithmetic reads it
    /// with `ToNumber`.
    pub features_length: f64,
    cached_typed_arrays: RefCell<HashMap<String, Rc<TypedArray>>>,
}

fn range_error(message: String) -> PipelineError {
    PipelineError::Range(message)
}

/// JavaScript `ToIndex`.
fn to_index(value: f64) -> Result<usize, PipelineError> {
    let integer = if value.is_nan() { 0.0 } else { value.trunc() };
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        return Err(range_error(format!("Invalid typed array length: {integer}")));
    }
    Ok(integer as usize)
}

impl Cesium3DTileFeatureTable {
    pub fn new(feature_table_json: Value, feature_table_binary: BufferSource) -> Self {
        Self {
            json: feature_table_json,
            buffer: feature_table_binary,
            features_length: 0.0,
            cached_typed_arrays: RefCell::new(HashMap::new()),
        }
    }

    fn property(&self, semantic: &str) -> Option<&Value> {
        js::get(Some(&self.json), semantic).filter(|v| !v.is_null())
    }

    fn cached(
        &self,
        semantic: &str,
        create: impl FnOnce() -> Result<TypedArray, PipelineError>,
    ) -> Result<Rc<TypedArray>, PipelineError> {
        if let Some(array) = self.cached_typed_arrays.borrow().get(semantic) {
            return Ok(array.clone());
        }
        let array = Rc::new(create()?);
        self.cached_typed_arrays.borrow_mut().insert(semantic.to_string(), array.clone());
        Ok(array)
    }

    /// `new TypedArray(tileBuffer, buffer.byteOffset + byteOffset, length)`.
    fn binary_view(
        &self,
        datatype: ComponentDatatype,
        byte_offset: f64,
        length: f64,
    ) -> Result<TypedArray, PipelineError> {
        let size = datatype.get_size_in_bytes();
        let start = to_index(self.buffer.byte_offset as f64 + byte_offset)?;
        if !start.is_multiple_of(size) {
            return Err(range_error(format!("start offset of {datatype:?} array should be a multiple of {size}")));
        }
        let length = to_index(length)?;
        let tile = self.buffer.buffer.as_slice();
        if start > tile.len() {
            return Err(range_error(format!("Start offset {start} is outside the bounds of the buffer")));
        }
        datatype
            .create_array_buffer_view(tile, start, Some(length))
            .ok_or_else(|| range_error(format!("Invalid typed array length: {length}")))
    }

    /// `ComponentDatatype.createTypedArray(componentType, jsonValue)`.
    fn from_json(datatype: ComponentDatatype, value: &Value) -> Result<TypedArray, PipelineError> {
        let values = match value {
            Value::Array(items) => items.iter().map(|v| js::to_number(Some(v))).collect(),
            Value::Number(_) | Value::String(_) | Value::Bool(_) => vec![0.0; to_index(js::to_number(Some(value)))?],
            _ => Vec::new(),
        };
        Ok(TypedArray::from_values(datatype, &values))
    }

    /// `getGlobalProperty`: binary globals default to one UNSIGNED_INT.
    pub fn get_global_property(
        &self,
        semantic: &str,
        component_type: Option<ComponentDatatype>,
        component_length: Option<usize>,
    ) -> Result<Option<GlobalProperty>, PipelineError> {
        let Some(json) = self.property(semantic) else { return Ok(None) };
        let Some(byte_offset) = js::get(Some(json), "byteOffset").filter(|v| !v.is_null()) else {
            return Ok(Some(GlobalProperty::Json(json.clone())));
        };
        let datatype = component_type.unwrap_or(ComponentDatatype::UnsignedInt);
        let length = component_length.unwrap_or(1) as f64;
        let offset = js::to_number(Some(byte_offset));
        let array = self.cached(semantic, || self.binary_view(datatype, offset, length))?;
        Ok(Some(GlobalProperty::Binary(array)))
    }

    pub fn has_property(&self, semantic: &str) -> bool {
        self.property(semantic).is_some()
    }

    /// `getPropertyArray`: `featuresLength * componentLength` elements from the
    /// binary body (its `componentType` overrides the requested one), or the
    /// JSON array converted to `component_type`.
    pub fn get_property_array(
        &self,
        semantic: &str,
        component_type: ComponentDatatype,
        component_length: usize,
    ) -> Result<Option<Rc<TypedArray>>, PipelineError> {
        let Some(json) = self.property(semantic) else { return Ok(None) };
        let array = match js::get(Some(json), "byteOffset").filter(|v| !v.is_null()) {
            Some(byte_offset) => {
                let datatype = match js::get(Some(json), "componentType").filter(|v| !v.is_null()) {
                    Some(name) => ComponentDatatype::from_name(&js::key(name))
                        .ok_or_else(|| PipelineError::Developer("componentDatatype is not a valid value.".into()))?,
                    None => component_type,
                };
                let offset = js::to_number(Some(byte_offset));
                let length = self.features_length * component_length as f64;
                self.cached(semantic, || self.binary_view(datatype, offset, length))?
            }
            None => self.cached(semantic, || Self::from_json(component_type, json))?,
        };
        Ok(Some(array))
    }

    /// `getProperty`: feature `feature_id`'s `component_length` values. Elements
    /// past the end are undefined (`None`).
    pub fn get_property(
        &self,
        semantic: &str,
        component_type: ComponentDatatype,
        component_length: usize,
        feature_id: usize,
    ) -> Result<Option<Vec<Option<f64>>>, PipelineError> {
        let Some(array) = self.get_property_array(semantic, component_type, component_length)? else {
            return Ok(None);
        };
        let values = array.to_f64_vec();
        let start = component_length * feature_id;
        Ok(Some((start..start + component_length).map(|i| values.get(i).copied()).collect()))
    }
}
