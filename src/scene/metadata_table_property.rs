//! CesiumJS `MetadataTableProperty`: one binary column of a 3D Metadata
//! property table, read from and written to its buffer views.
//!
//! Cesium keeps typed-array views on the glTF buffers, so a write lands in
//! the buffer itself; here each property owns a copy of the bytes it reads.
//! The fallbacks for platforms without `BigInt64Array` are not ported: every
//! target has it.
//!
//! Inside an array value, an element Cesium reads as `undefined` (an enum
//! value without a name, or an offset past the end of the values) is
//! [`MetadataValue::Null`].

use super::metadata_class_property::{apply_transform_leaf, unapply_transform_leaf, ValueTransform};
use super::metadata_value::{arithmetic, developer, JsMap, MetadataError, MetadataValue, Numeric};
use super::{MetadataClassProperty, MetadataComponentType, MetadataType};
use crate::core::get_string_from_typed_array::get_string_from_typed_array;
use crate::core::math::{js_number_to_string, to_int32, to_uint32};
use crate::core::{ComponentDatatype, TypedArray};
use crate::scene::gltf_pipeline::js;
use serde_json::Value;
use std::sync::Arc;

type Result<T> = std::result::Result<T, MetadataError>;

/// A `Uint8Array`: the bytes from `byte_offset` on in `buffer`. Typed arrays
/// made from it read through the rest of the buffer, past its own end.
#[derive(Clone, Copy, Debug)]
pub struct ByteView<'a> {
    pub buffer: &'a [u8],
    pub byte_offset: usize,
}

/// The typed array a table property's values are stored in.
#[derive(Clone, Debug, PartialEq)]
pub enum MetadataTypedArray {
    Numbers(TypedArray),
    BigInt64(Vec<i64>),
    BigUint64(Vec<u64>),
}

/// An integer-indexed element: `typedArray[index]` reads nothing at a
/// fractional, negative or out-of-range index.
pub(crate) fn element_index(index: f64) -> Option<usize> {
    (index >= 0.0 && index.fract() == 0.0 && index.is_finite()).then_some(index as usize)
}

/// JavaScript `ToIndex`, as the typed-array constructors apply it.
pub(crate) fn to_index(value: f64, what: &str) -> Result<usize> {
    if value.is_nan() {
        return Ok(0);
    }
    let integer = value.trunc();
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        return Err(MetadataError::Range(format!("Invalid typed array {what}: {}", js_number_to_string(value))));
    }
    Ok(integer as usize)
}

/// `new XArray(buffer, byteOffset, length)`: the length must fit the buffer
/// and the offset be a multiple of the element size.
fn checked_range(name: &str, size: usize, buffer: &[u8], byte_offset: f64, length: f64) -> Result<(usize, usize)> {
    let offset = to_index(byte_offset, "offset")?;
    if offset % size != 0 {
        return Err(MetadataError::Range(format!("start offset of {name} should be a multiple of {size}")));
    }
    let length = to_index(length, "length")?;
    if offset as f64 + length as f64 * size as f64 > buffer.len() as f64 {
        return Err(MetadataError::Range(format!("Invalid typed array length: {length}")));
    }
    Ok((offset, length))
}

/// `ComponentDatatype.createArrayBufferView` with JavaScript's errors.
pub(crate) fn typed_array_view(
    datatype: ComponentDatatype,
    buffer: &[u8],
    byte_offset: f64,
    length: f64,
) -> Result<TypedArray> {
    let name = match datatype {
        ComponentDatatype::Byte => "Int8Array",
        ComponentDatatype::UnsignedByte => "Uint8Array",
        ComponentDatatype::Short => "Int16Array",
        ComponentDatatype::UnsignedShort => "Uint16Array",
        ComponentDatatype::Int => "Int32Array",
        ComponentDatatype::UnsignedInt => "Uint32Array",
        ComponentDatatype::Float => "Float32Array",
        ComponentDatatype::Double => "Float64Array",
    };
    let (offset, length) = checked_range(name, datatype.get_size_in_bytes(), buffer, byte_offset, length)?;
    datatype
        .create_array_buffer_view(buffer, offset, Some(length))
        .ok_or_else(|| MetadataError::Range(format!("Invalid typed array length: {length}")))
}

/// `ToNumber` as a typed-array store applies it: a `BigInt` is a TypeError.
pub(crate) fn store_number(value: Option<&MetadataValue>) -> Result<f64> {
    match value {
        None => Ok(f64::NAN),
        Some(MetadataValue::BigInt(_)) => Err(MetadataError::Type("Cannot convert a BigInt value to a number".into())),
        Some(v) => Ok(v.to_number()),
    }
}

/// JavaScript `BigInt(value)`.
fn to_bigint(value: Option<&MetadataValue>) -> Result<i128> {
    match value {
        None => Err(MetadataError::Type("Cannot convert undefined to a BigInt".into())),
        Some(MetadataValue::Null) => Err(MetadataError::Type("Cannot convert null to a BigInt".into())),
        Some(MetadataValue::Boolean(b)) => Ok(i128::from(*b)),
        Some(MetadataValue::BigInt(b)) => Ok(*b),
        Some(MetadataValue::Number(n)) => {
            if !n.is_finite() || n.fract() != 0.0 {
                return Err(MetadataError::Range(format!(
                    "The number {} cannot be converted to a BigInt because it is not an integer",
                    js_number_to_string(*n)
                )));
            }
            // Past 2^127 a double is a multiple of 2^64, which wraps to 0.
            Ok(if n.abs() >= 2f64.powi(127) { 0 } else { *n as i128 })
        }
        Some(MetadataValue::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(0);
            }
            t.parse().map_err(|_| MetadataError::Type(format!("Cannot convert {s} to a BigInt")))
        }
        Some(other) => Err(MetadataError::Type(format!("Cannot convert {} to a BigInt", other.js_string()))),
    }
}

impl MetadataTypedArray {
    fn view(component_type: MetadataComponentType, view: &ByteView<'_>, length: Numeric) -> Result<Self> {
        let length = match length {
            Numeric::Number(n) => n,
            Numeric::BigInt(_) => {
                return Err(MetadataError::Type("Cannot convert a BigInt value to a number".into()));
            }
        };
        let offset = view.byte_offset as f64;
        let read64 = |name| -> Result<Vec<[u8; 8]>> {
            let (offset, length) = checked_range(name, 8, view.buffer, offset, length)?;
            Ok(view.buffer[offset..offset + length * 8].chunks_exact(8).map(|c| c.try_into().unwrap()).collect())
        };
        Ok(match component_type {
            MetadataComponentType::Int64 => {
                Self::BigInt64(read64("BigInt64Array")?.into_iter().map(i64::from_le_bytes).collect())
            }
            MetadataComponentType::Uint64 => {
                Self::BigUint64(read64("BigUint64Array")?.into_iter().map(u64::from_le_bytes).collect())
            }
            other => {
                let datatype = other.to_component_datatype().expect("only the 64-bit types have no datatype");
                Self::Numbers(typed_array_view(datatype, view.buffer, offset, length)?)
            }
        })
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Numbers(a) => a.len(),
            Self::BigInt64(v) => v.len(),
            Self::BigUint64(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn byte_length(&self) -> usize {
        match self {
            Self::Numbers(a) => a.len() * a.component_datatype().get_size_in_bytes(),
            _ => self.len() * 8,
        }
    }

    /// `typedArray[index]`, `None` where that is `undefined`.
    pub fn get(&self, index: f64) -> Option<Numeric> {
        let i = element_index(index)?;
        match self {
            Self::Numbers(a) => a.get(i).map(Numeric::Number),
            Self::BigInt64(v) => v.get(i).map(|&x| Numeric::BigInt(i128::from(x))),
            Self::BigUint64(v) => v.get(i).map(|&x| Numeric::BigInt(i128::from(x))),
        }
    }

    /// Cesium's `BufferView.set`: 64-bit values go through `BigInt(value)`
    /// and wrap; the rest convert as typed arrays do. Writes outside the
    /// array are ignored, after the conversion.
    fn set(&mut self, index: f64, value: Option<&MetadataValue>) -> Result<()> {
        let i = element_index(index);
        match self {
            Self::Numbers(a) => {
                let n = store_number(value)?;
                if let Some(i) = i {
                    a.set(i, n);
                }
            }
            Self::BigInt64(v) => {
                let b = to_bigint(value)? as i64;
                if let Some(x) = i.and_then(|i| v.get_mut(i)) {
                    *x = b;
                }
            }
            Self::BigUint64(v) => {
                let b = to_bigint(value)? as u64;
                if let Some(x) = i.and_then(|i| v.get_mut(i)) {
                    *x = b;
                }
            }
        }
        Ok(())
    }

    fn byte(&self, index: f64) -> Option<u8> {
        match self {
            Self::Numbers(TypedArray::Uint8(v)) => element_index(index).and_then(|i| v.get(i).copied()),
            _ => None,
        }
    }

    fn set_byte(&mut self, index: f64, value: u8) {
        if let (Self::Numbers(TypedArray::Uint8(v)), Some(i)) = (self, element_index(index)) {
            if let Some(x) = v.get_mut(i) {
                *x = value;
            }
        }
    }
}

/// `undefined` (or a value never `BigInt` here) as a number.
fn number(value: Option<Numeric>) -> f64 {
    match value {
        Some(Numeric::Number(n)) => n,
        Some(Numeric::BigInt(b)) => b as f64,
        None => f64::NAN,
    }
}

fn numeric(value: Option<Numeric>) -> Numeric {
    value.unwrap_or(Numeric::Number(f64::NAN))
}

/// `index >> 3` and `1 << (index % 8)` of the boolean bit field.
fn bit_position(index: f64) -> (f64, u32) {
    let byte = f64::from(to_int32(index) >> 3);
    let bit = to_uint32(index % 8.0) & 31;
    (byte, bit)
}

/// JavaScript truthiness.
fn truthy(value: Option<&MetadataValue>) -> bool {
    match value {
        None | Some(MetadataValue::Null) => false,
        Some(MetadataValue::Boolean(b)) => *b,
        Some(MetadataValue::Number(n)) => *n != 0.0 && !n.is_nan(),
        Some(MetadataValue::BigInt(b)) => *b != 0,
        Some(MetadataValue::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `ToPropertyKey`, for looking a value up in an enum's names.
fn property_key(value: Option<&MetadataValue>) -> String {
    match value {
        None => "undefined".into(),
        Some(MetadataValue::Number(n)) => js_number_to_string(*n),
        Some(v) => v.js_string(),
    }
}

/// `new Array(length)`: a number that is not an array length throws.
fn array_length(length: f64) -> Result<usize> {
    if !(length >= 0.0 && length.fract() == 0.0 && length <= u32::MAX as f64) {
        return Err(MetadataError::Range("Invalid array length".into()));
    }
    Ok(length as usize)
}

/// `a ?? b` of two JSON properties.
fn nullish<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    a.filter(|v| !v.is_null()).or(b)
}

/// `MetadataComponentType[name] ?? MetadataComponentType.UINT32`.
fn offset_type(name: Option<&Value>) -> MetadataComponentType {
    name.and_then(Value::as_str)
        .and_then(MetadataComponentType::from_str_value)
        .unwrap_or(MetadataComponentType::Uint32)
}

/// `bufferViews[id]`.
fn buffer_view<'v, 'a>(buffer_views: &'v JsMap<ByteView<'a>>, id: Option<&Value>) -> Result<&'v ByteView<'a>> {
    let key = id.map_or_else(|| "undefined".to_string(), js::key);
    buffer_views
        .get(&key)
        .ok_or_else(|| MetadataError::Type("Cannot read properties of undefined (reading 'buffer')".into()))
}

/// Cesium's `flatten`: one level of nested arrays, as the typed arrays
/// store vectors.
fn flatten(value: MetadataValue) -> MetadataValue {
    match value {
        MetadataValue::Array(items) => MetadataValue::Array(
            items
                .into_iter()
                .flat_map(|item| match item {
                    MetadataValue::Array(inner) => inner,
                    other => vec![other],
                })
                .collect(),
        ),
        other => other,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueKind {
    String,
    Boolean,
    Enum,
    Number,
}

/// A binary property of a [`MetadataTable`](super::MetadataTable).
#[derive(Clone, Debug)]
pub struct MetadataTableProperty {
    array_offsets: Option<MetadataTypedArray>,
    string_offsets: Option<MetadataTypedArray>,
    values: Option<MetadataTypedArray>,
    class_property: Arc<MetadataClassProperty>,
    count: usize,
    vector_component_count: usize,
    offset: MetadataValue,
    scale: MetadataValue,
    has_value_transform: bool,
    unpacked_values: Option<Vec<Option<MetadataValue>>>,
    extras: Option<Value>,
    extensions: Option<Value>,
    byte_length: usize,
    kind: ValueKind,
}

impl MetadataTableProperty {
    /// `count` rows of the property JSON (`EXT_structural_metadata` or
    /// `EXT_feature_metadata`), with buffer views by id.
    pub fn new(
        count: usize,
        property: &Value,
        class_property: Arc<MetadataClassProperty>,
        buffer_views: &JsMap<ByteView<'_>>,
    ) -> Result<Self> {
        if count == 0 {
            return Err(developer("Expected options.count to be greater than 0, actual value was 0"));
        }
        if !(property.is_object() || property.is_array()) {
            return Err(developer("Expected options.property to be typeof object"));
        }
        let get = |key: &str| property.get(key);
        let metadata_type = class_property.metadata_type();
        let kind = match metadata_type {
            MetadataType::String => ValueKind::String,
            MetadataType::Boolean => ValueKind::Boolean,
            _ if class_property.enum_type().is_some() => ValueKind::Enum,
            _ => ValueKind::Number,
        };
        let mut byte_length = 0;

        let array_offsets = if class_property.is_variable_length_array() {
            let offsets = MetadataTypedArray::view(
                offset_type(nullish(get("arrayOffsetType"), get("offsetType"))),
                buffer_view(buffer_views, nullish(get("arrayOffsets"), get("arrayOffsetBufferView")))?,
                Numeric::Number(count as f64 + 1.0),
            )?;
            byte_length += offsets.byte_length();
            Some(offsets)
        } else {
            None
        };

        let vector_component_count = metadata_type.get_component_count();
        let array_component_count = match &array_offsets {
            Some(offsets) => arithmetic(numeric(offsets.get(count as f64)), numeric(offsets.get(0.0)), '-')?,
            None if class_property.is_array() => {
                Numeric::Number(count as f64 * js::to_number(class_property.array_length()))
            }
            None => Numeric::Number(count as f64),
        };
        let component_count =
            match arithmetic(Numeric::Number(vector_component_count as f64), array_component_count, '*')? {
                Numeric::Number(n) => n,
                Numeric::BigInt(_) => unreachable!("a number times a BigInt throws"),
            };

        let string_offsets = if kind == ValueKind::String {
            let offsets = MetadataTypedArray::view(
                offset_type(nullish(get("stringOffsetType"), get("offsetType"))),
                buffer_view(buffer_views, nullish(get("stringOffsets"), get("stringOffsetBufferView")))?,
                Numeric::Number(component_count + 1.0),
            )?;
            byte_length += offsets.byte_length();
            Some(offsets)
        } else {
            None
        };

        let value_type = match kind {
            ValueKind::String | ValueKind::Boolean => Some(MetadataComponentType::Uint8),
            _ => class_property.value_type(),
        };
        let value_count = match &string_offsets {
            Some(offsets) => arithmetic(numeric(offsets.get(component_count)), numeric(offsets.get(0.0)), '-')?,
            None if kind == ValueKind::Boolean => Numeric::Number((component_count / 8.0).ceil()),
            None => Numeric::Number(component_count),
        };
        let view = buffer_view(buffer_views, nullish(get("values"), get("bufferView")))?;
        let value_type = value_type.ok_or_else(|| developer("componentDatatype is required."))?;
        let values = MetadataTypedArray::view(value_type, view, value_count)?;
        byte_length += values.byte_length();

        let offset = MetadataValue::from_optional_json(get("offset"));
        let scale = MetadataValue::from_optional_json(get("scale"));
        let has_value_transform = class_property.has_value_transform() || offset.is_some() || scale.is_some();
        let offset = flatten(offset.unwrap_or_else(|| class_property.offset().clone()));
        let scale = flatten(scale.unwrap_or_else(|| class_property.scale().clone()));

        Ok(Self {
            array_offsets,
            string_offsets,
            values: Some(values),
            class_property,
            count,
            vector_component_count,
            offset,
            scale,
            has_value_transform,
            unpacked_values: None,
            extras: get("extras").cloned(),
            extensions: get("extensions").cloned(),
            byte_length,
            kind,
        })
    }

    pub fn has_value_transform(&self) -> bool {
        self.has_value_transform
    }

    /// The offset, flattened to one level as the values are stored.
    pub fn offset(&self) -> &MetadataValue {
        &self.offset
    }

    pub fn scale(&self) -> &MetadataValue {
        &self.scale
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }

    /// The bytes of the typed arrays read at construction.
    pub fn byte_length(&self) -> usize {
        self.byte_length
    }

    pub fn class_property(&self) -> &Arc<MetadataClassProperty> {
        &self.class_property
    }

    /// The value of row `index`, with no-data, default, normalization, offset
    /// and scale applied, and vectors and matrices unpacked. Reading a string
    /// property unpacks every row, after which there is no typed array.
    pub fn get(&mut self, index: usize) -> Result<Option<MetadataValue>> {
        self.check_index(index)?;
        let value = self.raw_get(index)?;
        let cp = Arc::clone(&self.class_property);
        let Some(value) = cp.handle_no_data(value).filter(|v| !matches!(v, MetadataValue::Null)) else {
            return match cp.default() {
                Some(default) => cp.unpack_vector_and_matrix_types(default.clone(), false).map(Some),
                // `MathType.unpack(undefined)` fails its `Check.defined`.
                None if cp.metadata_type().get_math_type().is_some() => {
                    Err(developer("array is required, actual value was undefined"))
                }
                None => Ok(None),
            };
        };
        let value = cp.normalize(value)?;
        let value = self.transform(value, &apply_transform_leaf)?;
        cp.unpack_vector_and_matrix_types(value, false).map(Some)
    }

    /// Stores a validated value in row `index`, packed, with offset and scale
    /// and normalization undone. A string property, or a variable-length
    /// array whose length changes, is unpacked first.
    pub fn set(&mut self, index: usize, value: MetadataValue) -> Result<()> {
        if matches!(value, MetadataValue::Null) {
            return Err(developer("value is required, actual value was null"));
        }
        self.check_index(index)?;
        let cp = Arc::clone(&self.class_property);
        if let Some(message) = cp.validate(Some(&value)) {
            return Err(developer(message));
        }
        let value = cp.pack_vector_and_matrix_types(value, false)?;
        let value = self.transform(value, &unapply_transform_leaf)?;
        let value = cp.unnormalize(value)?;
        self.raw_set(index, value)
    }

    /// The typed array of the values, or `None` once they are unpacked.
    pub fn get_typed_array(&self) -> Option<&MetadataTypedArray> {
        self.values.as_ref()
    }

    fn check_index(&self, index: usize) -> Result<()> {
        if index >= self.count {
            return Err(developer(format!(
                "index is required and between zero and count - 1. Actual value: {}",
                self.count as f64 - 1.0
            )));
        }
        Ok(())
    }

    fn transform(&self, value: MetadataValue, leaf: &ValueTransform) -> Result<MetadataValue> {
        if !self.has_value_transform || self.class_property.is_variable_length_array() {
            return Ok(value);
        }
        MetadataClassProperty::value_transform_in_place(value, Some(&self.offset), Some(&self.scale), leaf)
    }

    fn is_single(&self) -> bool {
        !self.class_property.is_array() && self.vector_component_count == 1
    }

    fn raw_get(&mut self, index: usize) -> Result<Option<MetadataValue>> {
        if self.unpacked_values.is_none() && self.kind == ValueKind::String {
            self.unpack()?;
        }
        if let Some(unpacked) = &self.unpacked_values {
            return Ok(unpacked[index].clone());
        }
        if self.is_single() {
            return self.get_value(index as f64);
        }
        Ok(Some(MetadataValue::Array(self.get_array_values(index)?)))
    }

    /// `arrayLength ?? 1` components of the vector size.
    fn fixed_components(&self) -> f64 {
        let length = self.class_property.array_length().filter(|v| !v.is_null());
        let length = if length.is_some() { js::to_number(length) } else { 1.0 };
        length * self.vector_component_count as f64
    }

    fn get_array_values(&self, index: usize) -> Result<Vec<MetadataValue>> {
        let (offset, length) = match &self.array_offsets {
            Some(offsets) => {
                let offset = number(offsets.get(index as f64));
                let length = number(offsets.get(index as f64 + 1.0)) - offset;
                let components = self.vector_component_count as f64;
                (offset * components, length * components)
            }
            None => {
                let components = self.fixed_components();
                (index as f64 * components, components)
            }
        };
        let length = array_length(length)?;
        let mut values = Vec::new();
        for i in 0..length {
            values.push(self.get_value(offset + i as f64)?.unwrap_or(MetadataValue::Null));
        }
        Ok(values)
    }

    fn get_value(&self, index: f64) -> Result<Option<MetadataValue>> {
        let Some(values) = &self.values else { return Ok(None) };
        Ok(match self.kind {
            ValueKind::String => Some(MetadataValue::String(self.get_string(index, values)?)),
            ValueKind::Boolean => {
                let (byte, bit) = bit_position(index);
                let byte = values.byte(byte).unwrap_or(0);
                Some(MetadataValue::Boolean((u32::from(byte) >> bit) & 1 == 1))
            }
            ValueKind::Enum => {
                let key = match values.get(index) {
                    None => "undefined".to_string(),
                    Some(Numeric::Number(n)) => js_number_to_string(n),
                    Some(Numeric::BigInt(b)) => b.to_string(),
                };
                let enum_type = self.class_property.enum_type().expect("enum properties have an enum type");
                enum_type.names_by_value.get(&key).cloned().map(MetadataValue::String)
            }
            ValueKind::Number => values.get(index).map(Numeric::value),
        })
    }

    /// `getStringFromTypedArray` of the bytes between two string offsets.
    fn get_string(&self, index: f64, values: &MetadataTypedArray) -> Result<String> {
        let offsets = self.string_offsets.as_ref().expect("string properties have string offsets");
        let start = offsets.get(index);
        let length = number(offsets.get(index + 1.0)) - number(start);
        let MetadataTypedArray::Numbers(TypedArray::Uint8(bytes)) = values else {
            unreachable!("strings are stored as UINT8")
        };
        let start = start.map(|s| number(Some(s)));
        if start.is_some_and(|s| s < 0.0) {
            return Err(developer("byteOffset cannot be negative."));
        }
        if length < 0.0 {
            return Err(developer("byteLength cannot be negative."));
        }
        if start.map_or(f64::NAN, |s| s + length) > bytes.len() as f64 {
            return Err(developer("sub-region exceeds array bounds."));
        }
        // `subarray(begin, end)`: NaN is 0 and the range is clamped.
        let clamp = |x: f64| if x.is_nan() { 0 } else { x.trunc().clamp(0.0, bytes.len() as f64) as usize };
        let begin = clamp(start.unwrap_or(0.0));
        let end = clamp(start.unwrap_or(0.0) + length).max(begin);
        Ok(get_string_from_typed_array(bytes, begin, Some(end - begin)))
    }

    fn unpack(&mut self) -> Result<()> {
        let mut unpacked = Vec::with_capacity(self.count);
        for i in 0..self.count {
            unpacked.push(if self.is_single() {
                self.get_value(i as f64)?
            } else {
                Some(MetadataValue::Array(self.get_array_values(i)?))
            });
        }
        self.unpacked_values = Some(unpacked);
        self.array_offsets = None;
        self.string_offsets = None;
        self.values = None;
        Ok(())
    }

    fn requires_unpack_for_set(&self, index: usize, value: &MetadataValue) -> bool {
        if self.unpacked_values.is_none() && self.kind == ValueKind::String {
            return true;
        }
        if let Some(offsets) = &self.array_offsets {
            let old = number(offsets.get(index as f64 + 1.0)) - number(offsets.get(index as f64));
            let new = value.as_array().map(|items| items.len() as f64);
            return new != Some(old);
        }
        false
    }

    fn raw_set(&mut self, index: usize, value: MetadataValue) -> Result<()> {
        if self.requires_unpack_for_set(index, &value) {
            self.unpack()?;
        }
        if let Some(unpacked) = &mut self.unpacked_values {
            unpacked[index] = Some(value);
            return Ok(());
        }
        if self.is_single() {
            return self.set_value(index as f64, Some(&value));
        }
        // Unlike reading, Cesium does not scale variable-length offsets by
        // the vector size here.
        let (offset, length) = match &self.array_offsets {
            Some(offsets) => {
                let offset = number(offsets.get(index as f64));
                (offset, number(offsets.get(index as f64 + 1.0)) - offset)
            }
            None => {
                let components = self.fixed_components();
                (index as f64 * components, components)
            }
        };
        let elements = value.as_array().unwrap_or(&[]);
        let stored = self.values.as_ref().map_or(0, MetadataTypedArray::len) as f64;
        let mut i = 0usize;
        while (i as f64) < length {
            let at = offset + i as f64;
            if i >= elements.len() && at >= stored {
                // Only conversions remain: `BigInt(undefined)` throws, the rest
                // write nothing.
                return match (self.kind, &self.values) {
                    (ValueKind::Number | ValueKind::Enum, Some(MetadataTypedArray::BigInt64(_)))
                    | (ValueKind::Number | ValueKind::Enum, Some(MetadataTypedArray::BigUint64(_))) => {
                        self.set_value(at, None)
                    }
                    _ => Ok(()),
                };
            }
            self.set_value(at, elements.get(i))?;
            i += 1;
        }
        Ok(())
    }

    fn set_value(&mut self, index: f64, value: Option<&MetadataValue>) -> Result<()> {
        match self.kind {
            ValueKind::String => Err(MetadataError::Type("property._setValue is not a function".into())),
            ValueKind::Boolean => {
                let (byte, bit) = bit_position(index);
                if let Some(values) = &mut self.values {
                    let current = values.byte(byte).unwrap_or(0);
                    let mask = (1u32 << bit) as u8;
                    values.set_byte(byte, if truthy(value) { current | mask } else { current & !mask });
                }
                Ok(())
            }
            ValueKind::Enum => {
                let enum_type = self.class_property.enum_type().expect("enum properties have an enum type");
                let integer = enum_type.values_by_name.get(&property_key(value)).copied().map(MetadataValue::Number);
                match &mut self.values {
                    Some(values) => values.set(index, integer.as_ref()),
                    None => Ok(()),
                }
            }
            ValueKind::Number => match &mut self.values {
                Some(values) => values.set(index, value),
                None => Ok(()),
            },
        }
    }
}
