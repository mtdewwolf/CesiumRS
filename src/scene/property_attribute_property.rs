//! CesiumJS `PropertyAttributeProperty`: a 3D Metadata property stored in a
//! glTF vertex attribute, with the offset and scale the shader applies.

use super::metadata_value::{developer, MetadataError, MetadataValue};
use super::MetadataClassProperty;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyAttributeProperty {
    attribute: Option<Value>,
    class_property: Arc<MetadataClassProperty>,
    offset: MetadataValue,
    scale: MetadataValue,
    has_value_transform: bool,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl PropertyAttributeProperty {
    /// The property JSON; its offset and scale fall back to the class
    /// property's.
    pub fn new(property: &Value, class_property: Arc<MetadataClassProperty>) -> Result<Self, MetadataError> {
        if !(property.is_object() || property.is_array()) {
            return Err(developer("Expected options.property to be typeof object"));
        }
        let offset = MetadataValue::from_optional_json(property.get("offset"));
        let scale = MetadataValue::from_optional_json(property.get("scale"));
        let has_value_transform = class_property.has_value_transform() || offset.is_some() || scale.is_some();
        // Applied on the GPU, so unpacked to the math types uniforms take.
        let offset = class_property
            .unpack_vector_and_matrix_types(offset.unwrap_or_else(|| class_property.offset().clone()), false)?;
        let scale = class_property
            .unpack_vector_and_matrix_types(scale.unwrap_or_else(|| class_property.scale().clone()), false)?;
        Ok(Self {
            attribute: property.get("attribute").cloned(),
            class_property,
            offset,
            scale,
            has_value_transform,
            extras: property.get("extras").cloned(),
            extensions: property.get("extensions").cloned(),
        })
    }

    /// The attribute's semantic, such as `_TEMPERATURE`.
    pub fn attribute(&self) -> Option<&Value> {
        self.attribute.as_ref()
    }

    pub fn has_value_transform(&self) -> bool {
        self.has_value_transform
    }

    pub fn offset(&self) -> &MetadataValue {
        &self.offset
    }

    pub fn scale(&self) -> &MetadataValue {
        &self.scale
    }

    pub fn class_property(&self) -> &Arc<MetadataClassProperty> {
        &self.class_property
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }
}
