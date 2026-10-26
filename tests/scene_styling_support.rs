mod common;

use cesium_rs::core::{Cartesian3, Color, Ellipsoid, GeographicProjection, Matrix3, OrientedBoundingBox, Rectangle};
use cesium_rs::scene::cesium_3d_tile_style::StyleInput;
use cesium_rs::scene::{
    compute_fly_to_location_for_rectangle, default_osm_buildings_style_expression, geospatial_features_url,
    imodel_export_to_tileset_url, is_supported_reality_data_mesh_type, is_supported_reality_data_source_type,
    rectangle_terrain_sample_points, Camera, CameraOptions, Cesium3DTileOptimizations, Cesium3DTilePass,
    Cesium3DTilePassState, Cesium3DTileStyle, Cesium3DTileStyleEngine, Cesium3DTilesetCache, Cesium3DTilesetHeatmap,
    Cesium3DTilesetStatistics, CreateOsmBuildingsOptions, CreditDisplay, GooglePhotorealistic3DTilesetConfig,
    GooglePhotorealisticApiOptions, GooglePhotorealisticSource, RealityDataTilesetOptions, SceneMode,
    SimpleTileContentCounts, StyleableTile, CESIUM_OSM_BUILDINGS_ION_ASSET_ID, DEFAULT_GOOGLE_TILES_CACHE_BYTES,
    DEFAULT_GOOGLE_TILES_CACHE_OVERFLOW_BYTES, GOOGLE_PHOTOREALISTIC_ION_ASSET_ID,
};
use common::*;
use serde_json::json;

#[test]
fn reference_cesium_3d_tileset_statistics() {
    run(
        "scene",
        "Cesium3DTilesetStatistics",
        |_| EXACT,
        |f, _a| match f {
            "basic_counts" => {
                let mut stats = Cesium3DTilesetStatistics::new();
                stats.selected = 12;
                stats.visited = 34;
                stats.number_of_commands = 56;
                stats.number_of_attempted_requests = 7;
                stats.number_of_pending_requests = 3;
                stats.number_of_tiles_processing = 2;
                stats.number_of_tiles_with_content_ready = 10;
                stats.number_of_tiles_total = 100;
                stats.number_of_loaded_tiles_total = 15;
                stats.number_of_tiles_culled_with_children_union = 4;

                let mut cloned = Cesium3DTilesetStatistics::new();
                stats.clone_cesium_into(&mut cloned);

                stats.clear();

                Some(Out::Ok(json!({
                    "cleared": {
                        "selected": stats.selected,
                        "visited": stats.visited,
                        "numberOfCommands": stats.number_of_commands,
                        "numberOfAttemptedRequests": stats.number_of_attempted_requests,
                        "numberOfPendingRequests": stats.number_of_pending_requests,
                        "numberOfTilesProcessing": stats.number_of_tiles_processing,
                        "numberOfTilesWithContentReady": stats.number_of_tiles_with_content_ready,
                        "numberOfTilesTotal": stats.number_of_tiles_total,
                        "numberOfLoadedTilesTotal": stats.number_of_loaded_tiles_total,
                        "numberOfTilesCulledWithChildrenUnion": stats.number_of_tiles_culled_with_children_union,
                    },
                    "cloned": {
                        "selected": cloned.selected,
                        "visited": cloned.visited,
                        "numberOfCommands": cloned.number_of_commands,
                        "numberOfAttemptedRequests": cloned.number_of_attempted_requests,
                        "numberOfPendingRequests": cloned.number_of_pending_requests,
                        "numberOfTilesProcessing": cloned.number_of_tiles_processing,
                        "numberOfTilesWithContentReady": cloned.number_of_tiles_with_content_ready,
                        "numberOfTilesTotal": cloned.number_of_tiles_total,
                        "numberOfLoadedTilesTotal": cloned.number_of_loaded_tiles_total,
                        "numberOfTilesCulledWithChildrenUnion": cloned.number_of_tiles_culled_with_children_union,
                    }
                })))
            }
            "content_selection_and_loads" => {
                let leaf1 = SimpleTileContentCounts {
                    features_length: 10,
                    points_length: 100,
                    triangles_length: 50,
                    geometry_byte_length: 2048,
                    batch_table_byte_length: 512,
                    textures_byte_length: 4096,
                    ..Default::default()
                };

                let leaf2 = SimpleTileContentCounts {
                    features_length: 5,
                    points_length: 20,
                    triangles_length: 10,
                    geometry_byte_length: 1024,
                    batch_table_byte_length: 256,
                    textures_byte_length: 2048,
                    ..Default::default()
                };

                let composite =
                    SimpleTileContentCounts { inner_contents: vec![leaf1.clone(), leaf2], ..Default::default() };

                let mut stats = Cesium3DTilesetStatistics::new();
                stats.increment_selection_counts_simple(&composite);
                let after_select = json!({
                    "numberOfFeaturesSelected": stats.number_of_features_selected,
                    "numberOfPointsSelected": stats.number_of_points_selected,
                    "numberOfTrianglesSelected": stats.number_of_triangles_selected,
                });

                stats.increment_load_counts_simple(&composite);
                let after_load = json!({
                    "numberOfFeaturesLoaded": stats.number_of_features_loaded,
                    "numberOfPointsLoaded": stats.number_of_points_loaded,
                    "geometryByteLength": stats.geometry_byte_length,
                    "batchTableByteLength": stats.batch_table_byte_length,
                    "texturesByteLength": stats.textures_byte_length,
                });

                stats.decrement_load_counts_simple(&leaf1);
                let after_decrement = json!({
                    "numberOfFeaturesLoaded": stats.number_of_features_loaded,
                    "numberOfPointsLoaded": stats.number_of_points_loaded,
                    "geometryByteLength": stats.geometry_byte_length,
                    "batchTableByteLength": stats.batch_table_byte_length,
                    "texturesByteLength": stats.textures_byte_length,
                });

                Some(Out::Ok(json!({
                    "afterSelect": after_select,
                    "afterLoad": after_load,
                    "afterDecrement": after_decrement,
                })))
            }
            "model_texture_reference_counting" => {
                let model1 = SimpleTileContentCounts {
                    features_length: 20,
                    triangles_length: 100,
                    geometry_byte_length: 4000,
                    batch_table_byte_length: 200,
                    is_model_content: true,
                    textures: vec![("texA".into(), 1000), ("texB".into(), 2000)],
                    ..Default::default()
                };

                let model2 = SimpleTileContentCounts {
                    features_length: 30,
                    triangles_length: 150,
                    geometry_byte_length: 6000,
                    batch_table_byte_length: 300,
                    is_model_content: true,
                    textures: vec![("texB".into(), 2000), ("texC".into(), 4000)],
                    ..Default::default()
                };

                let mut stats = Cesium3DTilesetStatistics::new();

                stats.increment_load_counts_simple(&model1);
                let step1 = json!({
                    "texturesByteLength": stats.textures_byte_length,
                    "refCounts": stats.textures_reference_counter_by_id,
                });

                stats.increment_load_counts_simple(&model2);
                let step2 = json!({
                    "texturesByteLength": stats.textures_byte_length,
                    "refCounts": stats.textures_reference_counter_by_id,
                });

                stats.decrement_load_counts_simple(&model1);
                let step3 = json!({
                    "texturesByteLength": stats.textures_byte_length,
                    "refCounts": stats.textures_reference_counter_by_id,
                });

                stats.decrement_load_counts_simple(&model2);
                let step4 = json!({
                    "texturesByteLength": stats.textures_byte_length,
                    "refCounts": stats.textures_reference_counter_by_id,
                });

                Some(Out::Ok(json!({
                    "step1": step1,
                    "step2": step2,
                    "step3": step3,
                    "step4": step4,
                })))
            }
            _ => None,
        },
    );
}

#[test]
fn reference_cesium_3d_tileset_heatmap() {
    run(
        "scene",
        "Cesium3DTilesetHeatmap",
        |_| TIGHT,
        |f, a| match f {
            "heatmap_color_palette_interpolation" => {
                let values = a[0].as_array().unwrap();
                let mut heatmap = Cesium3DTilesetHeatmap::new(Some("error".into()));
                heatmap.previous_minimum = 0.0;
                heatmap.previous_maximum = 100.0;

                let colors: Vec<_> = values
                    .iter()
                    .map(|val| {
                        let v = val.as_f64().unwrap();
                        let color = heatmap.colorize_value(v, true, true);
                        json!({
                            "val": v,
                            "color": color.map(|c| json!([c.red, c.green, c.blue, c.alpha])),
                        })
                    })
                    .collect();

                Some(Out::Ok(json!({ "colors": colors })))
            }
            "heatmap_lifecycle_min_max_reset" => {
                let mut heatmap = Cesium3DTilesetHeatmap::new(Some("error".into()));
                heatmap.colorize_value(20.0, true, true);
                heatmap.colorize_value(80.0, true, true);

                let frame1_min_max = json!({ "min": heatmap.minimum, "max": heatmap.maximum });
                heatmap.reset_minimum_maximum();
                let after_reset = json!({
                    "prevMin": heatmap.previous_minimum,
                    "prevMax": heatmap.previous_maximum,
                    "min": heatmap.minimum,
                    "max": heatmap.maximum,
                });

                let c2 = heatmap.colorize_value(50.0, true, true).unwrap();
                let frame2_color = json!([c2.red, c2.green, c2.blue, c2.alpha]);

                heatmap.set_reference_minimum_maximum(10.0, 200.0, "error");
                heatmap.reset_minimum_maximum();
                let after_reference = json!({
                    "prevMin": heatmap.previous_minimum,
                    "prevMax": heatmap.previous_maximum,
                });

                Some(Out::Ok(json!({
                    "frame1MinMax": frame1_min_max,
                    "afterReset": after_reset,
                    "frame2Color": frame2_color,
                    "afterReference": after_reference,
                })))
            }
            _ => None,
        },
    );
}

#[test]
fn reference_cesium_3d_tile_optimizations() {
    run(
        "scene",
        "Cesium3DTileOptimizations",
        |_| EXACT,
        |f, _a| match f {
            "check_children_within_parent_cases" => {
                let parent_box = OrientedBoundingBox {
                    center: Cartesian3::ZERO,
                    half_axes: Matrix3::from_scale(Cartesian3::new(10.0, 10.0, 10.0)),
                };

                let child_box_inside = OrientedBoundingBox {
                    center: Cartesian3::new(1.0, 2.0, 3.0),
                    half_axes: Matrix3::from_scale(Cartesian3::new(2.0, 2.0, 2.0)),
                };

                let child_box_outside = OrientedBoundingBox {
                    center: Cartesian3::new(8.0, 0.0, 0.0),
                    half_axes: Matrix3::from_scale(Cartesian3::new(5.0, 5.0, 5.0)),
                };

                let (ok_inside, hint_inside) = Cesium3DTileOptimizations::check_children_within_parent(
                    Some(&parent_box),
                    &[Some(&child_box_inside)],
                );
                let (ok_outside, hint_outside) = Cesium3DTileOptimizations::check_children_within_parent(
                    Some(&parent_box),
                    &[Some(&child_box_outside)],
                );
                let (ok_missing_child, hint_missing_child) =
                    Cesium3DTileOptimizations::check_children_within_parent(Some(&parent_box), &[None]);
                let (ok_missing_parent, hint_missing_parent) =
                    Cesium3DTileOptimizations::check_children_within_parent(None, &[Some(&child_box_inside)]);

                Some(Out::Ok(json!({
                    "resInside": { "result": ok_inside, "hint": hint_inside as i32 },
                    "resOutside": { "result": ok_outside, "hint": hint_outside as i32 },
                    "resMissingChild": { "result": ok_missing_child, "hint": hint_missing_child as i32 },
                    "resMissingParent": { "result": ok_missing_parent, "hint": hint_missing_parent as i32 },
                })))
            }
            _ => None,
        },
    );
}

#[test]
fn test_cesium_3d_tileset_cache_lru_and_trim() {
    let mut cache = Cesium3DTilesetCache::new();
    assert!(cache.is_empty());

    let tile1 = cache.add(101);
    let _tile2 = cache.add(102);
    let tile3 = cache.add(103);
    assert_eq!(cache.len(), 3);

    // Reset marks frame beginning, moving sentinel to tail
    cache.reset();

    // Touch tile1 and tile3: moved after sentinel
    cache.touch(tile1);
    cache.touch(tile3);

    let mut unloaded = Vec::new();
    // Before reset, unselected tiles left of sentinel (tile2) get unloaded
    cache.unload_tiles(|_trim| true, |id| unloaded.push(id));
    assert_eq!(unloaded, vec![102]);
    assert_eq!(cache.len(), 2);

    // Reset moves sentinel to tail, making all tiles eligible for unloading
    cache.reset();
    cache.touch(tile1); // tile1 saved

    unloaded.clear();
    cache.trim();
    cache.unload_tiles(|trim| trim, |id| unloaded.push(id));
    assert_eq!(unloaded, vec![103]);
    assert_eq!(cache.len(), 1);
}

struct MockTile {
    last_style_time: u64,
    features_count: usize,
    styled: bool,
}

impl StyleableTile for MockTile {
    fn last_style_time(&self) -> u64 {
        self.last_style_time
    }
    fn set_last_style_time(&mut self, time: u64) {
        self.last_style_time = time;
    }
    fn apply_style(&mut self, _style: Option<&Cesium3DTileStyle>) {
        self.styled = true;
    }
    fn features_length(&self) -> usize {
        self.features_count
    }
}

#[test]
fn test_cesium_3d_tile_style_engine() {
    let mut engine = Cesium3DTileStyleEngine::new();
    assert!(!engine.is_dirty());

    let mut style = Cesium3DTileStyle::default();
    style.set_show(Some(StyleInput::Boolean(true))).unwrap();
    engine.set_style(Some(style));
    assert!(engine.is_dirty());

    let mut tiles = [
        MockTile { last_style_time: 0, features_count: 5, styled: false },
        MockTile { last_style_time: 0, features_count: 10, styled: false },
    ];

    let (tiles_styled, features_styled) = engine.apply_style_to_tiles(&mut tiles);
    assert_eq!(tiles_styled, 2);
    assert_eq!(features_styled, 15);
    assert_eq!(engine.last_style_time(), 1);
    assert!(tiles[0].styled);
    assert!(tiles[1].styled);

    // Applying again without dirty does not re-style
    engine.reset_dirty();
    let (t2, f2) = engine.apply_style_to_tiles(&mut tiles);
    assert_eq!(t2, 0);
    assert_eq!(f2, 0);
}

#[test]
fn test_cesium_3d_tile_pass_state() {
    let state = Cesium3DTilePassState::for_pass(Cesium3DTilePass::Render);
    assert_eq!(state.pass, Cesium3DTilePass::Render);
    assert!(!state.ready);
    assert!(state.camera.is_none());
    assert!(state.culling_volume.is_none());
    assert!(state.command_list.is_none());
}

#[test]
fn test_credit_display_lifecycle() {
    let mut display = CreditDisplay::new(Some(" | ".into()));
    assert_eq!(display.delimiter(), " | ");

    let c1 = cesium_rs::core::Credit::new("Attribution A", true);
    let c2 = cesium_rs::core::Credit::new("Attribution B", false);

    display.add_credit_to_next_frame(c1.clone());
    display.add_credit_to_next_frame(c1.clone());
    display.add_credit_to_next_frame(c2.clone());

    display.end_frame();
    let screen = display.current_screen_credits();
    assert_eq!(screen.len(), 1);
    assert_eq!(screen[0].credit.html(), "Attribution A");
    assert_eq!(screen[0].count, 2);

    let lightbox = display.current_lightbox_credits();
    assert_eq!(lightbox.len(), 1);
    assert_eq!(lightbox[0].credit.html(), "Attribution B");
    assert_eq!(lightbox[0].count, 1);

    display.begin_frame();
    display.end_frame();
    assert!(display.current_screen_credits().is_empty());
}

#[test]
fn test_compute_fly_to_location_for_rectangle() {
    let rect = Rectangle::new(0.1, 0.2, 0.3, 0.4);
    let points = rectangle_terrain_sample_points(&rect);
    assert_eq!(points.len(), 5);

    let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
    let projection = GeographicProjection::default();

    let loc_3d = compute_fly_to_location_for_rectangle(
        &rect,
        &mut camera,
        SceneMode::Scene3D,
        &Ellipsoid::WGS84,
        &projection,
        Some(&[Some(100.0), Some(250.0), Some(50.0)]),
    );
    assert!(loc_3d.is_some());
    let cart = loc_3d.unwrap();
    assert!(cart.height >= 250.0);
}

#[test]
fn test_create_osm_buildings_async() {
    assert_eq!(CESIUM_OSM_BUILDINGS_ION_ASSET_ID, 96188);
    let expr = default_osm_buildings_style_expression(Some(Color::RED));
    assert!(expr.contains("rgb(255,0,0)"));
    assert!(expr.contains("Boolean(${feature['cesium#color']})"));

    let opts = CreateOsmBuildingsOptions::default();
    let resolved = opts.resolve_style();
    assert!(resolved.color().is_some());
}

#[test]
fn test_create_google_photorealistic_3d_tileset() {
    assert_eq!(GOOGLE_PHOTOREALISTIC_ION_ASSET_ID, 2275207);
    assert_eq!(DEFAULT_GOOGLE_TILES_CACHE_BYTES, 1610612736);
    assert_eq!(DEFAULT_GOOGLE_TILES_CACHE_OVERFLOW_BYTES, 1073741824);

    let config_ion = GooglePhotorealistic3DTilesetConfig::new(None);
    assert_eq!(config_ion.source, GooglePhotorealisticSource::IonAssetId(GOOGLE_PHOTOREALISTIC_ION_ASSET_ID));
    assert!(config_ion.requires_geocoder_warning);

    let config_direct = GooglePhotorealistic3DTilesetConfig::new(Some(GooglePhotorealisticApiOptions {
        key: Some("test_api_key_123".into()),
        only_using_with_google_geocoder: true,
    }));
    assert!(!config_direct.requires_geocoder_warning);
    match config_direct.source {
        GooglePhotorealisticSource::DirectUrl { url, api_key } => {
            assert!(url.contains("v1/3dtiles/root.json"));
            assert_eq!(api_key, "test_api_key_123");
        }
        _ => panic!("expected direct url"),
    }
}

#[test]
fn test_itwin_data() {
    use cesium_rs::core::itwin_platform::RealityDataType;

    assert!(is_supported_reality_data_mesh_type(RealityDataType::Cesium3DTiles));
    assert!(is_supported_reality_data_mesh_type(RealityDataType::Pnts));
    assert!(is_supported_reality_data_mesh_type(RealityDataType::RealityMesh3DTiles));
    assert!(is_supported_reality_data_mesh_type(RealityDataType::Terrain3DTiles));
    assert!(!is_supported_reality_data_mesh_type(RealityDataType::Kml));

    assert!(is_supported_reality_data_source_type(RealityDataType::Kml));
    assert!(is_supported_reality_data_source_type(RealityDataType::GeoJson));
    assert!(!is_supported_reality_data_source_type(RealityDataType::Pnts));

    let export_url = imodel_export_to_tileset_url("https://api.bentley.com/mesh/123").unwrap();
    assert_eq!(export_url, "https://api.bentley.com/mesh/123/tileset.json");

    let feat_url = geospatial_features_url("https://api.bentley.com", "itwin1", "col1", Some(500)).unwrap();
    assert_eq!(feat_url, "https://api.bentley.com/geospatial-features/itwins/itwin1/ogc/collections/col1/items?limit=500&client=CesiumJS");

    let reality_opts = RealityDataTilesetOptions::new("itwin1", "real1");
    assert_eq!(reality_opts.maximum_screen_space_error, 4.0);
}
