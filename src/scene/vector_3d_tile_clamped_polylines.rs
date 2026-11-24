//! CesiumJS `Vector3DTileClampedPolylines`: vector-tile polylines expanded to
//! eight-vertex volumes that classify terrain and 3D Tiles.
//!
//! The pinned constructor starts an asynchronous `ApproximateTerrainHeights`
//! initialization but invokes `initialize(...).then(createVertexArray(...))`
//! with the worker call evaluated immediately, so the decoded geometry uses the
//! default minimum and maximum heights while the bounding volume is updated with
//! the table. This port keeps that order. The bundled height table replaces the
//! browser fetch, so initialization settles on the first update.

use super::{
    cesium_3d_tile_style::Cesium3DTileStyle,
    scene::Result,
    vector_3d_tile_primitive::{BatchTableRef, FeatureList, Vector3DTileFeature},
    ClassificationType,
};
use crate::{
    core::{
        approximate_terrain_heights::{DEFAULT_MAX_TERRAIN_HEIGHT, DEFAULT_MIN_TERRAIN_HEIGHT},
        ApproximateTerrainHeights, Cartesian2, Cartesian3, Color, ComponentDatatype, Ellipsoid, Geometry,
        GeometryAttribute, GeometryAttributes, OrientedBoundingBox, PrimitiveType, Rectangle, RuntimeError, TypedArray,
    },
    renderer::{
        render_state::{Cull, StencilOperation as StencilOps, StencilTest},
        UniformValue, VertexArray,
    },
    shaders::root::{POLYLINE_COMMON, VECTOR3D_TILE_CLAMPED_POLYLINES_FS, VECTOR3D_TILE_CLAMPED_POLYLINES_VS},
    workers::vector_tiles::{
        create_vector_tile_clamped_polylines, DecodedVectorPositions, VectorTileClampedPolylines,
        VectorTilePolylinesParameters,
    },
};
use std::{
    cell::Cell,
    rc::Rc,
};

/// Constructor options for [`Vector3DTileClampedPolylines`].
#[derive(Clone)]
pub struct Vector3DTileClampedPolylinesOptions {
    pub positions: Vec<u16>,
    pub counts: Vec<u32>,
    pub widths: Vec<u16>,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub rectangle: Rectangle,
    pub ellipsoid: Option<Ellipsoid>,
    pub center: Option<Cartesian3>,
    pub batch_table: Option<BatchTableRef>,
    pub batch_ids: Vec<u16>,
    pub classification_type: ClassificationType,
    pub keep_decoded_positions: bool,
}

impl Default for Vector3DTileClampedPolylinesOptions {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            counts: Vec::new(),
            widths: Vec::new(),
            minimum_height: 0.0,
            maximum_height: 0.0,
            rectangle: Rectangle::default(),
            ellipsoid: None,
            center: None,
            batch_table: None,
            batch_ids: Vec::new(),
            classification_type: ClassificationType::Both,
            keep_decoded_positions: false,
        }
    }
}

pub fn clamped_attribute_locations() -> std::collections::BTreeMap<String, u32> {
    std::collections::BTreeMap::from([
        ("startEllipsoidNormal".to_string(), 0),
        ("endEllipsoidNormal".to_string(), 1),
        ("startPositionAndHeight".to_string(), 2),
        ("endPositionAndHeight".to_string(), 3),
        ("startFaceNormalAndVertexCorner".to_string(), 4),
        ("endFaceNormalAndHalfWidth".to_string(), 5),
        ("a_batchId".to_string(), 6),
    ])
}

/// A batch of clamped vector-tile polylines.
pub struct Vector3DTileClampedPolylines<G: crate::renderer::Gl + 'static> {
    positions: Vec<u16>,
    widths: Vec<u16>,
    counts: Vec<u32>,
    batch_ids: Vec<u16>,
    ellipsoid: Ellipsoid,
    minimum_height: f64,
    maximum_height: f64,
    center: Cartesian3,
    rectangle: Rectangle,
    batch_table: Option<BatchTableRef>,
    pub keep_decoded_positions: bool,
    bounding_volume: Option<OrientedBoundingBox>,
    minimum_maximum_heights: Cartesian2,
    classification_type: ClassificationType,

    decoded_positions: Option<DecodedVectorPositions>,
    pending: Option<VectorTileClampedPolylines>,
    pending_indices: Option<TypedArray>,
    pending_arrays: Option<PendingClampedArrays>,
    ready: bool,

    pub constant_color: Color,
    pub highlight_color: Color,
    highlight_cell: Rc<Cell<Color>>,
    context: Option<crate::renderer::Context<G>>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    program: Option<Rc<crate::renderer::ShaderProgram<G>>>,
    render_state: crate::renderer::RenderState,
    render_state_3d_tiles: crate::renderer::RenderState,
    terrain_command: Option<super::scene::SceneCommand>,
    tileset_command: Option<super::scene::SceneCommand>,
    triangles_length: f64,
    geometry_byte_length: usize,
    heights_initialized: bool,
    destroyed: bool,
}

struct PendingClampedArrays {
    start_ellipsoid_normals: Vec<f32>,
    end_ellipsoid_normals: Vec<f32>,
    start_position_and_heights: Vec<f32>,
    start_face_normal_and_vertex_corner_ids: Vec<f32>,
    end_position_and_heights: Vec<f32>,
    end_face_normal_and_half_widths: Vec<f32>,
    vertex_batch_ids: Vec<u16>,
}

impl<G: crate::renderer::Gl + 'static> Vector3DTileClampedPolylines<G> {
    pub fn new(options: Vector3DTileClampedPolylinesOptions) -> Self {
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let bounding_volume =
            OrientedBoundingBox::from_rectangle(&options.rectangle, DEFAULT_MIN_TERRAIN_HEIGHT, DEFAULT_MAX_TERRAIN_HEIGHT, &ellipsoid);
        Self {
            positions: options.positions,
            widths: options.widths,
            counts: options.counts,
            batch_ids: options.batch_ids,
            ellipsoid,
            minimum_height: options.minimum_height,
            maximum_height: options.maximum_height,
            center: options.center.unwrap_or(Cartesian3::ZERO),
            rectangle: options.rectangle,
            batch_table: options.batch_table,
            keep_decoded_positions: options.keep_decoded_positions,
            bounding_volume,
            minimum_maximum_heights: Cartesian2::new(DEFAULT_MIN_TERRAIN_HEIGHT, DEFAULT_MAX_TERRAIN_HEIGHT),
            classification_type: options.classification_type,
            decoded_positions: None,
            pending: None,
            pending_indices: None,
            pending_arrays: None,
            ready: false,
            constant_color: Color::WHITE,
            highlight_color: Color::WHITE,
            highlight_cell: Rc::new(Cell::new(Color::WHITE)),
            context: None,
            vertex_array: None,
            program: None,
            render_state: default_render_state(false),
            render_state_3d_tiles: default_render_state(true),
            terrain_command: None,
            tileset_command: None,
            triangles_length: 0.0,
            geometry_byte_length: 0,
            heights_initialized: false,
            destroyed: false,
        }
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn triangles_length(&self) -> f64 {
        self.triangles_length
    }
    pub fn geometry_byte_length(&self) -> usize {
        self.geometry_byte_length
    }
    pub fn bounding_volume(&self) -> Option<OrientedBoundingBox> {
        self.bounding_volume
    }
    pub fn minimum_maximum_heights(&self) -> Cartesian2 {
        self.minimum_maximum_heights
    }
    pub fn decoded_positions(&self) -> Option<&DecodedVectorPositions> {
        self.decoded_positions.as_ref()
    }
    pub fn batch_ids(&self) -> &[u16] {
        &self.batch_ids
    }
    pub fn counts(&self) -> &[u32] {
        &self.counts
    }
    pub fn widths(&self) -> &[u16] {
        &self.widths
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
    pub fn center(&self) -> Cartesian3 {
        self.center
    }
    pub fn rectangle(&self) -> Rectangle {
        self.rectangle
    }

    pub fn pack_buffer(
        rectangle: &Rectangle,
        minimum_height: f64,
        maximum_height: f64,
        ellipsoid: &Ellipsoid,
        center: Cartesian3,
    ) -> Vec<f64> {
        let mut buffer = vec![0.0; 2 + Rectangle::PACKED_LENGTH + Ellipsoid::PACKED_LENGTH + Cartesian3::PACKED_LENGTH];
        buffer[0] = minimum_height;
        buffer[1] = maximum_height;
        let mut offset = 2;
        rectangle.pack(&mut buffer, offset);
        offset += Rectangle::PACKED_LENGTH;
        ellipsoid.pack(&mut buffer, offset);
        offset += Ellipsoid::PACKED_LENGTH;
        center.pack(&mut buffer, offset);
        buffer
    }

    fn update_minimum_maximum_heights(&mut self) {
        let (min, max) = ApproximateTerrainHeights::bundled()
            .get_minimum_maximum_heights(&self.rectangle, &self.ellipsoid)
            .unwrap_or((DEFAULT_MIN_TERRAIN_HEIGHT, DEFAULT_MAX_TERRAIN_HEIGHT));
        self.minimum_maximum_heights = Cartesian2::new(min, max);
        self.bounding_volume = OrientedBoundingBox::from_rectangle(&self.rectangle, min, max, &self.ellipsoid);
        self.heights_initialized = true;
    }

    fn create_vertex_array(&mut self) -> Result<()> {
        let packed = Self::pack_buffer(&self.rectangle, self.minimum_height, self.maximum_height, &self.ellipsoid, self.center);
        let result = create_vector_tile_clamped_polylines(VectorTilePolylinesParameters {
            positions: &self.positions,
            widths: &self.widths,
            counts: &self.counts,
            batch_ids: &self.batch_ids,
            packed_buffer: &packed,
            keep_decoded_positions: self.keep_decoded_positions,
        })?;
        self.pending = Some(result);
        Ok(())
    }

    fn finish_vertex_array(&mut self, result: VectorTileClampedPolylines) {
        if self.vertex_array.is_some() {
            return;
        }
        if self.keep_decoded_positions {
            self.decoded_positions = result.decoded.clone();
        }
        let bytes = result.start_ellipsoid_normals.len() * 4
            + result.end_ellipsoid_normals.len() * 4
            + result.start_position_and_heights.len() * 4
            + result.end_position_and_heights.len() * 4
            + result.start_face_normal_and_vertex_corner_ids.len() * 4
            + result.end_face_normal_and_half_widths.len() * 4
            + result.vertex_batch_ids.len() * 2
            + match &result.indices {
                TypedArray::Uint16(v) => v.len() * 2,
                TypedArray::Uint32(v) => v.len() * 4,
                other => other.len() * 4,
            };
        self.triangles_length = result.indices.len() as f64 / 3.0;
        self.geometry_byte_length = bytes;
        self.pending_indices = Some(result.indices);
        self.pending_arrays = Some(PendingClampedArrays {
            start_ellipsoid_normals: result.start_ellipsoid_normals,
            end_ellipsoid_normals: result.end_ellipsoid_normals,
            start_position_and_heights: result.start_position_and_heights,
            start_face_normal_and_vertex_corner_ids: result.start_face_normal_and_vertex_corner_ids,
            end_position_and_heights: result.end_position_and_heights,
            end_face_normal_and_half_widths: result.end_face_normal_and_half_widths,
            vertex_batch_ids: result.vertex_batch_ids,
        });
        self.positions.clear();
        self.widths.clear();
        self.counts.clear();
    }

    fn ensure_gpu(&mut self, frame: &super::FrameState) -> Result<()> {
        if self.vertex_array.is_some() {
            return Ok(());
        }
        let Some(context) = frame.context.as_ref().and_then(|c| c.get::<crate::renderer::Context<G>>()).cloned() else {
            return Ok(());
        };
        let Some(arrays) = self.pending_arrays.take() else {
            return Ok(());
        };
        let Some(indices) = self.pending_indices.take() else {
            return Ok(());
        };
        let floats = |values: &[f32]| values.iter().map(|&v| f64::from(v)).collect::<Vec<_>>();
        let mut attributes = GeometryAttributes {
            position: Some(GeometryAttribute::floats(3, &floats(&arrays.start_ellipsoid_normals))),
            ..Default::default()
        };
        attributes.others.push((
            "startEllipsoidNormal".into(),
            GeometryAttribute::floats(3, &floats(&arrays.start_ellipsoid_normals)),
        ));
        attributes.others.push((
            "endEllipsoidNormal".into(),
            GeometryAttribute::floats(3, &floats(&arrays.end_ellipsoid_normals)),
        ));
        attributes.others.push((
            "startPositionAndHeight".into(),
            GeometryAttribute::floats(4, &floats(&arrays.start_position_and_heights)),
        ));
        attributes.others.push((
            "endPositionAndHeight".into(),
            GeometryAttribute::floats(4, &floats(&arrays.end_position_and_heights)),
        ));
        attributes.others.push((
            "startFaceNormalAndVertexCorner".into(),
            GeometryAttribute::floats(4, &floats(&arrays.start_face_normal_and_vertex_corner_ids)),
        ));
        attributes.others.push((
            "endFaceNormalAndHalfWidth".into(),
            GeometryAttribute::floats(4, &floats(&arrays.end_face_normal_and_half_widths)),
        ));
        attributes.others.push((
            "a_batchId".into(),
            GeometryAttribute::new(
                ComponentDatatype::UnsignedShort,
                1,
                TypedArray::Uint16(arrays.vertex_batch_ids),
            ),
        ));
        let geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Triangles);
        let va = VertexArray::from_geometry(
            &context,
            &geometry,
            crate::renderer::GeometryUploadOptions {
                buffer_usage: crate::renderer::BufferUsage::StaticDraw,
                attribute_locations: clamped_attribute_locations().into_iter().collect(),
                interleave: false,
                vertex_array_attributes: vec![],
            },
        )?;
        self.vertex_array = Some(Rc::new(va));

        let table = self.batch_table.clone();
        let vs_base = match &table {
            Some(table) => table.borrow().vertex_shader_source(VECTOR3D_TILE_CLAMPED_POLYLINES_VS.source),
            None => VECTOR3D_TILE_CLAMPED_POLYLINES_VS.source.to_string(),
        };
        let fs_base = match &table {
            Some(table) => table.borrow().fragment_shader_source(VECTOR3D_TILE_CLAMPED_POLYLINES_FS.source),
            None => VECTOR3D_TILE_CLAMPED_POLYLINES_FS.source.to_string(),
        };
        let vs = crate::renderer::ShaderSource {
            defines: vec!["VECTOR_TILE".into(), "CLIP_POLYLINE".into()],
            sources: vec![POLYLINE_COMMON.source.into(), vs_base],
            ..Default::default()
        };
        let fs = crate::renderer::ShaderSource {
            defines: vec!["VECTOR_TILE".into()],
            sources: vec![fs_base],
            ..Default::default()
        };
        let options = crate::renderer::ShaderProgramOptions {
            vertex_shader_source: vs,
            fragment_shader_source: fs,
            attribute_locations: clamped_attribute_locations(),
        };
        self.program = Some(context.with_shader_cache(|cache| cache.get_shader_program(&options))?);
        self.context = Some(context);
        Ok(())
    }

    fn uniform_map(&self, context: crate::renderer::Context<G>) -> crate::renderer::UniformMap {
        let center = self.center;
        let highlight = self.highlight_cell.clone();
        let heights = Rc::new(Cell::new(self.minimum_maximum_heights));
        let table = self.batch_table.clone();
        Rc::new(move || {
            let view = context.uniform_state().view;
            let modified = view.set_translation(view.multiply_by_point(center));
            let mut uniforms = vec![
                (
                    "u_modifiedModelView".into(),
                    UniformValue::Matrix { dimension: 4, values: modified.0.map(|v| v as f32).to_vec() },
                ),
                (
                    "u_highlightColor".into(),
                    UniformValue::Float(vec![
                        highlight.get().red as f32,
                        highlight.get().green as f32,
                        highlight.get().blue as f32,
                        highlight.get().alpha as f32,
                    ]),
                ),
                (
                    "u_minimumMaximumVectorHeights".into(),
                    UniformValue::Float(vec![heights.get().x as f32, heights.get().y as f32]),
                ),
            ];
            if let Some(table) = &table {
                uniforms.extend(table.borrow().uniforms());
            }
            Ok(uniforms)
        })
    }

    fn ensure_commands(&mut self) {
        if self.terrain_command.is_some() {
            return;
        }
        let (Some(context), Some(vertex_array), Some(program)) =
            (self.context.clone(), self.vertex_array.clone(), self.program.clone())
        else {
            return;
        };
        let uniform_map = self.uniform_map(context.clone());
        let bounding_volume = self.bounding_volume;
        let metadata = crate::renderer::DrawMetadata {
            bounding_volume: None,
            oriented_bounding_box: bounding_volume,
            ..Default::default()
        };
        let mut options = super::scene::SceneDrawOptions::new(vertex_array.clone(), program.clone());
        options.state = self.render_state.clone();
        options.uniform_map = Some(uniform_map.clone());
        self.terrain_command = Some(super::scene::SceneCommand::from_draw(
            "vector-3d-tile-clamped-polylines",
            context.clone(),
            crate::renderer::Pass::TerrainClassification,
            metadata.clone(),
            options,
        ));
        let mut options = super::scene::SceneDrawOptions::new(vertex_array, program);
        options.state = self.render_state_3d_tiles.clone();
        options.uniform_map = Some(uniform_map);
        self.tileset_command = Some(super::scene::SceneCommand::from_draw(
            "vector-3d-tile-clamped-polylines-3dtiles",
            context,
            crate::renderer::Pass::Cesium3DTileClassification,
            metadata,
            options,
        ));
    }

    pub fn create_features(&self, features: &mut FeatureList) {
        for &batch_id in &self.batch_ids {
            let index = batch_id as usize;
            if features.len() <= index {
                features.resize(index + 1, None);
            }
            features[index] = Some(Vector3DTileFeature::new(index));
        }
    }

    pub fn apply_debug_settings(&mut self, enabled: bool, color: Color) {
        self.highlight_color = if enabled { color } else { self.constant_color };
        self.highlight_cell.set(self.highlight_color);
    }

    pub fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>, features: &mut FeatureList) -> Result<()> {
        let Some(style) = style else {
            for &batch_id in &self.batch_ids {
                if let Some(Some(feature)) = features.get_mut(batch_id as usize) {
                    feature.show = true;
                    feature.color = Color::WHITE;
                }
            }
            return Ok(());
        };
        for &batch_id in &self.batch_ids {
            let Some(Some(feature)) = features.get_mut(batch_id as usize) else {
                continue;
            };
            let (color, show) = {
                let style_feature: &dyn super::expression::StyleFeature = feature;
                let color = match style.color() {
                    Some(expression) => expression.evaluate_color(Some(style_feature))?.unwrap_or(Color::WHITE),
                    None => Color::WHITE,
                };
                let show = match style.show() {
                    Some(expression) => match expression.evaluate(Some(style_feature))? {
                        super::expression::StyleValue::Boolean(value) => value,
                        super::expression::StyleValue::Undefined | super::expression::StyleValue::Null => true,
                        other => other.to_boolean(),
                    },
                    None => true,
                };
                (color, show)
            };
            feature.color = color;
            feature.show = show;
        }
        Ok(())
    }

    pub fn get_positions(&self, batch_id: u16) -> Option<Vec<f64>> {
        super::vector_3d_tile_polylines::get_polyline_positions(
            &self.batch_ids,
            self.decoded_positions.as_ref(),
            batch_id,
        )
    }

    pub fn update(&mut self, frame: &mut super::FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.ready {
            if self.pending.is_none() && self.vertex_array.is_none() {
                self.create_vertex_array()?;
                if !self.heights_initialized {
                    self.update_minimum_maximum_heights();
                }
                return Ok(());
            }
            if let Some(result) = self.pending.take() {
                self.finish_vertex_array(result);
                self.ready = true;
            }
        }
        self.ensure_gpu(frame)?;
        self.ensure_commands();
        if frame.passes.render || frame.passes.pick {
            let queue_terrain = self.classification_type != ClassificationType::Cesium3DTile;
            let queue_tiles = self.classification_type != ClassificationType::Terrain;
            if queue_terrain {
                if let Some(command) = &self.terrain_command {
                    frame.command_list.push(command.clone());
                }
            }
            if queue_tiles {
                if let Some(command) = &self.tileset_command {
                    frame.command_list.push(command.clone());
                }
            }
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
        self.vertex_array = None;
        self.program = None;
        self.terrain_command = None;
        self.tileset_command = None;
        self.destroyed = true;
        Ok(())
    }
}

fn get_render_state(mask_3d_tiles: bool) -> crate::renderer::RenderState {
    crate::renderer::RenderState {
        cull: Cull { enabled: true, face: crate::core::webgl_constants::FRONT },
        blending: super::blending_state::pre_multiplied_alpha_blend(),
        depth_mask: false,
        stencil_test: StencilTest {
            enabled: mask_3d_tiles,
            front_function: crate::core::webgl_constants::EQUAL,
            front_operation: StencilOps {
                fail: crate::core::webgl_constants::KEEP,
                z_fail: crate::core::webgl_constants::KEEP,
                z_pass: crate::core::webgl_constants::KEEP,
            },
            back_function: crate::core::webgl_constants::EQUAL,
            back_operation: StencilOps {
                fail: crate::core::webgl_constants::KEEP,
                z_fail: crate::core::webgl_constants::KEEP,
                z_pass: crate::core::webgl_constants::KEEP,
            },
            reference: super::stencil_constants::CESIUM_3D_TILE_MASK,
            mask: i64::from(super::stencil_constants::CESIUM_3D_TILE_MASK),
        },
        ..Default::default()
    }
}

fn default_render_state(mask_3d_tiles: bool) -> crate::renderer::RenderState {
    get_render_state(mask_3d_tiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bounds_use_approximate_terrain_defaults() {
        let polyline = Vector3DTileClampedPolylines::<TestGl>::new(Vector3DTileClampedPolylinesOptions {
            rectangle: Rectangle::new(-0.1, -0.1, 0.1, 0.1),
            ..Default::default()
        });
        assert_eq!(
            polyline.minimum_maximum_heights(),
            Cartesian2::new(DEFAULT_MIN_TERRAIN_HEIGHT, DEFAULT_MAX_TERRAIN_HEIGHT)
        );
        assert!(polyline.bounding_volume().is_some());
    }

    struct TestGl;
    impl crate::renderer::Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
            Ok([8, 8])
        }
        fn invoke(&self, name: &str, _: &[crate::renderer::GlValue<u32>]) -> Result<crate::renderer::GlValue<u32>> {
            if name.starts_with("create") {
                Ok(crate::renderer::GlValue::Object(1))
            } else if matches!(name, "getShaderParameter" | "getProgramParameter") {
                Ok(crate::renderer::GlValue::Bool(true))
            } else {
                Ok(crate::renderer::GlValue::Null)
            }
        }
    }
}
