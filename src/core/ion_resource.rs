//! Ion assets, attribution and shared asynchronous credential refresh.
use super::query::QueryValue;
use super::resource::{DerivedResourceOptions, QueryParameters};
use super::resource_client::{request_error, ResourceFuture};
use super::{Credit, Ion, RequestErrorEvent, Resource, ResourceClient, RuntimeError};
use futures_util::{
    future::{LocalBoxFuture, Shared},
    FutureExt,
};
use serde_json::Value;
use std::{cell::RefCell, ops::Deref, rc::Rc};
#[derive(Default)]
pub struct IonAssetOptions {
    pub access_token: Option<String>,
    pub server: Option<Resource>,
    pub query_parameters: QueryParameters,
}
pub type IonRefreshCallback = Rc<dyn Fn(&mut Value)>;
type Refresh = Shared<LocalBoxFuture<'static, Result<Value, RequestErrorEvent>>>;
struct Root {
    endpoint: RefCell<Value>,
    endpoint_resource: Resource,
    client: ResourceClient,
    headers: Vec<(String, String)>,
    credits: RefCell<Option<Vec<Credit>>>,
    pending: RefCell<Option<Refresh>>,
    callback: Rc<RefCell<Option<IonRefreshCallback>>>,
}
pub struct IonResource {
    resource: Resource,
    root: Rc<Root>,
    is_external: bool,
    pub refresh_callback: Rc<RefCell<Option<IonRefreshCallback>>>,
}
fn authority(url: &str) -> String {
    super::request_scheduler::authority(&super::uri::Uri::parse(url))
}
impl Root {
    fn refresh(self: &Rc<Self>, callback: Rc<RefCell<Option<IonRefreshCallback>>>) -> Refresh {
        if let Some(pending) = self.pending.borrow().as_ref() {
            return pending.clone();
        }
        let weak = Rc::downgrade(self);
        let response = self.client.fetch_json(&self.endpoint_resource);
        let future = async move {
            let result = match response {
                Some(f) => f.await,
                None => Err(request_error("Ion endpoint request was throttled.")),
            };
            if let Some(root) = weak.upgrade() {
                *root.pending.borrow_mut() = None;
                if let Ok(mut endpoint) = result {
                    let hook = callback.borrow().clone().or_else(|| root.callback.borrow().clone());
                    if let Some(hook) = hook {
                        hook(&mut endpoint);
                    }
                    *root.endpoint.borrow_mut() = endpoint.clone();
                    return Ok(endpoint);
                }
            }
            result
        }
        .boxed_local()
        .shared();
        *self.pending.borrow_mut() = Some(future.clone());
        future
    }
}
impl IonResource {
    pub fn create_endpoint_resource(asset_id: u64, ion: &Ion, o: &IonAssetOptions) -> Resource {
        let token = o.access_token.as_ref().or(ion.default_access_token.as_ref());
        let mut query =
            token.map(|v| vec![("access_token".into(), Some(QueryValue::Single(v.clone())))]).unwrap_or_default();
        for (k, v) in &o.query_parameters {
            if let Some((_, old)) = query.iter_mut().find(|(key, _)| key == k) {
                *old = v.clone();
            } else {
                query.push((k.clone(), v.clone()));
            }
        }
        o.server
            .as_ref()
            .unwrap_or(&ion.default_server)
            .get_derived_resource(DerivedResourceOptions {
                url: Some(format!("v1/assets/{asset_id}/endpoint")),
                query_parameters: query,
                headers: ion.client_headers(),
                ..Default::default()
            })
            .unwrap()
    }
    pub async fn from_asset_id(
        asset_id: u64,
        ion: &Ion,
        o: IonAssetOptions,
        client: &ResourceClient,
    ) -> Result<Self, RequestErrorEvent> {
        let endpoint_resource = Self::create_endpoint_resource(asset_id, ion, &o);
        let endpoint = client
            .fetch_json(&endpoint_resource)
            .ok_or_else(|| request_error("Ion endpoint request was throttled."))?
            .await?;
        Self::new(endpoint, endpoint_resource, client.clone(), ion.client_headers())
            .map_err(|e| request_error(e.to_string()))
    }
    pub fn new(
        endpoint: Value,
        endpoint_resource: Resource,
        client: ResourceClient,
        headers: Vec<(String, String)>,
    ) -> Result<Self, RuntimeError> {
        let is_external = endpoint.get("externalType").is_some_and(|v| !v.is_null());
        let url =
            if is_external {
                match endpoint["externalType"].as_str() {
                    Some("3DTILES" | "STK_TERRAIN_SERVER") => endpoint["options"]["url"].as_str(),
                    _ => return Err(RuntimeError::new(
                        "Ion.createResource does not support external imagery assets; use IonImageryProvider instead.",
                    )),
                }
            } else {
                endpoint["url"].as_str()
            }
            .ok_or_else(|| RuntimeError::new("Invalid ion asset URL."))?;
        let resource = Resource::new(url).ok_or_else(|| RuntimeError::new("Invalid ion asset URL."))?;
        let callback = Rc::new(RefCell::new(None));
        let root = Rc::new(Root {
            endpoint: RefCell::new(endpoint),
            endpoint_resource,
            client,
            headers,
            credits: RefCell::new(None),
            pending: RefCell::new(None),
            callback: callback.clone(),
        });
        let value = Self { resource, root, is_external, refresh_callback: callback };
        value.install_hooks();
        Ok(value)
    }
    fn install_hooks(&self) {
        if self.is_external {
            return;
        }
        let domain = authority(self.root.endpoint.borrow()["url"].as_str().unwrap_or(""));
        let root = self.root.clone();
        self.resource.options_mut().request_transform = Some(Rc::new(move |request| {
            if authority(&request.url) != domain {
                return;
            }
            for (k, v) in &root.headers {
                set_header(&mut request.headers, k, v.clone());
            }
            set_header(
                &mut request.headers,
                "Authorization",
                format!("Bearer {}", root.endpoint.borrow()["accessToken"].as_str().unwrap_or("undefined")),
            );
        }));
        let root = self.root.clone();
        let callback = self.refresh_callback.clone();
        let mut options = self.resource.options_mut();
        options.retry_attempts = 1;
        options.fallible_retry_callback = Some(Rc::new(move |_, error| {
            if error.status_code != Some(401) {
                return Box::pin(async { Ok(false) });
            }
            let pending = root.refresh(callback.clone());
            Box::pin(async move { pending.await.map(|_| true) })
        }));
    }
    pub fn endpoint(&self) -> Value {
        self.root.endpoint.borrow().clone()
    }
    pub fn is_external(&self) -> bool {
        self.is_external
    }
    pub fn credits(&self) -> Vec<Credit> {
        if let Some(credits) = &*self.root.credits.borrow() {
            return credits.clone();
        }
        let mut credits = Self::get_credits_from_endpoint(&self.root.endpoint.borrow(), &self.root.endpoint_resource);
        *self.root.credits.borrow_mut() = Some(credits.clone());
        std::mem::take(&mut credits)
    }
    pub fn get_credits_from_endpoint(endpoint: &Value, resource: &Resource) -> Vec<Credit> {
        let mut credits = endpoint["attributions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| {
                Some(Credit::get_ion_credit(&super::Attribution {
                    html: v["html"].as_str()?.into(),
                    collapsible: v["collapsible"].as_bool(),
                }))
            })
            .collect::<Vec<_>>();
        let o = resource.options();
        let token = o.query_parameters.iter().find(|(k, _)| k == "access_token").and_then(|(_, v)| match v {
            Some(QueryValue::Single(s)) => Some(s.as_str()),
            _ => None,
        });
        if let Some(credit) = Ion::get_default_token_credit(token) {
            credits.push(credit);
        }
        credits
    }
    pub fn get_derived_resource(&self, options: DerivedResourceOptions) -> Option<Self> {
        let result = Self {
            resource: self.resource.get_derived_resource(options)?,
            root: self.root.clone(),
            is_external: self.is_external,
            refresh_callback: Rc::new(RefCell::new(None)),
        };
        result.install_hooks();
        Some(result)
    }
    pub fn fetch_image<H: super::ImageHost + 'static>(
        &self,
        client: &ResourceClient,
        host: Rc<H>,
        mut options: super::FetchImageOptions,
    ) -> Option<ResourceFuture<Option<H::Image>>> {
        if !self.is_external {
            options.prefer_blob = true;
            options.skip_color_space_conversion = false;
        }
        client.fetch_image(&self.resource, host, options)
    }
    pub fn as_resource(&self) -> &Resource {
        &self.resource
    }
}
fn set_header(headers: &mut Vec<(String, String)>, key: &str, value: String) {
    if let Some((_, v)) = headers.iter_mut().find(|(k, _)| k == key) {
        *v = value;
    } else {
        headers.push((key.into(), value));
    }
}
impl Clone for IonResource {
    fn clone(&self) -> Self {
        self.get_derived_resource(Default::default()).unwrap()
    }
}
impl Deref for IonResource {
    type Target = Resource;
    fn deref(&self) -> &Resource {
        &self.resource
    }
}
