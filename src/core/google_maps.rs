//! Instance-owned Google Maps API defaults and required attribution.
use super::{Credit, Resource};
pub struct GoogleMaps {
    pub default_api_key: Option<String>,
    pub map_tiles_api_endpoint: Resource,
    pub default_street_view_static_api_key: Option<String>,
    pub street_view_static_api_endpoint: Resource,
}
impl Default for GoogleMaps {
    fn default() -> Self {
        Self {
            default_api_key: None,
            map_tiles_api_endpoint: Resource::new("https://tile.googleapis.com/").unwrap(),
            default_street_view_static_api_key: None,
            street_view_static_api_endpoint: Resource::new("https://maps.googleapis.com/maps/api/streetview").unwrap(),
        }
    }
}
impl GoogleMaps {
    pub fn get_default_credit() -> Credit {
        Credit::new("<img alt=\"Google\" src=\"https://assets.ion.cesium.com/google-credit.png\" style=\"vertical-align:-6px\">",true)
    }
}
