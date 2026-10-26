use cesium_rs::core::Resource;
use cesium_rs::scene::arc_gis_map_service::{get_default_token_credit, ArcGisMapService, DEFAULT_ACCESS_TOKEN};

#[test]
fn arcgis_defaults_and_attribution() {
    let mut service = ArcGisMapService::default();
    assert_eq!(service.default_access_token, DEFAULT_ACCESS_TOKEN);
    assert_eq!(
        service.default_world_imagery_server.url(),
        "https://ibasemaps-api.arcgis.com/arcgis/rest/services/World_Imagery/MapServer"
    );
    assert_eq!(
        service.default_world_hillshade_server.url(),
        "https://ibasemaps-api.arcgis.com/arcgis/rest/services/Elevation/World_Hillshade/MapServer"
    );
    assert_eq!(
        service.default_world_ocean_server.url(),
        "https://ibasemaps-api.arcgis.com/arcgis/rest/services/Ocean/World_Ocean_Base/MapServer"
    );
    let credit = get_default_token_credit(DEFAULT_ACCESS_TOKEN).unwrap();
    assert!(credit.show_on_screen);
    assert!(credit.html().contains("https://developers.arcgis.com/"));
    assert!(std::ptr::eq(credit, get_default_token_credit(DEFAULT_ACCESS_TOKEN).unwrap()));
    assert!(get_default_token_credit("my-token").is_none());
    service.default_access_token = "my-token".into();
    service.default_world_imagery_server = Resource::new("https://example.com/map").unwrap();
    assert!(get_default_token_credit(&service.default_access_token).is_none());
    assert_eq!(service.default_world_imagery_server.url(), "https://example.com/map");
    assert!(get_default_token_credit(DEFAULT_ACCESS_TOKEN).is_some());
}
