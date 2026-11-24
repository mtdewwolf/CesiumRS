//! CesiumJS `Vector3DTilePolylines`: subdivided draped polylines decoded by the
//! `createVectorTilePolylines` worker.
//!
//! Vertices are the pinned five-attribute layout (previous, current and next
//! position, expand-and-width, batch id) drawn as a translucent screen-space
//! polyline. The decoded positions and their offsets can be retained and are
//! returned by [`Vector3DTilePolylines::get_positions`].

use super::{
    cesium_3d_tile_style::Cesium3DTileStyle,
    scene::Result,
    vector_3d_tile_primitive::{BatchTableRef, FeatureList, Vector3DTileFeature},
};
use crate::{
    core::{
        BoundingSphere, Cartesian3, Color, ComponentDatatype, Ellipsoid, Geometry, GeometryAttribute,
        GeometryAttributes, PrimitiveType, Rectangle, RuntimeError, TypedArray,
    },
    renderer::{
        render_state::{DepthTest, PolygonOffset},
        UniformValue, VertexArray,
    },
    shaders::root::{POLYLINE_COMMON, VECTOR3D_TILE_POLYLINES_VS},
    workers::vector_tiles::{
        create_vector_tile_polylines, DecodedVectorPositions, VectorTilePolylines, VectorTilePolylinesParameters,
    },
};
use std::{cell::Cell, rc::Rc};

/// Constructor options for [`Vector3DTilePolylines`].
#[derive(Clone, Default)]
pub struct Vector3DTilePolylinesOptions {
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
    pub bounding_volume: BoundingSphere,
    pub keep_decoded_positions: bool,
}

/// The fragment shader built by `Vector3DTilePolylines.createShaders`.
pub const POLYLINE_FS: &str =
    "uniform vec4 u_highlightColor; \nvoid main()\n{\n    out_FragColor = u_highlightColor;\n}\n";

pub fn polyline_attribute_locations() -> std::collections::BTreeMap<String, u32> {
    std::collections::BTreeMap::from([
        ("previousPosition".to_string(), 0),
        ("currentPosition".to_string(), 1),
        ("nextPosition".to_string(), 2),
        ("expandAndWidth".to_string(), 3),
        ("a_batchId".to_string(), 4),
    ])
}

/// Cesium's `getPolylinePositions` for either polyline primitive.
pub fn get_polyline_positions(
    batch_ids: &[u16],
    decoded: Option<&DecodedVectorPositions>,
    batch_id: u16,
) -> Option<Vec<f64>> {
    let decoded = decoded?;
    if batch_ids.is_empty() {
        return None;
    }
    let mut positions_length = 0usize;
    for (i, &id) in batch_ids.iter().enumerate() {
        if id == batch_id {
            let start = *decoded.offsets.get(i)? as usize;
            let end = *decoded.offsets.get(i + 1)? as usize;
            positions_length += end.saturating_sub(start);
        }
    }
    if positions_length == 0 {
        return None;
    }
    let mut results = Vec::with_capacity(positions_length * 3);
    for (i, &id) in batch_ids.iter().enumerate() {
        if id == batch_id {
            let offset = *decoded.offsets.get(i)? as usize;
            let count = *decoded.offsets.get(i + 1)? as usize - offset;
            for j in 0..count {
                let decoded_offset = (offset + j) * 3;
                results.push(decoded.positions[decoded_offset]);
                results.push(decoded.positions[decoded_offset + 1]);
                results.push(decoded.positions[decoded_offset + 2]);
            }
        }
    }
    Some(results)
}

/// A batch of vector-tile polylines.
pub struct Vector3DTilePolylines<G: crate::renderer::Gl + 'static> {
    positions: Vec<u16>,
    widths: Vec<u16>,
    counts: Vec<u32>,
    batch_ids: Vec<u16>,
    ellipsoid: Ellipsoid,
    minimum_height: f64,
    maximum_height: f64,
    center: Cartesian3,
    rectangle: Rectangle,
    bounding_volume: BoundingSphere,
    batch_table: Option<BatchTableRef>,
    pub keep_decoded_positions: bool,

    decoded_positions: Option<DecodedVectorPositions>,
    pending: Option<VectorTilePolylines>,
    ready: bool,

    pub constant_color: Color,
    pub highlight_color: Color,
    highlight_cell: Rc<Cell<Color>>,
    pub debug_wireframe: bool,
    pub force_rebatch: bool,
    pub classification_type: super::ClassificationType,

    context: Option<crate::renderer::Context<G>>,
    pending_indices: Option<TypedArray>,
    pending_current: Option<PendingPolylineArrays>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    program: Option<Rc<crate::renderer::ShaderProgram<G>>>,
    render_state: crate::renderer::RenderState,
    command: Option<super::scene::SceneCommand>,
    triangles_length: f64,
    geometry_byte_length: usize,
    destroyed: bool,
}

impl<G: crate::renderer::Gl + 'static> Vector3DTilePolylines<G> {
    pub fn new(options: Vector3DTilePolylinesOptions) -> Self {
        Self {
            positions: options.positions,
            widths: options.widths,
            counts: options.counts,
            batch_ids: options.batch_ids,
            ellipsoid: options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
            minimum_height: options.minimum_height,
            maximum_height: options.maximum_height,
            center: options.center.unwrap_or(Cartesian3::ZERO),
            rectangle: options.rectangle,
            bounding_volume: options.bounding_volume,
            batch_table: options.batch_table,
            keep_decoded_positions: options.keep_decoded_positions,
            decoded_positions: None,
            pending: None,
            ready: false,
            constant_color: Color::WHITE,
            highlight_color: Color::WHITE,
            highlight_cell: Rc::new(Cell::new(Color::WHITE)),
            debug_wireframe: false,
            force_rebatch: false,
            classification_type: super::ClassificationType::Both,
            context: None,
            pending_indices: None,
            pending_current: None,
            vertex_array: None,
            program: None,
            render_state: default_render_state(),
            command: None,
            triangles_length: 0.0,
            geometry_byte_length: 0,
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
    pub fn polygon_batch_ids(&self) -> &[u16] {
        &self.batch_ids
    }
    pub fn decoded_positions(&self) -> Option<&DecodedVectorPositions> {
        self.decoded_positions.as_ref()
    }
    pub fn command(&self) -> Option<&super::scene::SceneCommand> {
        self.command.as_ref()
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

    fn create_vertex_array(&mut self) -> Result<()> {
        let packed =
            Self::pack_buffer(&self.rectangle, self.minimum_height, self.maximum_height, &self.ellipsoid, self.center);
        let result = create_vector_tile_polylines(VectorTilePolylinesParameters {
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

    fn finish_vertex_array(&mut self, result: VectorTilePolylines) {
        if self.vertex_array.is_some() {
            return;
        }
        if self.keep_decoded_positions {
            self.decoded_positions = result.decoded.clone();
        }
        let mut bytes = result.previous_positions.len() * 4
            + result.current_positions.len() * 4
            + result.next_positions.len() * 4
            + result.expand_and_width.len() * 4
            + result.batch_ids.len() * 2;
        bytes += match &result.indices {
            TypedArray::Uint16(v) => v.len() * 2,
            TypedArray::Uint32(v) => v.len() * 4,
            other => other.len() * 4,
        };
        self.triangles_length = result.indices.len() as f64 / 3.0;
        self.geometry_byte_length = bytes;
        self.pending_indices = Some(result.indices.clone());
        self.pending_current = Some(PendingPolylineArrays {
            current: result.current_positions,
            previous: result.previous_positions,
            next: result.next_positions,
            expand_and_width: result.expand_and_width,
            batch_ids: result.batch_ids,
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
        let Some(arrays) = self.pending_current.take() else {
            return Ok(());
        };
        let Some(indices) = self.pending_indices.take() else {
            return Ok(());
        };
        let mut attributes = GeometryAttributes::default();
        let floats = |values: &[f32]| values.iter().map(|&v| f64::from(v)).collect::<Vec<_>>();
        attributes.others.push(("previousPosition".into(), GeometryAttribute::floats(3, &floats(&arrays.previous))));
        attributes.others.push(("currentPosition".into(), GeometryAttribute::floats(3, &floats(&arrays.current))));
        attributes.others.push(("nextPosition".into(), GeometryAttribute::floats(3, &floats(&arrays.next))));
        attributes
            .others
            .push(("expandAndWidth".into(), GeometryAttribute::floats(2, &floats(&arrays.expand_and_width))));
        attributes.others.push((
            "a_batchId".into(),
            GeometryAttribute::new(ComponentDatatype::UnsignedShort, 1, TypedArray::Uint16(arrays.batch_ids.clone())),
        ));
        let geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Triangles);
        let va = VertexArray::from_geometry(
            &context,
            &geometry,
            crate::renderer::GeometryUploadOptions {
                buffer_usage: crate::renderer::BufferUsage::StaticDraw,
                attribute_locations: polyline_attribute_locations().into_iter().collect(),
                interleave: false,
                vertex_array_attributes: vec![],
            },
        )?;
        self.vertex_array = Some(Rc::new(va));

        let table = self.batch_table.clone();
        let vs_base = match &table {
            Some(table) => table.borrow().vertex_shader_source(VECTOR3D_TILE_POLYLINES_VS.source),
            None => VECTOR3D_TILE_POLYLINES_VS.source.to_string(),
        };
        let fs_base = match &table {
            Some(table) => table.borrow().fragment_shader_source(POLYLINE_FS),
            None => POLYLINE_FS.to_string(),
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
            attribute_locations: polyline_attribute_locations(),
        };
        let program = context.with_shader_cache(|cache| cache.get_shader_program(&options))?;
        self.program = Some(program);
        Ok(())
    }

    fn ensure_command(&mut self) {
        if self.command.is_some() {
            return;
        }
        let (Some(context), Some(vertex_array), Some(program)) =
            (self.context.clone(), self.vertex_array.clone(), self.program.clone())
        else {
            return;
        };
        let table = self.batch_table.clone();
        let center = self.center;
        let extra = table.as_ref().map(|table| table.borrow().uniforms()).unwrap_or_default();
        let highlight = self.highlight_cell.clone();
        let draw_context = context.clone();
        let uniform_map: crate::renderer::UniformMap = Rc::new(move || {
            let view = draw_context.uniform_state().view;
            let modified = view.set_translation(view.multiply_by_point(center));
            let mut uniforms = extra.clone();
            uniforms.push((
                "u_modifiedModelView".into(),
                UniformValue::Matrix { dimension: 4, values: modified.0.map(|v| v as f32).to_vec() },
            ));
            let color = highlight.get();
            uniforms.push((
                "u_highlightColor".into(),
                UniformValue::Float(vec![color.red as f32, color.green as f32, color.blue as f32, color.alpha as f32]),
            ));
            Ok(uniforms)
        });
        let mut options = super::scene::SceneDrawOptions::new(vertex_array, program);
        options.state = self.render_state.clone();
        options.uniform_map = Some(uniform_map);
        let metadata =
            crate::renderer::DrawMetadata { bounding_volume: Some(self.bounding_volume), ..Default::default() };
        self.command = Some(super::scene::SceneCommand::from_draw(
            "vector-3d-tile-polylines",
            context,
            crate::renderer::Pass::Translucent,
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
        get_polyline_positions(&self.batch_ids, self.decoded_positions.as_ref(), batch_id)
    }

    pub fn update(&mut self, frame: &mut super::FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.ready {
            if self.pending.is_none() && self.vertex_array.is_none() {
                self.create_vertex_array()?;
                return Ok(());
            }
            if let Some(result) = self.pending.take() {
                self.finish_vertex_array(result);
                self.ready = true;
            }
        }
        self.context = frame.context.as_ref().and_then(|c| c.get::<crate::renderer::Context<G>>()).cloned();
        self.ensure_gpu(frame)?;
        self.ensure_command();
        if frame.passes.render || frame.passes.pick {
            if let Some(command) = &self.command {
                frame.command_list.push(command.clone());
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
        self.command = None;
        self.destroyed = true;
        Ok(())
    }
}

struct PendingPolylineArrays {
    current: Vec<f32>,
    previous: Vec<f32>,
    next: Vec<f32>,
    expand_and_width: Vec<f32>,
    batch_ids: Vec<u16>,
}

fn default_render_state() -> crate::renderer::RenderState {
    crate::renderer::RenderState {
        blending: super::blending_state::alpha_blend(),
        depth_mask: false,
        depth_test: DepthTest { enabled: true, func: crate::core::webgl_constants::LEQUAL },
        polygon_offset: PolygonOffset { enabled: true, factor: -5.0, units: -5.0 },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_filter_by_batch_id() {
        let decoded = DecodedVectorPositions {
            positions: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
            offsets: vec![0, 1, 2, 3],
        };
        assert_eq!(get_polyline_positions(&[0, 1, 0], Some(&decoded), 0), Some(vec![0.0, 1.0, 2.0, 6.0, 7.0, 8.0]));
        assert_eq!(get_polyline_positions(&[0, 1], Some(&decoded), 9), None);
        assert_eq!(get_polyline_positions(&[0, 1], None, 0), None);
    }
}
