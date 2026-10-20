//! Cesium ion defaults. Configuration is instance-owned rather than global.
use super::{Credit, Resource};
use std::sync::OnceLock;
fn defaults() -> &'static serde_json::Value {
    static DATA: OnceLock<serde_json::Value> = OnceLock::new();
    DATA.get_or_init(|| serde_json::from_str(include_str!("../../assets/ion-defaults.json")).unwrap())
}
pub struct Ion {
    pub default_access_token: Option<String>,
    pub default_server: Resource,
    pub client_version: Option<String>,
}
impl Default for Ion {
    fn default() -> Self {
        Self {
            default_access_token: Some(defaults()["accessToken"].as_str().unwrap().into()),
            default_server: Resource::new("https://api.cesium.com/").unwrap(),
            client_version: Some("1.139.0".into()),
        }
    }
}
impl Ion {
    pub fn get_default_token_credit(token: Option<&str>) -> Option<Credit> {
        (token == defaults()["accessToken"].as_str()).then(|| Credit::new(defaults()["credit"].as_str().unwrap(), true))
    }
    pub fn client_headers(&self) -> Vec<(String, String)> {
        let mut h = vec![("X-Cesium-Client".into(), "CesiumJS".into())];
        if let Some(version) = &self.client_version {
            h.push(("X-Cesium-Client-Version".into(), version.clone()));
        }
        h
    }
}
