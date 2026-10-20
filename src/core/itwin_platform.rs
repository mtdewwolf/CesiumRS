//! iTwin request construction, authorization precedence and API error mapping.
use super::query::QueryValue;
use super::resource::ResourceOptions;
use super::{RequestErrorEvent, Resource, ResourceClient, RuntimeError};
use serde_json::Value;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStatus {
    NotStarted,
    InProgress,
    Complete,
    Invalid,
}
impl ExportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "NotStarted",
            Self::InProgress => "InProgress",
            Self::Complete => "Complete",
            Self::Invalid => "Invalid",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportType {
    IModel,
    Cesium,
    ThreeDTiles,
}
impl ExportType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IModel => "IMODEL",
            Self::Cesium => "CESIUM",
            Self::ThreeDTiles => "3DTILES",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealityDataType {
    Cesium3DTiles,
    Pnts,
    RealityMesh3DTiles,
    Terrain3DTiles,
    Kml,
    GeoJson,
    Unstructured,
}
impl RealityDataType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cesium3DTiles => "Cesium3DTiles",
            Self::Pnts => "PNTS",
            Self::RealityMesh3DTiles => "RealityMesh3DTiles",
            Self::Terrain3DTiles => "Terrain3DTiles",
            Self::Kml => "KML",
            Self::GeoJson => "GeoJSON",
            Self::Unstructured => "Unstructured",
        }
    }
}
pub struct ITwinPlatform {
    pub default_access_token: Option<String>,
    pub default_share_key: Option<String>,
    pub api_endpoint: Resource,
    pub client_version: Option<String>,
}
impl Default for ITwinPlatform {
    fn default() -> Self {
        Self {
            default_access_token: None,
            default_share_key: None,
            api_endpoint: Resource::new("https://api.bentley.com").unwrap(),
            client_version: Some("1.139.0".into()),
        }
    }
}
fn param(k: &str, v: impl Into<String>) -> (String, Option<QueryValue>) {
    (k.into(), Some(QueryValue::Single(v.into())))
}
impl ITwinPlatform {
    pub fn authorization_header(&self) -> Result<String, RuntimeError> {
        if let Some(key) = &self.default_share_key {
            Ok(format!("Basic {key}"))
        } else if let Some(token) = &self.default_access_token {
            Ok(format!("Bearer {token}"))
        } else {
            Err(RuntimeError::new("Must set ITwinPlatform.defaultAccessToken or ITwinPlatform.defaultShareKey first"))
        }
    }
    fn resource(
        &self,
        path: &str,
        query_parameters: super::resource::QueryParameters,
        exports: bool,
    ) -> Result<Resource, RuntimeError> {
        let mut headers = vec![
            ("Authorization".into(), self.authorization_header()?),
            ("Accept".into(), "application/vnd.bentley.itwin-platform.v1+json".into()),
        ];
        if exports {
            headers.push(("Prefer".into(), "return=representation".into()));
        }
        Resource::from_options(ResourceOptions {
            url: format!("{}{path}", self.api_endpoint.url()),
            headers,
            query_parameters,
            ..Default::default()
        })
        .ok_or_else(|| RuntimeError::new("Invalid iTwin URL."))
    }
    pub fn exports_resource(&self, model: &str, changeset: Option<&str>) -> Result<Resource, RuntimeError> {
        let q = vec![
            param("iModelId", model),
            param("exportType", "3DTILES"),
            param("$top", "5"),
            param("client", "CesiumJS"),
        ];
        let resource = self.resource("mesh-export", q, true)?;
        if let Some(version) = &self.client_version {
            resource.append_query_parameters(&vec![param("clientVersion", version)]);
        }
        if let Some(c) = changeset.filter(|s| !s.is_empty()) {
            resource.append_query_parameters(&vec![param("changesetId", c)]);
        }
        Ok(resource)
    }
    pub fn metadata_resource(&self, twin: &str, data: &str) -> Result<Resource, RuntimeError> {
        self.resource(&format!("reality-management/reality-data/{data}"), vec![param("iTwinId", twin)], false)
    }
    pub fn read_access_resource(&self, twin: &str, data: &str) -> Result<Resource, RuntimeError> {
        self.resource(
            &format!("reality-management/reality-data/{data}/readaccess"),
            vec![param("iTwinId", twin)],
            false,
        )
    }
    pub async fn get_exports(
        &self,
        model: &str,
        changeset: Option<&str>,
        client: &ResourceClient,
    ) -> Result<Value, RuntimeError> {
        self.fetch(self.exports_resource(model, changeset)?, None, client).await
    }
    pub async fn get_reality_data_metadata(
        &self,
        twin: &str,
        data: &str,
        client: &ResourceClient,
    ) -> Result<Value, RuntimeError> {
        let response = self.fetch(self.metadata_resource(twin, data)?, Some((twin, data)), client).await?;
        Ok(response["realityData"].clone())
    }
    pub async fn get_reality_data_url(
        &self,
        twin: &str,
        data: &str,
        document: &str,
        client: &ResourceClient,
    ) -> Result<String, RuntimeError> {
        let response = self.fetch(self.read_access_resource(twin, data)?, Some((twin, data)), client).await?;
        Self::reality_data_url(&response, document)
    }
    pub fn reality_data_url(response: &Value, document: &str) -> Result<String, RuntimeError> {
        let base = response["_links"]["containerUrl"]["href"]
            .as_str()
            .ok_or_else(|| RuntimeError::new("Invalid container URL."))?;
        let mut url = url::Url::parse(base).map_err(|e| RuntimeError::new(e.to_string()))?;
        url.set_path(&format!("{}/{document}", url.path()));
        Ok(url.into())
    }
    async fn fetch(
        &self,
        r: Resource,
        reality: Option<(&str, &str)>,
        client: &ResourceClient,
    ) -> Result<Value, RuntimeError> {
        client
            .fetch_json(&r)
            .ok_or_else(|| RuntimeError::new("iTwin request was throttled."))?
            .await
            .map_err(|e| Self::map_error(&e, reality))
    }
    pub fn map_error(error: &RequestErrorEvent, reality: Option<(&str, &str)>) -> RuntimeError {
        let data: Value = error.response.as_ref().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or(Value::Null);
        let text = |v: &Value| v.as_str().unwrap_or("undefined").to_owned();
        RuntimeError::new(match error.status_code {
            Some(401) => format!(
                "Unauthorized, bad token, wrong scopes or headers bad. {}",
                data["error"]["details"][0]["code"].as_str().unwrap_or("")
            ),
            Some(403) => "Not allowed, forbidden".into(),
            Some(404) if reality.is_some() => {
                let (t, d) = reality.unwrap();
                format!("Reality data not found: {t}, {d}")
            }
            Some(422) => {
                format!("Unprocessable Entity:{} {}", text(&data["error"]["code"]), text(&data["error"]["message"]))
            }
            Some(429) => "Too many requests".into(),
            code => format!("Unknown request failure {}", code.map_or("undefined".into(), |n| n.to_string())),
        })
    }
}
