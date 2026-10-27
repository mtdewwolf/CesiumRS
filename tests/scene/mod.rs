//! Path shim for the imagery provider batch while `src/scene/mod.rs` wiring
//! is owned by the parent integration change.

pub use cesium_rs::scene::*;

#[path = "../../src/scene/arc_gis_map_server_imagery_provider.rs"]
pub mod arc_gis_map_server_imagery_provider;
#[path = "../../src/scene/azure2_d_imagery_provider.rs"]
pub mod azure2_d_imagery_provider;
#[path = "../../src/scene/bing_maps_imagery_provider.rs"]
pub mod bing_maps_imagery_provider;
#[path = "../../src/scene/create_world_imagery.rs"]
pub mod create_world_imagery;
#[path = "../../src/scene/google2_d_imagery_provider.rs"]
pub mod google2_d_imagery_provider;
#[path = "../../src/scene/google_earth_enterprise_imagery_provider.rs"]
pub mod google_earth_enterprise_imagery_provider;
#[path = "../../src/scene/google_earth_enterprise_maps_provider.rs"]
pub mod google_earth_enterprise_maps_provider;
#[path = "../../src/scene/grid_imagery_provider.rs"]
pub mod grid_imagery_provider;
#[path = "../../src/scene/ion_imagery_provider.rs"]
pub mod ion_imagery_provider;
#[path = "../../src/scene/tile_coordinates_imagery_provider.rs"]
pub mod tile_coordinates_imagery_provider;
#[path = "../../src/scene/tile_map_service_imagery_provider.rs"]
pub mod tile_map_service_imagery_provider;
#[path = "../../src/scene/time_dynamic_imagery.rs"]
pub mod time_dynamic_imagery;
#[path = "../../src/scene/web_map_service_imagery_provider.rs"]
pub mod web_map_service_imagery_provider;
#[path = "../../src/scene/web_map_tile_service_imagery_provider.rs"]
pub mod web_map_tile_service_imagery_provider;
