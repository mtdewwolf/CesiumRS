//! CesiumJS `Proxy` and `DefaultProxy`.

use super::uri::encode_uri_component;

pub trait Proxy: Send + Sync {
    fn get_url(&self, resource: &str) -> String;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultProxy {
    pub proxy: String,
}

impl DefaultProxy {
    pub fn new(proxy: impl Into<String>) -> Self {
        Self { proxy: proxy.into() }
    }
}

impl Proxy for DefaultProxy {
    fn get_url(&self, resource: &str) -> String {
        let prefix = if self.proxy.contains('?') { "" } else { "?" };
        format!("{}{prefix}{}", self.proxy, encode_uri_component(resource))
    }
}
