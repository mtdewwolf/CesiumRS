//! CesiumJS `Vector3DTilePrimitive`: a batch of classification meshes sharing
//! one vertex array, drawn once per colour batch and per mesh for picking.
//!
//! The CPU side ports Cesium's colour re-batching exactly, including the
//! descending `toRgba` sort, the delayed rebatch (`framesSinceLastRebatch`),
//! the per-feature `updateCommands` split, and the vertex-array index rewrite.
//! GPU commands use the crate's `SceneCommand` boundary. The pinned upstream's
//! `Cesium3DTileBatchTable` is not ported, so its shader and colour hooks are a
//! typed [`Vector3DTileBatchTable`] trait here; [`SimpleVector3DTileBatchTable`]
//! supplies the default white colours and base shaders.

use super::{
    cesium_3d_tile_style::Cesium3DTileStyle,
    classification_type::ClassificationType,
    expression::StyleFeature,
    scene::{Result, SceneCommand, SceneDrawOptions},
    stencil_constants::{CESIUM_3D_TILE_MASK, CLASSIFICATION_MASK},
    stencil_function::StencilFunction,
    Vector3DTileBatch,
};
use crate::{
    core::{
        webgl_constants as gl, BoundingSphere, Cartesian3, Color, ComponentDatatype, Geometry, GeometryAttribute,
        GeometryAttributes, Matrix4, PrimitiveType, RuntimeError, TypedArray,
    },
    renderer::{
        render_state::{ColorMask, DepthTest, StencilOperation as StencilOps, StencilTest},
        UniformValue, VertexArray,
    },
    shaders::root::{SHADOW_VOLUME_FS, VECTOR_TILE_VS},
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

/// Batch-table behaviour used by the vector tile primitives. The 3D Tiles core
/// port owns the concrete `Cesium3DTileBatchTable`; this trait is the boundary
/// those primitives need: feature colours, shader decoration, live uniforms and
/// a pick colour.
pub trait Vector3DTileBatchTable {
    fn features_length(&self) -> usize;
    fn batch_table_byte_length(&self) -> usize {
        0
    }
    fn get_color(&self, batch_id: usize) -> Color {
        let _ = batch_id;
        Color::WHITE
    }
    fn get_show(&self, batch_id: usize) -> bool {
        let _ = batch_id;
        true
    }
    fn set_color(&mut self, batch_id: usize, color: Color) {
        let _ = (batch_id, color);
    }
    fn set_show(&mut self, batch_id: usize, show: bool) {
        let _ = (batch_id, show);
    }
    /// Decorates `VectorTileVS`, as `getVertexShaderCallback` does.
    fn vertex_shader_source(&self, source: &str) -> String {
        source.to_string()
    }
    /// Decorates `ShadowVolumeFS` or a polyline fragment shader.
    fn fragment_shader_source(&self, source: &str) -> String {
        source.to_string()
    }
    /// Extra uniforms installed on every command, as `getUniformMapCallback` does.
    fn uniforms(&self) -> Vec<(String, UniformValue)> {
        Vec::new()
    }
    /// The fragment expression written into the pick shader, e.g. `czm_pickColor`.
    fn pick_color_expression(&self) -> Option<String> {
        None
    }
    fn has_property(&self, _batch_id: usize, _name: &str) -> bool {
        false
    }
    fn property(&self, _batch_id: usize, _name: &str) -> Option<serde_json::Value> {
        None
    }
    fn property_names(&self) -> Vec<String> {
        Vec::new()
    }
    fn update(&mut self, _frame: &mut super::FrameState) -> Result<()> {
        Ok(())
    }
}

/// The no-batch-table baseline: white, shown features and the pinned shaders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimpleVector3DTileBatchTable {
    colors: Vec<Color>,
    shows: Vec<bool>,
    properties: BTreeMap<String, Vec<serde_json::Value>>,
    pick_color_expression: Option<String>,
    batch_table_byte_length: usize,
}

impl SimpleVector3DTileBatchTable {
    pub fn new(features_length: usize) -> Self {
        Self { colors: vec![Color::WHITE; features_length], shows: vec![true; features_length], ..Default::default() }
    }
    pub fn with_colors(colors: Vec<Color>) -> Self {
        let shows = vec![true; colors.len()];
        Self { colors, shows, ..Default::default() }
    }
    pub fn set_pick_color_expression(&mut self, expression: Option<String>) {
        self.pick_color_expression = expression;
    }
    pub fn set_batch_table_byte_length(&mut self, bytes: usize) {
        self.batch_table_byte_length = bytes;
    }
    /// Replaces the JSON property arrays decoded from a batch table.
    pub fn set_properties(&mut self, properties: BTreeMap<String, Vec<serde_json::Value>>) {
        self.properties = properties;
    }
}

impl Vector3DTileBatchTable for SimpleVector3DTileBatchTable {
    fn features_length(&self) -> usize {
        self.colors.len()
    }
    fn batch_table_byte_length(&self) -> usize {
        self.batch_table_byte_length
    }
    fn get_color(&self, batch_id: usize) -> Color {
        self.colors.get(batch_id).copied().unwrap_or(Color::WHITE)
    }
    fn get_show(&self, batch_id: usize) -> bool {
        self.shows.get(batch_id).copied().unwrap_or(true)
    }
    fn set_color(&mut self, batch_id: usize, color: Color) {
        if batch_id < self.colors.len() {
            self.colors[batch_id] = color;
        }
    }
    fn set_show(&mut self, batch_id: usize, show: bool) {
        if batch_id < self.shows.len() {
            self.shows[batch_id] = show;
        }
    }
    fn pick_color_expression(&self) -> Option<String> {
        self.pick_color_expression.clone()
    }
    fn has_property(&self, batch_id: usize, name: &str) -> bool {
        self.properties.get(name).is_some_and(|values| batch_id < values.len())
    }
    fn property(&self, batch_id: usize, name: &str) -> Option<serde_json::Value> {
        self.properties.get(name).and_then(|values| values.get(batch_id)).cloned()
    }
    fn property_names(&self) -> Vec<String> {
        self.properties.keys().cloned().collect()
    }
}

/// A feature created for a mesh or polyline batch id.
#[derive(Clone, Debug)]
pub struct Vector3DTileFeature {
    pub batch_id: usize,
    pub show: bool,
    pub color: Color,
    pub properties: BTreeMap<String, super::expression::StyleValue>,
    /// Present for point features; meshes and polylines leave it empty.
    pub point: Option<super::vector_3d_tile_points::Vector3DTilePointFeature>,
}

impl StyleFeature for Vector3DTileFeature {
    fn property_inherited(&self, name: &str) -> Option<super::expression::StyleValue> {
        self.properties.get(name).cloned()
    }
}

impl Vector3DTileFeature {
    pub fn new(batch_id: usize) -> Self {
        Self { batch_id, show: true, color: Color::WHITE, properties: BTreeMap::new(), point: None }
    }
}

/// Per-mesh bounds. Geometry uses spheres and polygons oriented boxes, as the
/// pinned workers return.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Vector3DTileBoundingVolume {
    Sphere(BoundingSphere),
    Box(crate::core::OrientedBoundingBox),
}

impl Vector3DTileBoundingVolume {
    fn apply(&self, metadata: &mut crate::renderer::DrawMetadata) {
        match self {
            Self::Sphere(sphere) => metadata.bounding_volume = Some(*sphere),
            Self::Box(oriented) => metadata.oriented_bounding_box = Some(*oriented),
        }
    }
}

pub type BatchTableRef = Rc<RefCell<dyn Vector3DTileBatchTable>>;
pub type FeatureList = Vec<Option<Vector3DTileFeature>>;

/// Constructor options for [`Vector3DTilePrimitive`].
#[derive(Clone)]
pub struct Vector3DTilePrimitiveOptions {
    pub positions: Vec<f32>,
    pub vertex_batch_ids: Vec<u16>,
    pub indices: TypedArray,
    pub index_counts: Vec<u32>,
    pub index_offsets: Vec<u32>,
    pub batched_indices: Vec<Vector3DTileBatch>,
    pub batch_ids: Vec<u16>,
    pub bounding_volume: BoundingSphere,
    pub bounding_volumes: Option<Vec<Vector3DTileBoundingVolume>>,
    pub center: Cartesian3,
    pub classification_type: ClassificationType,
    pub batch_table: Option<BatchTableRef>,
    pub debug_wireframe: bool,
    pub force_rebatch: bool,
    pub vertex_shader_source: Option<String>,
    pub fragment_shader_source: Option<String>,
    pub attribute_locations: Option<BTreeMap<String, u32>>,
    pub model_matrix: Option<Matrix4>,
    pub pick_color_expression: Option<String>,
    pub extra_uniforms: Vec<(String, UniformValue)>,
}

impl Default for Vector3DTilePrimitiveOptions {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            vertex_batch_ids: Vec::new(),
            indices: TypedArray::Uint16(Vec::new()),
            index_counts: Vec::new(),
            index_offsets: Vec::new(),
            batched_indices: Vec::new(),
            batch_ids: Vec::new(),
            bounding_volume: BoundingSphere::new(Cartesian3::ZERO, 0.0),
            bounding_volumes: None,
            center: Cartesian3::ZERO,
            classification_type: ClassificationType::Both,
            batch_table: None,
            debug_wireframe: false,
            force_rebatch: false,
            vertex_shader_source: None,
            fragment_shader_source: None,
            attribute_locations: None,
            model_matrix: None,
            pick_color_expression: None,
            extra_uniforms: Vec::new(),
        }
    }
}

/// A queued draw, exposed for structural validation without executing GPU work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vector3DTileCommandRecord {
    pub pass: crate::renderer::Pass,
    pub offset: usize,
    pub count: usize,
    pub wireframe: bool,
    pub pick: bool,
}

fn index_bytes_per_element(indices: &TypedArray) -> usize {
    match indices {
        TypedArray::Uint16(_) => 2,
        TypedArray::Uint32(_) => 4,
        TypedArray::Uint8(_) => 1,
        TypedArray::Int8(_) => 1,
        TypedArray::Int16(_) => 2,
        TypedArray::Int32(_) => 4,
        TypedArray::Float32(_) => 4,
        TypedArray::Float64(_) => 8,
    }
}

fn indices_len(indices: &TypedArray) -> usize {
    indices.len()
}

fn copy_index_range(indices: &TypedArray, offset: usize, count: usize) -> Vec<f64> {
    (offset..offset + count).map(|i| indices.get(i).unwrap_or(0.0)).collect()
}

fn write_indices(target: &mut TypedArray, values: &[f64], start: usize) {
    fn put<V: Copy>(vec: &mut [V], start: usize, values: &[f64], convert: impl Fn(f64) -> V) {
        for (i, value) in values.iter().enumerate() {
            if let Some(slot) = vec.get_mut(start + i) {
                *slot = convert(*value);
            }
        }
    }
    match target {
        TypedArray::Uint16(v) => put(v, start, values, |x| x as u16),
        TypedArray::Uint32(v) => put(v, start, values, |x| x as u32),
        TypedArray::Uint8(v) => put(v, start, values, |x| x as u8),
        TypedArray::Int8(v) => put(v, start, values, |x| x as i8),
        TypedArray::Int16(v) => put(v, start, values, |x| x as i16),
        TypedArray::Int32(v) => put(v, start, values, |x| x as i32),
        TypedArray::Float32(v) => put(v, start, values, |x| x as f32),
        TypedArray::Float64(v) => put(v, start, values, |x| x),
    }
}

/// Cesium's `VectorTileVS` built by a backed shader program cache.
pub fn vector_tile_vertex_shader(batch_table: Option<&BatchTableRef>) -> String {
    let base = VECTOR_TILE_VS.source;
    match batch_table {
        Some(table) => table.borrow().vertex_shader_source(base),
        None => base.to_string(),
    }
}

/// Cesium's `ShadowVolumeFS` plus the `VECTOR_TILE` define.
pub fn vector_tile_fragment_shader(batch_table: Option<&BatchTableRef>) -> String {
    let base = SHADOW_VOLUME_FS.source;
    match batch_table {
        Some(table) => table.borrow().fragment_shader_source(base),
        None => base.to_string(),
    }
}

fn get_stencil_depth_render_state(mask_3d_tiles: bool) -> crate::renderer::RenderState {
    let stencil_function = if mask_3d_tiles {
        StencilFunction::Equal.value()
    } else {
        StencilFunction::Always.value()
    };
    crate::renderer::RenderState {
        color_mask: ColorMask { red: false, green: false, blue: false, alpha: false },
        stencil_test: StencilTest {
            enabled: true,
            front_function: stencil_function,
            front_operation: StencilOps {
                fail: super::stencil_operation::StencilOperation::Keep.value(),
                z_fail: super::stencil_operation::StencilOperation::DecrementWrap.value(),
                z_pass: super::stencil_operation::StencilOperation::Keep.value(),
            },
            back_function: stencil_function,
            back_operation: StencilOps {
                fail: super::stencil_operation::StencilOperation::Keep.value(),
                z_fail: super::stencil_operation::StencilOperation::IncrementWrap.value(),
                z_pass: super::stencil_operation::StencilOperation::Keep.value(),
            },
            reference: CESIUM_3D_TILE_MASK,
            mask: i64::from(CESIUM_3D_TILE_MASK),
        },
        stencil_mask: i64::from(CLASSIFICATION_MASK),
        depth_test: DepthTest { enabled: true, func: gl::LEQUAL },
        depth_mask: false,
        ..Default::default()
    }
}

fn classification_stencil() -> StencilTest {
    StencilTest {
        enabled: true,
        front_function: StencilFunction::NotEqual.value(),
        front_operation: StencilOps {
            fail: super::stencil_operation::StencilOperation::Zero.value(),
            z_fail: super::stencil_operation::StencilOperation::Zero.value(),
            z_pass: super::stencil_operation::StencilOperation::Zero.value(),
        },
        back_function: StencilFunction::NotEqual.value(),
        back_operation: StencilOps {
            fail: super::stencil_operation::StencilOperation::Zero.value(),
            z_fail: super::stencil_operation::StencilOperation::Zero.value(),
            z_pass: super::stencil_operation::StencilOperation::Zero.value(),
        },
        reference: 0,
        mask: i64::from(CLASSIFICATION_MASK),
    }
}

/// The colour-pass render state, including pre-multiplied blending.
pub fn color_render_state() -> crate::renderer::RenderState {
    crate::renderer::RenderState {
        stencil_test: classification_stencil(),
        stencil_mask: i64::from(CLASSIFICATION_MASK),
        depth_test: DepthTest { enabled: false, ..Default::default() },
        depth_mask: false,
        blending: super::blending_state::pre_multiplied_alpha_blend(),
        ..Default::default()
    }
}

/// The pick render state: the colour state without blending.
pub fn pick_render_state() -> crate::renderer::RenderState {
    crate::renderer::RenderState {
        stencil_test: classification_stencil(),
        stencil_mask: i64::from(CLASSIFICATION_MASK),
        depth_test: DepthTest { enabled: false, ..Default::default() },
        depth_mask: false,
        ..Default::default()
    }
}

/// A batch of classification meshes.
pub struct Vector3DTilePrimitive<G: crate::renderer::Gl + 'static> {
    batch_table: Option<BatchTableRef>,
    batch_ids: Vec<u16>,

    pub positions: Vec<f32>,
    pub vertex_batch_ids: Vec<u16>,
    pub indices: TypedArray,
    pub index_counts: Vec<u32>,
    pub index_offsets: Vec<u32>,
    pub batched_indices: Vec<Vector3DTileBatch>,

    pub bounding_volume: BoundingSphere,
    pub bounding_volumes: Option<Vec<Vector3DTileBoundingVolume>>,
    pub center: Cartesian3,

    pub debug_wireframe: bool,
    pub force_rebatch: bool,
    pub classification_type: ClassificationType,
    pub force_pick_commands: bool,

    vertex_shader_source: Option<String>,
    fragment_shader_source: Option<String>,
    attribute_locations: BTreeMap<String, u32>,
    model_matrix: Matrix4,
    pick_color_expression: Option<String>,
    extra_uniforms: Vec<(String, UniformValue)>,

    pub constant_color: Color,
    pub highlight_color: Color,
    highlight_cell: Rc<Cell<Color>>,

    batch_dirty: bool,
    pick_commands_dirty: bool,
    frames_since_last_rebatch: u32,
    updating_all_commands: bool,
    triangles_length: f64,
    geometry_byte_length: usize,
    batch_id_lookup: HashMap<u16, usize>,

    built_debug_wireframe: bool,
    wireframe_dirty: bool,
    commands_dirty: bool,

    context: Option<crate::renderer::Context<G>>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    color_render_state: crate::renderer::RenderState,
    stencil_3dtiles_render_state: crate::renderer::RenderState,
    pick_render_state: crate::renderer::RenderState,
    wireframe_render_state: crate::renderer::RenderState,
    program: Option<Rc<crate::renderer::ShaderProgram<G>>>,
    program_stencil: Option<Rc<crate::renderer::ShaderProgram<G>>>,
    program_pick: Option<Rc<crate::renderer::ShaderProgram<G>>>,

    color_commands: Vec<SceneCommand>,
    color_commands_3d_tiles: Vec<SceneCommand>,
    pick_commands: Vec<SceneCommand>,
    pick_commands_3d_tiles: Vec<SceneCommand>,
    ignore_show_commands: Vec<SceneCommand>,
    pub command_records: Vec<Vector3DTileCommandRecord>,
    pub pick_command_records: Vec<Vector3DTileCommandRecord>,

    destroyed: bool,
}

impl<G: crate::renderer::Gl + 'static> Vector3DTilePrimitive<G> {
    pub fn new(options: Vector3DTilePrimitiveOptions) -> Self {
        let triangles_length = indices_len(&options.indices) as f64 / 3.0;
        let geometry_byte_length = indices_len(&options.indices) * index_bytes_per_element(&options.indices)
            + options.positions.len() * 4
            + options.vertex_batch_ids.len() * 2;
        let mut batch_id_lookup = HashMap::new();
        for (i, &batch_id) in options.batch_ids.iter().enumerate() {
            batch_id_lookup.insert(batch_id, i);
        }
        let attribute_locations = options
            .attribute_locations
            .clone()
            .unwrap_or_else(|| BTreeMap::from([("position".to_string(), 0), ("a_batchId".to_string(), 1)]));

        Self {
            batch_table: options.batch_table,
            batch_ids: options.batch_ids,
            positions: options.positions,
            vertex_batch_ids: options.vertex_batch_ids,
            indices: options.indices,
            index_counts: options.index_counts,
            index_offsets: options.index_offsets,
            batched_indices: options.batched_indices,
            bounding_volume: options.bounding_volume,
            bounding_volumes: options.bounding_volumes,
            center: options.center,
            debug_wireframe: options.debug_wireframe,
            force_rebatch: options.force_rebatch,
            classification_type: options.classification_type,
            force_pick_commands: false,
            vertex_shader_source: options.vertex_shader_source,
            fragment_shader_source: options.fragment_shader_source,
            attribute_locations,
            model_matrix: options.model_matrix.unwrap_or(Matrix4::IDENTITY),
            pick_color_expression: options.pick_color_expression,
            extra_uniforms: options.extra_uniforms,
            constant_color: Color::WHITE,
            highlight_color: Color::WHITE,
            highlight_cell: Rc::new(Cell::new(Color::WHITE)),
            batch_dirty: true,
            pick_commands_dirty: true,
            frames_since_last_rebatch: 0,
            updating_all_commands: false,
            triangles_length,
            geometry_byte_length,
            batch_id_lookup,
            built_debug_wireframe: false,
            wireframe_dirty: false,
            commands_dirty: true,
            context: None,
            vertex_array: None,
            color_render_state: color_render_state(),
            stencil_3dtiles_render_state: get_stencil_depth_render_state(true),
            pick_render_state: pick_render_state(),
            wireframe_render_state: crate::renderer::RenderState::default(),
            program: None,
            program_stencil: None,
            program_pick: None,
            color_commands: Vec::new(),
            color_commands_3d_tiles: Vec::new(),
            pick_commands: Vec::new(),
            pick_commands_3d_tiles: Vec::new(),
            ignore_show_commands: Vec::new(),
            command_records: Vec::new(),
            pick_command_records: Vec::new(),
            destroyed: false,
        }
    }

    pub fn triangles_length(&self) -> f64 {
        self.triangles_length
    }
    pub fn geometry_byte_length(&self) -> usize {
        self.geometry_byte_length
    }
    pub fn batch_table(&self) -> Option<&BatchTableRef> {
        self.batch_table.as_ref()
    }
    pub fn model_matrix(&self) -> Matrix4 {
        self.model_matrix
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Cesium's `createFeatures`: one feature per batch id, placed at that id.
    pub fn create_features(&self, features: &mut FeatureList) {
        for &batch_id in &self.batch_ids {
            let index = batch_id as usize;
            if features.len() <= index {
                features.resize(index + 1, None);
            }
            features[index] = Some(Vector3DTileFeature::new(index));
        }
    }

    /// Cesium's `applyDebugSettings`.
    pub fn apply_debug_settings(&mut self, enabled: bool, color: Color) {
        self.highlight_color = if enabled { color } else { self.constant_color };
        self.highlight_cell.set(self.highlight_color);
    }

    /// Cesium's `applyStyle`.
    pub fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>, features: &mut FeatureList) -> Result<()> {
        let Some(style) = style else {
            self.clear_style(features);
            return Ok(());
        };
        let color_expression = style.color();
        let is_simple_style = color_expression.is_some_and(|expression| {
            matches!(expression.json_value(), serde_json::Value::String(source) if !source.contains('$'))
        });
        self.updating_all_commands = is_simple_style;

        for &batch_id in &self.batch_ids {
            let index = batch_id as usize;
            let Some(Some(feature)) = features.get_mut(index) else {
                continue;
            };
            let (color, show) = {
                let style_feature: &dyn StyleFeature = feature;
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

        if is_simple_style {
            for batch in &mut self.batched_indices {
                batch.color = Color::WHITE;
            }
            self.updating_all_commands = false;
            self.batch_dirty = true;
        }
        Ok(())
    }

    fn clear_style(&mut self, features: &mut FeatureList) {
        self.updating_all_commands = true;
        for &batch_id in &self.batch_ids {
            let index = batch_id as usize;
            if let Some(Some(feature)) = features.get_mut(index) {
                feature.show = true;
                feature.color = Color::WHITE;
            }
        }
        for batch in &mut self.batched_indices {
            batch.color = Color::WHITE;
        }
        self.updating_all_commands = false;
        self.batch_dirty = true;
    }

    /// Cesium's `updateCommands`: split a batch around a changed feature.
    pub fn update_commands(&mut self, batch_id: u16, color: Color) {
        if self.updating_all_commands {
            return;
        }
        let Some(&index) = self.batch_id_lookup.get(&batch_id) else {
            return;
        };
        let Some(&offset) = self.index_offsets.get(index) else {
            return;
        };
        let Some(&count) = self.index_counts.get(index) else {
            return;
        };

        let mut found = None;
        for (i, batch) in self.batched_indices.iter().enumerate() {
            if offset as usize >= batch.offset && (offset as usize) < batch.offset + batch.count {
                found = Some(i);
                break;
            }
        }

        self.batched_indices.push(Vector3DTileBatch {
            offset: offset as usize,
            count: count as usize,
            color,
            batch_ids: vec![u32::from(batch_id)],
        });
        let i = found.unwrap_or(self.batched_indices.len() - 1);

        let mut start_ids = Vec::new();
        let mut end_ids = Vec::new();
        let existing = self.batched_indices[i].clone();
        for &id in &existing.batch_ids {
            if id == u32::from(batch_id) {
                continue;
            }
            let Some(&offset_index) = self.batch_id_lookup.get(&(id as u16)) else {
                continue;
            };
            if self.index_offsets.get(offset_index).copied().unwrap_or(0) < offset {
                start_ids.push(id);
            } else {
                end_ids.push(id);
            }
        }

        if !end_ids.is_empty() {
            self.batched_indices.push(Vector3DTileBatch {
                offset: offset as usize + count as usize,
                count: existing.offset + existing.count - (offset as usize + count as usize),
                color: existing.color,
                batch_ids: end_ids,
            });
        }

        if !start_ids.is_empty() {
            self.batched_indices[i].count = offset as usize - existing.offset;
            self.batched_indices[i].batch_ids = start_ids;
        } else {
            self.batched_indices.remove(i);
        }

        self.batch_dirty = true;
    }

    /// Cesium's `rebatchCommands`, including the 120-frame delay.
    pub fn rebatch_commands(&mut self) -> bool {
        if !self.batch_dirty {
            return false;
        }
        let mut seen = HashMap::new();
        let mut need_to_rebatch = false;
        for batch in &self.batched_indices {
            if seen.insert(batch.color.to_rgba(), ()).is_some() {
                need_to_rebatch = true;
                break;
            }
        }
        if !need_to_rebatch {
            self.batch_dirty = false;
            return false;
        }
        if !self.force_rebatch && self.frames_since_last_rebatch < 120 {
            self.frames_since_last_rebatch += 1;
            return false;
        }

        self.batched_indices.sort_by_key(|b| std::cmp::Reverse(b.color.to_rgba()));
        self.rebatch_cpu();
        self.frames_since_last_rebatch = 0;
        self.batch_dirty = false;
        self.pick_commands_dirty = true;
        self.wireframe_dirty = true;
        true
    }

    /// The CPU index rewrite of `rebatchCPU`, also used to model the WebGL2
    /// buffer-to-buffer variant because both produce identical index arrays.
    fn rebatch_cpu(&mut self) {
        let indices = self.indices.clone();
        let mut new_indices = indices.clone();
        let mut current = self.batched_indices.pop();
        let Some(mut current) = current.take() else {
            return;
        };
        let mut new_batched = vec![current.clone()];
        let mut current_offset =
            self.copy_indices_cpu(&indices, &mut new_indices, 0, &current.batch_ids);
        current.offset = 0;
        current.count = current_offset;

        while let Some(mut next) = self.batched_indices.pop() {
            if next.color == current.color {
                current_offset = self.copy_indices_cpu(&indices, &mut new_indices, current_offset, &next.batch_ids);
                current.batch_ids.extend(next.batch_ids);
                current.count = current_offset - current.offset;
                *new_batched.last_mut().expect("current batch") = current.clone();
            } else {
                let offset = current_offset;
                current_offset = self.copy_indices_cpu(&indices, &mut new_indices, current_offset, &next.batch_ids);
                next.offset = offset;
                next.count = current_offset - offset;
                new_batched.push(next.clone());
                current = next;
            }
        }

        self.indices = new_indices;
        self.batched_indices = new_batched;
    }

    fn copy_indices_cpu(
        &mut self,
        indices: &TypedArray,
        new_indices: &mut TypedArray,
        mut current_offset: usize,
        batch_ids: &[u32],
    ) -> usize {
        for &id in batch_ids {
            let Some(&index) = self.batch_id_lookup.get(&(id as u16)) else {
                continue;
            };
            let offset = self.index_offsets.get(index).copied().unwrap_or(0) as usize;
            let count = self.index_counts.get(index).copied().unwrap_or(0) as usize;
            let values = copy_index_range(indices, offset, count);
            write_indices(new_indices, &values, current_offset);
            if let Some(slot) = self.index_offsets.get_mut(index) {
                *slot = current_offset as u32;
            }
            current_offset += count;
        }
        current_offset
    }

    fn context_of(frame: &super::FrameState) -> Result<crate::renderer::Context<G>> {
        frame
            .context
            .as_ref()
            .and_then(|c| c.get::<crate::renderer::Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("frameState.context is required."))
    }

    fn ensure_gpu(&mut self, frame: &super::FrameState) -> Result<()> {
        if self.vertex_array.is_some() && self.program.is_some() {
            return Ok(());
        }
        let context = Self::context_of(frame)?;
        self.context = Some(context.clone());

        if self.vertex_array.is_none() {
            let mut attributes = GeometryAttributes {
                position: Some(GeometryAttribute::floats(
                    3,
                    &self.positions.iter().map(|&v| f64::from(v)).collect::<Vec<_>>(),
                )),
                ..Default::default()
            };
            attributes.others.push((
                "a_batchId".into(),
                GeometryAttribute::new(
                    ComponentDatatype::UnsignedShort,
                    1,
                    TypedArray::Uint16(self.vertex_batch_ids.clone()),
                ),
            ));
            let geometry = Geometry::new(attributes, Some(self.indices.clone()), PrimitiveType::Triangles);
            let va = VertexArray::from_geometry(
                &context,
                &geometry,
                crate::renderer::GeometryUploadOptions {
                    buffer_usage: crate::renderer::BufferUsage::DynamicDraw,
                    attribute_locations: self.attribute_locations.clone().into_iter().collect(),
                    interleave: false,
                    vertex_array_attributes: vec![],
                },
            )?;
            self.vertex_array = Some(Rc::new(va));
        }

        if self.program.is_none() {
            let table = self.batch_table.clone();
            let (vertex_base, fragment_base) = match (&self.vertex_shader_source, &self.fragment_shader_source) {
                (Some(vs), Some(fs)) => (vs.clone(), fs.clone()),
                _ => (vector_tile_vertex_shader(table.as_ref()), vector_tile_fragment_shader(table.as_ref())),
            };
            let mut fragment = crate::renderer::ShaderSource {
                defines: vec!["VECTOR_TILE".into()],
                sources: vec![fragment_base.clone()],
                ..Default::default()
            };
            if self.vertex_shader_source.is_some() {
                fragment.defines.clear();
            }
            let vs = crate::renderer::ShaderSource { sources: vec![vertex_base.clone()], ..Default::default() };
            let options = crate::renderer::ShaderProgramOptions {
                vertex_shader_source: vs,
                fragment_shader_source: fragment,
                attribute_locations: self.attribute_locations.clone(),
            };
            self.program = Some(context.with_shader_cache(|cache| cache.get_shader_program(&options))?);

            if self.vertex_shader_source.is_some() {
                self.program_stencil = self.program.clone();
            } else {
                let stencil_vs =
                    crate::renderer::ShaderSource { sources: vec![VECTOR_TILE_VS.source.into()], ..Default::default() };
                let stencil_fs = crate::renderer::ShaderSource {
                    defines: vec!["VECTOR_TILE".into()],
                    sources: vec![SHADOW_VOLUME_FS.source.into()],
                    ..Default::default()
                };
                let options = crate::renderer::ShaderProgramOptions {
                    vertex_shader_source: stencil_vs,
                    fragment_shader_source: stencil_fs,
                    attribute_locations: self.attribute_locations.clone(),
                };
                self.program_stencil = Some(context.with_shader_cache(|cache| cache.get_shader_program(&options))?);
            }

            let pick_expression = self
                .pick_color_expression
                .clone()
                .or_else(|| table.as_ref().and_then(|table| table.borrow().pick_color_expression()));
            if let Some(pick_expression) = pick_expression {
                let pick_fragment = format!(
                    "{}\nvoid main() \n{{ \n    czm_non_pick_main(); \n    out_FragColor = {pick_expression}; \n}} \n",
                    crate::renderer::ShaderSource::replace_main(&fragment_base, "czm_non_pick_main")
                );
                let vs =
                    crate::renderer::ShaderSource { sources: vec![vertex_base], ..Default::default() };
                let fs = crate::renderer::ShaderSource {
                    defines: vec!["VECTOR_TILE".into()],
                    sources: vec![pick_fragment],
                    ..Default::default()
                };
                let options = crate::renderer::ShaderProgramOptions {
                    vertex_shader_source: vs,
                    fragment_shader_source: fs,
                    attribute_locations: self.attribute_locations.clone(),
                };
                self.program_pick = Some(context.with_shader_cache(|cache| cache.get_shader_program(&options))?);
            }
        }
        Ok(())
    }

    fn uniform_map(&self, context: crate::renderer::Context<G>) -> crate::renderer::UniformMap {
        let table = self.batch_table.clone();
        let center = self.center;
        let highlight = self.highlight_cell.clone();
        let extra = self.extra_uniforms.clone();
        Rc::new(move || {
            let mut uniforms = extra.clone();
            let view = context.uniform_state().view;
            let projection = context.uniform_state().projection;
            let modified = view.set_translation(view.multiply_by_point(center));
            let mvp = projection.multiply(&modified);
            uniforms.push((
                "u_modifiedModelViewProjection".into(),
                UniformValue::Matrix { dimension: 4, values: mvp.0.map(|v| v as f32).to_vec() },
            ));
            let color = highlight.get();
            uniforms.push((
                "u_highlightColor".into(),
                UniformValue::Float(vec![
                    color.red as f32,
                    color.green as f32,
                    color.blue as f32,
                    color.alpha as f32,
                ]),
            ));
            if let Some(table) = &table {
                uniforms.extend(table.borrow().uniforms());
            }
            Ok(uniforms)
        })
    }

    fn metadata(&self, bounding_volume: BoundingSphere) -> crate::renderer::DrawMetadata {
        crate::renderer::DrawMetadata { bounding_volume: Some(bounding_volume), ..Default::default() }
    }

    fn mesh_metadata(&self, index: usize) -> crate::renderer::DrawMetadata {
        let mut metadata = self.metadata(self.bounding_volume);
        if let Some(volumes) = &self.bounding_volumes {
            if let Some(volume) = volumes.get(index) {
                volume.apply(&mut metadata);
            }
        }
        metadata
    }

    fn build_commands(&mut self, context: crate::renderer::Context<G>) -> Result<()> {
        let Some(vertex_array) = self.vertex_array.clone() else {
            return Ok(());
        };
        let Some(program) = self.program.clone() else {
            return Ok(());
        };
        let wireframe = self.debug_wireframe;
        let primitive_type = if wireframe { PrimitiveType::Lines } else { PrimitiveType::Triangles };
        let color_state = if wireframe { self.wireframe_render_state.clone() } else { self.color_render_state.clone() };

        self.color_commands.clear();
        self.color_commands_3d_tiles.clear();
        self.command_records.clear();
        let uniform_map = self.uniform_map(context.clone());
        for batch in &self.batched_indices {
            let offset = batch.offset;
            let count = batch.count;
            let metadata = self.metadata(self.bounding_volume);

            let mut stencil_options = SceneDrawOptions::new(vertex_array.clone(), program.clone());
            stencil_options.state = get_stencil_depth_render_state(false);
            stencil_options.primitive_type = PrimitiveType::Triangles;
            stencil_options.offset = offset;
            stencil_options.count = Some(count);
            stencil_options.uniform_map = Some(uniform_map.clone());
            self.color_commands.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-stencil",
                context.clone(),
                crate::renderer::Pass::TerrainClassification,
                metadata.clone(),
                stencil_options,
            ));
            self.command_records.push(Vector3DTileCommandRecord {
                pass: crate::renderer::Pass::TerrainClassification,
                offset,
                count,
                wireframe: false,
                pick: false,
            });

            let mut stencil_options = SceneDrawOptions::new(vertex_array.clone(), program.clone());
            stencil_options.state = self.stencil_3dtiles_render_state.clone();
            stencil_options.primitive_type = PrimitiveType::Triangles;
            stencil_options.offset = offset;
            stencil_options.count = Some(count);
            stencil_options.uniform_map = Some(uniform_map.clone());
            self.color_commands_3d_tiles.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-stencil-3dtiles",
                context.clone(),
                crate::renderer::Pass::Cesium3DTileClassification,
                metadata.clone(),
                stencil_options,
            ));

            let mut color_options = SceneDrawOptions::new(vertex_array.clone(), program.clone());
            color_options.state = color_state.clone();
            color_options.primitive_type = primitive_type;
            color_options.offset = offset;
            color_options.count = Some(count);
            color_options.uniform_map = Some(uniform_map.clone());
            self.color_commands.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-color",
                context.clone(),
                crate::renderer::Pass::TerrainClassification,
                metadata.clone(),
                color_options,
            ));
            self.command_records.push(Vector3DTileCommandRecord {
                pass: crate::renderer::Pass::TerrainClassification,
                offset,
                count,
                wireframe,
                pick: false,
            });
            let mut color_options_3d_tiles = SceneDrawOptions::new(vertex_array.clone(), program.clone());
            color_options_3d_tiles.state = color_state.clone();
            color_options_3d_tiles.primitive_type = primitive_type;
            color_options_3d_tiles.offset = offset;
            color_options_3d_tiles.count = Some(count);
            color_options_3d_tiles.uniform_map = Some(uniform_map.clone());
            self.color_commands_3d_tiles.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-color-3dtiles",
                context.clone(),
                crate::renderer::Pass::Cesium3DTileClassification,
                metadata,
                color_options_3d_tiles,
            ));
        }

        self.commands_dirty = false;
        self.pick_commands_dirty = true;
        self.built_debug_wireframe = wireframe;
        self.wireframe_dirty = false;
        Ok(())
    }

    fn build_ignore_show_commands(&mut self, frame: &super::FrameState, context: crate::renderer::Context<G>) {
        self.ignore_show_commands.clear();
        if self.classification_type == ClassificationType::Terrain || !frame.invert_classification {
            return;
        }
        let (Some(vertex_array), Some(program_stencil)) = (self.vertex_array.clone(), self.program_stencil.clone()) else {
            return;
        };
        let uniform_map = self.uniform_map(context.clone());
        for batch in &self.batched_indices {
            let mut options = SceneDrawOptions::new(vertex_array.clone(), program_stencil.clone());
            options.state = self.color_render_state.clone();
            options.primitive_type = PrimitiveType::Triangles;
            options.offset = batch.offset;
            options.count = Some(batch.count);
            options.uniform_map = Some(uniform_map.clone());
            self.ignore_show_commands.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-ignore-show",
                context.clone(),
                crate::renderer::Pass::Cesium3DTileClassificationIgnoreShow,
                self.metadata(self.bounding_volume),
                options,
            ));
        }
    }

    fn build_pick_commands(&mut self, context: crate::renderer::Context<G>) -> Result<()> {
        if !self.pick_commands_dirty {
            return Ok(());
        }
        self.pick_commands.clear();
        self.pick_commands_3d_tiles.clear();
        self.pick_command_records.clear();
        let (Some(vertex_array), Some(program_stencil), Some(program_pick)) =
            (self.vertex_array.clone(), self.program_stencil.clone(), self.program_pick.clone())
        else {
            return Ok(());
        };
        let uniform_map = self.uniform_map(context.clone());
        for (j, _) in self.index_offsets.iter().enumerate() {
            let offset = self.index_offsets.get(j).copied().unwrap_or(0) as usize;
            let count = self.index_counts.get(j).copied().unwrap_or(0) as usize;
            let metadata = self.mesh_metadata(j);

            let mut stencil_options = SceneDrawOptions::new(vertex_array.clone(), program_stencil.clone());
            stencil_options.state = get_stencil_depth_render_state(false);
            stencil_options.offset = offset;
            stencil_options.count = Some(count);
            stencil_options.uniform_map = Some(uniform_map.clone());
            self.pick_commands.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-pick-stencil",
                context.clone(),
                crate::renderer::Pass::TerrainClassification,
                metadata.clone(),
                stencil_options,
            ));
            self.pick_command_records.push(Vector3DTileCommandRecord {
                pass: crate::renderer::Pass::TerrainClassification,
                offset,
                count,
                wireframe: false,
                pick: true,
            });

            let mut stencil_options = SceneDrawOptions::new(vertex_array.clone(), program_stencil.clone());
            stencil_options.state = self.stencil_3dtiles_render_state.clone();
            stencil_options.offset = offset;
            stencil_options.count = Some(count);
            stencil_options.uniform_map = Some(uniform_map.clone());
            self.pick_commands_3d_tiles.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-pick-stencil-3dtiles",
                context.clone(),
                crate::renderer::Pass::Cesium3DTileClassification,
                metadata.clone(),
                stencil_options,
            ));

            let mut color_options = SceneDrawOptions::new(vertex_array.clone(), program_pick.clone());
            color_options.state = self.pick_render_state.clone();
            color_options.offset = offset;
            color_options.count = Some(count);
            color_options.uniform_map = Some(uniform_map.clone());
            self.pick_commands.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-pick",
                context.clone(),
                crate::renderer::Pass::TerrainClassification,
                metadata.clone(),
                color_options,
            ));
            self.pick_command_records.push(Vector3DTileCommandRecord {
                pass: crate::renderer::Pass::TerrainClassification,
                offset,
                count,
                wireframe: false,
                pick: true,
            });
            let mut color_options_3d_tiles = SceneDrawOptions::new(vertex_array.clone(), program_pick.clone());
            color_options_3d_tiles.state = self.pick_render_state.clone();
            color_options_3d_tiles.offset = offset;
            color_options_3d_tiles.count = Some(count);
            color_options_3d_tiles.uniform_map = Some(uniform_map.clone());
            self.pick_commands_3d_tiles.push(SceneCommand::from_draw(
                "vector-3d-tile-primitive-pick-3dtiles",
                context.clone(),
                crate::renderer::Pass::Cesium3DTileClassification,
                metadata,
                color_options_3d_tiles,
            ));
        }
        self.pick_commands_dirty = false;
        Ok(())
    }

    /// Cesium's `update`.
    pub fn update(&mut self, frame: &mut super::FrameState) -> Result<()> {
        let needs_gpu = frame.passes.render || frame.passes.pick;
        if needs_gpu {
            self.context = Some(Self::context_of(frame)?);
        }

        let context = self.context.clone();
        if let Some(_context) = context {
            self.ensure_gpu(frame)?;
        }

        let rebatched = self.rebatch_commands();
        if rebatched || self.commands_dirty || self.built_debug_wireframe != self.debug_wireframe {
            if let Some(context) = self.context.clone() {
                self.build_commands(context)?;
            }
        }

        if frame.passes.render {
            if let Some(context) = self.context.clone() {
                self.build_ignore_show_commands(frame, context);
            }
            let queue_terrain = self.classification_type != ClassificationType::Cesium3DTile;
            let queue_3dtiles = self.classification_type != ClassificationType::Terrain;
            let wireframe = self.debug_wireframe;
            if wireframe {
                if queue_terrain {
                    for (i, command) in self.color_commands.iter().enumerate() {
                        if i % 2 == 1 {
                            frame.command_list.push(command.clone());
                        }
                    }
                }
            } else {
                if queue_terrain {
                    frame.command_list.extend(self.color_commands.iter().cloned());
                }
                if queue_3dtiles {
                    frame.command_list.extend(self.color_commands_3d_tiles.iter().cloned());
                }
            }
            if frame.invert_classification {
                frame.command_list.extend(self.ignore_show_commands.iter().cloned());
            }
        }

        if frame.passes.pick {
            if let Some(context) = self.context.clone() {
                self.build_pick_commands(context)?;
            }
            let queue_terrain = self.classification_type != ClassificationType::Cesium3DTile;
            let queue_3dtiles = self.classification_type != ClassificationType::Terrain;
            if queue_terrain {
                frame.command_list.extend(self.pick_commands.iter().cloned());
            }
            if queue_3dtiles {
                frame.command_list.extend(self.pick_commands_3d_tiles.iter().cloned());
            }
        }
        Ok(())
    }

    /// Cesium's `isDestroyed`.
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.vertex_array = None;
        self.program = None;
        self.program_stencil = None;
        self.program_pick = None;
        self.color_commands.clear();
        self.color_commands_3d_tiles.clear();
        self.pick_commands.clear();
        self.pick_commands_3d_tiles.clear();
        self.ignore_show_commands.clear();
        self.batch_table = None;
        self.destroyed = true;
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{Gl, GlValue};

    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
            Ok([256, 256])
        }
        fn invoke(&self, name: &str, _args: &[GlValue<u32>]) -> Result<GlValue<u32>> {
            if name.starts_with("create") {
                Ok(GlValue::Object(1))
            } else if matches!(name, "getShaderParameter" | "getProgramParameter") {
                Ok(GlValue::Bool(true))
            } else {
                Ok(GlValue::Null)
            }
        }
    }

    fn batch(offset: usize, count: usize, color: Color, ids: &[u32]) -> Vector3DTileBatch {
        Vector3DTileBatch { offset, count, color, batch_ids: ids.to_vec() }
    }

    fn primitive() -> Vector3DTilePrimitive<TestGl> {
        Vector3DTilePrimitive::new(Vector3DTilePrimitiveOptions {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            vertex_batch_ids: vec![0, 0, 1],
            indices: TypedArray::Uint16(vec![0, 1, 2, 0, 2, 1, 1, 2, 0, 0, 1, 2]),
            index_counts: vec![6, 6],
            index_offsets: vec![0, 6],
            batched_indices: vec![batch(0, 6, Color::RED, &[0]), batch(6, 6, Color::BLUE, &[1])],
            batch_ids: vec![0, 1],
            ..Default::default()
        })
    }

    #[test]
    fn rebatch_only_runs_for_duplicate_colours() {
        let mut p = primitive();
        p.force_rebatch = true;
        // No duplicate colour: Cesium marks the batches clean and keeps order.
        assert!(!p.rebatch_commands());
        assert_eq!(p.batched_indices[0].color, Color::RED);
        assert_eq!(p.index_offsets, vec![0, 6]);
    }

    #[test]
    fn rebatch_sorts_descending_and_merges_duplicate_colours() {
        let mut p = primitive();
        p.force_rebatch = true;
        p.indices = TypedArray::Uint16(vec![0; 18]);
        p.index_counts = vec![6, 6, 6];
        p.index_offsets = vec![0, 6, 12];
        p.batch_ids = vec![0, 1, 2];
        p.batch_id_lookup = [(0, 0), (1, 1), (2, 2)].into_iter().collect();
        p.batched_indices = vec![
            batch(0, 6, Color::RED, &[0]),
            batch(6, 6, Color::BLUE, &[1]),
            batch(12, 6, Color::BLUE, &[2]),
        ];
        assert!(p.rebatch_commands());
        // BLUE = 0x0000ffff, RED = 0xff0000ff: descending puts RED first, then
        // the two equal blue batches merge.
        assert_eq!(p.batched_indices.len(), 2);
        assert_eq!(p.batched_indices[0].color, Color::RED);
        assert_eq!((p.batched_indices[0].offset, p.batched_indices[0].count), (0, 6));
        assert_eq!(p.batched_indices[1].color, Color::BLUE);
        assert_eq!((p.batched_indices[1].offset, p.batched_indices[1].count), (6, 12));
        assert_eq!(p.batched_indices[1].batch_ids, vec![2, 1]);
    }

    #[test]
    fn duplicate_colours_fill_the_120_frame_grace_period() {
        let mut p = primitive();
        p.batched_indices.push(batch(12, 6, Color::BLUE, &[1]));
        p.batch_dirty = true;
        assert!(!p.rebatch_commands());
        assert_eq!(p.frames_since_last_rebatch, 1);
        p.force_rebatch = true;
        assert!(p.rebatch_commands());
        assert_eq!(p.frames_since_last_rebatch, 0);
    }

    #[test]
    fn update_commands_splits_the_containing_batch() {
        let mut p = primitive();
        p.index_counts = vec![3, 9];
        p.index_offsets = vec![0, 3];
        p.batched_indices = vec![batch(0, 12, Color::WHITE, &[0, 1])];
        p.update_commands(1, Color::RED);
        assert_eq!(p.batched_indices.len(), 2);
        let white = p.batched_indices.iter().find(|b| b.color == Color::WHITE).unwrap();
        assert_eq!((white.offset, white.count), (0, 3));
        assert_eq!(white.batch_ids, vec![0]);
        let red = p.batched_indices.iter().find(|b| b.color == Color::RED).unwrap();
        assert_eq!((red.offset, red.count), (3, 9));
    }

    #[test]
    fn missing_batch_id_is_ignored() {
        let mut p = primitive();
        p.update_commands(9, Color::RED);
        assert_eq!(p.batched_indices.len(), 2);
    }
}
