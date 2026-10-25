//! CesiumJS `Scene/Model/ModelStatistics`: memory and geometry statistics for a `Model`.

use crate::renderer::{Buffer, Gl, Texture};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// Lightweight handle for a 3D Tiles 1.0 `BatchTexture`'s memory footprint.
#[derive(Clone, Debug)]
pub struct BatchTextureStatistics {
    pub id: String,
    pub byte_length: Rc<Cell<usize>>,
}

impl BatchTextureStatistics {
    pub fn new(id: impl Into<String>, byte_length: usize) -> Self {
        Self { id: id.into(), byte_length: Rc::new(Cell::new(byte_length)) }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModelStatistics {
    pub points_length: usize,
    pub triangles_length: usize,
    pub geometry_byte_length: usize,
    textures_byte_length: usize,
    pub property_tables_byte_length: usize,
    buffer_id_set: HashMap<String, bool>,
    texture_id_set: HashMap<String, usize>,
    texture_ids: Vec<String>,
    batch_texture_id_map: HashMap<String, Rc<Cell<usize>>>,
}

impl ModelStatistics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total size of all textures in bytes, including batch textures.
    pub fn textures_byte_length(&self) -> usize {
        let batch_textures_byte_length: usize = self.batch_texture_id_map.values().map(|b| b.get()).sum();
        self.textures_byte_length + batch_textures_byte_length
    }

    pub fn clear(&mut self) {
        self.points_length = 0;
        self.triangles_length = 0;
        self.geometry_byte_length = 0;
        self.textures_byte_length = 0;
        self.property_tables_byte_length = 0;
        self.buffer_id_set.clear();
        self.texture_id_set.clear();
        self.texture_ids.clear();
        self.batch_texture_id_map.clear();
    }

    pub fn add_buffer<G: Gl>(&mut self, buffer: &Buffer<G>, has_cpu_copy: bool) {
        let id = buffer.id();
        if !self.buffer_id_set.contains_key(id) {
            let copies = if has_cpu_copy { 2 } else { 1 };
            self.geometry_byte_length += buffer.size_in_bytes() * copies;
        }
        self.buffer_id_set.insert(id.to_string(), has_cpu_copy);
    }

    pub fn add_texture<G: Gl>(&mut self, texture: &Texture<G>) {
        let id = texture.id();
        if !self.texture_id_set.contains_key(id) {
            let size = texture.size_in_bytes();
            self.textures_byte_length += size;
            self.texture_id_set.insert(id.to_string(), size);
            self.texture_ids.push(id.to_string());
        }
    }

    pub fn get_texture_ids(&self) -> &[String] {
        &self.texture_ids
    }

    pub fn get_texture_byte_length_by_id(&self, id: &str) -> Option<usize> {
        self.texture_id_set.get(id).copied()
    }

    pub fn add_batch_texture(&mut self, batch_texture: &BatchTextureStatistics) {
        self.batch_texture_id_map.entry(batch_texture.id.clone()).or_insert_with(|| batch_texture.byte_length.clone());
    }
}
