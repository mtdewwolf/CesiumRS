//! CesiumJS `Vector3DTileGeometry`: boxes, cylinders, ellipsoids and spheres
//! decoded by the `createVectorTileGeometries` worker into one
//! [`Vector3DTilePrimitive`](super::Vector3DTilePrimitive).
//!
//! The worker itself is already ported and reference-validated. This module
//! mirrors the pinned two-frame lifecycle: the first update schedules and runs
//! the decode, the next installs the primitive and reports `ready`.

use super::{
    cesium_3d_tile_style::Cesium3DTileStyle,
    scene::Result,
    vector_3d_tile_primitive::{
        BatchTableRef, FeatureList, Vector3DTilePrimitive, Vector3DTilePrimitiveOptions,
    },
    ClassificationType, Vector3DTileBatch,
};
use crate::{
    core::{BoundingSphere, Cartesian3, Color, Matrix4, RuntimeError},
    workers::vector_tiles::{create_vector_tile_geometries, VectorTileGeometries, VectorTileGeometriesParameters},
};

/// Constructor options for [`Vector3DTileGeometry`].
#[derive(Clone, Default)]
pub struct Vector3DTileGeometryOptions {
    pub boxes: Option<Vec<f32>>,
    pub box_batch_ids: Option<Vec<u16>>,
    pub cylinders: Option<Vec<f32>>,
    pub cylinder_batch_ids: Option<Vec<u16>>,
    pub ellipsoids: Option<Vec<f32>>,
    pub ellipsoid_batch_ids: Option<Vec<u16>>,
    pub spheres: Option<Vec<f32>>,
    pub sphere_batch_ids: Option<Vec<u16>>,
    pub center: Option<Cartesian3>,
    pub model_matrix: Matrix4,
    pub batch_table: Option<BatchTableRef>,
    pub bounding_volume: Option<BoundingSphere>,
}

/// A batch of classification geometries decoded from a vector tile.
pub struct Vector3DTileGeometry<G: crate::renderer::Gl + 'static> {
    boxes: Option<Vec<f32>>,
    box_batch_ids: Option<Vec<u16>>,
    cylinders: Option<Vec<f32>>,
    cylinder_batch_ids: Option<Vec<u16>>,
    ellipsoids: Option<Vec<f32>>,
    ellipsoid_batch_ids: Option<Vec<u16>>,
    spheres: Option<Vec<f32>>,
    sphere_batch_ids: Option<Vec<u16>>,
    model_matrix: Matrix4,
    batch_table: Option<BatchTableRef>,
    bounding_volume: Option<BoundingSphere>,
    center: Cartesian3,
    bounding_volumes: Option<Vec<BoundingSphere>>,
    batched_indices: Vec<Vector3DTileBatch>,
    batch_table_colors: Option<Vec<u32>>,
    packed_buffer: Option<Vec<f64>>,
    pending: Option<VectorTileGeometries>,
    primitive: Option<Vector3DTilePrimitive<G>>,
    pub debug_wireframe: bool,
    pub force_rebatch: bool,
    pub classification_type: ClassificationType,
    ready: bool,
    error: Option<RuntimeError>,
    destroyed: bool,
}

impl<G: crate::renderer::Gl + 'static> Vector3DTileGeometry<G> {
    pub const PACKED_BOX_LENGTH: usize = Matrix4::PACKED_LENGTH + Cartesian3::PACKED_LENGTH;
    pub const PACKED_CYLINDER_LENGTH: usize = Matrix4::PACKED_LENGTH + 2;
    pub const PACKED_ELLIPSOID_LENGTH: usize = Matrix4::PACKED_LENGTH + Cartesian3::PACKED_LENGTH;
    pub const PACKED_SPHERE_LENGTH: usize = Cartesian3::PACKED_LENGTH + 1;

    pub fn new(options: Vector3DTileGeometryOptions) -> Self {
        let center = options.center.unwrap_or_else(|| {
            options
                .bounding_volume
                .map(|volume| volume.center)
                .unwrap_or(Cartesian3::ZERO)
        });
        Self {
            boxes: options.boxes,
            box_batch_ids: options.box_batch_ids,
            cylinders: options.cylinders,
            cylinder_batch_ids: options.cylinder_batch_ids,
            ellipsoids: options.ellipsoids,
            ellipsoid_batch_ids: options.ellipsoid_batch_ids,
            spheres: options.spheres,
            sphere_batch_ids: options.sphere_batch_ids,
            model_matrix: options.model_matrix,
            batch_table: options.batch_table,
            bounding_volume: options.bounding_volume,
            center,
            bounding_volumes: None,
            batched_indices: Vec::new(),
            batch_table_colors: None,
            packed_buffer: None,
            pending: None,
            primitive: None,
            debug_wireframe: false,
            force_rebatch: false,
            classification_type: ClassificationType::Both,
            ready: false,
            error: None,
            destroyed: false,
        }
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn triangles_length(&self) -> f64 {
        self.primitive.as_ref().map_or(0.0, |p| p.triangles_length())
    }
    pub fn geometry_byte_length(&self) -> usize {
        self.primitive.as_ref().map_or(0, |p| p.geometry_byte_length())
    }
    pub fn primitive(&self) -> Option<&Vector3DTilePrimitive<G>> {
        self.primitive.as_ref()
    }

    pub fn pack_buffer(center: Cartesian3, model_matrix: &Matrix4) -> Vec<f64> {
        let mut buffer = vec![0.0; Matrix4::PACKED_LENGTH + Cartesian3::PACKED_LENGTH];
        let mut offset = 0;
        center.pack(&mut buffer, offset);
        offset += Cartesian3::PACKED_LENGTH;
        model_matrix.pack(&mut buffer, offset);
        buffer
    }

    fn create_primitive(&mut self) -> Result<()> {
        if self.primitive.is_some() {
            return Ok(());
        }
        if self.batch_table_colors.is_none() {
            let length = self.box_batch_ids.as_ref().map_or(0, Vec::len)
                + self.cylinder_batch_ids.as_ref().map_or(0, Vec::len)
                + self.ellipsoid_batch_ids.as_ref().map_or(0, Vec::len)
                + self.sphere_batch_ids.as_ref().map_or(0, Vec::len);
            let mut colors = Vec::with_capacity(length);
            for i in 0..length {
                let color = self
                    .batch_table
                    .as_ref()
                    .map(|table| table.borrow().get_color(i))
                    .unwrap_or(Color::WHITE);
                colors.push(color.to_rgba());
            }
            self.batch_table_colors = Some(colors);
            self.packed_buffer = Some(Self::pack_buffer(self.center, &self.model_matrix));
        }

        let packed = self.packed_buffer.clone().unwrap_or_default();
        let colors = self.batch_table_colors.clone().unwrap_or_default();
        let result = create_vector_tile_geometries(VectorTileGeometriesParameters {
            boxes: self.boxes.as_deref(),
            box_batch_ids: self.box_batch_ids.as_deref(),
            cylinders: self.cylinders.as_deref(),
            cylinder_batch_ids: self.cylinder_batch_ids.as_deref(),
            ellipsoids: self.ellipsoids.as_deref(),
            ellipsoid_batch_ids: self.ellipsoid_batch_ids.as_deref(),
            spheres: self.spheres.as_deref(),
            sphere_batch_ids: self.sphere_batch_ids.as_deref(),
            batch_table_colors: &colors,
            packed_buffer: &packed,
        })?;
        self.pending = Some(result);
        Ok(())
    }

    fn unpack_buffer(&mut self, packed: &[f64]) -> usize {
        let mut offset = 0;
        let indices_bytes_per_element = packed[offset] as usize;
        offset += 1;
        let num_bv = packed[offset] as usize;
        offset += 1;
        let mut bvs = Vec::with_capacity(num_bv);
        for _ in 0..num_bv {
            bvs.push(BoundingSphere::unpack(packed, offset));
            offset += 4;
        }
        self.bounding_volumes = Some(bvs);

        let num_batches = packed[offset] as usize;
        offset += 1;
        let mut batches = Vec::with_capacity(num_batches);
        for _ in 0..num_batches {
            let color = Color::unpack(packed, offset);
            offset += Color::PACKED_LENGTH;
            let index_offset = packed[offset] as usize;
            let count = packed[offset + 1] as usize;
            let length = packed[offset + 2] as usize;
            offset += 3;
            let mut batch_ids = Vec::with_capacity(length);
            for _ in 0..length {
                batch_ids.push(packed[offset] as u32);
                offset += 1;
            }
            batches.push(Vector3DTileBatch { offset: index_offset, count, color, batch_ids });
        }
        self.batched_indices = batches;
        indices_bytes_per_element
    }

    fn finish_primitive(&mut self, result: VectorTileGeometries) {
        if self.primitive.is_some() {
            return;
        }
        let indices_bytes_per_element = self.unpack_buffer(&result.packed_buffer);
        let _ = indices_bytes_per_element;
        let primitive = Vector3DTilePrimitive::new(Vector3DTilePrimitiveOptions {
            positions: result.positions,
            vertex_batch_ids: result.vertex_batch_ids,
            indices: result.indices,
            index_counts: result.index_counts,
            index_offsets: result.index_offsets,
            batched_indices: std::mem::take(&mut self.batched_indices),
            batch_ids: result.batch_ids,
            bounding_volume: self.bounding_volume.unwrap_or(BoundingSphere::new(Cartesian3::ZERO, 0.0)),
            bounding_volumes: self
                .bounding_volumes
                .take()
                .map(|volumes| volumes.into_iter().map(super::vector_3d_tile_primitive::Vector3DTileBoundingVolume::Sphere).collect()),
            center: self.center,
            classification_type: self.classification_type,
            batch_table: self.batch_table.clone(),
            ..Default::default()
        });
        self.primitive = Some(primitive);
        self.boxes = None;
        self.box_batch_ids = None;
        self.cylinders = None;
        self.cylinder_batch_ids = None;
        self.ellipsoids = None;
        self.ellipsoid_batch_ids = None;
        self.spheres = None;
        self.sphere_batch_ids = None;
        self.batch_table_colors = None;
        self.packed_buffer = None;
    }

    /// Cesium's `createFeatures`.
    pub fn create_features(&self, features: &mut FeatureList) {
        if let Some(primitive) = &self.primitive {
            primitive.create_features(features);
        }
    }

    pub fn apply_debug_settings(&mut self, enabled: bool, color: Color) {
        if let Some(primitive) = &mut self.primitive {
            primitive.apply_debug_settings(enabled, color);
        }
    }

    pub fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>, features: &mut FeatureList) -> Result<()> {
        if let Some(primitive) = &mut self.primitive {
            primitive.apply_style(style, features)?;
        }
        Ok(())
    }

    pub fn update_commands(&mut self, batch_id: u16, color: Color) {
        if let Some(primitive) = &mut self.primitive {
            primitive.update_commands(batch_id, color);
        }
    }

    /// Cesium's `update`.
    pub fn update(&mut self, frame: &mut super::FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.ready {
            if self.primitive.is_none() && self.pending.is_none() {
                self.create_primitive()?;
                return Ok(());
            }
            if let Some(error) = self.error.take() {
                return Err(error);
            }
            if let Some(result) = self.pending.take() {
                self.finish_primitive(result);
                self.ready = true;
            }
            return Ok(());
        }
        if let Some(primitive) = &mut self.primitive {
            primitive.debug_wireframe = self.debug_wireframe;
            primitive.force_rebatch = self.force_rebatch;
            primitive.classification_type = self.classification_type;
            primitive.update(frame)?;
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if let Some(primitive) = &mut self.primitive {
            primitive.destroy()?;
        }
        self.primitive = None;
        self.pending = None;
        self.destroyed = true;
        Ok(())
    }
}
