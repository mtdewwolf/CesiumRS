//! CPU collection and layout management for CesiumJS `LabelCollection`.
//!
//! # Architecture and Parity Status
//! This module provides partial CPU-side behavior for CesiumJS `LabelCollection`:
//! - Complete collection management: `add`, `remove`, `remove_all`, `contains`, `get`, `len`, `is_empty`.
//! - Bounding sphere aggregation (`base_volume`, `base_volume_wc`).
//! - Approximate CPU text layout using fixed simulated glyph metrics:
//!   - Line break parsing (`\n`).
//!   - Scaled background padding (`background_padding / relative_size`).
//!   - Origin offsets for [`HorizontalOrigin`] and [`VerticalOrigin`].
//!   - Background billboard dimensions and translate offsets.
//! - Dirty state flushing and batch update tracking.
//! - Single-threaded memory safety via [`LabelHandle`] (`Rc<RefCell<Label>>`), without
//!   unverified `Send` or `Sync` implementations.
//!
//! # Limitations
//! - GPU Draw Commands: WebGL/WebGPU resource allocation, shader compilation
//!   (`LabelCollectionVS.glsl` / `LabelCollectionFS.glsl`), and render command generation
//!   require graphics context integration.
//! - Glyph Texture Atlas: True font rasterization into a texture atlas is omitted on the CPU;
//!   glyphs use estimated / simulated canvas metrics without live GPU texture allocations.
//! - Pick IDs: Pick IDs and pick buffers are not fabricated on the CPU; true picking requires
//!   GPU render pass integration.
//! - Terrain Clamping: Automatic height sampling on globe terrain requires a live Scene and
//!   terrain provider.

use crate as _root;

use super::label::*;
use _root::{
    core::{BoundingSphere, Cartesian2, Cartesian3, Matrix4},
    scene::{BlendOption, HorizontalOrigin, VerticalOrigin},
};
use std::{cell::RefCell, rc::Rc};

/// Shared handle modeling CesiumJS label mutable reference semantics.
pub type LabelHandle = Rc<RefCell<Label>>;

/// Calculates the horizontal offset for text lines matching CesiumJS `calculateWidthOffset`.
pub fn calculate_width_offset(
    line_width: f64,
    horizontal_origin: HorizontalOrigin,
    background_padding: Cartesian2,
) -> f64 {
    match horizontal_origin {
        HorizontalOrigin::Center => -line_width / 2.0,
        HorizontalOrigin::Right => -(line_width + background_padding.x),
        HorizontalOrigin::Left => background_padding.x,
    }
}

/// Partial CPU collection of labels. Layout uses simulated fixed-width glyphs;
/// bounds enclose positions only and are not safe for text culling. Handle
/// borrows must be released before collection mutation. A label borrowed while
/// its collection is dropped may retain stale attachment state.
#[derive(Debug)]
pub struct LabelCollection {
    labels: Vec<LabelHandle>,
    labels_to_update: Vec<LabelHandle>,
    show: bool,
    model_matrix: Matrix4,
    debug_show_bounding_volume: bool,
    blend_option: BlendOption,
    coarse_depth_test_distance: f64,
    three_point_depth_test_distance: f64,
    enable_right_to_left_detection: bool,
    total_glyph_count: usize,
    base_volume_cached: Option<BoundingSphere>,
    base_volume_wc_cached: Option<BoundingSphere>,
}

impl Default for LabelCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl LabelCollection {
    /// Creates an empty label collection with default settings.
    pub fn new() -> Self {
        Self {
            labels: Vec::new(),
            labels_to_update: Vec::new(),
            show: true,
            model_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: false,
            blend_option: BlendOption::OpaqueAndTranslucent,
            coarse_depth_test_distance: 0.0,
            three_point_depth_test_distance: 0.0,
            enable_right_to_left_detection: false,
            total_glyph_count: 0,
            base_volume_cached: None,
            base_volume_wc_cached: None,
        }
    }

    pub fn show(&self) -> bool {
        self.show
    }

    pub fn set_show(&mut self, show: bool) {
        self.show = show;
    }

    pub fn model_matrix(&self) -> &Matrix4 {
        &self.model_matrix
    }

    pub fn set_model_matrix(&mut self, matrix: Matrix4) {
        self.model_matrix = matrix;
        self.base_volume_wc_cached = None;
    }

    pub fn debug_show_bounding_volume(&self) -> bool {
        self.debug_show_bounding_volume
    }

    pub fn set_debug_show_bounding_volume(&mut self, debug: bool) {
        self.debug_show_bounding_volume = debug;
    }

    pub fn blend_option(&self) -> BlendOption {
        self.blend_option
    }

    pub fn set_blend_option(&mut self, blend: BlendOption) {
        self.blend_option = blend;
    }

    pub fn coarse_depth_test_distance(&self) -> f64 {
        self.coarse_depth_test_distance
    }

    pub fn set_coarse_depth_test_distance(&mut self, distance: f64) {
        self.coarse_depth_test_distance = distance;
    }

    pub fn three_point_depth_test_distance(&self) -> f64 {
        self.three_point_depth_test_distance
    }

    pub fn set_three_point_depth_test_distance(&mut self, distance: f64) {
        self.three_point_depth_test_distance = distance;
    }

    pub fn enable_right_to_left_detection(&self) -> bool {
        self.enable_right_to_left_detection
    }

    pub fn set_enable_right_to_left_detection(&mut self, enable: bool) {
        for label in &self.labels {
            assert!(label.try_borrow_mut().is_ok(), "cannot change RTL mode while a label is borrowed");
        }
        self.enable_right_to_left_detection = enable;
        for label in &self.labels {
            label.borrow_mut().set_rtl_detection(enable);
        }
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    pub fn total_glyph_count(&self) -> usize {
        self.total_glyph_count
    }

    /// Adds a label with the specified options to the collection.
    pub fn add(&mut self, options: LabelOptions) -> Result<LabelHandle, &'static str> {
        let index = self.labels.len();
        let label = Label::new(options, index, self.enable_right_to_left_detection)?;
        let handle = Rc::new(RefCell::new(label));
        self.labels.push(Rc::clone(&handle));
        self.labels_to_update.push(Rc::clone(&handle));
        self.invalidate_bounds();
        Ok(handle)
    }

    /// Removes a label from the collection.
    ///
    /// Detaches the label, compacts remaining indices, and invalidates bounding volumes.
    pub fn remove(&mut self, handle: &LabelHandle) -> bool {
        let index = self.labels.iter().position(|item| Rc::ptr_eq(item, handle));
        if let Some(pos) = index {
            if self.labels[pos..].iter().any(|label| label.try_borrow_mut().is_err()) {
                return false;
            }
            let removed = self.labels.remove(pos);
            removed.borrow_mut().detach();

            // Re-index remaining labels
            for (idx, lbl) in self.labels.iter().enumerate().skip(pos) {
                lbl.borrow_mut().set_collection_index(idx);
            }

            self.labels_to_update.retain(|item| !Rc::ptr_eq(item, handle));
            self.invalidate_bounds();
            true
        } else {
            false
        }
    }

    /// Removes all labels from the collection.
    pub fn remove_all(&mut self) {
        for label in &self.labels {
            assert!(label.try_borrow_mut().is_ok(), "cannot remove labels while a handle is borrowed");
        }
        for handle in self.labels.drain(..) {
            handle.borrow_mut().detach();
        }
        self.labels_to_update.clear();
        self.total_glyph_count = 0;
        self.invalidate_bounds();
    }

    /// Checks if the collection contains the specified label.
    pub fn contains(&self, handle: &LabelHandle) -> bool {
        self.labels.iter().any(|item| Rc::ptr_eq(item, handle))
    }

    /// Gets the label handle at the specified index.
    pub fn get(&self, index: usize) -> Option<LabelHandle> {
        self.labels.get(index).cloned()
    }

    fn invalidate_bounds(&mut self) {
        self.base_volume_cached = None;
        self.base_volume_wc_cached = None;
    }

    /// Computes the bounding sphere enclosing all label positions in model space.
    pub fn base_volume(&mut self) -> BoundingSphere {
        if self.labels.is_empty() {
            let sphere = BoundingSphere::new(Cartesian3::ZERO, 0.0);
            self.base_volume_cached = Some(sphere);
            return sphere;
        }

        let points: Vec<Cartesian3> = self
            .labels
            .iter()
            .map(|handle| handle.borrow().actual_position())
            .collect();

        let sphere = BoundingSphere::from_points(&points);
        self.base_volume_cached = Some(sphere);
        sphere
    }

    /// Computes the bounding sphere transformed to world coordinates via `model_matrix`.
    pub fn base_volume_wc(&mut self) -> BoundingSphere {
        let base = self.base_volume();
        let sphere = base.transform(&self.model_matrix);
        self.base_volume_wc_cached = Some(sphere);
        sphere
    }

    /// Flushes dirty state and updates CPU glyph / background layout metrics.
    ///
    /// Simulates CesiumJS `LabelCollection.prototype.update` CPU passes:
    /// - `rebind_all_glyphs`: updates glyph counts and text metrics.
    /// - `reposition_all_glyphs`: computes line heights, background billboard dimensions,
    ///   and origin translations for screen-space positioning.
    pub fn update(&mut self) {
        if !self.show {
            return;
        }

        self.labels_to_update.clear();
        self.total_glyph_count = 0;
        for handle in &self.labels {
            let mut label = handle.borrow_mut();
            if !label.is_attached() {
                continue;
            }

            let rendered_text = label.rendered_text().to_string();
            let text_len = rendered_text.chars().count();
            self.total_glyph_count += text_len;

            // Perform CPU simulated layout calculation
            if label.reposition_all_glyphs() || label.rebind_all_glyphs() {
                let lines: Vec<&str> = rendered_text.split('\n').collect();
                let num_lines = lines.len();

                let relative_size = label.relative_size();
                let mut bg_pad = label.background_padding();
                bg_pad.x /= relative_size;
                bg_pad.y /= relative_size;

                // Simulated glyph metrics per character at SDF font size (48px)
                // Width = 24.0, Ascent = 36.0, Descent = 12.0
                const GLYPH_WIDTH: f64 = 24.0;
                const GLYPH_HEIGHT: f64 = 48.0;
                const GLYPH_DESCENT: f64 = 12.0;
                const GLYPH_Y: f64 = 36.0; // height - descent

                let mut max_line_width = 0.0f64;
                for line in &lines {
                    let char_count = line.chars().count();
                    let line_width = char_count as f64 * GLYPH_WIDTH;
                    if line_width > max_line_width {
                        max_line_width = line_width;
                    }
                }

                let line_spacing = label
                    .parsed_font()
                    .line_height
                    .unwrap_or(1.2 * label.parsed_font().size)
                    / relative_size;
                let other_lines_height = line_spacing * (num_lines.saturating_sub(1) as f64);
                let max_line_height = GLYPH_HEIGHT;

                let mut total_line_width = max_line_width;
                let mut total_line_height = max_line_height + other_lines_height;

                if label.show_background() {
                    total_line_width += bg_pad.x * 2.0;
                    total_line_height += bg_pad.y * 2.0;
                }

                let total_scale = label.total_scale();
                let horizontal_origin = label.horizontal_origin();
                let vertical_origin = label.vertical_origin();

                let width_offset = match horizontal_origin {
                    HorizontalOrigin::Center => -max_line_width / 2.0 - bg_pad.x,
                    HorizontalOrigin::Right => -(max_line_width + bg_pad.x * 2.0),
                    HorizontalOrigin::Left => 0.0,
                };

                let y_offset = match vertical_origin {
                    VerticalOrigin::Top => max_line_height - GLYPH_Y - GLYPH_DESCENT,
                    VerticalOrigin::Center => (max_line_height - GLYPH_Y) / 2.0 - GLYPH_DESCENT,
                    VerticalOrigin::Baseline => -bg_pad.y - GLYPH_DESCENT,
                    VerticalOrigin::Bottom => 0.0,
                };

                let translate = Cartesian2::new(
                    width_offset * total_scale,
                    y_offset * total_scale,
                );

                label.background_dimensions = Some(Cartesian2::new(total_line_width, total_line_height));
                label.background_translate = Some(translate);
            }

            label.clear_dirty();
        }
    }
}

impl Drop for LabelCollection {
    fn drop(&mut self) {
        for label in &self.labels {
            if let Ok(mut label) = label.try_borrow_mut() { label.detach(); }
        }
    }
}
