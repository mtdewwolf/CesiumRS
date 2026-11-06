//! CesiumJS `PointPrimitiveCollection`: points drawn from one vertex array
//! facade with one draw command per vertex array and pass.
//!
//! The update follows Cesium step for step: compact removed points, recompute
//! actual positions for the scene mode, pick a buffer usage per attribute from
//! how often its properties changed, rewrite every point or only the dirty
//! ones, grow the bounding volume by the largest point's size in pixels,
//! compile the shaders the points need and queue the draw commands. The
//! vertex bytes, commands and shader defines are checked against a capture of
//! the pinned CesiumJS engine on a real WebGL2 context in
//! `tests/scene_point_primitive_collection.rs`.
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{
    blending_state,
    point_primitive::*,
    scene::{Result, SceneDrawOptions, ScenePrimitive},
    scene_transforms::SceneTransformState,
    BlendOption, Cesium3DTilePass, FrameState, SceneCommand, SceneMode,
};
use crate::{
    core::{
        webgl_constants as gl, AttributeCompression, BoundingSphere, Cartesian2, Color, EncodedCartesian3, Matrix4,
        PrimitiveType, RuntimeError,
    },
    renderer::{
        render_state::DepthTest, BufferUsage, Context, DrawMetadata, FacadeAttribute, Gl, Pass, RenderState,
        ShaderProgram, ShaderProgramOptions, ShaderSource, UniformFrustum, UniformValue, VertexArrayFacade,
    },
    shaders::root::{POINT_PRIMITIVE_COLLECTION_FS, POINT_PRIMITIVE_COLLECTION_VS},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

const POSITION_HIGH_AND_SIZE: u32 = 0;
const POSITION_LOW_AND_OUTLINE: u32 = 1;
const COMPRESSED_ATTRIBUTE_0: u32 = 2;
const COMPRESSED_ATTRIBUTE_1: u32 = 3;
const SCALE_BY_DISTANCE: u32 = 4;
const DISTANCE_DISPLAY_CONDITION_DEPTH_SPLIT: u32 = 5;

/// Cesium's `attributeLocations`.
pub fn attribute_locations() -> BTreeMap<String, u32> {
    [
        ("positionHighAndSize", POSITION_HIGH_AND_SIZE),
        ("positionLowAndOutline", POSITION_LOW_AND_OUTLINE),
        ("compressedAttribute0", COMPRESSED_ATTRIBUTE_0),
        ("compressedAttribute1", COMPRESSED_ATTRIBUTE_1),
        ("scaleByDistance", SCALE_BY_DISTANCE),
        ("distanceDisplayConditionAndDisableDepthAndSplitDirection", DISTANCE_DISPLAY_CONDITION_DEPTH_SPLIT),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointPrimitiveCollectionOptions {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    pub blend_option: BlendOption,
}

impl Default for PointPrimitiveCollectionOptions {
    fn default() -> Self {
        Self {
            show: true,
            model_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: false,
            blend_option: BlendOption::OpaqueAndTranslucent,
        }
    }
}

/// What the collection put in one draw command, as Cesium's `_colorCommands`.
#[derive(Clone, Debug, PartialEq)]
pub struct PointPrimitiveCommand {
    pub pass: Pass,
    pub primitive_type: PrimitiveType,
    /// Index into the facade's vertex arrays.
    pub vertex_array: usize,
    /// Drawn with the opaque program (`_sp`) rather than `_spTranslucent`.
    pub opaque_program: bool,
    pub render_state: RenderState,
    pub bounding_volume: BoundingSphere,
    pub model_matrix: Matrix4,
    pub pick_id: String,
    pub debug_show_bounding_volume: bool,
}

/// The vertex and fragment sources of a compiled program.
#[derive(Clone, Debug, PartialEq)]
pub struct PointPrimitiveProgram {
    pub vertex_defines: Vec<String>,
    pub fragment_defines: Vec<String>,
}

struct Program<G: Gl> {
    program: Rc<ShaderProgram<G>>,
    sources: PointPrimitiveProgram,
}

/// Which shader features the written points need (`_shader*`) and which the
/// current programs were compiled with (`_compiledShader*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointPrimitiveShaderFlags {
    pub scale_by_distance: bool,
    pub translucency_by_distance: bool,
    pub distance_display_condition: bool,
    pub disable_depth_distance: bool,
}

pub struct PointPrimitiveCollection<G: Gl + 'static> {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    pub blend_option: BlendOption,
    id: u64,
    points: Vec<Option<PointPrimitive>>,
    queue: Rc<RefCell<UpdateQueue>>,
    points_removed: bool,
    create_vertex_array: bool,
    shader: PointPrimitiveShaderFlags,
    compiled_shader: PointPrimitiveShaderFlags,
    max_pixel_size: f64,
    base_volume: BoundingSphere,
    base_volume_wc: BoundingSphere,
    base_volume_2d: BoundingSphere,
    bounding_volume_dirty: bool,
    applied_model_matrix: Matrix4,
    applied_blend_option: Option<BlendOption>,
    mode: SceneMode,
    max_total_point_size: Rc<Cell<f64>>,
    buffers_usage: Vec<BufferUsage>,
    rs_opaque: Option<RenderState>,
    rs_translucent: Option<RenderState>,
    sp: Option<Program<G>>,
    sp_translucent: Option<Program<G>>,
    vaf: Option<VertexArrayFacade<G>>,
    commands: Vec<PointPrimitiveCommand>,
    destroyed: bool,
}

impl<G: Gl + 'static> Default for PointPrimitiveCollection<G> {
    fn default() -> Self {
        Self::new(PointPrimitiveCollectionOptions::default())
    }
}

/// `_sp` and `_spTranslucent`.
pub type ShaderProgramPair<G> = (Option<Rc<ShaderProgram<G>>>, Option<Rc<ShaderProgram<G>>>);

type Writer<G> = fn(&mut PointPrimitiveCollection<G>, &Context<G>, &PointPrimitive) -> Result<()>;

impl<G: Gl + 'static> PointPrimitiveCollection<G> {
    pub fn new(options: PointPrimitiveCollectionOptions) -> Self {
        Self {
            show: options.show,
            model_matrix: options.model_matrix,
            debug_show_bounding_volume: options.debug_show_bounding_volume,
            blend_option: options.blend_option,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            points: Vec::new(),
            queue: Rc::default(),
            points_removed: false,
            create_vertex_array: false,
            shader: Default::default(),
            compiled_shader: Default::default(),
            max_pixel_size: 1.0,
            base_volume: BoundingSphere::default(),
            base_volume_wc: BoundingSphere::default(),
            base_volume_2d: BoundingSphere::default(),
            bounding_volume_dirty: false,
            applied_model_matrix: Matrix4::IDENTITY,
            applied_blend_option: None,
            mode: SceneMode::Scene3D,
            max_total_point_size: Rc::new(Cell::new(1.0)),
            buffers_usage: vec![BufferUsage::StaticDraw; 9],
            rs_opaque: None,
            rs_translucent: None,
            sp: None,
            sp_translucent: None,
            vaf: None,
            commands: Vec::new(),
            destroyed: false,
        }
    }

    fn check(&self) -> Result<()> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }

    /// `removePointPrimitives`: drop removed slots and renumber.
    fn compact(&mut self) {
        if self.points_removed {
            self.points_removed = false;
            self.points.retain(Option::is_some);
            for (i, p) in self.points.iter().flatten().enumerate() {
                p.0.borrow_mut().index = i;
            }
        }
    }

    pub fn length(&mut self) -> usize {
        self.compact();
        self.points.len()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Adds a point; the vertex arrays are rebuilt on the next update.
    pub fn add(&mut self, options: PointPrimitiveOptions) -> Result<PointPrimitive> {
        self.check()?;
        let point = PointPrimitive::new(options, &self.queue, self.id, self.points.len())?;
        self.points.push(Some(point.clone()));
        self.create_vertex_array = true;
        Ok(point)
    }

    /// Whether `point` belongs to this collection.
    pub fn contains(&self, point: &PointPrimitive) -> bool {
        point.collection_id() == Some(self.id)
    }

    /// Removes `point`, releasing its pick id. Returns `false` when the
    /// collection does not hold it.
    pub fn remove(&mut self, point: &PointPrimitive) -> Result<bool> {
        self.check()?;
        if !self.contains(point) {
            return Ok(false);
        }
        let index = point.index();
        self.points[index] = None;
        self.points_removed = true;
        self.create_vertex_array = true;
        point.destroy();
        Ok(true)
    }

    pub fn remove_all(&mut self) -> Result<()> {
        self.check()?;
        for p in self.points.drain(..).flatten() {
            p.destroy();
        }
        self.queue.borrow_mut().to_update.clear();
        self.points_removed = false;
        self.create_vertex_array = true;
        Ok(())
    }

    pub fn get(&mut self, index: usize) -> Option<PointPrimitive> {
        self.compact();
        self.points.get(index).cloned().flatten()
    }

    /// `computeNewBuffersUsage`: a property that changed since the last
    /// rebuild streams, the rest are static. `_buffersUsage` starts with
    /// nine entries for eleven properties, so, as in Cesium, the first call
    /// always reports a change.
    fn compute_new_buffers_usage(&mut self) -> bool {
        let changed = self.queue.borrow().properties_changed;
        let mut usage_changed = false;
        for (k, count) in changed.iter().enumerate() {
            let usage = if *count == 0 { BufferUsage::StaticDraw } else { BufferUsage::StreamDraw };
            match self.buffers_usage.get_mut(k) {
                Some(current) => {
                    usage_changed |= *current != usage;
                    *current = usage;
                }
                None => {
                    usage_changed = true;
                    self.buffers_usage.push(usage);
                }
            }
        }
        usage_changed
    }

    fn create_vaf(&self, context: &Context<G>, size: usize) -> Result<VertexArrayFacade<G>> {
        let attribute = |index, property: usize| {
            let mut a = FacadeAttribute::new(index, 4);
            a.usage = self.buffers_usage[property];
            a
        };
        VertexArrayFacade::new(
            context,
            vec![
                attribute(POSITION_HIGH_AND_SIZE, POSITION_INDEX),
                attribute(POSITION_LOW_AND_OUTLINE, POSITION_INDEX),
                attribute(COMPRESSED_ATTRIBUTE_0, COLOR_INDEX),
                attribute(COMPRESSED_ATTRIBUTE_1, TRANSLUCENCY_BY_DISTANCE_INDEX),
                attribute(SCALE_BY_DISTANCE, SCALE_BY_DISTANCE_INDEX),
                attribute(DISTANCE_DISPLAY_CONDITION_DEPTH_SPLIT, DISTANCE_DISPLAY_CONDITION_INDEX),
            ],
            size,
            false,
        )
    }

    fn write(&mut self, attribute: u32, index: usize, values: [f64; 4]) -> Result<()> {
        self.vaf.as_mut().ok_or_else(|| RuntimeError::new("No vertex array facade"))?.write(attribute, index, &values)
    }

    fn write_position_size_and_outline(&mut self, _: &Context<G>, point: &PointPrimitive) -> Result<()> {
        let (index, position, pixel_size, outline_width) = {
            let s = point.0.borrow();
            (s.index, s.actual_position, s.options.pixel_size, s.options.outline_width)
        };
        if self.mode == SceneMode::Scene3D {
            self.base_volume = self.base_volume.expand(position);
            self.bounding_volume_dirty = true;
        }
        let encoded = EncodedCartesian3::from_cartesian(position);
        self.max_pixel_size = self.max_pixel_size.max(pixel_size + outline_width);
        let (high, low) = (encoded.high, encoded.low);
        self.write(POSITION_HIGH_AND_SIZE, index, [high.x, high.y, high.z, pixel_size])?;
        self.write(POSITION_LOW_AND_OUTLINE, index, [low.x, low.y, low.z, outline_width])
    }

    fn write_compressed_attrib0(&mut self, context: &Context<G>, point: &PointPrimitive) -> Result<()> {
        let pick = point.get_pick_id(context)?;
        let (index, color, outline) = {
            let s = point.0.borrow();
            (s.index, s.options.color, s.options.outline_color)
        };
        let alphas = Color::new(color.alpha, outline.alpha, pick.alpha, 1.0);
        self.write(
            COMPRESSED_ATTRIBUTE_0,
            index,
            [
                AttributeCompression::encode_rgb8(color),
                AttributeCompression::encode_rgb8(outline),
                AttributeCompression::encode_rgb8(pick),
                AttributeCompression::encode_rgb8(alphas),
            ],
        )
    }

    fn write_compressed_attrib1(&mut self, _: &Context<G>, point: &PointPrimitive) -> Result<()> {
        let (index, o, cluster_show) = {
            let s = point.0.borrow();
            (s.index, s.options.clone(), s.cluster_show)
        };
        let (mut near, mut near_value, mut far, mut far_value) = (0.0, 1.0, 1.0, 1.0);
        if let Some(t) = o.translucency_by_distance {
            (near, near_value, far, far_value) = (t.near, t.near_value, t.far, t.far_value);
            if near_value != 1.0 || far_value != 1.0 {
                self.shader.translucency_by_distance = true;
            }
        }
        let show = o.show && cluster_show && !(o.color.alpha == 0.0 && o.outline_color.alpha == 0.0);
        let quantize = |v: f64| {
            let v = crate::core::math::clamp(v, 0.0, 1.0);
            if v == 1.0 {
                255.0
            } else {
                js_to_int32(v * 255.0)
            }
        };
        let compressed0 = if show { 256.0 } else { 0.0 } + quantize(near_value);
        self.write(COMPRESSED_ATTRIBUTE_1, index, [compressed0, quantize(far_value), near, far])
    }

    fn write_scale_by_distance(&mut self, _: &Context<G>, point: &PointPrimitive) -> Result<()> {
        let (index, scale) = {
            let s = point.0.borrow();
            (s.index, s.options.scale_by_distance)
        };
        let (mut near, mut near_value, mut far, mut far_value) = (0.0, 1.0, 1.0, 1.0);
        if let Some(s) = scale {
            (near, near_value, far, far_value) = (s.near, s.near_value, s.far, s.far_value);
            if near_value != 1.0 || far_value != 1.0 {
                self.shader.scale_by_distance = true;
            }
        }
        self.write(SCALE_BY_DISTANCE, index, [near, near_value, far, far_value])
    }

    fn write_distance_display_condition_depth_split(&mut self, _: &Context<G>, point: &PointPrimitive) -> Result<()> {
        let (index, o) = {
            let s = point.0.borrow();
            (s.index, s.options.clone())
        };
        let (mut near, mut far) = (0.0, f64::MAX);
        if let Some(d) = o.distance_display_condition {
            (near, far) = (d.near * d.near, d.far * d.far);
            self.shader.distance_display_condition = true;
        }
        let mut disable = o.disable_depth_test_distance * o.disable_depth_test_distance;
        if disable > 0.0 {
            self.shader.disable_depth_distance = true;
            if disable == f64::INFINITY {
                disable = -1.0;
            }
        }
        self.write(
            DISTANCE_DISPLAY_CONDITION_DEPTH_SPLIT,
            index,
            [near, far, disable, o.split_direction.value() as f64],
        )
    }

    fn write_point(&mut self, context: &Context<G>, point: &PointPrimitive) -> Result<()> {
        self.write_position_size_and_outline(context, point)?;
        self.write_compressed_attrib0(context, point)?;
        self.write_compressed_attrib1(context, point)?;
        self.write_scale_by_distance(context, point)?;
        self.write_distance_display_condition_depth_split(context, point)
    }

    /// `recomputeActualPositions`.
    fn recompute_actual_positions(&mut self, frame: &FrameState, points: &[PointPrimitive], recompute_volume: bool) {
        let in_3d = frame.mode == SceneMode::Scene3D;
        if in_3d {
            self.bounding_volume_dirty = true;
        }
        let state = SceneTransformState::from_frame(frame, Cartesian2::ZERO);
        let mut positions = Vec::new();
        for point in points {
            let position = point.position();
            if let Some(actual) = PointPrimitive::compute_actual_position(position, &state, &self.applied_model_matrix)
            {
                point.set_actual_position(actual);
                if recompute_volume {
                    positions.push(actual);
                } else if in_3d {
                    self.base_volume = self.base_volume.expand(actual);
                } else {
                    self.base_volume_2d = self.base_volume_2d.expand(actual);
                }
            }
        }
        if recompute_volume {
            let volume = BoundingSphere::from_points(&positions);
            if in_3d {
                self.base_volume = volume;
            } else {
                self.base_volume_2d = volume;
            }
        }
    }

    /// `updateMode`.
    fn update_mode(&mut self, frame: &FrameState) {
        let mode = frame.mode;
        let points: Vec<PointPrimitive> = self.points.iter().flatten().cloned().collect();
        if self.create_vertex_array
            || self.mode != mode
            || (mode != SceneMode::Scene3D && self.applied_model_matrix != self.model_matrix)
        {
            self.mode = mode;
            self.applied_model_matrix = self.model_matrix;
            self.create_vertex_array = true;
            if matches!(mode, SceneMode::Scene3D | SceneMode::Scene2D | SceneMode::ColumbusView) {
                self.recompute_actual_positions(frame, &points, true);
            }
        } else if mode == SceneMode::Morphing {
            self.recompute_actual_positions(frame, &points, true);
        } else if matches!(mode, SceneMode::Scene2D | SceneMode::ColumbusView) {
            let queued = self.queue.borrow().to_update.clone();
            self.recompute_actual_positions(frame, &queued, false);
        }
    }

    /// `updateBoundingVolume`: grow the volume by the largest point's size at
    /// the volume's distance.
    fn grow_bounding_volume(
        &self,
        frame: &FrameState,
        context: &Context<G>,
        volume: &mut BoundingSphere,
    ) -> Result<()> {
        let camera = &frame.uniform.camera;
        let to_center = camera.position - volume.center;
        let projection = camera.direction.multiply_by_scalar(to_center.dot(camera.direction));
        let distance = (projection.magnitude() - volume.radius).max(0.0);
        let [width, height] = context.drawing_buffer_size()?;
        let (width, height, ratio) = (width as f64, height as f64, frame.pixel_ratio);
        let pixel = match &camera.frustum {
            UniformFrustum::Perspective(f) => f.get_pixel_dimensions(width, height, distance, ratio),
            UniformFrustum::PerspectiveOffCenter(f) => f.get_pixel_dimensions(width, height, distance, ratio),
            UniformFrustum::Orthographic(f) => f.get_pixel_dimensions(width, height, distance, ratio),
            UniformFrustum::OrthographicOffCenter(f) => f.get_pixel_dimensions(width, height, distance, ratio),
        };
        volume.radius += pixel.x.max(pixel.y) * self.max_pixel_size;
        Ok(())
    }

    fn compile(&mut self, context: &Context<G>) -> Result<()> {
        let mut vertex = ShaderSource::new(POINT_PRIMITIVE_COLLECTION_VS.source);
        let s = self.shader;
        for (enabled, define) in [
            (s.scale_by_distance, "EYE_DISTANCE_SCALING"),
            (s.translucency_by_distance, "EYE_DISTANCE_TRANSLUCENCY"),
            (s.distance_display_condition, "DISTANCE_DISPLAY_CONDITION"),
            (s.disable_depth_distance, "DISABLE_DEPTH_DISTANCE"),
        ] {
            if enabled {
                vertex.defines.push(define.to_string());
            }
        }
        let build = |previous: Option<Program<G>>, fragment_defines: &[&str]| -> Result<Program<G>> {
            let mut fragment = ShaderSource::new(POINT_PRIMITIVE_COLLECTION_FS.source);
            fragment.defines = fragment_defines.iter().map(|d| d.to_string()).collect();
            let options = ShaderProgramOptions {
                vertex_shader_source: vertex.clone(),
                fragment_shader_source: fragment.clone(),
                attribute_locations: attribute_locations(),
            };
            let program =
                context.with_shader_cache(|c| c.replace_shader_program(previous.map(|p| p.program), &options))?;
            Ok(Program {
                program,
                sources: PointPrimitiveProgram {
                    vertex_defines: vertex.defines.clone(),
                    fragment_defines: fragment.defines,
                },
            })
        };
        match self.applied_blend_option {
            Some(BlendOption::OpaqueAndTranslucent) => {
                self.sp = Some(build(self.sp.take(), &["OPAQUE"])?);
                self.sp_translucent = Some(build(self.sp_translucent.take(), &["TRANSLUCENT"])?);
            }
            Some(BlendOption::Opaque) => self.sp = Some(build(self.sp.take(), &[])?),
            Some(BlendOption::Translucent) => self.sp_translucent = Some(build(self.sp_translucent.take(), &[])?),
            None => {}
        }
        self.compiled_shader = self.shader;
        Ok(())
    }

    /// Cesium's `update`, with the context taken from `frame.context`.
    fn update_frame(&mut self, frame: &mut FrameState) -> Result<()> {
        self.check()?;
        self.compact();
        self.commands.clear();
        if !self.show {
            return Ok(());
        }
        let context = frame
            .context
            .as_ref()
            .and_then(|c| c.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("frameState.context is required."))?;
        self.max_total_point_size.set(context.limits()?.maximum_aliased_point_size);

        self.update_mode(frame);

        let points: Vec<PointPrimitive> = self.points.iter().flatten().cloned().collect();
        let to_update = self.queue.borrow().to_update.clone();
        let picking = frame.passes.pick;

        if self.create_vertex_array || (!picking && self.compute_new_buffers_usage()) {
            self.create_vertex_array = false;
            self.queue.borrow_mut().properties_changed = [0; NUMBER_OF_PROPERTIES];
            if let Some(mut vaf) = self.vaf.take() {
                vaf.destroy()?;
            }
            if !points.is_empty() {
                self.vaf = Some(self.create_vaf(&context, points.len())?);
                for point in &points {
                    point.0.borrow_mut().dirty = false;
                    self.write_point(&context, point)?;
                }
                self.vaf.as_mut().unwrap().commit(None)?;
            }
            self.queue.borrow_mut().to_update.clear();
        } else if !to_update.is_empty() {
            let changed = self.queue.borrow().properties_changed;
            let mut writers: Vec<Writer<G>> = Vec::new();
            if changed[POSITION_INDEX] > 0 || changed[OUTLINE_WIDTH_INDEX] > 0 || changed[PIXEL_SIZE_INDEX] > 0 {
                writers.push(Self::write_position_size_and_outline);
            }
            if changed[COLOR_INDEX] > 0 || changed[OUTLINE_COLOR_INDEX] > 0 {
                writers.push(Self::write_compressed_attrib0);
            }
            if changed[SHOW_INDEX] > 0 || changed[TRANSLUCENCY_BY_DISTANCE_INDEX] > 0 {
                writers.push(Self::write_compressed_attrib1);
            }
            if changed[SCALE_BY_DISTANCE_INDEX] > 0 {
                writers.push(Self::write_scale_by_distance);
            }
            if changed[DISTANCE_DISPLAY_CONDITION_INDEX] > 0
                || changed[DISABLE_DEPTH_DISTANCE_INDEX] > 0
                || changed[SPLIT_DIRECTION_INDEX] > 0
            {
                writers.push(Self::write_distance_display_condition_depth_split);
            }
            let rewrite_all = to_update.len() as f64 / points.len() as f64 > 0.1;
            for point in &to_update {
                point.0.borrow_mut().dirty = false;
                for writer in &writers {
                    writer(self, &context, point)?;
                }
                if !rewrite_all {
                    let index = point.index();
                    self.vaf.as_ref().unwrap().sub_commit(index, 1)?;
                }
            }
            let vaf = self.vaf.as_mut().unwrap();
            if rewrite_all {
                vaf.commit(None)?;
            } else {
                vaf.end_sub_commits();
            }
            self.queue.borrow_mut().to_update.clear();
        }

        let Some(arrays) = self.vaf.as_ref().and_then(|v| v.vertex_arrays()) else {
            return Ok(());
        };
        let arrays: Vec<_> = arrays.iter().map(|a| a.vertex_array.clone()).collect();

        if self.bounding_volume_dirty {
            self.bounding_volume_dirty = false;
            self.base_volume_wc = self.base_volume.transform(&self.model_matrix);
        }
        let (model_matrix, mut bounding_volume) = if frame.mode == SceneMode::Scene3D {
            (self.model_matrix, self.base_volume_wc)
        } else {
            (Matrix4::IDENTITY, self.base_volume_2d)
        };
        self.grow_bounding_volume(frame, &context, &mut bounding_volume)?;

        let blend_option_changed = self.applied_blend_option != Some(self.blend_option);
        self.applied_blend_option = Some(self.blend_option);
        if blend_option_changed {
            let depth_test = DepthTest { enabled: true, func: gl::LEQUAL };
            self.rs_opaque = matches!(self.blend_option, BlendOption::Opaque | BlendOption::OpaqueAndTranslucent)
                .then(|| RenderState { depth_test: depth_test.clone(), depth_mask: true, ..Default::default() });
            self.rs_translucent =
                matches!(self.blend_option, BlendOption::Translucent | BlendOption::OpaqueAndTranslucent).then(|| {
                    RenderState {
                        depth_test,
                        depth_mask: false,
                        blending: blending_state::alpha_blend(),
                        ..Default::default()
                    }
                });
        }

        self.shader.disable_depth_distance |= frame.minimum_disable_depth_test_distance.unwrap_or(0.0) != 0.0;
        let (s, c) = (self.shader, self.compiled_shader);
        if blend_option_changed
            || (s.scale_by_distance && !c.scale_by_distance)
            || (s.translucency_by_distance && !c.translucency_by_distance)
            || (s.distance_display_condition && !c.distance_display_condition)
            || s.disable_depth_distance != c.disable_depth_distance
        {
            self.compile(&context)?;
        }

        if frame.passes.render || picking {
            let opaque = self.blend_option == BlendOption::Opaque;
            let both = self.blend_option == BlendOption::OpaqueAndTranslucent;
            let total = if both { arrays.len() * 2 } else { arrays.len() };
            for j in 0..total {
                let opaque_command = opaque || (both && j % 2 == 0);
                let index = if both { j / 2 } else { j };
                let pass = if opaque_command || !both { Pass::Opaque } else { Pass::Translucent };
                let (program, state) = if opaque_command {
                    (self.sp.as_ref(), self.rs_opaque.clone())
                } else {
                    (self.sp_translucent.as_ref(), self.rs_translucent.clone())
                };
                let program = program.ok_or_else(|| RuntimeError::new("Point shader program is not compiled"))?;
                let state = state.ok_or_else(|| RuntimeError::new("Point render state is not created"))?;
                let command = PointPrimitiveCommand {
                    pass,
                    primitive_type: PrimitiveType::Points,
                    vertex_array: index,
                    opaque_program: opaque_command,
                    render_state: state.clone(),
                    bounding_volume,
                    model_matrix,
                    pick_id: "v_pickColor".into(),
                    debug_show_bounding_volume: self.debug_show_bounding_volume,
                };
                let metadata = DrawMetadata {
                    bounding_volume: Some(bounding_volume),
                    model_matrix: Some(model_matrix),
                    owner: Some(format!("PointPrimitiveCollection#{}", self.id)),
                    pick_id: Some(command.pick_id.clone()),
                    debug_show_bounding_volume: self.debug_show_bounding_volume,
                    ..Default::default()
                };
                let mut options = SceneDrawOptions::new(arrays[index].clone(), program.program.clone());
                options.state = state;
                options.primitive_type = PrimitiveType::Points;
                let size = self.max_total_point_size.clone();
                options.uniform_map = Some(Rc::new(move || {
                    Ok(vec![("u_maxTotalPointSize".to_string(), UniformValue::Float(vec![size.get() as f32]))])
                }));
                frame.command_list.push(SceneCommand::from_draw(
                    format!("point-primitive-collection-{}-{j}", self.id),
                    context.clone(),
                    pass,
                    metadata,
                    options,
                ));
                self.commands.push(command);
            }
        }
        Ok(())
    }

    /// `PointPrimitive.computeScreenSpacePosition`: the point's actual
    /// position, moved by the collection's model matrix, in window
    /// coordinates with y measured from the top of the canvas. `None` when
    /// the position cannot be projected.
    pub fn compute_screen_space_position(
        &self,
        point: &PointPrimitive,
        state: &SceneTransformState<'_>,
        camera: &mut super::Camera,
    ) -> Result<Option<Cartesian2>> {
        if !self.contains(point) {
            return Err(RuntimeError::new("PointPrimitive must be in a collection."));
        }
        let world = self.model_matrix.multiply_by_point(point.actual_position());
        Ok(super::SceneTransforms::world_to_window_coordinates(state, camera, world)?
            .map(|w| Cartesian2::new(w.x, state.client_dimensions.y - w.y)))
    }

    /// The commands queued by the last update.
    pub fn commands(&self) -> &[PointPrimitiveCommand] {
        &self.commands
    }

    /// `_vaf`: the vertex array facade, once points have been written.
    pub fn vertex_array_facade(&self) -> Option<&VertexArrayFacade<G>> {
        self.vaf.as_ref()
    }

    /// The defines of `_sp` and `_spTranslucent`.
    pub fn programs(&self) -> (Option<&PointPrimitiveProgram>, Option<&PointPrimitiveProgram>) {
        (self.sp.as_ref().map(|p| &p.sources), self.sp_translucent.as_ref().map(|p| &p.sources))
    }

    /// The compiled `_sp` and `_spTranslucent`.
    pub fn shader_programs(&self) -> ShaderProgramPair<G> {
        (self.sp.as_ref().map(|p| p.program.clone()), self.sp_translucent.as_ref().map(|p| p.program.clone()))
    }

    /// The shader features the written points need.
    pub fn shader_flags(&self) -> PointPrimitiveShaderFlags {
        self.shader
    }

    /// `_maxPixelSize`: the largest `pixelSize + outlineWidth` written.
    pub fn max_pixel_size(&self) -> f64 {
        self.max_pixel_size
    }

    /// `u_maxTotalPointSize`: the context's maximum aliased point size.
    pub fn max_total_point_size(&self) -> f64 {
        self.max_total_point_size.get()
    }

    /// `_buffersUsage`, per property.
    pub fn buffers_usage(&self) -> &[BufferUsage] {
        &self.buffers_usage
    }

    /// `_propertiesChanged`, per property.
    pub fn properties_changed(&self) -> [u32; NUMBER_OF_PROPERTIES] {
        self.queue.borrow().properties_changed
    }

    /// `_pointPrimitivesToUpdateIndex`: points waiting for a rewrite.
    pub fn points_to_update(&self) -> usize {
        self.queue.borrow().to_update.len()
    }
}

/// JavaScript `x | 0`.
fn js_to_int32(value: f64) -> f64 {
    crate::core::math::to_int32(value) as f64
}

impl<G: Gl + 'static> ScenePrimitive for PointPrimitiveCollection<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.update_frame(frame)
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<()> {
        Ok(())
    }
    /// Releases the programs, the vertex arrays and every point's pick id.
    fn destroy(&mut self) -> Result<()> {
        self.check()?;
        self.sp = None;
        self.sp_translucent = None;
        if let Some(mut vaf) = self.vaf.take() {
            vaf.destroy()?;
        }
        for p in self.points.iter().flatten() {
            p.destroy();
        }
        self.commands.clear();
        self.destroyed = true;
        Ok(())
    }
}
