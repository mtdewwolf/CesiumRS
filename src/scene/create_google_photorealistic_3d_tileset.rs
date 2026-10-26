//! CesiumJS `createGooglePhotorealistic3DTileset`: creates a tileset instance for Google
//! Photorealistic 3D Tiles via direct API key or cached Cesium Ion asset.

use crate::core::GoogleMaps;

/// The canonical Ion asset id for Google Photorealistic 3D Tiles.
pub const GOOGLE_PHOTOREALISTIC_ION_ASSET_ID: u64 = 2275207;

/// Default cache size in bytes (1.5 GB).
pub const DEFAULT_GOOGLE_TILES_CACHE_BYTES: usize = 1536 * 1024 * 1024;

/// Default maximum cache overflow in bytes (1 GB).
pub const DEFAULT_GOOGLE_TILES_CACHE_OVERFLOW_BYTES: usize = 1024 * 1024 * 1024;

/// API options for Google Photorealistic 3D Tiles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GooglePhotorealisticApiOptions {
    /// Google Maps API key.
    pub key: Option<String>,

    /// Confirmation that the tileset will only be used with the Google geocoder.
    pub only_using_with_google_geocoder: bool,
}

/// Resolved URL and configuration parameters for loading Google Photorealistic 3D Tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GooglePhotorealisticSource {
    IonAssetId(u64),
    DirectUrl { url: String, api_key: String },
}

/// Configuration parameters for Google Photorealistic 3D Tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GooglePhotorealistic3DTilesetConfig {
    pub source: GooglePhotorealisticSource,
    pub cache_bytes: usize,
    pub maximum_cache_overflow_bytes: usize,
    pub enable_collision: bool,
    pub requires_geocoder_warning: bool,
}

impl GooglePhotorealistic3DTilesetConfig {
    pub fn new(api_options: Option<GooglePhotorealisticApiOptions>) -> Self {
        let api_opts = api_options.unwrap_or_default();
        let default_maps = GoogleMaps::default();
        let key = api_opts.key.or(default_maps.default_api_key);

        let source = match key {
            Some(api_key) => {
                let endpoint = default_maps.map_tiles_api_endpoint.url();
                let url = format!("{endpoint}v1/3dtiles/root.json");
                GooglePhotorealisticSource::DirectUrl { url, api_key }
            }
            None => GooglePhotorealisticSource::IonAssetId(GOOGLE_PHOTOREALISTIC_ION_ASSET_ID),
        };

        Self {
            source,
            cache_bytes: DEFAULT_GOOGLE_TILES_CACHE_BYTES,
            maximum_cache_overflow_bytes: DEFAULT_GOOGLE_TILES_CACHE_OVERFLOW_BYTES,
            enable_collision: true,
            requires_geocoder_warning: !api_opts.only_using_with_google_geocoder,
        }
    }
}
