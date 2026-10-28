//! CesiumJS `IonImageryProviderFactory`: supported external imagery asset
//! types and factory mapping for Cesium ion asset endpoints.

use serde_json::Value;

/// Known external imagery service types supported by Cesium ion endpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IonImageryExternalType {
    ArcGisMapServer,
    Bing,
    GoogleEarth,
    Mapbox,
    SingleTile,
    Tms,
    UrlTemplate,
    Wms,
    Wmts,
    Google2DMaps,
    AzureMaps,
}

impl IonImageryExternalType {
    /// Parses an external type string as returned in an ion endpoint JSON.
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "ARCGIS_MAPSERVER" => Some(Self::ArcGisMapServer),
            "BING" => Some(Self::Bing),
            "GOOGLE_EARTH" => Some(Self::GoogleEarth),
            "MAPBOX" => Some(Self::Mapbox),
            "SINGLE_TILE" => Some(Self::SingleTile),
            "TMS" => Some(Self::Tms),
            "URL_TEMPLATE" => Some(Self::UrlTemplate),
            "WMS" => Some(Self::Wms),
            "WMTS" => Some(Self::Wmts),
            "GOOGLE_2D_MAPS" => Some(Self::Google2DMaps),
            "AZURE_MAPS" => Some(Self::AzureMaps),
            _ => None,
        }
    }

    /// The string identifier matching CesiumJS.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ArcGisMapServer => "ARCGIS_MAPSERVER",
            Self::Bing => "BING",
            Self::GoogleEarth => "GOOGLE_EARTH",
            Self::Mapbox => "MAPBOX",
            Self::SingleTile => "SINGLE_TILE",
            Self::Tms => "TMS",
            Self::UrlTemplate => "URL_TEMPLATE",
            Self::Wms => "WMS",
            Self::Wmts => "WMTS",
            Self::Google2DMaps => "GOOGLE_2D_MAPS",
            Self::AzureMaps => "AZURE_MAPS",
        }
    }
}

/// Factory utility mapping supported external imagery asset types.
pub struct IonImageryProviderFactory;

impl IonImageryProviderFactory {
    /// Returns whether the specified string corresponds to a supported external type.
    pub fn is_supported_external_type(external_type: &str) -> bool {
        IonImageryExternalType::from_str_value(external_type).is_some()
    }

    /// List of all supported external types.
    pub fn supported_external_types() -> &'static [&'static str] {
        &[
            "ARCGIS_MAPSERVER",
            "BING",
            "GOOGLE_EARTH",
            "MAPBOX",
            "SINGLE_TILE",
            "TMS",
            "URL_TEMPLATE",
            "WMS",
            "WMTS",
            "GOOGLE_2D_MAPS",
            "AZURE_MAPS",
        ]
    }

    /// Extracts the service URL from an ion endpoint payload. External assets
    /// carry their service URL in `options.url`; native TMS assets carry it at
    /// the endpoint's top level.
    pub fn resolve_service_url(endpoint: &Value) -> Option<String> {
        let options = endpoint.get("options");
        options
            .and_then(|o| o.get("url"))
            .and_then(|u| u.as_str())
            .or_else(|| endpoint.get("url").and_then(|u| u.as_str()))
            .map(|s| s.to_string())
    }

    /// Extracts the external asset type name from the endpoint JSON, if present.
    pub fn external_type(endpoint: &Value) -> Option<&str> {
        endpoint.get("externalType").and_then(|v| v.as_str())
    }
}
