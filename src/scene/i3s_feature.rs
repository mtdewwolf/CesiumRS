//! Port of CesiumJS `Scene/I3SFeature`.

use crate::core::resource::{DerivedResourceOptions, Resource};
use crate::core::resource_client::ResourceClient;
use crate::scene::scene::Result;
use crate::scene::i3s_data_provider::I3SDataProvider;
use crate::scene::i3s_node::I3SNode;
use serde_json::Value;

/// Port of `I3SFeature`: an I3S feature. Do not construct directly; access
/// tiles through [`I3SNode::feature_data`].
#[derive(Clone)]
pub struct I3SFeature {
    resource: Resource,
    data: Option<Value>,
}

impl I3SFeature {
    /// Port of `new I3SFeature(parent, uri)`.
    pub fn new(parent: &I3SNode, uri: &str) -> Self {
        let resource = if parent.node_index().is_some() {
            let layer = parent.layer().expect("I3S feature parent has a layer");
            let node_resource = parent
                .data()
                .and_then(|data| data.get("mesh"))
                .and_then(|mesh| mesh.get("attribute"))
                .and_then(|attribute| attribute.get("resource"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            layer
                .resource()
                .get_derived_resource(DerivedResourceOptions {
                    url: Some(format!("nodes/{node_resource}/{uri}")),
                    ..DerivedResourceOptions::default()
                })
        } else {
            parent.resource().and_then(|resource| {
                resource.get_derived_resource(DerivedResourceOptions {
                    url: Some(uri.to_string()),
                    ..DerivedResourceOptions::default()
                })
            })
        };
        Self { resource: resource.expect("valid I3S feature URI"), data: None }
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    /// Port of the `data` getter: the feature document, or `None` until loaded.
    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    /// Port of `I3SFeature.prototype.load`.
    pub async fn load(&mut self, client: &ResourceClient) -> Result<Value> {
        let data = I3SDataProvider::load_json(&self.resource, client).await?;
        self.data = Some(data.clone());
        Ok(data)
    }
}
