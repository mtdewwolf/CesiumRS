//! Port of CesiumJS `Scene/I3SSublayer`.

use crate::core::resource::Resource;
use crate::scene::i3s_data_provider::I3SDataProvider;
use crate::scene::i3s_layer::{I3SLayer, LayerParent};
use crate::scene::scene::Result;
use serde_json::Value;
use std::rc::{Rc, Weak};

/// Port of `I3SSublayer`: a sublayer of an I3S Building Scene Layer. Do not
/// construct directly; access through [`I3SDataProvider::sublayers`] and
/// [`I3SSublayer::sublayers`].
pub struct I3SSublayer {
    data_provider: Weak<I3SDataProvider>,
    parent: LayerParent,
    data: Value,
    name: Option<String>,
    model_name: Option<String>,
    visibility: bool,
    resource: Option<Resource>,
    sublayers: Vec<Rc<I3SSublayer>>,
    i3s_layers: Vec<Rc<I3SLayer>>,
}

impl I3SSublayer {
    /// Port of `new I3SSublayer(dataProvider, parent, sublayerData)`.
    pub fn new(data_provider: &Rc<I3SDataProvider>, parent: LayerParent, sublayer_data: Value) -> Self {
        let name = sublayer_data.get("name").and_then(Value::as_str).map(str::to_owned);
        let model_name = sublayer_data.get("modelName").and_then(Value::as_str).map(str::to_owned);
        let visibility = sublayer_data.get("visibility").and_then(Value::as_bool).unwrap_or(true);
        Self {
            data_provider: Rc::downgrade(data_provider),
            parent,
            data: sublayer_data,
            name,
            model_name,
            visibility,
            resource: None,
            sublayers: Vec::new(),
            i3s_layers: Vec::new(),
        }
    }

    /// Port of the `data` getter.
    pub fn data(&self) -> &Value {
        &self.data
    }

    /// Port of the `name` getter.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Port of the `modelName` getter.
    pub fn model_name(&self) -> Option<&str> {
        self.model_name.as_deref()
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> Option<&Resource> {
        self.resource.as_ref()
    }

    /// Port of the `sublayers` getter: child sublayers.
    pub fn sublayers(&self) -> &[Rc<I3SSublayer>] {
        &self.sublayers
    }

    /// Port of the `visibility` getter and setter. Setting it flips the
    /// visibility of the layers this sublayer owns.
    pub fn visibility(&self) -> bool {
        self.visibility
    }

    pub fn set_visibility(&mut self, value: bool) {
        if self.visibility != value {
            self.visibility = value;
            for layer in &self.i3s_layers {
                layer.update_visibility();
            }
        }
    }

    /// Port of the `show` getter: `visibility && parent.show`.
    pub fn show(&self) -> bool {
        self.visibility && parent_show(&self.parent)
    }

    /// The layers owned by this sublayer, as `Rc`s so a group can share them
    /// with the provider.
    pub fn i3s_layers(&self) -> &[Rc<I3SLayer>] {
        &self.i3s_layers
    }

    /// Port of `I3SSublayer._fromData`.
    pub async fn from_data(
        data_provider: &Rc<I3SDataProvider>,
        building_layer_url: &str,
        sublayer_data: Value,
        parent: LayerParent,
        client: &crate::core::resource_client::ResourceClient,
    ) -> Result<Rc<I3SSublayer>> {
        let mut sublayer = Rc::new(Self::new(data_provider, parent, sublayer_data));
        let layer_type = sublayer
            .data
            .get("layerType")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if layer_type.as_deref() == Some("group") {
            let children = sublayer
                .data
                .get("sublayers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut child_sublayers = Vec::new();
            for child_data in children {
                child_sublayers.push(
                    Box::pin(Self::from_data(
                        data_provider,
                        building_layer_url,
                        child_data,
                        LayerParent::Sublayer(Rc::downgrade(&sublayer)),
                        client,
                    ))
                    .await?,
                );
            }
            let group = Rc::get_mut(&mut sublayer).expect("sublayer is not shared yet");
            for child in child_sublayers {
                group.i3s_layers.extend(child.i3s_layers.iter().cloned());
                group.sublayers.push(child);
            }
        } else if layer_type.as_deref() == Some("3DObject") {
            let sublayer_id = sublayer
                .data
                .get("id")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let sublayer_url = format!("{building_layer_url}/sublayers/{sublayer_id}");
            let resource = Resource::new(sublayer_url.clone()).expect("valid sublayer URI");
            resource.set_query_parameters(&data_provider.resource().options().query_parameters.clone(), false);
            resource.append_forward_slash();
            let layer_data = I3SDataProvider::load_json(&resource, client).await?;
            let layer = I3SLayer::new(data_provider, LayerParent::Sublayer(Rc::downgrade(&sublayer)), layer_data)?;
            Rc::get_mut(&mut sublayer).expect("sublayer is not shared yet").i3s_layers.push(layer);
        } else {
            eprintln!(
                "{} layer {} is skipped as not supported.",
                sublayer
                    .data
                    .get("layerType")
                    .and_then(Value::as_str)
                    .unwrap_or("undefined"),
                sublayer
                    .data
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("undefined")
            );
        }
        Ok(sublayer)
    }
}

/// `parent.show` for both parent kinds.
fn parent_show(parent: &LayerParent) -> bool {
    match parent {
        LayerParent::Provider(provider) => provider.upgrade().is_some_and(|p| p.show()),
        LayerParent::Sublayer(sublayer) => sublayer.upgrade().is_some_and(|s| s.show()),
    }
}
