//! Port of CesiumJS `Scene/I3SField`.

use crate::core::resource::{DerivedResourceOptions, Resource};
use crate::core::resource_client::ResourceClient;
use crate::core::runtime_error::RuntimeError;
use crate::scene::i3s_data_provider::I3SDataProvider;
use crate::scene::i3s_node::I3SNode;
use crate::scene::scene::Result;
use serde_json::{Map, Value};

/// Port of `I3SField`: a custom attribute field attached to I3S nodes.
/// Do not construct directly; access through [`I3SNode::fields`].
pub struct I3SField {
    storage_info: Value,
    load_started: bool,
    resource: Resource,
    data: Option<Vec<u8>>,
    header: Option<Map<String, Value>>,
    values: Map<String, Value>,
}

impl I3SField {
    /// Port of `new I3SField(parent, storageInfo)`.
    pub fn new(parent: &I3SNode, storage_info: Value) -> Self {
        let uri = format!(
            "attributes/{}/0",
            storage_info.get("key").and_then(Value::as_str).unwrap_or_default()
        );
        let resource = if parent.node_index().is_some() {
            let layer = parent.layer().expect("I3S field parent has a layer");
            let node_resource = parent
                .data()
                .and_then(|data| data.get("mesh"))
                .and_then(|mesh| mesh.get("attribute"))
                .and_then(|attribute| attribute.get("resource"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            layer.resource().get_derived_resource(DerivedResourceOptions {
                url: Some(format!("nodes/{node_resource}/{uri}")),
                ..DerivedResourceOptions::default()
            })
        } else {
            parent
                .resource()
                .and_then(|resource| resource.get_derived_resource(DerivedResourceOptions {
                    url: Some(uri),
                    ..DerivedResourceOptions::default()
                }))
        };
        Self {
            storage_info,
            load_started: false,
            resource: resource.expect("valid I3S field URI"),
            data: None,
            header: None,
            values: Map::new(),
        }
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    /// Port of the `name` getter: the storage info `name`.
    pub fn name(&self) -> Option<&str> {
        self.storage_info.get("name").and_then(Value::as_str)
    }

    /// Port of the `header` getter: the parsed attribute header, if loaded.
    pub fn header(&self) -> Option<&Map<String, Value>> {
        self.header.as_ref()
    }

    /// Port of the `values` getter: attribute values or object IDs.
    pub fn values(&self) -> Vec<Value> {
        self.values
            .get("attributeValues")
            .and_then(Value::as_array)
            .cloned()
            .or_else(|| self.values.get("objectIds").and_then(Value::as_array).cloned())
            .unwrap_or_default()
    }

    /// Port of `I3SField.prototype.load`. Errors are logged and swallowed, as
    /// in Cesium; callers that need the result should use [`I3SField::load`].
    pub async fn load(&mut self, client: &ResourceClient) -> Result<()> {
        if self.load_started {
            return Ok(());
        }
        self.load_started = true;
        let data = I3SDataProvider::load_binary(&self.resource, client).await?;
        self.data = Some(data.clone());
        let view = DataView::new(&data);
        self.validate_header(&view)?;
        let header_size = self.parse_header(&view);
        let offset = self.get_body_offset(header_size);
        self.validate_body(&view, offset)?;
        self.parse_body(&view, offset);
        Ok(())
    }

    /// Port of `I3SField.prototype._validateHeader`.
    fn validate_header(&self, view: &DataView) -> Result<()> {
        let header_size: usize = self
            .storage_info
            .get("header")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| value_type_size(item.get("valueType").and_then(Value::as_str).unwrap_or_default()))
                    .sum()
            })
            .unwrap_or(0);
        if view.len() < header_size {
            return Err(RuntimeError::new(format!(
                "Invalid attribute buffer size (field: {}, header: {}, actual: {})",
                self.name().unwrap_or("undefined"),
                header_size,
                view.len()
            )));
        }
        Ok(())
    }

    /// Port of `I3SField.prototype._parseHeader`.
    fn parse_header(&mut self, view: &DataView) -> usize {
        let mut offset = 0;
        let mut header = Map::new();
        if let Some(items) = self.storage_info.get("header").and_then(Value::as_array).cloned() {
            for item in &items {
                let type_ = item.get("valueType").and_then(Value::as_str).unwrap_or_default();
                let (value, new_offset) = self.parse_value(view, type_, offset);
                if let Some(property) = item.get("property").and_then(Value::as_str) {
                    header.insert(property.to_string(), value);
                }
                offset = new_offset;
            }
        }
        self.header = Some(header);
        offset
    }

    /// Port of `I3SField.prototype._getBodyOffset`.
    fn get_body_offset(&self, header_size: usize) -> usize {
        let mut value_size = 0;
        for key in ["attributeValues", "objectIds"] {
            if let Some(desc) = self.storage_info.get(key) {
                value_size = numeric_type_size(desc.get("valueType").and_then(Value::as_str).unwrap_or_default());
                break;
            }
        }
        if value_size > 0 {
            ((header_size as f64 / value_size as f64).ceil() as usize) * value_size
        } else {
            header_size
        }
    }

    /// Port of `I3SField.prototype._validateBody`.
    fn validate_body(&self, view: &DataView, offset: usize) -> Result<()> {
        let count = self
            .header
            .as_ref()
            .and_then(|header| header.get("count"))
            .and_then(Value::as_u64);
        let Some(count) = count else {
            return Err(RuntimeError::new(format!(
                "Invalid attribute buffer (field: {}, count is missing)",
                self.name().unwrap_or("undefined")
            )));
        };
        let count = count as usize;
        let mut offset = offset;
        let mut byte_counts_offset: Option<usize> = None;
        let ordering = self
            .storage_info
            .get("ordering")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for ordering_value in &ordering {
            if offset >= view.len() {
                break;
            }
            let item = match ordering_value.as_str() {
                Some("ObjectIds") => "objectIds".to_string(),
                Some(item) => item.to_string(),
                None => continue,
            };
            let Some(desc) = self.storage_info.get(&item) else {
                return Err(RuntimeError::new(format!(
                    "Invalid attribute buffer (field: {}, {} is missing)",
                    self.name().unwrap_or("undefined"),
                    item
                )));
            };
            let type_ = desc.get("valueType").and_then(Value::as_str).unwrap_or_default();
            if type_ != "String" {
                if item == "attributeByteCounts" {
                    byte_counts_offset = Some(offset);
                }
                offset += numeric_type_size(type_) * count;
            } else {
                let Some(byte_counts_offset) = byte_counts_offset else {
                    return Err(RuntimeError::new(format!(
                        "Invalid attribute buffer (field: {}, attributeByteCounts is missing)",
                        self.name().unwrap_or("undefined")
                    )));
                };
                let byte_counts_type = self
                    .storage_info
                    .get("attributeByteCounts")
                    .and_then(|desc| desc.get("valueType"))
                    .and_then(Value::as_str)
                    .unwrap_or("UInt32");
                let mut byte_counts_offset = byte_counts_offset;
                for _ in 0..count {
                    if offset >= view.len() {
                        break;
                    }
                    let (value, new_offset) = self.parse_value(view, byte_counts_type, byte_counts_offset);
                    offset += value.as_u64().unwrap_or_default() as usize;
                    byte_counts_offset = new_offset;
                }
            }
        }
        if view.len() < offset {
            return Err(RuntimeError::new(format!(
                "Invalid attribute buffer size (field: {}, expected: {}, actual: {})",
                self.name().unwrap_or("undefined"),
                offset,
                view.len()
            )));
        }
        Ok(())
    }

    /// Port of `I3SField.prototype._parseBody`.
    fn parse_body(&mut self, view: &DataView, offset: usize) {
        let mut offset = offset;
        let count = self
            .header
            .as_ref()
            .and_then(|header| header.get("count"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let ordering = self
            .storage_info
            .get("ordering")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for ordering_value in &ordering {
            let item = match ordering_value.as_str() {
                Some("ObjectIds") => "objectIds".to_string(),
                Some(item) => item.to_string(),
                None => continue,
            };
            let Some(desc) = self.storage_info.get(&item).cloned() else { continue };
            let type_ = desc.get("valueType").and_then(Value::as_str).unwrap_or_default();
            let mut parsed_values: Vec<Value> = Vec::with_capacity(count);
            if type_ != "String" {
                for _ in 0..count {
                    let (value, new_offset) = self.parse_value(view, type_, offset);
                    parsed_values.push(value);
                    offset = new_offset;
                }
            } else {
                for index in 0..count as u64 {
                    let string_len = self
                        .values
                        .get("attributeByteCounts")
                        .and_then(|values| values.get(index as usize))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let mut string_content = String::new();
                    for _ in 0..string_len {
                        let (value, new_offset) = self.parse_value(view, "String", offset);
                        if let Some(c) = value.as_str().and_then(|s| s.chars().next()) {
                            if c as u32 != 0 {
                                string_content.push(c);
                            }
                        }
                        offset = new_offset;
                    }
                    parsed_values.push(Value::String(string_content));
                }
            }
            self.values.insert(item, Value::Array(parsed_values));
        }
    }

    /// Port of `I3SField.prototype._parseValue`.
    fn parse_value(&self, view: &DataView, type_: &str, offset: usize) -> (Value, usize) {
        match type_ {
            "UInt8" => (Value::from(view.u8_at(offset)), offset + 1),
            "Int8" => (Value::from(view.i8_at(offset)), offset + 1),
            "UInt16" => (Value::from(view.u16_at_le(offset)), offset + 2),
            "Int16" => (Value::from(view.i16_at_le(offset)), offset + 2),
            "UInt32" => (Value::from(view.u32_at_le(offset)), offset + 4),
            "Oid32" => (Value::from(view.u32_at_le(offset)), offset + 4),
            "Int32" => (Value::from(view.i32_at_le(offset)), offset + 4),
            "UInt64" => {
                let left = view.u32_at_le(offset) as u64;
                let right = view.u32_at_le(offset + 4) as u64;
                (Value::from(left as f64 + 4294967296.0 * right as f64), offset + 8)
            }
            "Int64" => {
                let left = view.u32_at_le(offset) as u64;
                let right = view.u32_at_le(offset + 4) as u64;
                let value = if right < 2147483648u64 {
                    left as f64 + 4294967296.0 * right as f64
                } else {
                    left as f64 + 4294967296.0 * (right as f64 - 4294967296.0)
                };
                (Value::from(value), offset + 8)
            }
            "Float32" => (Value::from(view.f32_at_le(offset)), offset + 4),
            "Float64" => (Value::from(view.f64_at_le(offset)), offset + 8),
            "String" => (Value::String(char::from(view.u8_at(offset)).to_string()), offset + 1),
            _ => (Value::Null, offset),
        }
    }
}

/// `getNumericTypeSize`: the storage size of a numeric I3S type, 0 if not one.
fn numeric_type_size(type_: &str) -> usize {
    match type_ {
        "UInt8" | "Int8" => 1,
        "UInt16" | "Int16" => 2,
        "UInt32" | "Int32" | "Oid32" | "Float32" => 4,
        "UInt64" | "Int64" | "Float64" => 8,
        _ => 0,
    }
}

/// `getValueTypeSize`: the storage size of a value type.
fn value_type_size(type_: &str) -> usize {
    if type_ == "String" {
        1
    } else {
        numeric_type_size(type_)
    }
}

/// Minimal `DataView` port over a byte slice: `get*(offset, true)` reads
/// little-endian.
struct DataView<'a> {
    bytes: &'a [u8],
}

impl<'a> DataView<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn u8_at(&self, offset: usize) -> u8 {
        self.bytes[offset]
    }

    fn i8_at(&self, offset: usize) -> i8 {
        self.bytes[offset] as i8
    }

    fn u16_at_le(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    fn i16_at_le(&self, offset: usize) -> i16 {
        i16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    fn u32_at_le(&self, offset: usize) -> u32 {
        u32::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
        ])
    }

    fn i32_at_le(&self, offset: usize) -> i32 {
        i32::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
        ])
    }

    fn f32_at_le(&self, offset: usize) -> f32 {
        f32::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
        ])
    }

    fn f64_at_le(&self, offset: usize) -> f64 {
        f64::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
            self.bytes[offset + 4],
            self.bytes[offset + 5],
            self.bytes[offset + 6],
            self.bytes[offset + 7],
        ])
    }
}
