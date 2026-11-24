//! CesiumJS `Vector3DTilePolygons`: pre-triangulated polygons draped on terrain
//! and 3D Tiles, decoded by the `createVectorTilePolygons` worker.
//!
//! The source has two quirks this port keeps: the constructor's `center` option
//! is overwritten with `undefined` immediately, so the centre is always the
//! rectangle centre projected onto the ellipsoid; and polygon colours come from
//! the batch table by polygon index.

use super::{
    cesium_3d_tile_style::Cesium3DTileStyle,
    scene::Result,
    vector_3d_tile_primitive::{
        BatchTableRef, FeatureList, Vector3DTilePrimitive, Vector3DTilePrimitiveOptions,
    },
    ClassificationType, Vector3DTileBatch,
};
use crate::{
    core::{
        BoundingSphere, Cartesian3, Color, Ellipsoid, OrientedBoundingBox, Rectangle, RuntimeError, TypedArray,
    },
    workers::vector_tiles::{create_vector_tile_polygons, VectorTilePolygons, VectorTilePolygonsParameters},
};

/// Constructor options for [`Vector3DTilePolygons`].
#[derive(Clone)]
pub struct Vector3DTilePolygonsOptions {
    pub positions: Vec<u16>,
    pub counts: Vec<u32>,
    pub indices: TypedArray,
    pub index_counts: Vec<u32>,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub polygon_minimum_heights: Option<Vec<f32>>,
    pub polygon_maximum_heights: Option<Vec<f32>>,
    pub rectangle: Rectangle,
    pub ellipsoid: Option<Ellipsoid>,
    /// Accepted and then discarded, matching the pinned constructor.
    pub center: Option<Cartesian3>,
    pub batch_table: Option<BatchTableRef>,
    pub batch_ids: Vec<u16>,
    pub bounding_volume: BoundingSphere,
}

impl Default for Vector3DTilePolygonsOptions {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            counts: Vec::new(),
            indices: TypedArray::Uint32(Vec::new()),
            index_counts: Vec::new(),
            minimum_height: 0.0,
            maximum_height: 0.0,
            polygon_minimum_heights: None,
            polygon_maximum_heights: None,
            rectangle: Rectangle::default(),
            ellipsoid: None,
            center: None,
            batch_table: None,
            batch_ids: Vec::new(),
            bounding_volume: BoundingSphere::new(Cartesian3::ZERO, 0.0),
        }
    }
}

/// A batch of vector-tile polygons.
pub struct Vector3DTilePolygons<G: crate::renderer::Gl + 'static> {
    batch_table: Option<BatchTableRef>,
    batch_ids: Vec<u16>,
    positions: Vec<u16>,
    counts: Vec<u32>,
    indices: TypedArray,
    index_counts: Vec<u32>,
    ellipsoid: Ellipsoid,
    minimum_height: f64,
    maximum_height: f64,
    polygon_minimum_heights: Option<Vec<f32>>,
    polygon_maximum_heights: Option<Vec<f32>>,
    rectangle: Rectangle,
    bounding_volume: BoundingSphere,
    bounding_volumes: Option<Vec<OrientedBoundingBox>>,
    batched_indices: Vec<Vector3DTileBatch>,
    transferrable_batch_ids: Option<Vec<u32>>,
    batch_table_colors: Option<Vec<u32>>,
    packed_buffer: Option<Vec<f64>>,
    pending: Option<VectorTilePolygons>,
    primitive: Option<Vector3DTilePrimitive<G>>,
    pub debug_wireframe: bool,
    pub force_rebatch: bool,
    pub classification_type: ClassificationType,
    ready: bool,
    destroyed: bool,
}

impl<G: crate::renderer::Gl + 'static> Vector3DTilePolygons<G> {
    pub fn new(options: Vector3DTilePolygonsOptions) -> Self {
        Self {
            batch_table: options.batch_table,
            batch_ids: options.batch_ids,
            positions: options.positions,
            counts: options.counts,
            indices: options.indices,
            index_counts: options.index_counts,
            ellipsoid: options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
            minimum_height: options.minimum_height,
            maximum_height: options.maximum_height,
            polygon_minimum_heights: options.polygon_minimum_heights,
            polygon_maximum_heights: options.polygon_maximum_heights,
            rectangle: options.rectangle,
            bounding_volume: options.bounding_volume,
            bounding_volumes: None,
            batched_indices: Vec::new(),
            transferrable_batch_ids: None,
            batch_table_colors: None,
            packed_buffer: None,
            pending: None,
            primitive: None,
            debug_wireframe: false,
            force_rebatch: false,
            classification_type: ClassificationType::Both,
            ready: false,
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
    pub fn batch_ids(&self) -> &[u16] {
        &self.batch_ids
    }
    pub fn counts(&self) -> &[u32] {
        &self.counts
    }
    pub fn positions_len(&self) -> usize {
        self.positions.len()
    }
    pub fn minimum_height(&self) -> f64 {
        self.minimum_height
    }
    pub fn maximum_height(&self) -> f64 {
        self.maximum_height
    }
    pub fn rectangle(&self) -> Rectangle {
        self.rectangle
    }
    pub fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
    pub fn polygon_minimum_heights(&self) -> Option<&[f32]> {
        self.polygon_minimum_heights.as_deref()
    }
    pub fn polygon_maximum_heights(&self) -> Option<&[f32]> {
        self.polygon_maximum_heights.as_deref()
    }

    pub fn pack_buffer(
        indices_bytes_per_element: usize,
        minimum_height: f64,
        maximum_height: f64,
        center: Cartesian3,
        ellipsoid: &Ellipsoid,
        rectangle: &Rectangle,
    ) -> Vec<f64> {
        let mut buffer = vec![0.0; 3 + Cartesian3::PACKED_LENGTH + Ellipsoid::PACKED_LENGTH + Rectangle::PACKED_LENGTH];
        buffer[0] = indices_bytes_per_element as f64;
        buffer[1] = minimum_height;
        buffer[2] = maximum_height;
        let mut offset = 3;
        center.pack(&mut buffer, offset);
        offset += Cartesian3::PACKED_LENGTH;
        ellipsoid.pack(&mut buffer, offset);
        offset += Ellipsoid::PACKED_LENGTH;
        rectangle.pack(&mut buffer, offset);
        buffer
    }

    fn indices_bytes_per_element(&self) -> usize {
        match self.indices {
            TypedArray::Uint16(_) => 2,
            TypedArray::Uint32(_) => 4,
            TypedArray::Uint8(_) => 1,
            _ => 4,
        }
    }

    fn create_primitive(&mut self) -> Result<()> {
        if self.primitive.is_some() {
            return Ok(());
        }
        if self.batch_table_colors.is_none() {
            let center = self
                .ellipsoid
                .cartographic_to_cartesian(&self.rectangle.center());
            self.transferrable_batch_ids =
                Some(self.batch_ids.iter().map(|&id| u32::from(id)).collect());
            let mut colors = Vec::with_capacity(self.batch_ids.len());
            for i in 0..self.batch_ids.len() {
                let color = self
                    .batch_table
                    .as_ref()
                    .map(|table| table.borrow().get_color(i))
                    .unwrap_or(Color::WHITE);
                colors.push(color.to_rgba());
            }
            self.batch_table_colors = Some(colors);
            self.packed_buffer = Some(Self::pack_buffer(
                self.indices_bytes_per_element(),
                self.minimum_height,
                self.maximum_height,
                center,
                &self.ellipsoid,
                &self.rectangle,
            ));
        }

        let packed = self.packed_buffer.clone().unwrap_or_default();
        let colors = self.batch_table_colors.clone().unwrap_or_default();
        let batch_ids = self.transferrable_batch_ids.clone().unwrap_or_default();
        let min_heights = self.polygon_minimum_heights.as_deref();
        let max_heights = self.polygon_maximum_heights.as_deref();
        let (min_heights, max_heights) = match (min_heights, max_heights) {
            (Some(min), Some(max)) => (Some(min), Some(max)),
            _ => (None, None),
        };
        let result = create_vector_tile_polygons(VectorTilePolygonsParameters {
            packed_buffer: &packed,
            indices: &self.indices,
            positions: &self.positions,
            counts: &self.counts,
            index_counts: &self.index_counts,
            batch_ids: &batch_ids,
            batch_table_colors: &colors,
            minimum_heights: min_heights,
            maximum_heights: max_heights,
        })?;
        self.pending = Some(result);
        Ok(())
    }

    fn unpack_buffer(&mut self, packed: &[f64]) {
        let mut offset = 1;
        let num_bv = packed[offset] as usize;
        offset += 1;
        let mut bvs = Vec::with_capacity(num_bv);
        for _ in 0..num_bv {
            bvs.push(OrientedBoundingBox::unpack(packed, offset));
            offset += OrientedBoundingBox::PACKED_LENGTH;
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
    }

    fn finish_primitive(&mut self, result: VectorTilePolygons) {
        if self.primitive.is_some() {
            return;
        }
        self.unpack_buffer(&result.packed_buffer);
        let bounding_volumes = self
            .bounding_volumes
            .as_ref()
            .map(|boxes| boxes.iter().map(|b| super::vector_3d_tile_primitive::Vector3DTileBoundingVolume::Box(*b)).collect());
        let primitive = Vector3DTilePrimitive::new(Vector3DTilePrimitiveOptions {
            positions: result.positions,
            vertex_batch_ids: result.batch_ids,
            indices: result.indices,
            index_counts: result.index_counts,
            index_offsets: result.index_offsets,
            batched_indices: std::mem::take(&mut self.batched_indices),
            batch_ids: self.batch_ids.clone(),
            bounding_volume: self.bounding_volume,
            bounding_volumes,
            center: self.ellipsoid.cartographic_to_cartesian(&self.rectangle.center()),
            classification_type: self.classification_type,
            batch_table: self.batch_table.clone(),
            ..Default::default()
        });
        self.primitive = Some(primitive);
        self.batch_table = None;
        self.positions.clear();
        self.counts.clear();
        self.index_counts.clear();
        self.polygon_minimum_heights = None;
        self.polygon_maximum_heights = None;
        self.transferrable_batch_ids = None;
        self.batch_table_colors = None;
        self.packed_buffer = None;
        self.bounding_volumes = None;
    }

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

    pub fn update(&mut self, frame: &mut super::FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.ready {
            if self.primitive.is_none() && self.pending.is_none() {
                self.create_primitive()?;
                return Ok(());
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
