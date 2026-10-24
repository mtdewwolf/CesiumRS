//! Counts each cache key once, including retained CPU copies of GPU geometry.
use std::collections::HashMap;
#[derive(Default, Clone, Debug)]
pub struct ResourceCacheStatistics {
    pub geometry_byte_length: u64,
    pub textures_byte_length: u64,
    geometry: HashMap<String, u64>,
    textures: HashMap<String, u64>,
}
impl ResourceCacheStatistics {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn add_geometry_loader(&mut self, key: &str, gpu_bytes: Option<u64>, cpu_bytes: Option<u64>) {
        if self.geometry.contains_key(key) {
            return;
        }
        let size = gpu_bytes.unwrap_or(0) + cpu_bytes.unwrap_or(0);
        self.geometry.insert(key.into(), size);
        self.geometry_byte_length += size;
    }
    pub fn add_texture_loader(&mut self, key: &str, size: u64) {
        if self.textures.contains_key(key) {
            return;
        }
        self.textures.insert(key.into(), size);
        self.textures_byte_length += size;
    }
    pub fn remove_loader(&mut self, key: &str) {
        if let Some(size) = self.geometry.remove(key) {
            self.geometry_byte_length -= size;
        }
        if let Some(size) = self.textures.remove(key) {
            self.textures_byte_length -= size;
        }
    }
}
