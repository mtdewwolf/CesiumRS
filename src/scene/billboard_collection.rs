//! CPU collection and vertex attribute preparation for CesiumJS `BillboardCollection`.
//!
//! # Architecture and Parity Status
//! This module provides partial CPU-side behavior for CesiumJS `BillboardCollection`:
//! - Complete collection management: `add`, `remove`, `remove_all`, `contains`, `get`, `len`.
//! - Deferred removal and compaction matching CesiumJS `removeBillboards` semantics.
//! - Base bounding spheres (`base_volume`, `base_volume_wc`).
//! - Preliminary vertex attribute packing based on `writeBillboard` in `BillboardCollection.js`.
//!
//! # Limitations
//! - GPU Draw Commands: WebGL/WebGPU buffer allocation, shader compilation
//!   (`BillboardCollectionVS.glsl` / `BillboardCollectionFS.glsl`), and render commands
//!   require graphics context integration.
//! - Pick IDs: Pick IDs and pick buffers are not fabricated on the CPU; real picking
//!   requires GPU render pass integration.
//! - Texture Atlas: GPU texture allocation and canvas rendering are omitted on the CPU side;
//!   image identifiers are retained, but image dimensions are unavailable.

use super::billboard::*;
use crate::{
    core::{
        AttributeCompression, BoundingSphere, Cartesian3, Color,
        EncodedCartesian3, Matrix4,
    },
    scene::{BlendOption, HeightReference, VerticalOrigin},
};
use std::{cell::RefCell, rc::Rc};

/// Shared handle modeling CesiumJS billboard mutable reference semantics.
pub type BillboardHandle = Rc<RefCell<Billboard>>;

const UPPER_BOUND: f64 = 32768.0; // 2^15
const LEFT_SHIFT16: f64 = 65536.0; // 2^16
const LEFT_SHIFT12: f64 = 4096.0; // 2^12
const LEFT_SHIFT8: f64 = 256.0; // 2^8
const LEFT_SHIFT7: f64 = 128.0;
const LEFT_SHIFT5: f64 = 32.0;
const LEFT_SHIFT3: f64 = 8.0;
const LEFT_SHIFT2: f64 = 4.0;
const RIGHT_SHIFT8: f64 = 1.0 / 256.0;

/// Preliminary packed attributes; GPU integration and complete parity remain unverified.
/// Texture coordinates are placeholders; when no pick color is provided, the
/// encoded color is white and does not represent an allocated pick ID.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BillboardVertexAttributes {
    pub position_high_and_scale: [f64; 4],
    pub position_low_and_rotation: [f64; 4],
    pub compressed_attribute0: [f64; 4],
    pub compressed_attribute1: [f64; 4],
    pub compressed_attribute2: [f64; 4],
    pub eye_offset: [f64; 4],
    pub scale_by_distance: [f64; 4],
    pub pixel_offset_scale_by_distance: [f64; 4],
    pub compressed_attribute3: [f64; 4],
    pub split_direction: f64,
}

impl BillboardVertexAttributes {
    /// Computes preliminary packed vertex attributes for a billboard.
    pub fn from_billboard(billboard: &Billboard, pick_color: Option<Color>) -> Self {
        let position = billboard.actual_position();
        let encoded = EncodedCartesian3::from_cartesian(position);
        let scale = billboard.scale();
        let rotation = billboard.rotation();

        let position_high_and_scale = [encoded.high.x, encoded.high.y, encoded.high.z, scale];
        let position_low_and_rotation = [encoded.low.x, encoded.low.y, encoded.low.z, rotation];

        // Compressed attribute 0: pixel offset, translate, origins, show
        let pixel_offset = billboard.pixel_offset();
        let translate = billboard.translate();
        let horizontal_origin = billboard.horizontal_origin() as i32 as f64;
        let mut vertical_origin = billboard.vertical_origin() as i32 as f64;
        let mut show = billboard.show() && billboard.cluster_show();
        if billboard.color().alpha == 0.0 {
            show = false;
        }
        if billboard.vertical_origin() == VerticalOrigin::Baseline {
            vertical_origin = VerticalOrigin::Bottom as i32 as f64;
        }

        let mut c0 = (pixel_offset.x.clamp(-UPPER_BOUND, UPPER_BOUND) + UPPER_BOUND).floor()
            * LEFT_SHIFT7;
        c0 += (horizontal_origin + 1.0) * LEFT_SHIFT5;
        c0 += (vertical_origin + 1.0) * LEFT_SHIFT3;
        c0 += (if show { 1.0 } else { 0.0 }) * LEFT_SHIFT2;

        let mut c1 = (pixel_offset.y.clamp(-UPPER_BOUND, UPPER_BOUND) + UPPER_BOUND).floor()
            * LEFT_SHIFT8;
        let mut c2 = ((translate.x * LEFT_SHIFT2).clamp(-UPPER_BOUND, UPPER_BOUND) + UPPER_BOUND)
            .floor()
            * LEFT_SHIFT8;

        let temp_translate_y = ((translate.y * LEFT_SHIFT2).clamp(-UPPER_BOUND, UPPER_BOUND)
            + UPPER_BOUND)
            * RIGHT_SHIFT8;
        let upper_translate_y = temp_translate_y.floor();
        let lower_translate_y = ((temp_translate_y - upper_translate_y) * LEFT_SHIFT8).floor();

        c1 += upper_translate_y;
        c2 += lower_translate_y;

        let compressed_attribute0 = [c0, c1, c2, 0.0];

        // Compressed attribute 1: image width, aligned axis, translucency
        let aligned_axis = billboard.aligned_axis();
        let image_width = billboard.width().unwrap_or(0.0).round();
        let mut ca1_0 = image_width.clamp(0.0, LEFT_SHIFT16);
        let mut ca1_1 = 0.0;
        if (aligned_axis.magnitude_squared() - 1.0).abs() < 1e-6 {
            ca1_1 = AttributeCompression::oct_encode_float(aligned_axis);
        }

        let (near, mut near_value, far, mut far_value) =
            if let Some(t) = billboard.translucency_by_distance() {
                (t.near, t.near_value, t.far, t.far_value)
            } else {
                (0.0, 1.0, 1.0, 1.0)
            };

        near_value = near_value.clamp(0.0, 1.0);
        let near_byte = if near_value == 1.0 {
            255.0
        } else {
            (near_value * 255.0).floor()
        };
        ca1_0 = ca1_0 * LEFT_SHIFT8 + near_byte;

        far_value = far_value.clamp(0.0, 1.0);
        let far_byte = if far_value == 1.0 {
            255.0
        } else {
            (far_value * 255.0).floor()
        };
        ca1_1 = ca1_1 * LEFT_SHIFT8 + far_byte;

        let compressed_attribute1 = [ca1_0, ca1_1, near, far];

        // Compressed attribute 2: color, pick color, size in meters, aligned axis valid, height, label horizontal origin
        let color = billboard.color();
        let pick_color = pick_color.unwrap_or(Color::WHITE);
        let size_in_meters = if billboard.size_in_meters() { 1.0 } else { 0.0 };
        let valid_aligned_axis = if (aligned_axis.magnitude_squared() - 1.0).abs() < 1e-6 {
            1.0
        } else {
            0.0
        };
        let image_height = billboard.height().unwrap_or(0.0);
        let label_horizontal_origin: f64 = 0.0; // (-2.0 default + 2.0)

        let col_rgb8 = AttributeCompression::encode_rgb8(color);
        let pick_rgb8 = AttributeCompression::encode_rgb8(pick_color);
        let col_alpha_byte = Color::float_to_byte(color.alpha) as f64;
        let pick_alpha_byte = Color::float_to_byte(pick_color.alpha) as f64;
        let ca2_2 = col_alpha_byte * LEFT_SHIFT16
            + pick_alpha_byte * LEFT_SHIFT8
            + (size_in_meters * 2.0 + valid_aligned_axis);
        let ca2_3 = image_height * LEFT_SHIFT2 + label_horizontal_origin;

        let compressed_attribute2 = [col_rgb8, pick_rgb8, ca2_2, ca2_3];

        // Eye offset
        let eye_offset = billboard.eye_offset();
        let mut eye_offset_z = eye_offset.z;
        if billboard.height_reference() != HeightReference::None {
            eye_offset_z *= 1.005;
        }
        let eye_offset = [eye_offset.x, eye_offset.y, eye_offset_z, 0.0];

        // Scale by distance
        let (s_near, s_near_val, s_far, s_far_val) =
            if let Some(s) = billboard.scale_by_distance() {
                (s.near, s.near_value, s.far, s.far_value)
            } else {
                (0.0, 1.0, 1.0, 1.0)
            };
        let scale_by_distance = [s_near, s_near_val, s_far, s_far_val];

        // Pixel offset scale by distance
        let (p_near, p_near_val, p_far, p_far_val) =
            if let Some(p) = billboard.pixel_offset_scale_by_distance() {
                (p.near, p.near_value, p.far, p.far_value)
            } else {
                (0.0, 1.0, 1.0, 1.0)
            };
        let pixel_offset_scale_by_distance = [p_near, p_near_val, p_far, p_far_val];

        // Compressed attribute 3: distance display condition, depth test distance, dimensions
        let (ddc_near, ddc_far) = if let Some(d) = billboard.distance_display_condition() {
            (d.near * d.near, d.far * d.far)
        } else {
            (0.0, f64::MAX)
        };
        let depth = if let Some(d) = billboard.disable_depth_test_distance() {
            if d == f64::INFINITY {
                -1.0
            } else {
                d * d
            }
        } else {
            0.0
        };
        let w = billboard
            .width()
            .unwrap_or(0.0)
            .clamp(0.0, LEFT_SHIFT12)
            .floor();
        let h = billboard
            .height()
            .unwrap_or(0.0)
            .clamp(0.0, LEFT_SHIFT12)
            .floor();
        let dimensions = w * LEFT_SHIFT12 + h;

        let compressed_attribute3 = [ddc_near, ddc_far, depth, dimensions];

        // Split direction
        let split_direction = billboard.split_direction().value() as f64;

        Self {
            position_high_and_scale,
            position_low_and_rotation,
            compressed_attribute0,
            compressed_attribute1,
            compressed_attribute2,
            eye_offset,
            scale_by_distance,
            pixel_offset_scale_by_distance,
            compressed_attribute3,
            split_direction,
        }
    }
}

/// A collection of billboards positioned in world or model space.
/// Do not hold a billboard handle borrow while calling collection methods that
/// access handles. Bounds are recomputed from current positions on each read.
/// A handle borrowed during collection drop may retain stale attachment state.
pub struct BillboardCollection {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    pub blend_option: BlendOption,
    pub coarse_depth_test_distance: Option<f64>,
    pub three_point_depth_test_distance: Option<f64>,
    pub destroy_texture_atlas: bool,

    billboards: Vec<Option<BillboardHandle>>,
    billboards_removed: bool,
    destroyed: bool,

    base_volume: BoundingSphere,
    base_volume_wc: BoundingSphere,
}

impl Default for BillboardCollection {
    fn default() -> Self {
        Self {
            show: true,
            model_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: false,
            blend_option: BlendOption::OpaqueAndTranslucent,
            coarse_depth_test_distance: None,
            three_point_depth_test_distance: None,
            destroy_texture_atlas: true,

            billboards: Vec::new(),
            billboards_removed: false,
            destroyed: false,

            base_volume: BoundingSphere::default(),
            base_volume_wc: BoundingSphere::default(),
        }
    }
}

impl BillboardCollection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Compacts removed billboards and re-indexes the collection, matching CesiumJS `removeBillboards`.
    pub fn compact(&mut self) {
        if !self.billboards_removed {
            return;
        }
        self.billboards_removed = false;
        let mut compacted = Vec::with_capacity(self.billboards.len());
        for handle in self.billboards.drain(..).flatten() {
            handle.borrow_mut().set_collection_index(compacted.len());
            compacted.push(Some(handle));
        }
        self.billboards = compacted;
    }

    pub fn len(&mut self) -> usize {
        self.compact();
        self.billboards.len()
    }

    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    /// Adds a new billboard with the given options.
    pub fn add(&mut self, options: BillboardOptions) -> Result<BillboardHandle, &'static str> {
        if self.destroyed {
            return Err("BillboardCollection has been destroyed.");
        }
        self.compact();
        let index = self.billboards.len();
        let billboard = Rc::new(RefCell::new(Billboard::new(options, index)?));
        self.billboards.push(Some(billboard.clone()));
        Ok(billboard)
    }

    /// Removes a billboard from the collection.
    pub fn remove(&mut self, billboard: &BillboardHandle) -> bool {
        if self.destroyed {
            return false;
        }
        let pos = self
            .billboards
            .iter()
            .position(|opt| opt.as_ref().is_some_and(|h| Rc::ptr_eq(h, billboard)));
        if let Some(index) = pos {
            if let Some(removed) = self.billboards[index].take() {
                removed.borrow_mut().detach();
                self.billboards_removed = true;
                return true;
            }
        }
        false
    }

    /// Removes all billboards from the collection.
    pub fn remove_all(&mut self) {
        for handle in self.billboards.drain(..).flatten() {
            handle.borrow_mut().detach();
        }
        self.billboards_removed = false;
    }

    /// Checks if a billboard belongs to this collection.
    pub fn contains(&self, billboard: &BillboardHandle) -> bool {
        if !billboard.borrow().is_attached() {
            return false;
        }
        self.billboards
            .iter()
            .any(|opt| opt.as_ref().is_some_and(|h| Rc::ptr_eq(h, billboard)))
    }

    /// Retrieves the billboard at `index`.
    pub fn get(&mut self, index: usize) -> Option<BillboardHandle> {
        self.compact();
        self.billboards.get(index).and_then(|opt| opt.clone())
    }

    /// Destroys the collection and detaches all billboards.
    pub fn destroy(&mut self) {
        if !self.destroyed {
            self.remove_all();
            self.destroyed = true;
        }
    }

    /// Updates base bounding volumes from billboard positions.
    pub fn update_bounding_volumes(&mut self) {
        self.compact();
        let positions: Vec<Cartesian3> = self
            .billboards
            .iter()
            .filter_map(|opt| opt.as_ref().map(|b| b.borrow().actual_position()))
            .collect();

        if positions.is_empty() {
            self.base_volume = BoundingSphere::default();
            self.base_volume_wc = BoundingSphere::default();
        } else {
            self.base_volume = BoundingSphere::from_points(&positions);
            self.base_volume_wc =
                BoundingSphere::transform(&self.base_volume, &self.model_matrix);
        }
    }

    /// Recomputes bounds so mutations through shared handles are reflected.
    pub fn base_volume(&mut self) -> &BoundingSphere {
        self.update_bounding_volumes();
        &self.base_volume
    }

    pub fn base_volume_wc(&mut self) -> &BoundingSphere {
        self.update_bounding_volumes();
        &self.base_volume_wc
    }

    /// Computes packed vertex attributes for all active billboards in the collection.
    pub fn attributes(&mut self) -> Vec<BillboardVertexAttributes> {
        self.compact();
        let mut result = Vec::with_capacity(self.billboards.len());
        for handle in self.billboards.iter().flatten() {
            let mut billboard = handle.borrow_mut();
            billboard.clear_dirty();
            result.push(BillboardVertexAttributes::from_billboard(
                &billboard, None,
            ));
        }
        result
    }
}

impl Drop for BillboardCollection {
    fn drop(&mut self) {
        for billboard in self.billboards.iter().flatten() {
            if let Ok(mut billboard) = billboard.try_borrow_mut() {
                billboard.detach();
            }
        }
    }
}
