//! CesiumJS `ITwinData`: helper functions for resolving iModel exports, reality data,
//! and geospatial features from the Bentley iTwin Platform.

use crate::core::itwin_platform::RealityDataType;
use crate::core::RuntimeError;

/// Validates whether a reality data type is supported as a 3D Tiles mesh / point cloud.
pub fn is_supported_reality_data_mesh_type(data_type: RealityDataType) -> bool {
    matches!(
        data_type,
        RealityDataType::Cesium3DTiles
            | RealityDataType::Pnts
            | RealityDataType::RealityMesh3DTiles
            | RealityDataType::Terrain3DTiles
    )
}

/// Validates whether a reality data type is supported as a vector data source (KML / GeoJSON).
pub fn is_supported_reality_data_source_type(data_type: RealityDataType) -> bool {
    matches!(data_type, RealityDataType::Kml | RealityDataType::GeoJson)
}

/// Converts a mesh export base URL into its canonical `tileset.json` URL.
pub fn imodel_export_to_tileset_url(export_mesh_href: &str) -> Result<String, RuntimeError> {
    if export_mesh_href.is_empty() {
        return Err(RuntimeError::new("iModel export mesh link is empty"));
    }
    let trimmed = export_mesh_href.trim_end_matches('/');
    if trimmed.ends_with("tileset.json") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("{trimmed}/tileset.json"))
    }
}

/// Constructs the Geospatial Features OGC items collection URL.
pub fn geospatial_features_url(
    api_endpoint: &str,
    itwin_id: &str,
    collection_id: &str,
    limit: Option<usize>,
) -> Result<String, RuntimeError> {
    let limit = limit.unwrap_or(10_000);
    if !(1..=10_000).contains(&limit) {
        return Err(RuntimeError::new("limit must be between 1 and 10,000"));
    }
    let endpoint = api_endpoint.trim_end_matches('/');
    Ok(format!(
        "{endpoint}/geospatial-features/itwins/{itwin_id}/ogc/collections/{collection_id}/items?limit={limit}&client=CesiumJS"
    ))
}

/// Options for loading a 3D tileset from Bentley reality data.
#[derive(Clone, Debug, PartialEq)]
pub struct RealityDataTilesetOptions {
    pub itwin_id: String,
    pub reality_data_id: String,
    pub data_type: Option<RealityDataType>,
    pub root_document: Option<String>,
    pub maximum_screen_space_error: f64,
}

impl RealityDataTilesetOptions {
    pub fn new(itwin_id: impl Into<String>, reality_data_id: impl Into<String>) -> Self {
        Self {
            itwin_id: itwin_id.into(),
            reality_data_id: reality_data_id.into(),
            data_type: None,
            root_document: None,
            maximum_screen_space_error: 4.0, // Default 4 for reality data per CesiumJS spec
        }
    }
}
