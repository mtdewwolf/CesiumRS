//! Port of CesiumJS `Scene/I3SStatistics`.

use crate::core::resource::Resource;
use crate::core::resource_client::ResourceClient;
use crate::scene::scene::Result;
use crate::scene::i3s_data_provider::I3SDataProvider;
use serde_json::Value;

/// Port of `I3SStatistics`: the statistics document of a Building Scene
/// Layer. Do not construct directly; access statistics through
/// [`I3SDataProvider::attribute_statistics`].
#[derive(Clone)]
pub struct I3SStatistics {
    resource: Resource,
    data: Option<Value>,
}

impl I3SStatistics {
    /// Port of `new I3SStatistics(dataProvider, uri)`.
    pub fn new(data_provider: &I3SDataProvider, uri: &str) -> Self {
        let resource = Resource::new(uri).expect("valid I3S statistics URI");
        resource.set_query_parameters(&data_provider.resource().options().query_parameters.clone(), false);
        resource.append_forward_slash();
        Self { resource, data: None }
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    /// Port of the `data` getter: the statistics document, or `None` until loaded.
    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    /// Port of `I3SStatistics.prototype.names`: the `fieldName`s in the summary.
    pub fn names(&self) -> Vec<String> {
        self.data
            .as_ref()
            .and_then(|data| data.get("summary"))
            .and_then(Value::as_array)
            .map(|summary| {
                summary
                    .iter()
                    .filter_map(|item| item.get("fieldName").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Port of `I3SStatistics.prototype.load`: fetches the document with
    /// [`I3SDataProvider::load_json`] and returns it.
    pub async fn load(&mut self, client: &ResourceClient) -> Result<Value> {
        let data = I3SDataProvider::load_json(&self.resource, client).await?;
        self.data = Some(data.clone());
        Ok(data)
    }

    /// Port of `I3SStatistics.prototype._getValues`: the `mostFrequentValues`
    /// of the summary entry with `attributeName`, or `None` when there is none.
    pub fn get_values(&self, attribute_name: &str) -> Option<Vec<Value>> {
        let summary = self.data.as_ref()?.get("summary").and_then(Value::as_array)?;
        for attribute in summary {
            if attribute.get("fieldName").and_then(Value::as_str) == Some(attribute_name) {
                return Some(
                    attribute
                        .get("mostFrequentValues")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
        }
        None
    }
}
