//! Defaults and evaluation-token attribution for ArcGIS map services.
//!
//! The mutable JavaScript namespace is represented as an owned configuration,
//! allowing hosts to choose their own token and endpoints without global state.

use crate::core::{Credit, Resource};
use std::sync::OnceLock;

/// CesiumJS's evaluation-only token. Applications should supply their own.
pub const DEFAULT_ACCESS_TOKEN: &str = "AAPTahokHpH_PEeQIduLwb1A8ZA..rA8bGH-48FscqzwJpCALUEqt2P_mHiflgl0WkBvCxPJzsFMRqAukz-mcG8oZ3eKnfk4MHGvCFMNsNrxXb2fGxfKH-3TQxCZVkCycztIPWRM5OIQ7pzTZIBOO9D7N8VXXpyPtp2bSsxWp5dUpv8qsgrfMHU4IXWGiiKWELBnANXn1QgzyLKngOHAgYWUq2OQ2XuULOpwJwOEWpyX5KvoLavJP0LFJdjB08Mmz-wCvnFqwr8CZjFpKaVSbAT1_aZs2dlMP";

/// Configurable defaults for ArcGIS imagery providers.
#[derive(Clone)]
pub struct ArcGisMapService {
    pub default_access_token: String,
    pub default_world_imagery_server: Resource,
    pub default_world_hillshade_server: Resource,
    pub default_world_ocean_server: Resource,
}

impl Default for ArcGisMapService {
    fn default() -> Self {
        Self {
            default_access_token: DEFAULT_ACCESS_TOKEN.into(),
            default_world_imagery_server: Resource::new(
                "https://ibasemaps-api.arcgis.com/arcgis/rest/services/World_Imagery/MapServer",
            )
            .unwrap(),
            default_world_hillshade_server: Resource::new(
                "https://ibasemaps-api.arcgis.com/arcgis/rest/services/Elevation/World_Hillshade/MapServer",
            )
            .unwrap(),
            default_world_ocean_server: Resource::new(
                "https://ibasemaps-api.arcgis.com/arcgis/rest/services/Ocean/World_Ocean_Base/MapServer",
            )
            .unwrap(),
        }
    }
}

/// Returns the shared on-screen warning only for Cesium's evaluation token.
/// Comparison is with the original token even if a configuration is changed.
pub fn get_default_token_credit(provided_key: &str) -> Option<&'static Credit> {
    static CREDIT: OnceLock<Credit> = OnceLock::new();
    (provided_key == DEFAULT_ACCESS_TOKEN).then(|| CREDIT.get_or_init(|| Credit::new(
        "<b>             This application is using a default ArcGIS access token. Please assign <i>Cesium.ArcGisMapService.defaultAccessToken</i>             with an API key from your ArcGIS Developer account before using the ArcGIS tile services.             You can sign up for a free ArcGIS Developer account at <a href=\"https://developers.arcgis.com/\">https://developers.arcgis.com/</a>.</b>", true)))
}
