//! World terrain factories retain Cesium's pinned asset IDs and option defaults.
use super::cesium_terrain_provider::CesiumTerrainOptions;
use super::*;
pub async fn create_world_terrain_async(
    ion: &Ion,
    client: ResourceClient,
    request_vertex_normals: bool,
    request_water_mask: bool,
) -> Result<CesiumTerrainProvider, RuntimeError> {
    CesiumTerrainProvider::from_ion_asset_id(
        1,
        ion,
        Default::default(),
        CesiumTerrainOptions {
            request_vertex_normals,
            request_water_mask,
            ellipsoid: Ellipsoid::WGS84,
            ..Default::default()
        },
        client,
    )
    .await
}
pub async fn create_world_bathymetry_async(
    ion: &Ion,
    client: ResourceClient,
    request_vertex_normals: bool,
) -> Result<CesiumTerrainProvider, RuntimeError> {
    CesiumTerrainProvider::from_ion_asset_id(
        2426648,
        ion,
        Default::default(),
        CesiumTerrainOptions { request_vertex_normals, ..Default::default() },
        client,
    )
    .await
}
