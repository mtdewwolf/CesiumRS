//! CesiumJS `ResourceCacheStatistics`: GPU memory counted once per cache key.
//!
//! Upstream reads `cacheKey`, `buffer.sizeInBytes`, `typedArray.byteLength` and
//! `texture.sizeInBytes` from the loader. Rust callers pass those sizes, so the
//! statistics do not depend on the loader types.

use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceCacheStatistics {
    pub geometry_byte_length: usize,
    pub textures_byte_length: usize,
    geometry_sizes: HashMap<String, usize>,
    texture_sizes: HashMap<String, usize>,
}

impl ResourceCacheStatistics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Counts a vertex or index buffer loader's GPU buffer and typed array,
    /// once per cache key.
    pub fn add_geometry_loader(
        &mut self,
        cache_key: &str,
        buffer_bytes: Option<usize>,
        typed_array_bytes: Option<usize>,
    ) {
        if self.geometry_sizes.contains_key(cache_key) {
            return;
        }
        let total = buffer_bytes.unwrap_or(0) + typed_array_bytes.unwrap_or(0);
        self.geometry_byte_length += total;
        self.geometry_sizes.insert(cache_key.to_string(), total);
    }

    /// Counts a texture loader's texture, once per cache key.
    pub fn add_texture_loader(&mut self, cache_key: &str, texture_bytes: usize) {
        if self.texture_sizes.contains_key(cache_key) {
            return;
        }
        self.textures_byte_length += texture_bytes;
        self.texture_sizes.insert(cache_key.to_string(), texture_bytes);
    }

    /// Stops counting whatever was counted for this cache key.
    pub fn remove_loader(&mut self, cache_key: &str) {
        if let Some(size) = self.geometry_sizes.remove(cache_key) {
            self.geometry_byte_length -= size;
        }
        if let Some(size) = self.texture_sizes.remove(cache_key) {
            self.textures_byte_length -= size;
        }
    }
}
