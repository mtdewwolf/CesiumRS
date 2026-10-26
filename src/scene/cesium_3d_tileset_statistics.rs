//! CesiumJS `Cesium3DTilesetStatistics`: tracks frame statistics, memory usage,
//! feature counts, and texture reference counters for a 3D tileset.

use std::collections::HashMap;

/// An abstract representation of content metadata counts for statistics calculation.
pub trait TileContentCounts {
    fn features_length(&self) -> usize {
        0
    }
    fn points_length(&self) -> usize {
        0
    }
    fn triangles_length(&self) -> usize {
        0
    }
    fn geometry_byte_length(&self) -> usize {
        0
    }
    fn textures_byte_length(&self) -> usize {
        0
    }
    fn batch_table_byte_length(&self) -> usize {
        0
    }
    fn is_model_content(&self) -> bool {
        false
    }
    fn texture_ids(&self) -> Vec<String> {
        Vec::new()
    }
    fn texture_byte_length_by_id(&self, _id: &str) -> usize {
        0
    }
    fn inner_contents(&self) -> Option<&[Box<dyn TileContentCounts>]> {
        None
    }
}

/// A concrete implementation of [`TileContentCounts`] for simple or synthetic tile contents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SimpleTileContentCounts {
    pub features_length: usize,
    pub points_length: usize,
    pub triangles_length: usize,
    pub geometry_byte_length: usize,
    pub textures_byte_length: usize,
    pub batch_table_byte_length: usize,
    pub is_model_content: bool,
    pub textures: Vec<(String, usize)>,
    pub inner_contents: Vec<SimpleTileContentCounts>,
}

impl TileContentCounts for SimpleTileContentCounts {
    fn features_length(&self) -> usize {
        self.features_length
    }
    fn points_length(&self) -> usize {
        self.points_length
    }
    fn triangles_length(&self) -> usize {
        self.triangles_length
    }
    fn geometry_byte_length(&self) -> usize {
        self.geometry_byte_length
    }
    fn textures_byte_length(&self) -> usize {
        self.textures_byte_length
    }
    fn batch_table_byte_length(&self) -> usize {
        self.batch_table_byte_length
    }
    fn is_model_content(&self) -> bool {
        self.is_model_content
    }
    fn texture_ids(&self) -> Vec<String> {
        self.textures.iter().map(|(id, _)| id.clone()).collect()
    }
    fn texture_byte_length_by_id(&self, id: &str) -> usize {
        self.textures.iter().find(|(tid, _)| tid == id).map_or(0, |(_, b)| *b)
    }
}

/// Tracks runtime memory, geometry, rendering, and loading counters for a tileset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cesium3DTilesetStatistics {
    // Rendering statistics
    pub selected: usize,
    pub visited: usize,

    // Loading statistics
    pub number_of_commands: usize,
    pub number_of_attempted_requests: usize,
    pub number_of_pending_requests: usize,
    pub number_of_tiles_processing: usize,
    pub number_of_tiles_with_content_ready: usize,
    pub number_of_tiles_total: usize,
    pub number_of_loaded_tiles_total: usize,

    // Features statistics
    pub number_of_features_selected: usize,
    pub number_of_features_loaded: usize,
    pub number_of_points_selected: usize,
    pub number_of_points_loaded: usize,
    pub number_of_triangles_selected: usize,

    // Styling statistics
    pub number_of_tiles_styled: usize,
    pub number_of_features_styled: usize,

    // Optimization statistics
    pub number_of_tiles_culled_with_children_union: usize,

    // Memory statistics
    pub geometry_byte_length: usize,
    pub textures_byte_length: usize,
    pub textures_reference_counter_by_id: HashMap<String, usize>,
    pub batch_table_byte_length: usize,
}

impl Cesium3DTilesetStatistics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets transient per-frame counters.
    pub fn clear(&mut self) {
        self.selected = 0;
        self.visited = 0;
        self.number_of_commands = 0;
        self.number_of_attempted_requests = 0;
        self.number_of_features_selected = 0;
        self.number_of_points_selected = 0;
        self.number_of_triangles_selected = 0;
        self.number_of_tiles_styled = 0;
        self.number_of_features_styled = 0;
        self.number_of_tiles_culled_with_children_union = 0;
    }

    /// Copies fields matching CesiumJS `Cesium3DTilesetStatistics.clone(statistics, result)`.
    pub fn clone_cesium_into(&self, result: &mut Self) {
        result.selected = self.selected;
        result.visited = self.visited;
        result.number_of_commands = self.number_of_commands;
        result.number_of_attempted_requests = self.number_of_attempted_requests;
        result.number_of_pending_requests = self.number_of_pending_requests;
        result.number_of_tiles_processing = self.number_of_tiles_processing;
        result.number_of_tiles_with_content_ready = self.number_of_tiles_with_content_ready;
        result.number_of_tiles_total = self.number_of_tiles_total;
        result.number_of_features_selected = self.number_of_features_selected;
        result.number_of_features_loaded = self.number_of_features_loaded;
        result.number_of_points_selected = self.number_of_points_selected;
        result.number_of_points_loaded = self.number_of_points_loaded;
        result.number_of_triangles_selected = self.number_of_triangles_selected;
        result.number_of_tiles_styled = self.number_of_tiles_styled;
        result.number_of_features_styled = self.number_of_features_styled;
        result.number_of_tiles_culled_with_children_union = self.number_of_tiles_culled_with_children_union;
        result.geometry_byte_length = self.geometry_byte_length;
        result.textures_byte_length = self.textures_byte_length;
        result.textures_reference_counter_by_id = self.textures_reference_counter_by_id.clone();
        result.batch_table_byte_length = self.batch_table_byte_length;
    }

    /// Recursively increments counters for points, triangles, and features selected for rendering.
    pub fn increment_selection_counts<C: TileContentCounts + ?Sized>(&mut self, content: &C) {
        self.number_of_features_selected += content.features_length();
        self.number_of_points_selected += content.points_length();
        self.number_of_triangles_selected += content.triangles_length();

        if let Some(inners) = content.inner_contents() {
            for inner in inners {
                self.increment_selection_counts(inner.as_ref());
            }
        }
    }

    /// Recursively increments load counters and texture memory bytes.
    pub fn increment_load_counts<C: TileContentCounts + ?Sized>(&mut self, content: &C) {
        self.number_of_features_loaded += content.features_length();
        self.number_of_points_loaded += content.points_length();
        self.geometry_byte_length += content.geometry_byte_length();
        self.batch_table_byte_length += content.batch_table_byte_length();

        if !content.is_model_content() {
            self.textures_byte_length += content.textures_byte_length();
        } else {
            for texture_id in content.texture_ids() {
                let count = self.textures_reference_counter_by_id.entry(texture_id.clone()).or_insert(0);
                if *count == 0 {
                    let byte_len = content.texture_byte_length_by_id(&texture_id);
                    self.textures_byte_length += byte_len;
                }
                *count += 1;
            }
        }

        if let Some(inners) = content.inner_contents() {
            for inner in inners {
                self.increment_load_counts(inner.as_ref());
            }
        }
    }

    /// Recursively decrements load counters and texture memory bytes.
    pub fn decrement_load_counts<C: TileContentCounts + ?Sized>(&mut self, content: &C) {
        self.number_of_features_loaded = self.number_of_features_loaded.saturating_sub(content.features_length());
        self.number_of_points_loaded = self.number_of_points_loaded.saturating_sub(content.points_length());
        self.geometry_byte_length = self.geometry_byte_length.saturating_sub(content.geometry_byte_length());
        self.batch_table_byte_length = self.batch_table_byte_length.saturating_sub(content.batch_table_byte_length());

        if !content.is_model_content() {
            self.textures_byte_length = self.textures_byte_length.saturating_sub(content.textures_byte_length());
        } else {
            for texture_id in content.texture_ids() {
                if let Some(count) = self.textures_reference_counter_by_id.get_mut(&texture_id) {
                    if *count <= 1 {
                        self.textures_reference_counter_by_id.remove(&texture_id);
                        let byte_len = content.texture_byte_length_by_id(&texture_id);
                        self.textures_byte_length = self.textures_byte_length.saturating_sub(byte_len);
                    } else {
                        *count -= 1;
                    }
                }
            }
        }

        if let Some(inners) = content.inner_contents() {
            for inner in inners {
                self.decrement_load_counts(inner.as_ref());
            }
        }
    }

    /// Helper for simple content counts struct.
    pub fn increment_selection_counts_simple(&mut self, content: &SimpleTileContentCounts) {
        self.number_of_features_selected += content.features_length;
        self.number_of_points_selected += content.points_length;
        self.number_of_triangles_selected += content.triangles_length;
        for inner in &content.inner_contents {
            self.increment_selection_counts_simple(inner);
        }
    }

    pub fn increment_load_counts_simple(&mut self, content: &SimpleTileContentCounts) {
        self.number_of_features_loaded += content.features_length;
        self.number_of_points_loaded += content.points_length;
        self.geometry_byte_length += content.geometry_byte_length;
        self.batch_table_byte_length += content.batch_table_byte_length;

        if !content.is_model_content {
            self.textures_byte_length += content.textures_byte_length;
        } else {
            for (texture_id, byte_len) in &content.textures {
                let count = self.textures_reference_counter_by_id.entry(texture_id.clone()).or_insert(0);
                if *count == 0 {
                    self.textures_byte_length += *byte_len;
                }
                *count += 1;
            }
        }

        for inner in &content.inner_contents {
            self.increment_load_counts_simple(inner);
        }
    }

    pub fn decrement_load_counts_simple(&mut self, content: &SimpleTileContentCounts) {
        self.number_of_features_loaded = self.number_of_features_loaded.saturating_sub(content.features_length);
        self.number_of_points_loaded = self.number_of_points_loaded.saturating_sub(content.points_length);
        self.geometry_byte_length = self.geometry_byte_length.saturating_sub(content.geometry_byte_length);
        self.batch_table_byte_length = self.batch_table_byte_length.saturating_sub(content.batch_table_byte_length);

        if !content.is_model_content {
            self.textures_byte_length = self.textures_byte_length.saturating_sub(content.textures_byte_length);
        } else {
            for (texture_id, byte_len) in &content.textures {
                if let Some(count) = self.textures_reference_counter_by_id.get_mut(texture_id) {
                    if *count <= 1 {
                        self.textures_reference_counter_by_id.remove(texture_id);
                        self.textures_byte_length = self.textures_byte_length.saturating_sub(*byte_len);
                    } else {
                        *count -= 1;
                    }
                }
            }
        }

        for inner in &content.inner_contents {
            self.decrement_load_counts_simple(inner);
        }
    }
}
