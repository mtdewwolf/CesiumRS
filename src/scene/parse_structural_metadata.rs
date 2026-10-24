//! CesiumJS `parseStructuralMetadata`: builds [`StructuralMetadata`] from a
//! glTF's `EXT_structural_metadata` extension, its loaded schema, buffer
//! views and textures.
//!
//! Each property table's GPU-compatible properties are packed into one RGBA8
//! texture, a row per property and a texel per feature, padded with zeros.
//! Class properties the table leaves out are filled with their `noData`
//! value, written the way CesiumJS writes it. Packing failures and oversized
//! tables are reported as [`StructuralMetadataWarning`]s and leave the table
//! without a texture. Without a context no texture is made.

use super::gltf_pipeline::js;
use super::metadata_table_property::ByteView;
use super::metadata_value::{developer, JsMap, MetadataError, MetadataValue};
use super::property_texture_property::GltfTextures;
use super::structural_metadata::{StructuralMetadataOptions, StructuralMetadataWarning};
use super::{
    FrameResource, MetadataClass, MetadataClassProperty, MetadataComponentType, MetadataNumber, MetadataSchema,
    MetadataTable, MetadataType, PropertyAttribute, PropertyTable, PropertyTableOptions, PropertyTexture,
    StructuralMetadata,
};
use crate::core::math::js_number_to_string;
use crate::core::{PixelDatatype, PixelFormat};
use crate::renderer::{Context, Gl, Sampler, Texture, TextureOptions};
use serde_json::Value;
use std::borrow::Cow;
use std::rc::Rc;
use std::sync::Arc;

/// Property table textures always use four channels.
const NUM_CHANNELS: usize = 4;

pub struct ParseStructuralMetadataOptions<'a, G: Gl> {
    pub extension: &'a Value,
    /// The schema, which the caller loads.
    pub schema: Arc<MetadataSchema>,
    /// The buffer views the property tables read, by glTF index.
    pub buffer_views: &'a JsMap<ByteView<'a>>,
    pub textures: &'a GltfTextures<G>,
    pub context: Option<&'a Context<G>>,
}

pub fn parse_structural_metadata<G: Gl + 'static>(
    options: &ParseStructuralMetadataOptions<'_, G>,
    warnings: &mut Vec<StructuralMetadataWarning>,
) -> Result<StructuralMetadata<G>, MetadataError> {
    let extension = options.extension;
    if !(extension.is_object() || extension.is_array()) {
        return Err(developer("Expected options.extension to be typeof object"));
    }
    let schema = &options.schema;

    let mut property_tables = Vec::new();
    for (i, property_table) in array(extension, "propertyTables").iter().enumerate() {
        let class = class_of(schema, property_table);
        let texture = create_texture_for_property_table(
            property_table,
            options.buffer_views,
            class.as_deref(),
            options.context,
            warnings,
        )?;
        let class = class.ok_or_else(|| developer("Expected options.class to be typeof object"))?;
        let count = count_of(property_table)?;
        let properties = js::get(Some(property_table), "properties").filter(|p| !p.is_null());
        let metadata_table = MetadataTable::new(count, properties, class, options.buffer_views)?;
        property_tables.push(PropertyTable::new(PropertyTableOptions {
            id: Some(i.into()),
            name: string(property_table, "name"),
            count,
            metadata_table: Some(metadata_table),
            extras: property_table.get("extras").cloned(),
            extensions: property_table.get("extensions").cloned(),
            texture,
            ..Default::default()
        }));
    }

    let mut property_textures = Vec::new();
    for (i, property_texture) in array(extension, "propertyTextures").iter().enumerate() {
        let class = class_of(schema, property_texture)
            .ok_or_else(|| developer("Expected options.class to be typeof object"))?;
        property_textures.push(PropertyTexture::new(
            property_texture,
            class,
            options.textures,
            string(property_texture, "name"),
            Some(i.into()),
        )?);
    }

    let mut property_attributes = Vec::new();
    for (i, property_attribute) in array(extension, "propertyAttributes").iter().enumerate() {
        let class = class_of(schema, property_attribute)
            .ok_or_else(|| developer("Expected options.class to be typeof object"))?;
        property_attributes.push(PropertyAttribute::new(
            property_attribute,
            class,
            string(property_attribute, "name"),
            Some(i),
        )?);
    }

    Ok(StructuralMetadata::new(StructuralMetadataOptions {
        schema: schema.clone(),
        property_tables: Some(property_tables),
        property_textures,
        property_attributes,
        statistics: extension.get("statistics").cloned(),
        extras: extension.get("extras").cloned(),
        extensions: extension.get("extensions").cloned(),
    }))
}

/// `extension[key]` when it is an array; `for (i < x.length)` reads nothing
/// from anything else.
pub(super) fn array<'a>(extension: &'a Value, key: &str) -> &'a [Value] {
    extension.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// `schema.classes[json.class]`.
pub(super) fn class_of(schema: &MetadataSchema, json: &Value) -> Option<Arc<MetadataClass>> {
    let key = json.get("class").map_or_else(|| "undefined".to_string(), js::key);
    schema.classes.get(&key).cloned()
}

pub(super) fn string(json: &Value, key: &str) -> Option<String> {
    json.get(key).and_then(Value::as_str).map(str::to_string)
}

/// A table's `count`, which `MetadataTable` requires to be positive.
pub(super) fn count_of(table: &Value) -> Result<usize, MetadataError> {
    match table.get("count").and_then(Value::as_f64) {
        Some(count) if count > 0.0 && count.fract() == 0.0 => Ok(count as usize),
        _ => Err(developer("Expected options.count to be greater than 0")),
    }
}

/// `String(value)`, with a missing value as `undefined`.
fn js_string(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".to_string(), js::key)
}

fn create_texture_for_property_table<G: Gl + 'static>(
    property_table: &Value,
    buffer_views: &JsMap<ByteView<'_>>,
    class: Option<&MetadataClass>,
    context: Option<&Context<G>>,
    warnings: &mut Vec<StructuralMetadataWarning>,
) -> Result<Option<FrameResource>, MetadataError> {
    let properties = property_table.get("properties").filter(|p| !p.is_null());
    let (Some(properties), Some(context)) = (properties, context) else {
        return Ok(None);
    };
    let count = property_table.get("count");
    let table = js_string(property_table.get("name"));
    let views = match collect_gpu_compatible_property_buffer_views(properties, buffer_views, class, count) {
        Ok(views) => views,
        Err(error) => {
            warnings.push(StructuralMetadataWarning::PropertyTableTextureFailed { table, error });
            return Ok(None);
        }
    };
    if views.is_empty() {
        return Ok(None);
    }
    // Every view matched the count, so it is a number.
    let num_features = js::to_number(count);
    let maximum_texture_size = context.limits().map_err(|e| MetadataError::Runtime(e.message))?.maximum_texture_size;
    if num_features > maximum_texture_size as f64 || views.len() > maximum_texture_size as usize {
        warnings
            .push(StructuralMetadataWarning::PropertyTableTextureExceedsMaximumSize { table, maximum_texture_size });
        return Ok(None);
    }
    if num_features <= 0.0 || num_features.fract() != 0.0 {
        return Err(developer("Width must be greater than zero."));
    }
    let num_features = num_features as usize;
    let packed = pack_property_table_properties_into_rgba8(&views, num_features);
    let texture = Texture::new(
        context,
        TextureOptions {
            width: num_features as u32,
            height: views.len() as u32,
            pixel_format: PixelFormat::Rgba,
            pixel_datatype: PixelDatatype::UnsignedByte,
            flip_y: false,
            // Raw data: no filtering or mipmaps.
            sampler: Sampler::NEAREST,
            pre_multiply_alpha: false,
            skip_color_space_conversion: false,
            id: None,
        },
        Some(&packed),
    )
    .map_err(|e| MetadataError::Runtime(e.message))?;
    Ok(Some(FrameResource::new(Rc::new(texture))))
}

/// Each property's bytes and element size.
type PropertyViews<'v> = Vec<(Cow<'v, [u8]>, usize)>;

/// The bytes of each GPU-compatible class property, in class order, with
/// its element size. `Err` is the message of what CesiumJS would throw.
fn collect_gpu_compatible_property_buffer_views<'v>(
    properties: &Value,
    buffer_views: &'v JsMap<ByteView<'_>>,
    class: Option<&MetadataClass>,
    count: Option<&Value>,
) -> Result<PropertyViews<'v>, String> {
    let class = class.ok_or("Cannot read properties of undefined (reading 'properties')")?;
    let mut views = Vec::new();
    // Every class property, not only the table's, so that shaders see the
    // defaults of properties a primitive leaves out.
    for (property_id, class_property) in class.properties.iter() {
        // Strings, variable-length arrays and 64-bit types do not fit.
        if !class_property.is_gpu_compatible(NUM_CHANNELS as f64).unwrap_or(false) {
            continue;
        }
        let property = js::get(Some(properties), property_id);
        let view: Cow<'v, [u8]> = if js::defined(property) {
            let key = js_string(js::get(property, "values"));
            let view = buffer_views.get(&key).ok_or("Cannot read properties of undefined (reading 'length')")?;
            Cow::Borrowed(view.buffer.get(view.byte_offset..).unwrap_or(&[]))
        } else {
            Cow::Owned(create_no_data_buffer_view(class_property, count)?)
        };
        let bytes_per_element = class_property.bytes_per_element().unwrap_or(f64::NAN);
        let num_buffer_elements = view.len() as f64 / bytes_per_element;
        let matches = count.and_then(Value::as_f64) == Some(num_buffer_elements);
        if !matches {
            return Err(format!(
                "Property with ID: \"{property_id}\" has ({}), which does not match number of features in the property table: ({}).",
                js_number_to_string(num_buffer_elements),
                js_string(count)
            ));
        }
        views.push((view, bytes_per_element as usize));
    }
    Ok(views)
}

/// A JavaScript value read while building the `noData` buffer, which may be
/// `undefined` or an array holding `undefined`.
#[derive(Clone)]
enum Slot {
    Undefined,
    Value(MetadataValue),
    Array(Vec<Slot>),
}

impl Slot {
    /// `value[index]`, with the `TypeError` reading a property of
    /// `undefined` or `null` throws.
    fn index(&self, index: usize) -> Result<Slot, String> {
        let found = match self {
            Self::Undefined => return Err(format!("Cannot read properties of undefined (reading '{index}')")),
            Self::Value(MetadataValue::Null) => {
                return Err(format!("Cannot read properties of null (reading '{index}')"))
            }
            Self::Array(items) => return Ok(items.get(index).cloned().unwrap_or(Self::Undefined)),
            Self::Value(MetadataValue::Array(items)) => items.get(index).cloned(),
            Self::Value(MetadataValue::String(s)) => {
                s.encode_utf16().nth(index).map(|unit| MetadataValue::String(String::from_utf16_lossy(&[unit])))
            }
            Self::Value(MetadataValue::Object(object)) => object.get(index.to_string()).map(MetadataValue::from_json),
            Self::Value(_) => None,
        };
        Ok(found.map_or(Self::Undefined, Self::Value))
    }

    /// `ToNumber`, as the `DataView` setters apply it.
    fn to_number(&self) -> Result<f64, String> {
        match self {
            Self::Undefined => Ok(f64::NAN),
            Self::Value(MetadataValue::BigInt(_)) => Err("Cannot convert a BigInt value to a number".into()),
            Self::Value(MetadataValue::Number(n)) => Ok(*n),
            Self::Value(v) => Ok(js::to_number(Some(&json(v)))),
            // Only noData itself is wrapped, and the elements read are never wrappers.
            Self::Array(_) => Ok(f64::NAN),
        }
    }
}

/// The JSON a metadata value was read from, for JavaScript conversions.
fn json(value: &MetadataValue) -> Value {
    match value {
        MetadataValue::Null => Value::Null,
        MetadataValue::Boolean(b) => Value::Bool(*b),
        MetadataValue::Number(n) => js::number(*n),
        MetadataValue::String(s) => Value::String(s.clone()),
        MetadataValue::Array(items) => Value::Array(items.iter().map(json).collect()),
        MetadataValue::Object(v) => v.clone(),
        _ => Value::Object(Default::default()),
    }
}

/// A property's `noData` value repeated for every feature. As in CesiumJS,
/// the offset of component `k` of array element `j` is
/// `bytesPerElement * i + j + k`, not scaled by the component size.
fn create_no_data_buffer_view(
    class_property: &MetadataClassProperty,
    count: Option<&Value>,
) -> Result<Vec<u8>, String> {
    let mut no_data = class_property.no_data().cloned().map_or(Slot::Undefined, Slot::Value);
    let component_count = class_property.metadata_type().get_component_count();
    let array_length = if class_property.is_array() {
        class_property.array_length().map_or(f64::NAN, |l| js::to_number(Some(l)))
    } else {
        1.0
    };
    // Enum noData values are names.
    if class_property.metadata_type() == MetadataType::Enum {
        let key = match &no_data {
            Slot::Value(v) => js::key(&json(v)),
            _ => "undefined".to_string(),
        };
        no_data = class_property
            .enum_type()
            .and_then(|e| e.values_by_name.get(&key).copied())
            .map_or(Slot::Undefined, |v| Slot::Value(MetadataValue::Number(v)));
    }
    // Wrapped (up to twice) so that every case indexes as noData[j][k].
    if component_count == 1 {
        no_data = Slot::Array(vec![no_data]);
    }
    if array_length == 1.0 {
        no_data = Slot::Array(vec![no_data]);
    }
    let bytes_per_element = class_property.bytes_per_element().unwrap_or(f64::NAN);
    let num_features = js::to_number(count);
    let byte_length = bytes_per_element * num_features;
    let byte_length = if byte_length.is_nan() { 0.0 } else { byte_length.trunc() };
    if !(0.0..=9_007_199_254_740_991.0).contains(&byte_length) {
        return Err("Invalid array buffer length".into());
    }
    if byte_length > u32::MAX as f64 {
        return Err("Array buffer allocation failed".into());
    }
    let mut buffer = vec![0u8; byte_length as usize];
    let component_type: Option<MetadataComponentType> = class_property.component_type();
    let mut i = 0usize;
    while (i as f64) < num_features {
        let mut j = 0usize;
        while (j as f64) < array_length {
            for k in 0..component_count {
                let value = no_data.index(j)?.index(k)?;
                // Enums have no component type, so there is no setter to call.
                let component_type = component_type.ok_or("setter is not a function")?;
                let offset = bytes_per_element as usize * i + j + k;
                component_type
                    .set_data_view_value(&mut buffer, offset, MetadataNumber::Number(value.to_number()?))
                    .ok_or("Offset is outside the bounds of the DataView")?;
            }
            j += 1;
        }
        i += 1;
    }
    Ok(buffer)
}

/// One RGBA8 row per property: each element's bytes, then zeros up to four
/// channels.
fn pack_property_table_properties_into_rgba8(views: &[(Cow<'_, [u8]>, usize)], num_features: usize) -> Vec<u8> {
    let mut packed = vec![0u8; views.len() * num_features * NUM_CHANNELS];
    let mut offset = 0;
    for (view, bytes_per_element) in views {
        let num_elements = view.len() / bytes_per_element;
        for j in 0..num_elements {
            let source = j * bytes_per_element;
            let destination = offset + j * NUM_CHANNELS;
            packed[destination..destination + bytes_per_element]
                .copy_from_slice(&view[source..source + bytes_per_element]);
        }
        offset += num_elements * NUM_CHANNELS;
    }
    packed
}
