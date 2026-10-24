//! CesiumJS `PntsParser`: reads a Point Cloud tile's feature table into
//! position, color, normal and batch ID attributes, or the
//! `3DTILES_draco_point_compression` buffer and property ids to decode.
//!
//! Attributes from the feature table come first only for what Draco does not
//! provide. As upstream:
//! - `hasConstantColor` reads a property the parse never sets, so it is false
//!   whenever colors are parsed here (`None` when Draco provides them);
//! - `POINTS_LENGTH` and `BATCH_LENGTH` are whatever `getGlobalProperty` reads,
//!   including a one-element typed array for a binary global;
//! - an empty batch table JSON is dropped, and without a batch table binary,
//!   properties pointing into one are removed unless Draco decodes them
//!   (upstream logs a one-time warning; the names are returned here).

use super::b3dm_parser::{batch_table, read_u32, section_json, uint8_view, SIZE_OF_UINT32};
use super::cesium_3d_tile_feature_table::{Cesium3DTileFeatureTable, GlobalProperty};
use super::gltf_pipeline::{js, PipelineError};
use super::{AttributeType, VertexAttributeSemantic};
use crate::core::{Cartesian3, Color, ComponentDatatype, RuntimeError, TypedArray};
use serde_json::{Map, Value};
use std::rc::Rc;
use std::sync::Arc;

/// `parsedContent.draco`: what the Draco decoder needs.
#[derive(Clone, Debug, PartialEq)]
pub struct PntsDraco {
    pub buffer: Vec<u8>,
    pub feature_table_properties: Value,
    pub batch_table_properties: Option<Value>,
    /// `combine(featureTableProperties, batchTableProperties)`.
    pub properties: Value,
    pub dequantize_in_shader: bool,
}

/// One parsed attribute. Fields upstream does not set for an attribute are `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct PntsAttribute {
    pub name: &'static str,
    pub semantic: VertexAttributeSemantic,
    pub set_index: Option<u32>,
    pub typed_array: Option<Rc<TypedArray>>,
    pub constant_color: Option<Color>,
    pub component_datatype: ComponentDatatype,
    pub attribute_type: AttributeType,
    pub normalized: Option<bool>,
    pub is_quantized: Option<bool>,
    pub is_rgb565: Option<bool>,
    pub is_translucent: Option<bool>,
    pub oct_encoded: Option<bool>,
    pub oct_encoded_zxy: Option<bool>,
    pub quantized_range: Option<u32>,
    pub quantized_volume_offset: Option<Cartesian3>,
    pub quantized_volume_scale: Option<Cartesian3>,
    pub quantized_component_datatype: Option<ComponentDatatype>,
    pub quantized_type: Option<AttributeType>,
}

impl PntsAttribute {
    fn new(
        name: &'static str,
        semantic: VertexAttributeSemantic,
        component_datatype: ComponentDatatype,
        attribute_type: AttributeType,
    ) -> Self {
        Self {
            name,
            semantic,
            set_index: None,
            typed_array: None,
            constant_color: None,
            component_datatype,
            attribute_type,
            normalized: None,
            is_quantized: None,
            is_rgb565: None,
            is_translucent: None,
            oct_encoded: None,
            oct_encoded_zxy: None,
            quantized_range: None,
            quantized_volume_offset: None,
            quantized_volume_scale: None,
            quantized_component_datatype: None,
            quantized_type: None,
        }
    }
}

/// `PntsParser.parse`'s result.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedPnts {
    pub draco: Option<PntsDraco>,
    pub has_positions: bool,
    pub has_colors: bool,
    pub has_normals: bool,
    pub has_batch_ids: bool,
    pub is_translucent: bool,
    pub has_constant_color: Option<bool>,
    pub rtc_center: Option<Cartesian3>,
    pub points_length: GlobalProperty,
    pub positions: Option<PntsAttribute>,
    pub normals: Option<PntsAttribute>,
    pub colors: Option<PntsAttribute>,
    pub batch_ids: Option<PntsAttribute>,
    pub batch_length: Option<GlobalProperty>,
    pub batch_table_json: Option<Value>,
    pub batch_table_binary: Option<Vec<u8>>,
    /// Batch table properties removed for pointing into a missing binary body.
    pub removed_batch_table_properties: Vec<String>,
}

fn error(e: PipelineError) -> RuntimeError {
    RuntimeError::new(e.to_string())
}

fn cartesian(value: &GlobalProperty) -> Cartesian3 {
    let c = |i| value.element(i).unwrap_or(f64::NAN);
    Cartesian3::new(c(0), c(1), c(2))
}

/// `Uint8Array.prototype.slice(start, end)` with relative, clamped indices.
fn slice(bytes: &[u8], start: f64, end: f64) -> Vec<u8> {
    let len = bytes.len() as f64;
    let index = |v: f64| {
        let v = if v.is_nan() { 0.0 } else { v.trunc() };
        (if v < 0.0 { (len + v).max(0.0) } else { v.min(len) }) as usize
    };
    let (start, end) = (index(start), index(end));
    if end > start {
        bytes[start..end].to_vec()
    } else {
        Vec::new()
    }
}

/// Cesium's `combine(a, b)`: `a`'s properties, then `b`'s that `a` lacks.
fn combine(a: Option<&Value>, b: Option<&Value>) -> Value {
    let mut result = Map::new();
    for object in [a, b].into_iter().flatten() {
        for key in js::for_in(Some(object)) {
            if !result.contains_key(&key) {
                result.insert(key.clone(), js::get(Some(object), &key).cloned().unwrap_or(Value::Null));
            }
        }
    }
    Value::Object(result)
}

struct DracoFlags {
    draco: Option<PntsDraco>,
    has_positions: bool,
    has_colors: bool,
    has_normals: bool,
    has_batch_ids: bool,
    is_translucent: bool,
}

fn parse_draco_properties(
    table: &Cesium3DTileFeatureTable,
    batch_table_json: Option<&Value>,
) -> Result<DracoFlags, RuntimeError> {
    let extension = |json: Option<&Value>| {
        js::get(js::get(json, "extensions"), "3DTILES_draco_point_compression").filter(|v| !v.is_null()).cloned()
    };
    let feature_table_draco = extension(Some(&table.json));
    let batch_table_properties = extension(batch_table_json).and_then(|d| js::get(Some(&d), "properties").cloned());
    let mut flags = DracoFlags {
        draco: None,
        has_positions: false,
        has_colors: false,
        has_normals: false,
        has_batch_ids: false,
        is_translucent: false,
    };
    let Some(draco) = feature_table_draco else { return Ok(flags) };
    let properties = js::get(Some(&draco), "properties").filter(|v| !v.is_null());
    let byte_offset = js::get(Some(&draco), "byteOffset").filter(|v| !v.is_null());
    let byte_length = js::get(Some(&draco), "byteLength").filter(|v| !v.is_null());
    let (Some(properties), Some(byte_offset), Some(byte_length)) = (properties, byte_offset, byte_length) else {
        return Err(RuntimeError::new("Draco properties, byteOffset, and byteLength must be defined"));
    };
    let start = js::to_number(Some(byte_offset));
    let buffer = slice(table.buffer.bytes(), start, start + js::to_number(Some(byte_length)));
    let has = |name| js::defined(js::get(Some(properties), name));
    flags.has_positions = has("POSITION");
    flags.has_colors = has("RGB") || has("RGBA");
    flags.has_normals = has("NORMAL");
    flags.has_batch_ids = has("BATCH_ID");
    flags.is_translucent = has("RGBA");
    flags.draco = Some(PntsDraco {
        buffer,
        feature_table_properties: properties.clone(),
        properties: combine(Some(properties), batch_table_properties.as_ref()),
        batch_table_properties,
        dequantize_in_shader: true,
    });
    Ok(flags)
}

fn parse_positions(table: &Cesium3DTileFeatureTable) -> Result<Option<PntsAttribute>, RuntimeError> {
    use VertexAttributeSemantic::Position;
    if table.has_property("POSITION") {
        let mut a = PntsAttribute::new("POSITION", Position, ComponentDatatype::Float, AttributeType::Vec3);
        a.typed_array = table.get_property_array("POSITION", ComponentDatatype::Float, 3).map_err(error)?;
        a.is_quantized = Some(false);
        return Ok(Some(a));
    }
    if !table.has_property("POSITION_QUANTIZED") {
        return Ok(None);
    }
    let positions =
        table.get_property_array("POSITION_QUANTIZED", ComponentDatatype::UnsignedShort, 3).map_err(error)?;
    let global = |name| table.get_global_property(name, Some(ComponentDatatype::Float), Some(3)).map_err(error);
    let scale = global("QUANTIZED_VOLUME_SCALE")?.ok_or_else(|| {
        RuntimeError::new("Global property: QUANTIZED_VOLUME_SCALE must be defined for quantized positions.")
    })?;
    let offset = global("QUANTIZED_VOLUME_OFFSET")?.ok_or_else(|| {
        RuntimeError::new("Global property: QUANTIZED_VOLUME_OFFSET must be defined for quantized positions.")
    })?;
    let mut a = PntsAttribute::new("POSITION", Position, ComponentDatatype::Float, AttributeType::Vec3);
    a.typed_array = positions;
    a.is_quantized = Some(true);
    a.quantized_range = Some((1 << 16) - 1);
    a.quantized_volume_offset = Some(cartesian(&offset));
    a.quantized_volume_scale = Some(cartesian(&scale));
    a.quantized_component_datatype = Some(ComponentDatatype::UnsignedShort);
    a.quantized_type = Some(AttributeType::Vec3);
    Ok(Some(a))
}

fn parse_colors(table: &Cesium3DTileFeatureTable) -> Result<Option<PntsAttribute>, RuntimeError> {
    use VertexAttributeSemantic::Color as ColorSemantic;
    let array = |name, datatype, length| table.get_property_array(name, datatype, length).map_err(error);
    let mut a;
    if table.has_property("RGBA") {
        a = PntsAttribute::new("COLOR", ColorSemantic, ComponentDatatype::UnsignedByte, AttributeType::Vec4);
        a.typed_array = array("RGBA", ComponentDatatype::UnsignedByte, 4)?;
        a.normalized = Some(true);
        a.is_rgb565 = Some(false);
        a.is_translucent = Some(true);
    } else if table.has_property("RGB") {
        a = PntsAttribute::new("COLOR", ColorSemantic, ComponentDatatype::UnsignedByte, AttributeType::Vec3);
        a.typed_array = array("RGB", ComponentDatatype::UnsignedByte, 3)?;
        a.normalized = Some(true);
        a.is_rgb565 = Some(false);
        a.is_translucent = Some(false);
    } else if table.has_property("RGB565") {
        a = PntsAttribute::new("COLOR", ColorSemantic, ComponentDatatype::Float, AttributeType::Vec3);
        a.typed_array = array("RGB565", ComponentDatatype::UnsignedShort, 1)?;
        a.normalized = Some(false);
        a.is_rgb565 = Some(true);
        a.is_translucent = Some(false);
    } else if table.has_property("CONSTANT_RGBA") {
        let rgba = table
            .get_global_property("CONSTANT_RGBA", Some(ComponentDatatype::UnsignedByte), Some(4))
            .map_err(error)?
            .expect("has CONSTANT_RGBA");
        let byte = |i| rgba.element(i).unwrap_or(f64::NAN);
        let alpha = byte(3);
        a = PntsAttribute::new("COLOR", ColorSemantic, ComponentDatatype::Float, AttributeType::Vec4);
        a.constant_color = Some(Color::from_bytes(byte(0), byte(1), byte(2), alpha));
        a.is_quantized = Some(false);
        a.is_translucent = Some(alpha < 255.0);
    } else {
        return Ok(None);
    }
    a.set_index = Some(0);
    Ok(Some(a))
}

fn parse_normals(table: &Cesium3DTileFeatureTable) -> Result<Option<PntsAttribute>, RuntimeError> {
    use VertexAttributeSemantic::Normal;
    let mut a = PntsAttribute::new("NORMAL", Normal, ComponentDatatype::Float, AttributeType::Vec3);
    if table.has_property("NORMAL") {
        a.typed_array = table.get_property_array("NORMAL", ComponentDatatype::Float, 3).map_err(error)?;
        a.oct_encoded = Some(false);
        a.oct_encoded_zxy = Some(false);
    } else if table.has_property("NORMAL_OCT16P") {
        a.typed_array = table.get_property_array("NORMAL_OCT16P", ComponentDatatype::UnsignedByte, 2).map_err(error)?;
        a.oct_encoded = Some(true);
        a.oct_encoded_zxy = Some(false);
        a.quantized_range = Some((1 << 8) - 1);
        a.quantized_type = Some(AttributeType::Vec2);
        a.quantized_component_datatype = Some(ComponentDatatype::UnsignedByte);
    } else {
        return Ok(None);
    }
    Ok(Some(a))
}

fn parse_batch_ids(table: &Cesium3DTileFeatureTable) -> Result<Option<PntsAttribute>, RuntimeError> {
    let Some(batch_ids) = table.get_property_array("BATCH_ID", ComponentDatatype::UnsignedShort, 1).map_err(error)?
    else {
        return Ok(None);
    };
    let datatype = ComponentDatatype::from_typed_array(&batch_ids);
    let mut a = PntsAttribute::new("_FEATURE_ID", VertexAttributeSemantic::FeatureId, datatype, AttributeType::Scalar);
    a.typed_array = Some(batch_ids);
    a.set_index = Some(0);
    Ok(Some(a))
}

/// `PntsParser.parse(arrayBuffer, byteOffset)`.
pub fn parse(array_buffer: &Arc<Vec<u8>>, byte_offset: usize) -> Result<ParsedPnts, RuntimeError> {
    let buffer = array_buffer.as_slice();
    let mut offset = byte_offset + SIZE_OF_UINT32; // magic
    let version = read_u32(buffer, offset)?;
    if version != 1 {
        return Err(RuntimeError::new(format!(
            "Only Point Cloud tile version 1 is supported.  Version {version} is not."
        )));
    }
    offset += SIZE_OF_UINT32;
    offset += SIZE_OF_UINT32; // byteLength is not read
    let feature_table_json_length = read_u32(buffer, offset)?;
    if feature_table_json_length == 0 {
        return Err(RuntimeError::new("Feature table must have a byte length greater than zero"));
    }
    offset += SIZE_OF_UINT32;
    let feature_table_binary_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let batch_table_json_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let batch_table_binary_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;

    let feature_table_json = section_json(buffer, offset, feature_table_json_length)?;
    offset += feature_table_json_length as usize;
    let feature_table_binary = uint8_view(array_buffer, offset, feature_table_binary_length as i64)?;
    offset += feature_table_binary_length as usize;
    let (mut batch_table_json, batch_table_binary) =
        batch_table(array_buffer, &mut offset, batch_table_json_length, batch_table_binary_length)?;
    if batch_table_json.as_ref().is_some_and(|json| js::key_count(Some(json)) == 0) {
        batch_table_json = None;
    }

    let mut table = Cesium3DTileFeatureTable::new(feature_table_json, feature_table_binary);
    let points_length = table
        .get_global_property("POINTS_LENGTH", None, None)
        .map_err(error)?
        .ok_or_else(|| RuntimeError::new("Feature table global property: POINTS_LENGTH must be defined"))?;
    table.features_length = points_length.to_number();
    let rtc_center = table
        .get_global_property("RTC_CENTER", Some(ComponentDatatype::Float), Some(3))
        .map_err(error)?
        .map(|c| cartesian(&c));

    let flags = parse_draco_properties(&table, batch_table_json.as_ref())?;
    let mut parsed = ParsedPnts {
        draco: flags.draco,
        has_positions: flags.has_positions,
        has_colors: flags.has_colors,
        has_normals: flags.has_normals,
        has_batch_ids: flags.has_batch_ids,
        is_translucent: flags.is_translucent,
        has_constant_color: None,
        rtc_center,
        points_length,
        positions: None,
        normals: None,
        colors: None,
        batch_ids: None,
        batch_length: None,
        batch_table_json: None,
        batch_table_binary: None,
        removed_batch_table_properties: Vec::new(),
    };
    if !parsed.has_positions {
        parsed.positions = parse_positions(&table)?;
        parsed.has_positions = parsed.positions.is_some();
    }
    if !parsed.has_positions {
        return Err(RuntimeError::new("Either POSITION or POSITION_QUANTIZED must be defined."));
    }
    if !parsed.has_normals {
        parsed.normals = parse_normals(&table)?;
        parsed.has_normals = parsed.normals.is_some();
    }
    if !parsed.has_colors {
        parsed.colors = parse_colors(&table)?;
        parsed.has_colors = parsed.colors.is_some();
        // Upstream reads `parsedContent.constantColor`, which is never set.
        parsed.has_constant_color = Some(false);
        parsed.is_translucent = parsed.colors.as_ref().is_some_and(|c| c.is_translucent == Some(true));
    }
    if !parsed.has_batch_ids {
        parsed.batch_ids = parse_batch_ids(&table)?;
        parsed.has_batch_ids = parsed.batch_ids.is_some();
    }
    if parsed.has_batch_ids {
        let batch_length = table.get_global_property("BATCH_LENGTH", None, None).map_err(error)?.ok_or_else(|| {
            RuntimeError::new("Global property: BATCH_LENGTH must be defined when BATCH_ID is defined.")
        })?;
        parsed.batch_length = Some(batch_length);
    }
    if batch_table_json.is_some() || batch_table_binary.is_some() {
        parsed.batch_table_json = batch_table_json;
        parsed.batch_table_binary = batch_table_binary;
    }
    remove_invalid_binary_body_references(&mut parsed)?;
    Ok(parsed)
}

fn remove_invalid_binary_body_references(parsed: &mut ParsedPnts) -> Result<(), RuntimeError> {
    let Some(json) = parsed.batch_table_json.as_mut() else { return Ok(()) };
    if parsed.batch_table_binary.is_some() {
        return Ok(());
    }
    let draco_names = parsed.draco.as_ref().map_or(Vec::new(), |d| js::for_in(d.batch_table_properties.as_ref()));
    let mut invalid = Vec::new();
    for name in js::for_in(Some(json)) {
        let property = js::get(Some(json), &name);
        if property.is_some_and(Value::is_null) {
            return Err(RuntimeError::new("TypeError: Cannot read properties of null (reading 'byteOffset')"));
        }
        if js::defined(js::get(property, "byteOffset")) && !draco_names.contains(&name) {
            invalid.push(name);
        }
    }
    for name in &invalid {
        js::delete(json, name);
    }
    parsed.removed_batch_table_properties = invalid;
    Ok(())
}
