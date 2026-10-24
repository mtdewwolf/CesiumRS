//! CesiumJS `parseBatchTable`: transcodes a 3D Tiles 1.0 batch table into
//! [`StructuralMetadata`] with a generated `_batchTable` class.
//!
//! JSON array properties become a [`JsonMetadataTable`], the batch table
//! hierarchy a [`BatchTableHierarchy`], and binary properties either a
//! binary [`MetadataTable`] or, for point clouds, a [`PropertyAttribute`]
//! whose values are returned as custom vertex attributes.

use super::gltf_pipeline::js;
use super::metadata_table_property::ByteView;
use super::metadata_value::{developer, JsMap, MetadataError};
use super::model_components::Attribute;
use super::structural_metadata::{StructuralMetadataOptions, StructuralMetadataWarning};
use super::{
    get_binary_accessor::{get_binary_accessor, ComponentType},
    AttributeType, BatchTableHierarchy, JsonMetadataTable, MetadataClass, MetadataSchema, MetadataTable,
    PropertyAttribute, PropertyTable, PropertyTableOptions, StructuralMetadata,
};
use crate::core::math::js_number_to_string;
use crate::core::{ComponentDatatype, TypedArray};
use crate::renderer::Gl;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub struct ParseBatchTableOptions<'a> {
    /// The number of features.
    pub count: usize,
    pub batch_table: &'a Value,
    /// The batch table binary: a view of the tile's buffer. Properties are
    /// read relative to its byte offset, and may run past its end.
    pub binary_body: Option<ByteView<'a>>,
    /// Point clouds read binary properties as vertex attributes.
    pub parse_as_property_attributes: bool,
    /// Draco-decoded `.pnts` properties by id (`property.typedArray` in
    /// CesiumJS), read in place of the binary body.
    pub typed_arrays: Option<&'a HashMap<String, TypedArray>>,
}

/// `custom_attribute_output` receives one attribute per binary property
/// when parsing as property attributes, which requires it.
pub fn parse_batch_table<G: Gl>(
    options: &ParseBatchTableOptions<'_>,
    custom_attribute_output: Option<&mut Vec<Attribute<G>>>,
    warnings: &mut Vec<StructuralMetadataWarning>,
) -> Result<StructuralMetadata<G>, MetadataError> {
    let batch_table = options.batch_table;
    if !(batch_table.is_object() || batch_table.is_array()) {
        return Err(developer("Expected options.batchTable to be typeof object"));
    }
    if options.parse_as_property_attributes && custom_attribute_output.is_none() {
        return Err(developer("customAttributeOutput is required when parsing batch table as property attributes"));
    }
    let feature_count = options.count;
    let partition = partition_properties(batch_table, warnings);

    let json_metadata_table = match &partition.json_properties {
        Some(properties) => Some(JsonMetadataTable::new(feature_count, &Value::Object(properties.clone()))?),
        None => None,
    };
    let hierarchy = match partition.hierarchy.filter(|h| !h.is_null()) {
        Some(extension) => Some(BatchTableHierarchy::new(extension, options.binary_body)?),
        None => None,
    };

    let class_name = MetadataClass::BATCH_TABLE_CLASS_NAME;
    let (metadata_table, property_attributes, schema) = match custom_attribute_output {
        Some(output) if options.parse_as_property_attributes => {
            let (schema, property_attribute) = transcode_binary_properties_as_property_attributes(
                feature_count,
                class_name,
                &partition.binary_properties,
                options,
                output,
                warnings,
            )?;
            (None, vec![property_attribute], schema)
        }
        _ => {
            let (schema, table) =
                transcode_binary_properties(feature_count, class_name, &partition.binary_properties, options)?;
            (Some(table), Vec::new(), schema)
        }
    };

    let mut property_tables = Vec::new();
    if metadata_table.is_some() || json_metadata_table.is_some() || hierarchy.is_some() {
        property_tables.push(PropertyTable::new(PropertyTableOptions {
            id: Some(0.into()),
            name: Some("Batch Table".into()),
            count: feature_count,
            metadata_table,
            json_metadata_table,
            batch_table_hierarchy: hierarchy,
            ..Default::default()
        }));
    }

    Ok(StructuralMetadata::new(StructuralMetadataOptions {
        schema,
        property_tables: Some(property_tables),
        property_textures: Vec::new(),
        property_attributes,
        statistics: None,
        extras: partition.extras.cloned(),
        extensions: partition.extensions.cloned(),
    }))
}

struct Partition<'a> {
    /// Every property that is not an array, in key order.
    binary_properties: Vec<(String, &'a Value)>,
    /// Array properties; `None` when there are none.
    json_properties: Option<Map<String, Value>>,
    hierarchy: Option<&'a Value>,
    extras: Option<&'a Value>,
    extensions: Option<&'a Value>,
}

fn partition_properties<'a>(batch_table: &'a Value, warnings: &mut Vec<StructuralMetadataWarning>) -> Partition<'a> {
    let legacy_hierarchy = js::get(Some(batch_table), "HIERARCHY");
    let extras = js::get(Some(batch_table), "extras");
    let extensions = js::get(Some(batch_table), "extensions");
    let hierarchy = if js::defined(legacy_hierarchy) {
        warnings.push(StructuralMetadataWarning::BatchTableHierarchyDeprecated);
        legacy_hierarchy
    } else if js::defined(extensions) {
        js::get(extensions, "3DTILES_batch_table_hierarchy")
    } else {
        None
    };

    let mut binary_properties = Vec::new();
    let mut json_properties: Option<Map<String, Value>> = None;
    for property_id in js::for_in(Some(batch_table)) {
        if matches!(property_id.as_str(), "HIERARCHY" | "extensions" | "extras") {
            continue;
        }
        let property = js::get(Some(batch_table), &property_id).expect("own key");
        if property.is_array() {
            json_properties.get_or_insert_with(Map::new).insert(property_id, property.clone());
        } else {
            binary_properties.push((property_id, property));
        }
    }
    Partition { binary_properties, json_properties, hierarchy, extras, extensions }
}

fn requires_binary(property_id: &str) -> MetadataError {
    MetadataError::Runtime(format!("Property {property_id} requires a batch table binary."))
}

fn read<'v>(property: &'v Value, key: &str) -> Result<Option<&'v Value>, MetadataError> {
    match property {
        Value::Null => Err(MetadataError::Type(format!("Cannot read properties of null (reading '{key}')"))),
        _ => Ok(js::get(Some(property), key)),
    }
}

/// `binaryBody.byteOffset + property.byteOffset`, with JavaScript's `+`: a
/// string offset concatenates.
fn property_byte_offset(body: &ByteView<'_>, byte_offset: Option<&Value>) -> f64 {
    let base = body.byte_offset as f64;
    match byte_offset {
        None => f64::NAN,
        Some(Value::Null) => base,
        Some(Value::Number(n)) => base + n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => base + f64::from(u8::from(*b)),
        Some(v) => js::to_number(Some(&Value::String(js_number_to_string(base) + &js::key(v)))),
    }
}

/// `ComponentDatatype.createArrayBufferView(datatype, body.buffer, offset,
/// components * count)`: the typed array constructor's `ToIndex` and
/// `RangeError`s.
fn create_array_buffer_view(
    property: &Value,
    body: &ByteView<'_>,
    feature_count: usize,
) -> Result<TypedArray, MetadataError> {
    let component_type = match read(property, "componentType")? {
        Some(Value::String(name)) => ComponentType::Name(name),
        Some(v) => ComponentType::Value(
            v.as_u64()
                .and_then(|value| ComponentDatatype::from_gl(value as u32))
                .ok_or_else(|| developer("componentDatatype is not a valid value."))?,
        ),
        None => return Err(developer("componentDatatype is required.")),
    };
    let attribute_type = js::get(Some(property), "type").and_then(Value::as_str).unwrap_or("");
    let accessor = get_binary_accessor(component_type, attribute_type)
        .ok_or_else(|| developer("attributeType is not a valid value."))?;
    let offset = property_byte_offset(body, js::get(Some(property), "byteOffset"));
    let offset = if offset.is_nan() { 0.0 } else { offset.trunc() };
    let size = accessor.component_datatype.get_size_in_bytes();
    let length = accessor.components_per_attribute * feature_count;
    if !(0.0..=body.buffer.len() as f64).contains(&offset) {
        return Err(MetadataError::Range(format!(
            "Start offset {} is outside the bounds of the buffer",
            js_number_to_string(offset)
        )));
    }
    let offset = offset as usize;
    if !offset.is_multiple_of(size) {
        return Err(MetadataError::Range(format!(
            "start offset of {} should be a multiple of {size}",
            typed_array_name(accessor.component_datatype)
        )));
    }
    accessor
        .create_array_buffer_view(body.buffer, offset, feature_count)
        .ok_or_else(|| MetadataError::Range(format!("Invalid typed array length: {length}")))
}

fn typed_array_name(datatype: ComponentDatatype) -> &'static str {
    match datatype {
        ComponentDatatype::Byte => "Int8Array",
        ComponentDatatype::UnsignedByte => "Uint8Array",
        ComponentDatatype::Short => "Int16Array",
        ComponentDatatype::UnsignedShort => "Uint16Array",
        ComponentDatatype::Int => "Int32Array",
        ComponentDatatype::UnsignedInt => "Uint32Array",
        ComponentDatatype::Float => "Float32Array",
        ComponentDatatype::Double => "Float64Array",
    }
}

/// The batch table's component type name as a metadata component type.
fn transcode_component_type(component_type: Option<&Value>) -> Option<&'static str> {
    Some(match component_type?.as_str()? {
        "BYTE" => "INT8",
        "UNSIGNED_BYTE" => "UINT8",
        "SHORT" => "INT16",
        "UNSIGNED_SHORT" => "UINT16",
        "INT" => "INT32",
        "UNSIGNED_INT" => "UINT32",
        "FLOAT" => "FLOAT32",
        "DOUBLE" => "FLOAT64",
        _ => return None,
    })
}

fn transcode_property_type(property: &Value) -> Result<Map<String, Value>, MetadataError> {
    let component_type = transcode_component_type(read(property, "componentType")?);
    let mut class_property = Map::new();
    if let Some(t) = js::get(Some(property), "type") {
        class_property.insert("type".into(), t.clone());
    }
    if let Some(c) = component_type {
        class_property.insert("componentType".into(), Value::from(c));
    }
    Ok(class_property)
}

fn schema_of(class_name: &str, class_properties: Map<String, Value>) -> Result<Arc<MetadataSchema>, MetadataError> {
    let schema = json!({ "classes": { class_name: { "properties": class_properties } } });
    Ok(Arc::new(MetadataSchema::from_json(&schema)?))
}

fn transcode_binary_properties(
    feature_count: usize,
    class_name: &str,
    binary_properties: &[(String, &Value)],
    options: &ParseBatchTableOptions<'_>,
) -> Result<(Arc<MetadataSchema>, MetadataTable), MetadataError> {
    let mut class_properties = Map::new();
    let mut feature_table_properties = Map::new();
    let mut offsets = Vec::new();
    for (i, (property_id, property)) in binary_properties.iter().enumerate() {
        let body = options.binary_body.as_ref().ok_or_else(|| requires_binary(property_id))?;
        // Validates the range the table reads, as the typed array would.
        create_array_buffer_view(property, body, feature_count)?;
        feature_table_properties.insert(property_id.clone(), json!({ "bufferView": i }));
        class_properties.insert(property_id.clone(), Value::Object(transcode_property_type(property)?));
        // Where the typed array starts in the tile buffer.
        let offset = property_byte_offset(body, js::get(Some(property), "byteOffset"));
        offsets.push((i.to_string(), if offset.is_nan() { 0 } else { offset as usize }));
    }
    let schema = schema_of(class_name, class_properties)?;
    let class = schema.classes.get(class_name).cloned().expect("transcoded class");
    let buffer = options.binary_body.map_or(&[][..], |b| b.buffer);
    let views: JsMap<ByteView<'_>> =
        offsets.iter().map(|(id, offset)| (id.clone(), ByteView { buffer, byte_offset: *offset })).collect();
    let table = MetadataTable::new(feature_count, Some(&Value::Object(feature_table_properties)), class, &views)?;
    Ok((schema, table))
}

/// CesiumJS `ModelUtility.sanitizeGlslIdentifier`: runs of characters other
/// than ASCII letters and digits become one underscore, a `gl_` prefix is
/// dropped, and a leading digit gets an underscore.
pub fn sanitize_glsl_identifier(identifier: &str) -> String {
    let mut sanitized = String::with_capacity(identifier.len());
    let mut in_run = false;
    for c in identifier.chars() {
        if c.is_ascii_alphanumeric() {
            sanitized.push(c);
            in_run = false;
        } else if !in_run {
            sanitized.push('_');
            in_run = true;
        }
    }
    let sanitized = sanitized.strip_prefix("gl_").map(str::to_string).unwrap_or(sanitized);
    if sanitized.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{sanitized}")
    } else {
        sanitized
    }
}

fn transcode_binary_properties_as_property_attributes<G: Gl>(
    feature_count: usize,
    class_name: &str,
    binary_properties: &[(String, &Value)],
    options: &ParseBatchTableOptions<'_>,
    output: &mut Vec<Attribute<G>>,
    warnings: &mut Vec<StructuralMetadataWarning>,
) -> Result<(Arc<MetadataSchema>, PropertyAttribute), MetadataError> {
    let mut class_properties = Map::new();
    let mut property_attribute_properties = Map::new();
    let mut next_placeholder_id = 0;
    for (property_id, property) in binary_properties {
        let decoded = options.typed_arrays.and_then(|t| t.get(property_id));
        if options.binary_body.is_none() {
            read(property, "typedArray")?;
            if decoded.is_none() {
                return Err(requires_binary(property_id));
            }
        }

        // An empty or repeated name gets a placeholder. That works for
        // styling, but tiles may number the placeholders differently.
        let mut sanitized_property_id = sanitize_glsl_identifier(property_id);
        if sanitized_property_id.is_empty() || class_properties.contains_key(&sanitized_property_id) {
            sanitized_property_id = format!("property_{next_placeholder_id}");
            next_placeholder_id += 1;
        }

        let mut class_property = transcode_property_type(property)?;
        class_property.insert("name".into(), Value::from(property_id.as_str()));
        class_properties.insert(sanitized_property_id.clone(), Value::Object(class_property));

        // Named like a glTF custom attribute: 'Temperature ℃' is _TEMPERATURE_.
        let mut custom_attribute_name = sanitized_property_id.to_ascii_uppercase();
        if !custom_attribute_name.starts_with('_') {
            custom_attribute_name = format!("_{custom_attribute_name}");
        }

        let mut typed_array = match decoded {
            Some(array) => array.clone(),
            None => create_array_buffer_view(property, options.binary_body.as_ref().expect("checked"), feature_count)?,
        };
        let component_datatype = ComponentDatatype::from_typed_array(&typed_array);
        if matches!(
            component_datatype,
            ComponentDatatype::Int | ComponentDatatype::UnsignedInt | ComponentDatatype::Double
        ) {
            warnings
                .push(StructuralMetadataWarning::PntsPropertyCastToFloats { attribute: custom_attribute_name.clone() });
            typed_array = TypedArray::Float32(typed_array.to_f64_vec().into_iter().map(|x| x as f32).collect());
        }
        output.push(Attribute {
            name: Some(custom_attribute_name.clone()),
            count: Some(feature_count),
            attribute_type: js::get(Some(property), "type")
                .and_then(Value::as_str)
                .and_then(AttributeType::from_str_value),
            component_datatype: Some(ComponentDatatype::from_typed_array(&typed_array)),
            typed_array: Some(typed_array),
            ..Default::default()
        });

        property_attribute_properties.insert(sanitized_property_id, json!({ "attribute": custom_attribute_name }));
    }
    let schema = schema_of(class_name, class_properties)?;
    let class = schema.classes.get(class_name).cloned().expect("transcoded class");
    let property_attribute =
        PropertyAttribute::new(&json!({ "properties": property_attribute_properties }), class, None, None)?;
    Ok((schema, property_attribute))
}
