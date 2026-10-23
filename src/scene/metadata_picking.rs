//! CesiumJS `MetadataPicking`: little-endian numeric decoding, normalization,
//! metadata value transforms, and vector/matrix object conversion.
use super::{
    metadata_class_property::apply_transform_leaf,
    metadata_value::{MetadataError, MetadataValue},
    MetadataClassProperty, MetadataClassPropertyOptions, MetadataComponentType, MetadataType,
};
type Result<T> = std::result::Result<T, MetadataError>;
pub struct MetadataPicking;
impl MetadataPicking {
    pub fn decode_raw_metadata_value(
        component_type: MetadataComponentType,
        bytes: &[u8],
        index: usize,
    ) -> Result<MetadataValue> {
        let end = index
            .checked_add(component_type.get_size_in_bytes())
            .ok_or_else(|| MetadataError::Range("Offset is outside the bounds of the DataView".into()))?;
        let bytes = bytes
            .get(index..end)
            .ok_or_else(|| MetadataError::Range("Offset is outside the bounds of the DataView".into()))?;
        macro_rules! number {
            ($ty:ty) => {
                MetadataValue::Number(<$ty>::from_le_bytes(bytes.try_into().expect("component width checked")) as f64)
            };
        }
        Ok(match component_type {
            MetadataComponentType::Int8 => number!(i8),
            MetadataComponentType::Uint8 => number!(u8),
            MetadataComponentType::Int16 => number!(i16),
            MetadataComponentType::Uint16 => number!(u16),
            MetadataComponentType::Int32 => number!(i32),
            MetadataComponentType::Uint32 => number!(u32),
            MetadataComponentType::Int64 => {
                MetadataValue::BigInt(i64::from_le_bytes(bytes.try_into().expect("component width checked")) as i128)
            }
            MetadataComponentType::Uint64 => {
                MetadataValue::BigInt(u64::from_le_bytes(bytes.try_into().expect("component width checked")) as i128)
            }
            MetadataComponentType::Float32 => number!(f32),
            MetadataComponentType::Float64 => number!(f64),
        })
    }
    pub fn decode_raw_metadata_value_component(
        property: &MetadataClassProperty,
        bytes: &[u8],
        offset: usize,
    ) -> Result<MetadataValue> {
        let component_type = property
            .component_type()
            .ok_or_else(|| MetadataError::Runtime("Invalid component type: undefined".into()))?;
        property.normalize(Self::decode_raw_metadata_value(component_type, bytes, offset)?)
    }
    pub fn decode_raw_metadata_value_element(
        property: &MetadataClassProperty,
        bytes: &[u8],
        element_index: usize,
    ) -> Result<MetadataValue> {
        let component_type = property
            .component_type()
            .ok_or_else(|| MetadataError::Runtime("Invalid component type: undefined".into()))?;
        let components = property.metadata_type().get_component_count();
        let size = component_type.get_size_in_bytes();
        let offset = element_index
            .checked_mul(components)
            .and_then(|n| n.checked_mul(size))
            .ok_or_else(|| MetadataError::Range("Offset is outside the bounds of the DataView".into()))?;
        if components == 1 {
            return Self::decode_raw_metadata_value_component(property, bytes, offset);
        }
        (0..components)
            .map(|i| Self::decode_raw_metadata_value_component(property, bytes, offset + i * size))
            .collect::<Result<Vec<_>>>()
            .map(MetadataValue::Array)
    }
    pub fn decode_raw_metadata_values(property: &MetadataClassProperty, bytes: &[u8]) -> Result<MetadataValue> {
        if property.is_array() {
            let count = property
                .array_length_count()
                .ok_or_else(|| MetadataError::Developer("Metadata picking requires a fixed-length array".into()))?;
            (0..count)
                .map(|i| Self::decode_raw_metadata_value_element(property, bytes, i))
                .collect::<Result<Vec<_>>>()
                .map(MetadataValue::Array)
        } else {
            Self::decode_raw_metadata_value_element(property, bytes, 0)
        }
    }
    pub fn convert_to_object_type(
        metadata_type: MetadataType,
        value: Option<MetadataValue>,
    ) -> Result<Option<MetadataValue>> {
        let Some(value) = value else {
            return Ok(None);
        };
        if metadata_type.get_math_type().is_none() {
            return Ok(Some(value));
        }
        MetadataClassProperty::new(MetadataClassPropertyOptions {
            metadata_type: Some(metadata_type),
            ..Default::default()
        })?
        .unpack_vector_and_matrix_types(value, false)
        .map(Some)
    }
    pub fn convert_from_object_type(
        metadata_type: MetadataType,
        value: Option<MetadataValue>,
    ) -> Result<Option<MetadataValue>> {
        let Some(value) = value else {
            return Ok(None);
        };
        if metadata_type.get_math_type().is_none() {
            return Ok(Some(value));
        }
        MetadataClassProperty::new(MetadataClassPropertyOptions {
            metadata_type: Some(metadata_type),
            ..Default::default()
        })?
        .pack_vector_and_matrix_types(value, false)
        .map(Some)
    }
    pub fn decode_metadata_values(
        property: &MetadataClassProperty,
        value_transform: Option<(&MetadataValue, &MetadataValue)>,
        bytes: &[u8],
    ) -> Result<MetadataValue> {
        let mut value = Self::decode_raw_metadata_values(property, bytes)?;
        if let Some((offset, scale)) = value_transform {
            let offset = Self::convert_from_object_type(property.metadata_type(), Some(offset.clone()))?;
            let scale = Self::convert_from_object_type(property.metadata_type(), Some(scale.clone()))?;
            value = MetadataClassProperty::value_transform_in_place(
                value,
                offset.as_ref(),
                scale.as_ref(),
                &apply_transform_leaf,
            )?;
        }
        if property.is_array() {
            let MetadataValue::Array(values) = value else {
                return Err(MetadataError::Type("value is not an array".into()));
            };
            values
                .into_iter()
                .map(|v| {
                    Self::convert_to_object_type(property.metadata_type(), Some(v)).map(|v| v.expect("defined input"))
                })
                .collect::<Result<Vec<_>>>()
                .map(MetadataValue::Array)
        } else {
            Ok(Self::convert_to_object_type(property.metadata_type(), Some(value))?.expect("defined input"))
        }
    }
}
