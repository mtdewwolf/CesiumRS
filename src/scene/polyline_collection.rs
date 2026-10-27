//! CPU collection and vertex attribute preparation for CesiumJS `PolylineCollection`.
//!
//! Rendering, GPU buffer lifecycle, WebGL pipeline state, and pick passes
//! remain to be implemented; this module provides the CPU collection lifecycle,
//! bounding volume synchronization, batch table attribute preparation, and
//! polyline geometry tessellation.

use super::polyline::{Polyline, PolylineOptions};
use crate::core::{EncodedCartesian3, Matrix4};
use std::{cell::RefCell, rc::Rc};

/// Shared handle modeling CesiumJS's mutable polyline objects and collection membership.
pub type PolylineHandle = Rc<RefCell<Polyline>>;

/// Batched attributes prepared for the GPU batch table / per-instance uniforms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolylineBatchAttributes {
    pub width_and_show: [f64; 2],
    pub center_high: [f64; 3],
    pub center_low_and_radius: [f64; 4],
    pub distance_display_condition: [f64; 2],
}

impl PolylineBatchAttributes {
    /// Extracts batch table attributes for a single polyline.
    pub fn from_polyline(polyline: &Polyline) -> Self {
        let width = polyline.width();
        let show = polyline.show() && width > 0.0;
        let bounding_sphere = polyline.bounding_volume_wc();
        let encoded_center = EncodedCartesian3::from_cartesian(bounding_sphere.center);

        let ddc = polyline
            .distance_display_condition()
            .map_or([0.0, f64::MAX], |d| [d.near, d.far]);

        Self {
            width_and_show: [width, if show { 1.0 } else { 0.0 }],
            center_high: [encoded_center.high.x, encoded_center.high.y, encoded_center.high.z],
            center_low_and_radius: [
                encoded_center.low.x,
                encoded_center.low.y,
                encoded_center.low.z,
                bounding_sphere.radius,
            ],
            distance_display_condition: ddc,
        }
    }
}

/// A tessellated polyline vertex for wide polyline expansion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolylineVertex {
    pub position: EncodedCartesian3,
    pub prev_position: EncodedCartesian3,
    pub next_position: EncodedCartesian3,
    pub tex_coord_expand_and_batch_index: [f64; 4],
}

/// Tessellated geometry data for a bucket of polylines sharing a material.
#[derive(Clone, Debug, PartialEq)]
pub struct PolylineGeometryData {
    pub material_type: String,
    pub vertices: Vec<PolylineVertex>,
    pub indices: Vec<u32>,
}

type GeometryBucket = (String, (Vec<PolylineVertex>, Vec<u32>));

/// A collection of polylines sharing a model matrix and coordinate space.
/// After changing `model_matrix`, call `update` before reading individual
/// polyline bounds or segments, including after mutating a polyline. Geometry
/// and batch-data methods update them.
#[derive(Debug)]
pub struct PolylineCollection {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    polylines: Vec<PolylineHandle>,
    destroyed: bool,
}

impl Default for PolylineCollection {
    fn default() -> Self {
        Self {
            show: true,
            model_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: false,
            polylines: Vec::new(),
            destroyed: false,
        }
    }
}

impl PolylineCollection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model_matrix(model_matrix: Matrix4) -> Self {
        Self {
            model_matrix,
            ..Default::default()
        }
    }

    pub fn len(&self) -> usize {
        self.polylines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.polylines.is_empty()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Adds a polyline to the collection.
    pub fn add(&mut self, options: PolylineOptions) -> Result<PolylineHandle, &'static str> {
        if self.destroyed {
            return Err("collection is destroyed");
        }
        let mut polyline = Polyline::new(options, Some(&self.model_matrix))?;
        polyline.set_index(self.polylines.len());
        let handle = Rc::new(RefCell::new(polyline));
        self.polylines.push(handle.clone());
        Ok(handle)
    }

    /// Determines whether the collection contains the specified polyline.
    pub fn contains(&self, polyline: &PolylineHandle) -> bool {
        polyline.borrow().is_attached() && self.polylines.iter().any(|p| Rc::ptr_eq(p, polyline))
    }

    /// Returns the polyline at the specified index.
    pub fn get(&self, index: usize) -> Option<PolylineHandle> {
        self.polylines.get(index).cloned()
    }

    /// Removes a polyline from the collection.
    pub fn remove(&mut self, polyline: &PolylineHandle) -> bool {
        let Some(index) = self.polylines.iter().position(|p| Rc::ptr_eq(p, polyline)) else {
            return false;
        };
        let removed = self.polylines.remove(index);
        removed.borrow_mut().detach();
        // Re-index remaining polylines
        for (i, p) in self.polylines.iter().enumerate().skip(index) {
            p.borrow_mut().set_index(i);
        }
        true
    }

    /// Removes all polylines from the collection.
    pub fn remove_all(&mut self) {
        for p in self.polylines.drain(..) {
            p.borrow_mut().detach();
        }
    }

    /// Destroys the collection and detaches all contained polylines.
    pub fn destroy(&mut self) {
        if !self.destroyed {
            self.remove_all();
            self.destroyed = true;
        }
    }

    /// Updates all contained polylines with the collection's model matrix.
    pub fn update(&mut self) {
        for p in &self.polylines {
            p.borrow_mut().update(&self.model_matrix);
        }
    }

    /// Returns the collection's polylines.
    pub fn polylines(&self) -> &[PolylineHandle] {
        &self.polylines
    }

    /// Prepares batch table attributes for all contained polylines.
    pub fn batch_attributes(&self) -> Vec<PolylineBatchAttributes> {
        self.polylines
            .iter()
            .map(|p| {
                let mut p = p.borrow_mut();
                p.update(&self.model_matrix);
                PolylineBatchAttributes::from_polyline(&p)
            })
            .collect()
    }

    /// Generates preliminary 3D CPU vertex quads, bucketed by material type.
    /// Projected and morph modes and GPU integration are not yet implemented.
    pub fn generate_vertex_data(&self) -> Vec<PolylineGeometryData> {
        let mut buckets: Vec<GeometryBucket> = Vec::new();

        for (batch_idx, p_cell) in self.polylines.iter().enumerate() {
            let mut p = p_cell.borrow_mut();
            p.update(&self.model_matrix);
            let positions = p.actual_positions();
            let lengths = [positions.len()];

            if positions.len() < 2 {
                p.clean();
                continue;
            }

            let mat_type = p.material().type_name.clone();
            let index = match buckets.iter().position(|(kind, _)| *kind == mat_type) {
                Some(index) => index,
                None => {
                    buckets.push((mat_type, (Vec::new(), Vec::new())));
                    buckets.len() - 1
                }
            };
            let (vertices, indices) = &mut buckets[index].1;

            let positions_len = positions.len();
            let mut segment_index = 0;
            let mut count = 0;
            let mut seg_quad_start = vertices.len() as u32;

            for j in 0..positions_len {
                let prev_pos = if j == 0 {
                    if p.r#loop() {
                        positions[positions_len - 2]
                    } else {
                        positions[0].subtract(positions[1]).add(positions[0])
                    }
                } else {
                    positions[j - 1]
                };

                let curr_pos = positions[j];

                let next_pos = if j == positions_len - 1 {
                    if p.r#loop() {
                        positions[1]
                    } else {
                        positions[positions_len - 1]
                            .subtract(positions[positions_len - 2])
                            .add(positions[positions_len - 1])
                    }
                } else {
                    positions[j + 1]
                };

                if segment_index < lengths.len() {
                    let seg_len = lengths[segment_index];
                    if j == count + seg_len {
                        count += seg_len;
                        segment_index += 1;
                    }
                }

                let current_seg_len = lengths.get(segment_index).copied().unwrap_or(positions_len);
                let segment_start = j == count;
                let segment_end = j == count + current_seg_len - 1;

                let start_k = if segment_start { 2 } else { 0 };
                let end_k = if segment_end { 2 } else { 4 };

                let enc_pos = EncodedCartesian3::from_cartesian(curr_pos);
                let enc_prev = EncodedCartesian3::from_cartesian(prev_pos);
                let enc_next = EncodedCartesian3::from_cartesian(next_pos);

                let s = j as f64 / (positions_len - 1).max(1) as f64;

                for k in start_k..end_k {
                    let direction = if k < 2 { -1.0 } else { 1.0 };
                    let expand = (2 * (k % 2) - 1) as f64;
                    vertices.push(PolylineVertex {
                        position: enc_pos,
                        prev_position: enc_prev,
                        next_position: enc_next,
                        tex_coord_expand_and_batch_index: [s, expand, direction, batch_idx as f64],
                    });
                }

                // If not at the end of the segment, emission of current point (k=2,3) and next
                // point (k=0,1) completes 4 vertices for one sub-segment quad.
                if !segment_start {
                    indices.extend_from_slice(&[
                        seg_quad_start,
                        seg_quad_start + 2,
                        seg_quad_start + 1,
                        seg_quad_start + 1,
                        seg_quad_start + 2,
                        seg_quad_start + 3,
                    ]);
                    seg_quad_start += 4;
                }
            }
            p.clean();
        }

        buckets
            .into_iter()
            .map(|(material_type, (vertices, indices))| PolylineGeometryData {
                material_type,
                vertices,
                indices,
            })
            .collect()
    }
}
