//! CesiumJS `MetadataClassProperty`: one property of a 3D Metadata class,
//! with its type, and the value pipeline Cesium runs on its values:
//! no-data sentinels, normalization, offset and scale, vector and matrix
//! packing, validation and the GLSL that unpacks it from a texture.

use super::metadata_component_type::{MetadataNumber, ScalarCategory};
use super::metadata_value::{
    arithmetic, compare, developer, json_defined, json_string, MetadataError, MetadataValue, Numeric,
};
use super::{MathType, MetadataComponentType, MetadataEnum, MetadataType};
use crate::core::math::js_number_to_string;
use crate::core::{Cartesian2, Cartesian3, Cartesian4, Matrix2, Matrix3, Matrix4};
use serde_json::Value;
use std::cmp::Ordering;
use std::sync::Arc;

type Result<T> = std::result::Result<T, MetadataError>;

/// `transformationFunction(value, offset, scale)`; `None` is `undefined`.
pub type ValueTransform =
    dyn Fn(&MetadataValue, Option<&MetadataValue>, Option<&MetadataValue>) -> Result<MetadataValue>;

/// `new MetadataClassProperty(options)`. Unset fields take Cesium's defaults.
#[derive(Clone, Debug, Default)]
pub struct MetadataClassPropertyOptions {
    pub id: String,
    pub metadata_type: Option<MetadataType>,
    pub component_type: Option<MetadataComponentType>,
    pub enum_type: Option<Arc<MetadataEnum>>,
    pub is_array: bool,
    pub is_variable_length_array: bool,
    /// The JSON `count` (or legacy `componentCount`).
    pub array_length: Option<Value>,
    pub normalized: bool,
    pub min: Option<MetadataValue>,
    pub max: Option<MetadataValue>,
    pub offset: Option<MetadataValue>,
    pub scale: Option<MetadataValue>,
    pub no_data: Option<MetadataValue>,
    pub default: Option<MetadataValue>,
    /// `true` unless set.
    pub required: Option<bool>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub semantic: Option<String>,
    pub extras: Option<Value>,
    pub extensions: Option<Value>,
    pub is_legacy_extension: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetadataClassProperty {
    id: String,
    name: Option<String>,
    description: Option<String>,
    semantic: Option<String>,
    is_legacy_extension: Option<bool>,
    metadata_type: MetadataType,
    component_type: Option<MetadataComponentType>,
    enum_type: Option<Arc<MetadataEnum>>,
    value_type: Option<MetadataComponentType>,
    is_array: bool,
    is_variable_length_array: bool,
    array_length: Option<Value>,
    min: Option<MetadataValue>,
    max: Option<MetadataValue>,
    normalized: bool,
    offset: MetadataValue,
    scale: MetadataValue,
    has_value_transform: bool,
    no_data: Option<MetadataValue>,
    default: Option<MetadataValue>,
    required: bool,
    extras: Option<Value>,
    extensions: Option<Value>,
}

/// `values[i]` of an array; `undefined` for anything else.
fn element_at(values: Option<&MetadataValue>, i: usize) -> Option<&MetadataValue> {
    match values {
        Some(MetadataValue::Array(items)) => items.get(i),
        _ => None,
    }
}

/// `new Array(length).fill(fill)`: a number must be an array length; any
/// other argument makes a one-element array.
fn js_new_array(length: Option<&Value>, fill: &MetadataValue) -> Result<Vec<MetadataValue>> {
    match length {
        Some(Value::Number(n)) => js_new_array_of(n.as_f64().unwrap_or(f64::NAN), fill),
        _ => Ok(vec![fill.clone()]),
    }
}

fn js_new_array_of(length: f64, fill: &MetadataValue) -> Result<Vec<MetadataValue>> {
    if !(length >= 0.0 && length.fract() == 0.0 && length <= u32::MAX as f64) {
        return Err(MetadataError::Range("Invalid array length".into()));
    }
    Ok(vec![fill.clone(); length as usize])
}

impl MetadataClassProperty {
    pub fn new(options: MetadataClassPropertyOptions) -> Result<Self> {
        let metadata_type =
            options.metadata_type.ok_or_else(|| developer("Expected options.type to be typeof string"))?;
        let normalized = options.component_type.is_some_and(|c| options.normalized && c.is_integer_type());
        let value_type = match &options.enum_type {
            Some(e) => Some(e.value_type),
            None => options.component_type,
        };
        let has_value_transform = options.offset.is_some() || options.scale.is_some();
        let mut property = Self {
            id: options.id,
            name: options.name,
            description: options.description,
            semantic: options.semantic,
            is_legacy_extension: options.is_legacy_extension,
            metadata_type,
            component_type: options.component_type,
            enum_type: options.enum_type,
            value_type,
            is_array: options.is_array,
            is_variable_length_array: options.is_variable_length_array,
            array_length: options.array_length,
            min: options.min,
            max: options.max,
            normalized,
            offset: MetadataValue::Null,
            scale: MetadataValue::Null,
            has_value_transform,
            no_data: options.no_data,
            default: options.default,
            required: options.required.unwrap_or(true),
            extras: options.extras,
            extensions: options.extensions,
        };
        property.offset = match options.offset {
            Some(offset) => offset,
            None => property.expand_constant(0.0, true)?,
        };
        property.scale = match options.scale {
            Some(scale) => scale,
            None => property.expand_constant(1.0, true)?,
        };
        Ok(property)
    }

    /// `MetadataClassProperty.fromJson({ id, property, enums })`, for 3D
    /// Metadata and the legacy `EXT_feature_metadata` schemas.
    pub fn from_json(
        id: &str,
        property: &Value,
        enums: &super::metadata_value::JsMap<Arc<MetadataEnum>>,
    ) -> Result<Self> {
        if !property.is_object() {
            return Err(developer("Expected options.property to be typeof object"));
        }
        if !property.get("type").is_some_and(Value::is_string) {
            return Err(developer("Expected options.property.type to be typeof string"));
        }
        let is_legacy_extension = is_legacy(property);
        let parsed = parse_type(property, enums)?;
        let required = match is_legacy_extension {
            None => false,
            Some(true) => match property.get("optional").filter(|v| !v.is_null()) {
                Some(optional) => !truthy(optional),
                None => true,
            },
            Some(false) => property.get("required").filter(|v| !v.is_null()).is_some_and(truthy),
        };
        let value = |key: &str| MetadataValue::from_optional_json(property.get(key));
        Self::new(MetadataClassPropertyOptions {
            id: id.to_string(),
            metadata_type: Some(parsed.metadata_type),
            component_type: parsed.component_type,
            enum_type: parsed.enum_type,
            is_array: parsed.is_array,
            is_variable_length_array: parsed.is_variable_length_array,
            array_length: parsed.array_length,
            normalized: property.get("normalized").is_some_and(truthy),
            min: value("min"),
            max: value("max"),
            offset: value("offset"),
            scale: value("scale"),
            no_data: value("noData"),
            default: value("default"),
            required: Some(required),
            name: json_string(property.get("name")),
            description: json_string(property.get("description")),
            semantic: json_string(property.get("semantic")),
            extras: property.get("extras").cloned(),
            extensions: property.get("extensions").cloned(),
            is_legacy_extension,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn metadata_type(&self) -> MetadataType {
        self.metadata_type
    }
    pub fn enum_type(&self) -> Option<&Arc<MetadataEnum>> {
        self.enum_type.as_ref()
    }
    pub fn component_type(&self) -> Option<MetadataComponentType> {
        self.component_type
    }
    /// The enum's value type for enums, else the component type.
    pub fn value_type(&self) -> Option<MetadataComponentType> {
        self.value_type
    }
    pub fn is_array(&self) -> bool {
        self.is_array
    }
    pub fn is_variable_length_array(&self) -> bool {
        self.is_variable_length_array
    }
    pub fn array_length(&self) -> Option<&Value> {
        self.array_length.as_ref()
    }
    /// The array length as a count, when it is one.
    pub fn array_length_count(&self) -> Option<usize> {
        self.array_length.as_ref().and_then(Value::as_u64).map(|n| n as usize)
    }
    pub fn normalized(&self) -> bool {
        self.normalized
    }
    pub fn max(&self) -> Option<&MetadataValue> {
        self.max.as_ref()
    }
    pub fn min(&self) -> Option<&MetadataValue> {
        self.min.as_ref()
    }
    pub fn no_data(&self) -> Option<&MetadataValue> {
        self.no_data.as_ref()
    }
    pub fn default(&self) -> Option<&MetadataValue> {
        self.default.as_ref()
    }
    pub fn required(&self) -> bool {
        self.required
    }
    pub fn semantic(&self) -> Option<&str> {
        self.semantic.as_deref()
    }
    pub fn has_value_transform(&self) -> bool {
        self.has_value_transform
    }
    /// The offset, or zeros shaped like a value when none is given.
    pub fn offset(&self) -> &MetadataValue {
        &self.offset
    }
    /// The scale, or ones shaped like a value when none is given.
    pub fn scale(&self) -> &MetadataValue {
        &self.scale
    }
    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }
    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }
    /// Whether the property came from `EXT_feature_metadata`; `None` when the
    /// JSON does not tell.
    pub fn is_legacy_extension(&self) -> Option<bool> {
        self.is_legacy_extension
    }

    fn component_count(&self) -> usize {
        self.metadata_type.get_component_count()
    }

    /// Maps integer values to [0, 1] or [-1, 1] when the property is
    /// normalized, through arrays.
    pub fn normalize(&self, value: MetadataValue) -> Result<MetadataValue> {
        if !self.normalized {
            return Ok(value);
        }
        let value_type = self.value_type.expect("normalized properties have a component type");
        normalize_in_place(value, &|v| {
            let number = match v {
                MetadataValue::Number(n) => MetadataNumber::Number(*n),
                MetadataValue::BigInt(b) => MetadataNumber::BigInt(*b),
                _ => return Err(developer("value must be a number or a BigInt")),
            };
            MetadataComponentType::normalize(number, value_type)
                .map(MetadataValue::Number)
                .ok_or_else(|| developer("type must be an integer type"))
        })
    }

    /// The inverse of [`normalize`](Self::normalize). 64-bit values come back
    /// as `BigInt`s.
    pub fn unnormalize(&self, value: MetadataValue) -> Result<MetadataValue> {
        if !self.normalized {
            return Ok(value);
        }
        let value_type = self.value_type.expect("normalized properties have a component type");
        normalize_in_place(value, &|v| {
            let MetadataValue::Number(n) = v else { return Err(developer("Expected value to be typeof number")) };
            match MetadataComponentType::unnormalize(*n, value_type) {
                Ok(Some(MetadataNumber::Number(x))) => Ok(MetadataValue::Number(x)),
                Ok(Some(MetadataNumber::BigInt(b))) => Ok(MetadataValue::BigInt(b)),
                Ok(None) => Err(developer("type must be an integer type")),
                Err(e) => Err(MetadataError::Range(e.0)),
            }
        })
    }

    /// `scale * value + offset`, element by element. Variable-length arrays
    /// and properties without a transform are unchanged.
    pub fn apply_value_transform(&self, value: MetadataValue) -> Result<MetadataValue> {
        if !self.has_value_transform || self.is_variable_length_array {
            return Ok(value);
        }
        Self::value_transform_in_place(value, Some(&self.offset), Some(&self.scale), &apply_transform_leaf)
    }

    /// `(value - offset) / scale`, or 0 where the scale is 0.
    pub fn unapply_value_transform(&self, value: MetadataValue) -> Result<MetadataValue> {
        if !self.has_value_transform || self.is_variable_length_array {
            return Ok(value);
        }
        Self::value_transform_in_place(value, Some(&self.offset), Some(&self.scale), &unapply_transform_leaf)
    }

    /// `MetadataClassProperty.valueTransformInPlace`: `transform(value,
    /// offset, scale)` on each leaf, indexing `offsets` and `scales` along with
    /// the values. `None` is `undefined`.
    pub fn value_transform_in_place(
        values: MetadataValue,
        offsets: Option<&MetadataValue>,
        scales: Option<&MetadataValue>,
        transform: &ValueTransform,
    ) -> Result<MetadataValue> {
        match values {
            MetadataValue::Array(items) => {
                // `offsets[i]` of undefined or null throws; of a primitive it is undefined.
                for operand in [offsets, scales] {
                    let missing = match operand {
                        None => Some("undefined"),
                        Some(MetadataValue::Null) => Some("null"),
                        Some(_) => None,
                    };
                    if let (Some(missing), false) = (missing, items.is_empty()) {
                        return Err(MetadataError::Type(format!("Cannot read properties of {missing} (reading '0')")));
                    }
                }
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.into_iter().enumerate() {
                    out.push(Self::value_transform_in_place(
                        item,
                        element_at(offsets, i),
                        element_at(scales, i),
                        transform,
                    )?);
                }
                Ok(MetadataValue::Array(out))
            }
            value => transform(&value, offsets, scales),
        }
    }

    /// `constant` shaped like one value: a scalar, a component array, an array
    /// of the array length, or (with `enable_nested_arrays`) an array of
    /// component arrays.
    pub fn expand_constant(&self, constant: f64, enable_nested_arrays: bool) -> Result<MetadataValue> {
        let constant = MetadataValue::Number(constant);
        let component_count = self.component_count();
        let is_nested = self.is_array && component_count > 1;
        if !self.is_array && component_count == 1 {
            return Ok(constant);
        }
        if !self.is_array {
            return Ok(MetadataValue::Array(vec![constant; component_count]));
        }
        if !is_nested {
            return Ok(MetadataValue::Array(js_new_array(self.array_length.as_ref(), &constant)?));
        }
        if !enable_nested_arrays {
            // `arrayLength * componentCount`: NaN, a RangeError, without a length.
            let length =
                MetadataValue::from_optional_json(self.array_length.as_ref()).map_or(f64::NAN, |l| l.to_number());
            return Ok(MetadataValue::Array(js_new_array_of(length * component_count as f64, &constant)?));
        }
        let inner = MetadataValue::Array(vec![constant; component_count]);
        Ok(MetadataValue::Array(js_new_array(self.array_length.as_ref(), &inner)?))
    }

    /// `undefined` when the value equals the no-data sentinel, element by
    /// element with `===`.
    pub fn handle_no_data(&self, value: Option<MetadataValue>) -> Option<MetadataValue> {
        let Some(sentinel) = &self.no_data else { return value };
        let value = value?;
        if array_equals(&value, sentinel) {
            None
        } else {
            Some(value)
        }
    }

    fn math_type(&self) -> Option<MathType> {
        self.metadata_type.get_math_type()
    }

    /// Packed component arrays to Cartesian and Matrix values.
    pub fn unpack_vector_and_matrix_types(
        &self,
        value: MetadataValue,
        enable_nested_arrays: bool,
    ) -> Result<MetadataValue> {
        let Some(math) = self.math_type() else { return Ok(value) };
        let is_nested = self.is_array && self.component_count() > 1;
        if enable_nested_arrays && is_nested {
            let MetadataValue::Array(items) = value else {
                return Err(MetadataError::Type("value.map is not a function".into()));
            };
            return Ok(MetadataValue::Array(items.iter().map(|x| unpack(math, x)).collect::<Result<_>>()?));
        }
        if self.is_array {
            return unpack_array(math, &value);
        }
        unpack(math, &value)
    }

    /// Cartesian and Matrix values back to component arrays.
    pub fn pack_vector_and_matrix_types(
        &self,
        value: MetadataValue,
        enable_nested_arrays: bool,
    ) -> Result<MetadataValue> {
        let Some(math) = self.math_type() else { return Ok(value) };
        let is_nested = self.is_array && self.component_count() > 1;
        if enable_nested_arrays && is_nested {
            let MetadataValue::Array(items) = value else {
                return Err(MetadataError::Type("value.map is not a function".into()));
            };
            return Ok(MetadataValue::Array(items.iter().map(|x| pack(math, x)).collect::<Result<_>>()?));
        }
        if self.is_array {
            let MetadataValue::Array(items) = value else {
                return Err(developer("Expected array to be typeof object"));
            };
            let mut packed = Vec::new();
            for item in &items {
                if let MetadataValue::Array(components) = pack(math, item)? {
                    packed.extend(components);
                }
            }
            return Ok(MetadataValue::Array(packed));
        }
        pack(math, &value)
    }

    /// `None` when the value is valid, else Cesium's message.
    pub fn validate(&self, value: Option<&MetadataValue>) -> Option<String> {
        let defined = value.filter(|v| !matches!(v, MetadataValue::Null));
        if defined.is_none() && self.default.is_some() {
            return None;
        }
        if self.required && defined.is_none() {
            return Some("required property must have a value".into());
        }
        if self.is_array {
            return self.validate_array(value);
        }
        self.validate_single_value(value)
    }

    fn validate_array(&self, value: Option<&MetadataValue>) -> Option<String> {
        let Some(MetadataValue::Array(items)) = value else {
            return Some(format!("value {} must be an array", js_string(value)));
        };
        let matches_length = self.array_length.as_ref().and_then(Value::as_f64) == Some(items.len() as f64);
        if !self.is_variable_length_array && !matches_length {
            return Some("Array length does not match property.arrayLength".into());
        }
        items.iter().find_map(|item| self.validate_single_value(Some(item)))
    }

    fn validate_single_value(&self, value: Option<&MetadataValue>) -> Option<String> {
        let t = self.metadata_type;
        if t.is_vector_type() || t.is_matrix_type() {
            let kind = if t.is_vector_type() { "vector" } else { "matrix" };
            // `isVectorCompatible(undefined)` is a DeveloperError; treat a
            // missing component type as incompatible.
            if !self.component_type.is_some_and(MetadataComponentType::is_vector_compatible) {
                let component = self.component_type.map_or("undefined", MetadataComponentType::as_str);
                return Some(format!("componentType {component} is incompatible with {kind} type {}", t.as_str()));
            }
            let class = t.get_math_type().expect("vector and matrix types have a math type");
            let matches = matches!(
                (class, value),
                (MathType::Cartesian2, Some(MetadataValue::Cartesian2(_)))
                    | (MathType::Cartesian3, Some(MetadataValue::Cartesian3(_)))
                    | (MathType::Cartesian4, Some(MetadataValue::Cartesian4(_)))
                    | (MathType::Matrix2, Some(MetadataValue::Matrix2(_)))
                    | (MathType::Matrix3, Some(MetadataValue::Matrix3(_)))
                    | (MathType::Matrix4, Some(MetadataValue::Matrix4(_)))
            );
            if !matches {
                return Some(format!("{kind} value {} must be a {}", js_string(value), class.name()));
            }
            return None;
        }
        match t {
            MetadataType::String => {
                (!matches!(value, Some(MetadataValue::String(_)))).then(|| type_error_message(value, "STRING"))
            }
            MetadataType::Boolean => {
                (!matches!(value, Some(MetadataValue::Boolean(_)))).then(|| type_error_message(value, "BOOLEAN"))
            }
            MetadataType::Enum => {
                let enum_type = self.enum_type.as_ref()?;
                let valid =
                    matches!(value, Some(MetadataValue::String(name)) if enum_type.values_by_name.contains_key(name));
                (!valid).then(|| format!("value {} is not a valid enum name for {}", js_string(value), enum_type.id))
            }
            _ => self.validate_scalar(value),
        }
    }

    fn validate_scalar(&self, value: Option<&MetadataValue>) -> Option<String> {
        let component_type = self.component_type?;
        let bigint_allowed = matches!(component_type, MetadataComponentType::Int64 | MetadataComponentType::Uint64);
        let numeric = match value {
            Some(MetadataValue::Number(n)) => Numeric::Number(*n),
            Some(MetadataValue::BigInt(b)) if bigint_allowed => Numeric::BigInt(*b),
            _ => return Some(type_error_message(value, component_type.as_str())),
        };
        if let Numeric::Number(n) = numeric {
            if !n.is_finite() {
                return Some(format!("value {} of type {} must be finite", js_string(value), component_type.as_str()));
            }
        }
        let out_of_range = if self.normalized {
            let min = if component_type.is_unsigned_integer_type() { 0.0 } else { -1.0 };
            compare(numeric, Numeric::Number(min)) == Some(Ordering::Less)
                || compare(numeric, Numeric::Number(1.0)) == Some(Ordering::Greater)
        } else {
            let bound = |n: MetadataNumber| match n {
                MetadataNumber::Number(x) => Numeric::Number(x),
                MetadataNumber::BigInt(b) => Numeric::BigInt(b),
            };
            compare(numeric, bound(component_type.get_minimum())) == Some(Ordering::Less)
                || compare(numeric, bound(component_type.get_maximum())) == Some(Ordering::Greater)
        };
        out_of_range.then(|| {
            let mut message =
                format!("value {} is out of range for type {}", js_string(value), component_type.as_str());
            if self.normalized {
                message.push_str(" (normalized)");
            }
            message
        })
    }

    /// Bytes per value: components, times the array length, times the
    /// component size. `NaN` for a variable-length array.
    pub fn bytes_per_element(&self) -> Result<f64> {
        let value_type = self.value_type.ok_or_else(|| developer("Expected type to be typeof string"))?;
        let array_length =
            if self.is_array { self.array_length.as_ref().and_then(Value::as_f64).unwrap_or(f64::NAN) } else { 1.0 };
        Ok(self.component_count() as f64 * array_length * value_type.get_size_in_bytes() as f64)
    }

    /// Whether a value fits in `channels_length` texture channels.
    pub fn is_gpu_compatible(&self, channels_length: f64) -> Result<bool> {
        if channels_length <= 0.0 {
            return Err(developer("channelsLength must be a positive number."));
        }
        if self.is_variable_length_array || matches!(self.metadata_type, MetadataType::String | MetadataType::Boolean) {
            return Ok(false);
        }
        // A NaN size fails the comparison and counts as compatible, as in Cesium.
        Ok(self.bytes_per_element()?.partial_cmp(&channels_length) != Some(Ordering::Greater))
    }

    fn glsl_component_count(&self) -> Option<usize> {
        let count = self.component_count();
        if !self.is_array {
            return Some(count);
        }
        self.array_length_count().map(|n| n * count)
    }

    /// The WebGL 1 GLSL type: `float`… for normalized values, else `int`….
    pub fn get_glsl_type_webgl1(&self) -> Option<&'static str> {
        let count = if self.is_array { self.array_length_count()? } else { self.component_count() };
        if self.normalized {
            FLOAT_TYPES.get(count).copied().flatten()
        } else {
            INTEGER_TYPES.get(count).copied().flatten()
        }
    }

    /// The GLSL type a value unpacks to.
    pub fn get_glsl_type(&self) -> Result<Option<&'static str>> {
        let value_type = self
            .value_type
            .ok_or_else(|| developer(format!("Invalid MetadataComponentType: {:?}", self.value_type)))?;
        let Some(count) = self.glsl_component_count() else { return Ok(None) };
        let table = if !value_type.is_integer_type() || self.normalized {
            &FLOAT_TYPES
        } else if value_type.is_unsigned_integer_type() {
            &UNSIGNED_TYPES
        } else {
            &INTEGER_TYPES
        };
        Ok(table.get(count).copied().flatten())
    }

    /// Appends the GLSL that unpacks the value from `sampled_texture_expression`
    /// (whose channels are `channels_string`) to `shader_lines`, and returns
    /// the variable holding it.
    pub fn unpack_texture_in_shader(
        &self,
        sampled_texture_expression: &str,
        channels_string: &str,
        metadata_variable_name: &str,
        shader_lines: &mut Vec<String>,
    ) -> Result<String> {
        let glsl_type = self.get_glsl_type()?.unwrap_or("undefined");
        let value_type = self.value_type.expect("checked by get_glsl_type");
        let num_channels = channels_string.encode_utf16().count();
        let component_count = self.glsl_component_count().unwrap_or(0);
        let channels_per_component = num_channels.checked_div(component_count).unwrap_or(0);
        let raw_channels = format!("{metadata_variable_name}_rawChannels");
        let raw_bits = format!("{metadata_variable_name}_rawBits");
        let unpacked = format!("{metadata_variable_name}_unpackedValue");
        shader_lines.push(format!("{glsl_type} {unpacked};"));
        shader_lines.push(format!("uint {raw_bits};"));
        let raw_type = FLOAT_TYPES.get(num_channels).copied().flatten().unwrap_or("undefined");
        shader_lines.push(format!("{raw_type} {raw_channels} = {sampled_texture_expression};"));
        let cast = match value_type.category() {
            ScalarCategory::Float => "uintBitsToFloat",
            ScalarCategory::Integer => "int",
            ScalarCategory::UnsignedInteger => "",
        };
        for i in 0..component_count {
            let slice: String = "rgba".chars().skip(i * channels_per_component).take(channels_per_component).collect();
            let sub_channels = if num_channels > 1 { format!(".{slice}") } else { String::new() };
            let index = if component_count > 1 { format!("[{i}]") } else { String::new() };
            let (normalize, to_float) = if self.normalized {
                let max = match value_type.get_maximum() {
                    MetadataNumber::Number(x) => x,
                    MetadataNumber::BigInt(b) => b as f64,
                };
                (format!(" * {}", js_number_to_string(1.0 / max)), "float")
            } else {
                (String::new(), "")
            };
            shader_lines.push(format!("{raw_bits} = czm_unpackTexture({raw_channels}{sub_channels});"));
            shader_lines.push(format!("{unpacked}{index} = {to_float}({cast}({raw_bits})){normalize};"));
        }
        Ok(unpacked)
    }

    /// WebGL 1 unpacking: normalized values as sampled, else `255.0 *` the
    /// sample cast to an integer type.
    pub fn unpack_texture_in_shader_webgl1(&self, sampled_texture_expression: &str) -> String {
        if self.normalized {
            return sampled_texture_expression.to_string();
        }
        let glsl_type = self.get_glsl_type_webgl1().unwrap_or("undefined");
        format!("{glsl_type}(255.0 * {sampled_texture_expression})")
    }
}

/// `MetadataComponentType.applyValueTransform`: `scale * value + offset`.
pub(crate) fn apply_transform_leaf(
    value: &MetadataValue,
    offset: Option<&MetadataValue>,
    scale: Option<&MetadataValue>,
) -> Result<MetadataValue> {
    let product = arithmetic(numeric(scale), value.to_numeric(), '*')?.value();
    js_add(&product, offset)
}

/// `MetadataComponentType.unapplyValueTransform`: `(value - offset) /
/// scale`, or 0 where the scale is 0.
pub(crate) fn unapply_transform_leaf(
    value: &MetadataValue,
    offset: Option<&MetadataValue>,
    scale: Option<&MetadataValue>,
) -> Result<MetadataValue> {
    if matches!(scale, Some(MetadataValue::Number(z)) if *z == 0.0) {
        return Ok(MetadataValue::Number(0.0));
    }
    Ok(arithmetic(arithmetic(value.to_numeric(), numeric(offset), '-')?, numeric(scale), '/')?.value())
}

/// `ToNumeric` of an optional operand: `undefined` is NaN.
fn numeric(value: Option<&MetadataValue>) -> Numeric {
    value.map_or(Numeric::Number(f64::NAN), MetadataValue::to_numeric)
}

/// JavaScript `a + b`: after `ToPrimitive`, a string on either side
/// concatenates, which is what an array or Cartesian offset becomes.
fn js_add(a: &MetadataValue, b: Option<&MetadataValue>) -> Result<MetadataValue> {
    let textual = |v: &MetadataValue| {
        matches!(
            v,
            MetadataValue::String(_)
                | MetadataValue::Array(_)
                | MetadataValue::Object(_)
                | MetadataValue::Cartesian2(_)
                | MetadataValue::Cartesian3(_)
                | MetadataValue::Cartesian4(_)
                | MetadataValue::Matrix2(_)
                | MetadataValue::Matrix3(_)
                | MetadataValue::Matrix4(_)
        )
    };
    if textual(a) || b.is_some_and(textual) {
        let right = b.map_or_else(|| "undefined".to_string(), MetadataValue::js_string);
        return Ok(MetadataValue::String(format!("{}{right}", a.js_string())));
    }
    Ok(arithmetic(a.to_numeric(), numeric(b), '+')?.value())
}

const FLOAT_TYPES: [Option<&str>; 5] = [None, Some("float"), Some("vec2"), Some("vec3"), Some("vec4")];
const INTEGER_TYPES: [Option<&str>; 5] = [None, Some("int"), Some("ivec2"), Some("ivec3"), Some("ivec4")];
const UNSIGNED_TYPES: [Option<&str>; 5] = [None, Some("uint"), Some("uvec2"), Some("uvec3"), Some("uvec4")];

/// JavaScript truthiness of a JSON value.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn js_string(value: Option<&MetadataValue>) -> String {
    value.map_or_else(|| "undefined".into(), MetadataValue::js_string)
}

fn type_error_message(value: Option<&MetadataValue>, type_name: &str) -> String {
    format!("value {} does not match type {type_name}", js_string(value))
}

fn normalize_in_place(
    value: MetadataValue,
    f: &dyn Fn(&MetadataValue) -> Result<MetadataValue>,
) -> Result<MetadataValue> {
    match value {
        MetadataValue::Array(items) => {
            Ok(MetadataValue::Array(items.into_iter().map(|v| normalize_in_place(v, f)).collect::<Result<_>>()?))
        }
        other => f(&other),
    }
}

/// Cesium's `arrayEquals`: arrays element by element, the rest with `===`.
fn array_equals(left: &MetadataValue, right: &MetadataValue) -> bool {
    match (left, right) {
        (MetadataValue::Array(a), MetadataValue::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| array_equals(x, y))
        }
        (MetadataValue::Array(_), _) => false,
        _ => left.strict_equals(right),
    }
}

/// `value.x` (and `y`, `z`, `w`): defined on the Cartesian classes that
/// have the component; `undefined` (NaN) on anything else.
fn field(value: &MetadataValue, name: usize) -> f64 {
    let c: &[f64] = match value {
        MetadataValue::Cartesian2(c) => &[c.x, c.y],
        MetadataValue::Cartesian3(c) => &[c.x, c.y, c.z],
        MetadataValue::Cartesian4(c) => &[c.x, c.y, c.z, c.w],
        _ => &[],
    };
    c.get(name).copied().unwrap_or(f64::NAN)
}

/// `value[i]`: array elements and matrix entries; `undefined` (NaN) on
/// anything else, including the Cartesian classes.
fn index(value: &MetadataValue, i: usize) -> MetadataValue {
    let entry = |m: &[f64]| MetadataValue::Number(m.get(i).copied().unwrap_or(f64::NAN));
    match value {
        MetadataValue::Array(items) => items.get(i).cloned().unwrap_or(MetadataValue::Number(f64::NAN)),
        MetadataValue::Matrix2(m) => entry(&m.0),
        MetadataValue::Matrix3(m) => entry(&m.0),
        MetadataValue::Matrix4(m) => entry(&m.0),
        _ => MetadataValue::Number(f64::NAN),
    }
}

/// The elements of an array-like value: an array, or a matrix, whose
/// `length` getter makes it one.
fn array_like(value: &MetadataValue) -> Option<Vec<MetadataValue>> {
    let entries = |m: &[f64]| Some(m.iter().map(|&x| MetadataValue::Number(x)).collect());
    match value {
        MetadataValue::Array(items) => Some(items.clone()),
        MetadataValue::Matrix2(m) => entries(&m.0),
        MetadataValue::Matrix3(m) => entries(&m.0),
        MetadataValue::Matrix4(m) => entries(&m.0),
        _ => None,
    }
}

fn is_cartesian(math: MathType) -> bool {
    matches!(math, MathType::Cartesian2 | MathType::Cartesian3 | MathType::Cartesian4)
}

pub(crate) fn component_count(math: MathType) -> usize {
    match math {
        MathType::Number => 1,
        MathType::Cartesian2 => 2,
        MathType::Cartesian3 => 3,
        MathType::Cartesian4 | MathType::Matrix2 => 4,
        MathType::Matrix3 => 9,
        MathType::Matrix4 => 16,
    }
}

pub(crate) fn from_components(math: MathType, c: &[f64]) -> MetadataValue {
    match math {
        MathType::Cartesian2 => MetadataValue::Cartesian2(Cartesian2::unpack(c, 0)),
        MathType::Cartesian3 => MetadataValue::Cartesian3(Cartesian3::unpack(c, 0)),
        MathType::Cartesian4 => MetadataValue::Cartesian4(Cartesian4::unpack(c, 0)),
        MathType::Matrix2 => MetadataValue::Matrix2(Matrix2::unpack(c, 0)),
        MathType::Matrix3 => MetadataValue::Matrix3(Matrix3::unpack(c, 0)),
        MathType::Matrix4 => MetadataValue::Matrix4(Matrix4::unpack(c, 0)),
        MathType::Number => MetadataValue::Number(c[0]),
    }
}

/// `MathType.unpack(value)`: `value[0..n]`. `null` fails its `Check.defined`.
fn unpack(math: MathType, value: &MetadataValue) -> Result<MetadataValue> {
    if matches!(value, MetadataValue::Null) {
        return Err(developer("array is required, actual value was null"));
    }
    let n = component_count(math);
    Ok(from_components(math, &(0..n).map(|i| index(value, i).to_number()).collect::<Vec<_>>()))
}

fn unpack_array(math: MathType, value: &MetadataValue) -> Result<MetadataValue> {
    let Some(items) = array_like(value) else { return Err(developer("Expected array to be defined")) };
    let n = component_count(math);
    if items.len() < n {
        return Err(developer(format!(
            "Expected array.length to be greater than or equal to {n}, actual value was {}",
            items.len()
        )));
    }
    if items.len() % n != 0 {
        return Err(developer(format!("array length must be a multiple of {n}.")));
    }
    let numbers: Vec<f64> = items.iter().map(MetadataValue::to_number).collect();
    Ok(MetadataValue::Array(numbers.chunks(n).map(|c| from_components(math, c)).collect()))
}

/// `MathType.pack(value, [])`: the Cartesian classes read `value.x`… and the
/// matrices `value[i]`, whatever `value` is; what is missing is `undefined`
/// (NaN). Reading from `null` throws.
pub(crate) fn pack(math: MathType, value: &MetadataValue) -> Result<MetadataValue> {
    if matches!(value, MetadataValue::Null) {
        return Err(MetadataError::Type("Cannot read properties of null (reading 'x')".into()));
    }
    let n = component_count(math);
    let read = |i| if is_cartesian(math) { MetadataValue::Number(field(value, i)) } else { index(value, i) };
    Ok(MetadataValue::Array((0..n).map(read).collect()))
}

/// Cesium's `isLegacy`: whether the JSON is an `EXT_feature_metadata`
/// property, `None` when it cannot tell.
fn is_legacy(property: &Value) -> Option<bool> {
    let t = property.get("type").and_then(Value::as_str).unwrap_or("");
    if t == "ARRAY" {
        return Some(true);
    }
    if let Some(metadata_type) = MetadataType::from_str_value(t) {
        if metadata_type == MetadataType::Scalar || metadata_type.is_matrix_type() || metadata_type.is_vector_type() {
            return Some(false);
        }
    }
    if MetadataComponentType::from_str_value(t).is_some() {
        return Some(true);
    }
    let defined = |k: &str| json_defined(property.get(k));
    if ["noData", "scale", "offset", "required", "count", "array"].into_iter().any(defined) {
        return Some(false);
    }
    if defined("optional") {
        return Some(false);
    }
    None
}

struct ParsedType {
    metadata_type: MetadataType,
    component_type: Option<MetadataComponentType>,
    enum_type: Option<Arc<MetadataEnum>>,
    is_array: bool,
    is_variable_length_array: bool,
    array_length: Option<Value>,
}

fn parse_type(property: &Value, enums: &super::metadata_value::JsMap<Arc<MetadataEnum>>) -> Result<ParsedType> {
    let t = property.get("type").and_then(Value::as_str).unwrap_or("");
    let component = property.get("componentType").and_then(Value::as_str);
    let is_legacy_array = t == "ARRAY";
    let (is_array, array_length, is_variable_length_array) = if is_legacy_array {
        let length = property.get("componentCount").filter(|v| !v.is_null()).cloned();
        let variable = length.is_none();
        (true, length, variable)
    } else if property.get("array").is_some_and(truthy) {
        let length = property.get("count").filter(|v| !v.is_null()).cloned();
        let variable = length.is_none();
        (true, length, variable)
    } else {
        (false, None, false)
    };
    let enum_type = match property.get("enumType").filter(|v| !v.is_null()) {
        Some(id) => enums.get(&crate::scene::gltf_pipeline::js::key(id)).cloned(),
        None => None,
    };
    let parsed = |metadata_type, component_type, enum_type| ParsedType {
        metadata_type,
        component_type,
        enum_type,
        is_array,
        is_variable_length_array,
        array_length: array_length.clone(),
    };
    let missing_enum = || MetadataError::Type("Cannot read properties of undefined (reading 'valueType')".into());
    if t == "ENUM" || (is_legacy_array && component == Some("ENUM")) {
        let enum_type = enum_type.ok_or_else(missing_enum)?;
        return Ok(parsed(MetadataType::Enum, None, Some(enum_type)));
    }
    if let Some(metadata_type) = MetadataType::from_str_value(t) {
        if metadata_type == MetadataType::Scalar || metadata_type.is_matrix_type() || metadata_type.is_vector_type() {
            return Ok(parsed(metadata_type, component.and_then(MetadataComponentType::from_str_value), None));
        }
        if matches!(metadata_type, MetadataType::Boolean | MetadataType::String) {
            return Ok(parsed(metadata_type, None, None));
        }
    }
    if is_legacy_array {
        if let Some(metadata_type @ (MetadataType::Boolean | MetadataType::String)) =
            component.and_then(MetadataType::from_str_value)
        {
            return Ok(parsed(metadata_type, None, None));
        }
    }
    if let Some(component_type) = component.and_then(MetadataComponentType::from_str_value) {
        return Ok(parsed(MetadataType::Scalar, Some(component_type), None));
    }
    if let Some(component_type) = MetadataComponentType::from_str_value(t) {
        return Ok(parsed(MetadataType::Scalar, Some(component_type), None));
    }
    Err(developer(format!("unknown metadata type {{type: {t}, componentType: {}}})", component.unwrap_or("undefined"))))
}
