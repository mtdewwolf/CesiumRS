//! CesiumJS `PropertyTextureProperty`: a 3D Metadata property stored in the
//! channels of a glTF texture, with the offset and scale the shader applies.

use super::gltf_loader_util::create_model_texture_reader;
use super::gltf_pipeline::js;
use super::metadata_value::{developer, MetadataError, MetadataValue};
use super::model_components::TextureReader;
use super::MetadataClassProperty;
use crate::renderer::{Gl, Texture};
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// The model's textures by glTF texture index, written as a JavaScript
/// property key (`"0"`, `"1"`…), as `GltfStructuralMetadataLoader` collects
/// them.
pub type GltfTextures<G> = HashMap<String, Rc<Texture<G>>>;

pub struct PropertyTextureProperty<G: Gl> {
    texture_reader: TextureReader<G>,
    class_property: Arc<MetadataClassProperty>,
    offset: MetadataValue,
    scale: MetadataValue,
    has_value_transform: bool,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl<G: Gl> PropertyTextureProperty<G> {
    /// The property JSON, which is a glTF `textureInfo` plus `channels`; its
    /// offset and scale fall back to the class property's.
    pub fn new(
        property: &Value,
        class_property: Arc<MetadataClassProperty>,
        textures: &GltfTextures<G>,
    ) -> Result<Self, MetadataError> {
        if !(property.is_object() || property.is_array()) {
            return Err(developer("Expected options.property to be typeof object"));
        }
        let channels = match property.get("channels").filter(|c| !c.is_null()) {
            None => "r".to_string(),
            Some(Value::Array(channels)) => reformat_channels(channels),
            Some(_) => return Err(MetadataError::Type("channels.map is not a function".into())),
        };
        let index = property.get("index").map_or_else(|| "undefined".to_string(), js::key);
        let texture_reader = create_model_texture_reader(property, Some(channels), textures.get(&index).cloned());

        let offset = MetadataValue::from_optional_json(property.get("offset"));
        let scale = MetadataValue::from_optional_json(property.get("scale"));
        let has_value_transform = class_property.has_value_transform() || offset.is_some() || scale.is_some();
        // Applied on the GPU, so unpacked to the math types uniforms take.
        let offset = class_property
            .unpack_vector_and_matrix_types(offset.unwrap_or_else(|| class_property.offset().clone()), false)?;
        let scale = class_property
            .unpack_vector_and_matrix_types(scale.unwrap_or_else(|| class_property.scale().clone()), false)?;
        Ok(Self {
            texture_reader,
            class_property,
            offset,
            scale,
            has_value_transform,
            extras: property.get("extras").cloned(),
            extensions: property.get("extensions").cloned(),
        })
    }

    /// The texture, its coordinates and the channels (such as `"rg"`) the
    /// value is read from.
    pub fn texture_reader(&self) -> &TextureReader<G> {
        &self.texture_reader
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

/// `channels.map((i) => "rgba".charAt(i)).join("")`: indices outside 0..4
/// give the empty string.
fn reformat_channels(channels: &[Value]) -> String {
    channels
        .iter()
        .filter_map(|c| {
            let i = js::to_number(Some(c));
            // charAt reads ToIntegerOrInfinity, where NaN is 0.
            let i = if i.is_nan() { 0.0 } else { i.trunc() };
            (0.0..4.0).contains(&i).then(|| "rgba".as_bytes()[i as usize] as char)
        })
        .collect()
}
