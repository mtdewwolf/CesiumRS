//! CesiumJS `PropertyTexture`: a set of 3D Metadata properties of one class
//! stored in glTF textures.

use super::gltf_pipeline::js;
use super::metadata_value::{developer, JsMap, MetadataError};
use super::property_texture_property::{GltfTextures, PropertyTextureProperty};
use super::{MetadataClass, MetadataId};
use crate::renderer::Gl;
use serde_json::Value;
use std::sync::Arc;

pub struct PropertyTexture<G: Gl> {
    name: Option<String>,
    id: Option<MetadataId>,
    class: Arc<MetadataClass>,
    properties: JsMap<PropertyTextureProperty<G>>,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl<G: Gl> PropertyTexture<G> {
    /// The property texture JSON, each of whose properties the class must
    /// define; `textures` holds the model's textures by glTF index.
    pub fn new(
        property_texture: &Value,
        class: Arc<MetadataClass>,
        textures: &GltfTextures<G>,
        name: Option<String>,
        id: Option<MetadataId>,
    ) -> Result<Self, MetadataError> {
        if !(property_texture.is_object() || property_texture.is_array()) {
            return Err(developer("Expected options.propertyTexture to be typeof object"));
        }
        let mut properties = JsMap::new();
        let json = property_texture.get("properties").filter(|p| !p.is_null());
        for property_id in js::for_in(json) {
            let property = js::get(json, &property_id).expect("own property");
            let class_property = class.properties.get(&property_id).cloned().ok_or_else(|| {
                developer("Expected options.classProperty to be typeof object, actual typeof was undefined")
            })?;
            properties.insert(property_id, PropertyTextureProperty::new(property, class_property, textures)?);
        }
        Ok(Self {
            name,
            id,
            class,
            properties,
            extras: property_texture.get("extras").cloned(),
            extensions: property_texture.get("extensions").cloned(),
        })
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn id(&self) -> Option<&MetadataId> {
        self.id.as_ref()
    }

    pub fn class(&self) -> &Arc<MetadataClass> {
        &self.class
    }

    pub fn properties(&self) -> &JsMap<PropertyTextureProperty<G>> {
        &self.properties
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }

    pub fn get_property(&self, property_id: &str) -> Option<&PropertyTextureProperty<G>> {
        self.properties.get(property_id)
    }
}
