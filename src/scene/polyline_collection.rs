//! CesiumJS `PolylineCollection`: polylines drawn from one position buffer,
//! grouped into one bucket (and shader) per material type, with per-polyline
//! width, show, pick color, bounding sphere and distance display condition in
//! a [`BatchTable`].
//!
//! The update follows Cesium step for step: compact removed polylines, detect
//! mode and model-matrix changes, (re)create the batch table, choose the
//! position buffer usage (stream for 100 frames after a position change),
//! rebuild every buffer and vertex array (splitting index buffers at 64K
//! vertices) or write only changed positions and batch values, and queue one
//! command per run of polylines with the same material. Checked against a
//! SwiftShader capture of the pinned engine in
//! `tests/scene_polyline_collection.rs`.
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{
    batch_table::{BatchTable, BatchTableAttribute},
    blending_state,
    polyline::*,
    scene::{Result, SceneDrawOptions, ScenePrimitive},
    Cesium3DTilePass, FrameState, SceneCommand, SceneMode,
};
use crate::{
    core::{
        math, BoundingSphere, Cartesian2, Cartesian3, Cartesian4, Color, ComponentDatatype, EncodedCartesian3,
        IndexDatatype, Intersect, MapProjection, Matrix4, Plane, PrimitiveType, RuntimeError,
    },
    renderer::{
        render_state::DepthTest, AttributeSource, Buffer, BufferData, BufferKind, BufferUsage, Context, DrawMetadata,
        Gl, Pass, RenderState, SampledTexture, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformValue,
        VertexArray, VertexAttribute,
    },
    scene::material::MaterialValue,
    shaders::root::{POLYLINE_COMMON, POLYLINE_FS, POLYLINE_VS},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

const TEX_COORD_EXPAND_AND_BATCH_INDEX: u32 = 0;
const POSITION_3D_HIGH: u32 = 1;
const POSITION_3D_LOW: u32 = 2;
const POSITION_2D_HIGH: u32 = 3;
const POSITION_2D_LOW: u32 = 4;
const PREV_POSITION_3D_HIGH: u32 = 5;
const PREV_POSITION_3D_LOW: u32 = 6;
const PREV_POSITION_2D_HIGH: u32 = 7;
const PREV_POSITION_2D_LOW: u32 = 8;
const NEXT_POSITION_3D_HIGH: u32 = 9;
const NEXT_POSITION_3D_LOW: u32 = 10;
const NEXT_POSITION_2D_HIGH: u32 = 11;
const NEXT_POSITION_2D_LOW: u32 = 12;

/// Cesium's `attributeLocations`.
pub fn attribute_locations() -> BTreeMap<String, u32> {
    [
        ("texCoordExpandAndBatchIndex", TEX_COORD_EXPAND_AND_BATCH_INDEX),
        ("position3DHigh", POSITION_3D_HIGH),
        ("position3DLow", POSITION_3D_LOW),
        ("position2DHigh", POSITION_2D_HIGH),
        ("position2DLow", POSITION_2D_LOW),
        ("prevPosition3DHigh", PREV_POSITION_3D_HIGH),
        ("prevPosition3DLow", PREV_POSITION_3D_LOW),
        ("prevPosition2DHigh", PREV_POSITION_2D_HIGH),
        ("prevPosition2DLow", PREV_POSITION_2D_LOW),
        ("nextPosition3DHigh", NEXT_POSITION_3D_HIGH),
        ("nextPosition3DLow", NEXT_POSITION_3D_LOW),
        ("nextPosition2DHigh", NEXT_POSITION_2D_HIGH),
        ("nextPosition2DLow", NEXT_POSITION_2D_LOW),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

const SIXTY_FOUR_KILOBYTES: usize = math::SIXTY_FOUR_KILOBYTES as usize;

#[derive(Clone, Debug, PartialEq)]
pub struct PolylineCollectionOptions {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
}

impl Default for PolylineCollectionOptions {
    fn default() -> Self {
        Self { show: true, model_matrix: Matrix4::IDENTITY, debug_show_bounding_volume: false }
    }
}

/// What the collection put in one draw command.
#[derive(Clone, Debug, PartialEq)]
pub struct PolylineCommand {
    pub pass: Pass,
    pub count: usize,
    pub offset: usize,
    pub vertex_array: usize,
    /// The bucket whose program draws the command.
    pub bucket: String,
    pub render_state: RenderState,
    pub bounding_volume: BoundingSphere,
    pub model_matrix: Matrix4,
    pub pick_id: String,
    pub debug_show_bounding_volume: bool,
    /// Uniform names, sorted: the material's, the batch table's and
    /// `u_highlightColor`.
    pub uniforms: Vec<String>,
}

/// One bucket: the polylines of one material type and their shader.
pub struct PolylineBucket<G: Gl + 'static> {
    pub(crate) id: u64,
    pub material_type: String,
    polylines: Vec<Polyline>,
    pub length_of_positions: usize,
    material: SharedMaterial,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    defines: Vec<String>,
    mode: SceneMode,
    model_matrix: Matrix4,
}

impl<G: Gl + 'static> PolylineBucket<G> {
    /// The polylines' indices.
    pub fn polylines(&self) -> impl Iterator<Item = &Polyline> {
        self.polylines.iter()
    }
    /// The shader defines, once compiled.
    pub fn defines(&self) -> Option<&[String]> {
        self.shader_program.as_ref().map(|_| self.defines.as_slice())
    }
    pub fn shader_program(&self) -> Option<&Rc<ShaderProgram<G>>> {
        self.shader_program.as_ref()
    }
}

/// `positions`/`lengths` of `getSegments`.
struct Segments {
    positions: Vec<Cartesian3>,
    lengths: Vec<usize>,
}

/// `intersectsIDL`.
fn intersects_idl(polyline: &Polyline) -> bool {
    let bv = polyline.0.borrow().bounding_volume;
    Cartesian3::UNIT_X.dot(bv.center) < 0.0 || bv.intersect_plane(&Plane::ORIGIN_ZX_PLANE) == Intersect::Intersecting
}

fn next_bucket_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

struct VertexArrayEntry<G: Gl> {
    va: Rc<VertexArray<G>>,
    buckets: Vec<Rc<RefCell<Locator>>>,
}

pub struct PolylineCollection<G: Gl + 'static> {
    pub show: bool,
    pub debug_show_bounding_volume: bool,
    /// `_highlightColor`, used by vector tiles.
    pub highlight_color: Color,
    shared: Rc<PolylineShared>,
    polylines: Vec<Polyline>,
    polylines_removed: bool,
    create_vertex_array: bool,
    create_batch_table: bool,
    use_highlight_color: bool,
    buckets: Vec<PolylineBucket<G>>,
    position_buffer_usage: (BufferUsage, u32),
    mode: Option<SceneMode>,
    applied_model_matrix: Matrix4,
    vertex_arrays: Vec<VertexArrayEntry<G>>,
    position_buffer: Option<Buffer<G>>,
    batch_table: Option<BatchTable<G>>,
    opaque_rs: Option<RenderState>,
    translucent_rs: Option<RenderState>,
    commands: Vec<PolylineCommand>,
    destroyed: bool,
}

impl<G: Gl + 'static> Default for PolylineCollection<G> {
    fn default() -> Self {
        Self::new(PolylineCollectionOptions::default())
    }
}

/// `EncodedCartesian3.writeElements` into a `Float32Array`.
fn write_elements(c: Cartesian3, array: &mut [f32], index: usize) {
    EncodedCartesian3::write_elements_f32(c, array, index).expect("position array is large enough");
}

/// The position, previous and next positions of vertex `j` of `positions`.
fn neighbors(positions: &[Cartesian3], j: usize, r#loop: bool) -> (Cartesian3, Cartesian3, Cartesian3) {
    let n = positions.len();
    let prev = if j == 0 {
        if r#loop {
            positions[n - 2]
        } else {
            positions[0] + (positions[0] - positions[1])
        }
    } else {
        positions[j - 1]
    };
    let next = if j == n - 1 {
        if r#loop {
            positions[1]
        } else {
            positions[n - 1] + (positions[n - 1] - positions[n - 2])
        }
    } else {
        positions[j + 1]
    };
    (positions[j], prev, next)
}

/// Walks the vertices of `positions` with Cesium's segment bookkeeping,
/// calling `emit(j, position, prev, next, startK, endK)`.
fn walk(
    positions: &[Cartesian3],
    lengths: &[usize],
    r#loop: bool,
    mode: SceneMode,
    max_lon: f64,
    mut emit: impl FnMut(usize, Cartesian3, Cartesian3, Cartesian3, usize, usize),
) {
    let (mut segment_index, mut count) = (0usize, 0usize);
    let length_at = |i: usize| lengths.get(i).copied().map_or(f64::NAN, |l| l as f64);
    for j in 0..positions.len() {
        let (mut position, mut prev, mut next) = neighbors(positions, j, r#loop);
        if j as f64 == count as f64 + length_at(segment_index) {
            count += lengths[segment_index];
            segment_index += 1;
        }
        let segment_start = j == count;
        let segment_end = j as f64 == count as f64 + length_at(segment_index) - 1.0;
        if mode == SceneMode::Scene2D {
            prev.z = 0.0;
            position.z = 0.0;
            next.z = 0.0;
        }
        if matches!(mode, SceneMode::Scene2D | SceneMode::Morphing)
            && (segment_start || segment_end)
            && max_lon - position.x.abs() < 1.0
        {
            if (position.x < 0.0 && prev.x > 0.0) || (position.x > 0.0 && prev.x < 0.0) {
                prev = position;
            }
            if (position.x < 0.0 && next.x > 0.0) || (position.x > 0.0 && next.x < 0.0) {
                next = position;
            }
        }
        let start_k = if segment_start { 2 } else { 0 };
        let end_k = if segment_end { 2 } else { 4 };
        emit(j, position, prev, next, start_k, end_k);
    }
}

impl<G: Gl + 'static> PolylineCollection<G> {
    pub fn new(options: PolylineCollectionOptions) -> Self {
        Self {
            show: options.show,
            debug_show_bounding_volume: options.debug_show_bounding_volume,
            highlight_color: Color::WHITE,
            shared: Rc::new(PolylineShared {
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                model_matrix: Cell::new(options.model_matrix),
                updated: Cell::new(false),
                to_update: RefCell::default(),
                properties_changed: RefCell::new([0; NUMBER_OF_PROPERTIES]),
            }),
            polylines: Vec::new(),
            polylines_removed: false,
            create_vertex_array: false,
            create_batch_table: false,
            use_highlight_color: false,
            buckets: Vec::new(),
            position_buffer_usage: (BufferUsage::StaticDraw, 0),
            mode: None,
            applied_model_matrix: Matrix4::IDENTITY,
            vertex_arrays: Vec::new(),
            position_buffer: None,
            batch_table: None,
            opaque_rs: None,
            translucent_rs: None,
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

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn model_matrix(&self) -> Matrix4 {
        self.shared.model_matrix.get()
    }
    /// The model matrix polylines are drawn and re-split with.
    pub fn set_model_matrix(&mut self, value: Matrix4) {
        self.shared.model_matrix.set(value);
    }

    /// `removePolylines`: drop destroyed polylines and renumber. As in
    /// Cesium, every remaining polyline is then queued for update.
    fn compact(&mut self) {
        if self.polylines_removed {
            self.polylines_removed = false;
            self.polylines.retain(|p| !p.is_destroyed());
            for (i, p) in self.polylines.iter().enumerate() {
                p.0.borrow_mut().index = i;
            }
            *self.shared.to_update.borrow_mut() = self.polylines.clone();
        }
    }

    pub fn length(&mut self) -> usize {
        self.compact();
        self.polylines.len()
    }

    pub fn add(&mut self, options: PolylineOptions) -> Result<Polyline> {
        self.check()?;
        let polyline = Polyline::new(options, &self.shared, self.polylines.len())?;
        self.polylines.push(polyline.clone());
        self.create_vertex_array = true;
        self.create_batch_table = true;
        Ok(polyline)
    }

    pub fn contains(&self, polyline: &Polyline) -> bool {
        polyline.collection_id() == Some(self.shared.id)
    }

    pub fn remove(&mut self, polyline: &Polyline) -> Result<bool> {
        self.check()?;
        if !self.contains(polyline) {
            return Ok(false);
        }
        self.polylines_removed = true;
        self.create_vertex_array = true;
        self.create_batch_table = true;
        if let Some(bucket) = polyline.0.borrow().bucket {
            if let Some(b) = self.buckets.iter_mut().find(|b| b.id == bucket) {
                b.shader_program = None;
            }
        }
        polyline.destroy();
        Ok(true)
    }

    pub fn remove_all(&mut self) -> Result<()> {
        self.check()?;
        self.release_shaders();
        for p in &self.polylines {
            if !p.is_destroyed() {
                p.destroy();
            }
        }
        self.buckets.clear();
        self.polylines_removed = false;
        self.polylines.clear();
        self.shared.to_update.borrow_mut().clear();
        self.create_vertex_array = true;
        Ok(())
    }

    pub fn get(&mut self, index: usize) -> Option<Polyline> {
        self.compact();
        self.polylines.get(index).cloned()
    }

    fn release_shaders(&mut self) {
        for p in &self.polylines {
            if p.is_destroyed() {
                continue;
            }
            if let Some(bucket) = p.0.borrow().bucket {
                if let Some(b) = self.buckets.iter_mut().find(|b| b.id == bucket) {
                    b.shader_program = None;
                }
            }
        }
    }

    fn create_batch_table(&mut self, context: &Context<G>) -> Result<()> {
        if let Some(mut table) = self.batch_table.take() {
            table.destroy()?;
        }
        let attribute = |name: &str, datatype, components, normalize| BatchTableAttribute {
            function_name: name.into(),
            component_datatype: datatype,
            components_per_attribute: components,
            normalize,
        };
        self.batch_table = Some(BatchTable::new(
            context,
            vec![
                attribute("batchTable_getWidthAndShow", ComponentDatatype::UnsignedByte, 2, false),
                attribute("batchTable_getPickColor", ComponentDatatype::UnsignedByte, 4, true),
                attribute("batchTable_getCenterHigh", ComponentDatatype::Float, 3, false),
                attribute("batchTable_getCenterLowAndRadius", ComponentDatatype::Float, 4, false),
                attribute("batchTable_getDistanceDisplayCondition", ComponentDatatype::Float, 2, false),
            ],
            self.polylines.len(),
        )?);
        Ok(())
    }

    /// `computeNewBuffersUsage`: stream positions for 100 frames after they
    /// last changed.
    fn compute_new_buffers_usage(&mut self) -> bool {
        let changed = self.shared.properties_changed.borrow()[POSITION_INDEX] > 0;
        let (usage, frames) = &mut self.position_buffer_usage;
        if changed {
            let usage_changed = *usage != BufferUsage::StreamDraw;
            *usage = BufferUsage::StreamDraw;
            *frames = 100;
            usage_changed
        } else if *usage != BufferUsage::StaticDraw {
            if *frames == 0 {
                *usage = BufferUsage::StaticDraw;
                true
            } else {
                *frames -= 1;
                false
            }
        } else {
            false
        }
    }

    /// `sortPolylinesIntoBuckets`: one bucket per material type, in first-use
    /// order, for polylines with at least two positions.
    fn sort_polylines_into_buckets(&mut self) {
        let mode = self.mode.unwrap_or(SceneMode::Scene3D);
        self.buckets.clear();
        for p in self.polylines.clone() {
            if p.0.borrow().actual_positions.len() <= 1 {
                continue;
            }
            p.update();
            let material = p.material();
            let material_type = material.borrow().type_name.clone();
            let index = match self.buckets.iter().position(|b| b.material_type == material_type) {
                Some(i) => i,
                None => {
                    self.buckets.push(PolylineBucket {
                        id: next_bucket_id(),
                        material_type,
                        polylines: Vec::new(),
                        length_of_positions: 0,
                        material,
                        shader_program: None,
                        defines: Vec::new(),
                        mode,
                        model_matrix: self.applied_model_matrix,
                    });
                    self.buckets.len() - 1
                }
            };
            let bucket = &mut self.buckets[index];
            let length = Self::positions_length(bucket.mode, &p);
            p.0.borrow_mut().actual_length = Some(length);
            p.0.borrow_mut().bucket = Some(bucket.id);
            bucket.length_of_positions += length;
            bucket.polylines.push(p);
        }
    }

    /// `getPolylinePositionsLength`.
    fn positions_length(mode: SceneMode, p: &Polyline) -> usize {
        if mode == SceneMode::Scene3D || !intersects_idl(p) {
            return (p.0.borrow().actual_positions.len() * 4).saturating_sub(4);
        }
        p.0.borrow().segments.1.iter().map(|l| (l * 4).saturating_sub(4)).sum()
    }

    /// `updateShader`.
    fn update_shader(
        bucket: &mut PolylineBucket<G>,
        context: &Context<G>,
        batch_table: &BatchTable<G>,
        highlight: bool,
    ) -> Result<()> {
        if bucket.shader_program.is_some() {
            return Ok(());
        }
        let source = bucket.material.borrow().shader_source.clone();
        let mut defines = vec!["DISTANCE_DISPLAY_CONDITION".to_string()];
        if highlight {
            defines.push("VECTOR_TILE".into());
        }
        if has_polyline_angle(&source) {
            defines.push("POLYLINE_DASH".into());
        }
        defines.push("CLIP_POLYLINE".into());
        let fragment = ShaderSource {
            defines: defines.clone(),
            sources: vec!["in vec4 v_pickColor;\n".into(), source, POLYLINE_FS.source.into()],
            ..Default::default()
        };
        let vertex = ShaderSource {
            defines: defines.clone(),
            sources: vec![POLYLINE_COMMON.source.into(), batch_table.vertex_shader_callback()(POLYLINE_VS.source)],
            ..Default::default()
        };
        let options = ShaderProgramOptions {
            vertex_shader_source: vertex,
            fragment_shader_source: fragment,
            attribute_locations: attribute_locations(),
        };
        bucket.shader_program = Some(context.with_shader_cache(|c| c.get_shader_program(&options))?);
        bucket.defines = defines;
        Ok(())
    }

    /// `getSegments`: the bucket's positions, projected (and the 2D bounding
    /// volume updated) outside 3D.
    fn segments(bucket: &PolylineBucket<G>, p: &Polyline, projection: &dyn MapProjection) -> Result<Segments> {
        let s = p.0.borrow();
        if bucket.mode == SceneMode::Scene3D {
            return Ok(Segments { positions: s.actual_positions.clone(), lengths: vec![s.actual_positions.len()] });
        }
        let source = if intersects_idl(p) { &s.segments.0 } else { &s.actual_positions };
        let ellipsoid = projection.ellipsoid();
        let mut positions = Vec::with_capacity(source.len());
        for position in source {
            let world = bucket.model_matrix.multiply_by_point(*position);
            let cartographic = ellipsoid
                .cartesian_to_cartographic(world)
                .ok_or_else(|| RuntimeError::new("Cannot project a polyline position at the ellipsoid's center."))?;
            positions.push(projection.project(&cartographic));
        }
        let lengths = s.segments.1.clone();
        drop(s);
        if !positions.is_empty() {
            let bv = BoundingSphere::from_points(&positions);
            p.0.borrow_mut().bounding_volume_2d =
                BoundingSphere::new(Cartesian3::new(bv.center.z, bv.center.x, bv.center.y), bv.radius);
        }
        Ok(Segments { positions, lengths })
    }

    /// `PolylineBucket.write`.
    #[allow(clippy::too_many_arguments)]
    fn write_bucket(
        bucket: &PolylineBucket<G>,
        positions_out: &mut [f32],
        tex: &mut [f32],
        mut position_index: usize,
        mut tex_index: usize,
        batch_table: &mut BatchTable<G>,
        context: &Context<G>,
        projection: &dyn MapProjection,
    ) -> Result<()> {
        let max_lon = projection.ellipsoid().maximum_radius * std::f64::consts::PI;
        for p in &bucket.polylines {
            let (width, show, index, r#loop, ddc) = {
                let s = p.0.borrow();
                (s.width, s.show && s.width > 0.0, s.index, s.r#loop, s.distance_display_condition)
            };
            let segments = Self::segments(bucket, p, projection)?;
            let pick = p.get_pick_id(context)?;
            let n = segments.positions.len();
            walk(
                &segments.positions,
                &segments.lengths,
                r#loop,
                bucket.mode,
                max_lon,
                |j, position, prev, next, start_k, end_k| {
                    for k in start_k..end_k {
                        write_elements(position, positions_out, position_index);
                        write_elements(prev, positions_out, position_index + 6);
                        write_elements(next, positions_out, position_index + 12);
                        let direction = if (k as i32) - 2 < 0 { -1.0 } else { 1.0 };
                        tex[tex_index] = (j as f64 / (n as f64 - 1.0)) as f32;
                        tex[tex_index + 1] = (2 * (k % 2)) as f32 - 1.0;
                        tex[tex_index + 2] = direction;
                        tex[tex_index + 3] = index as f32;
                        position_index += 18;
                        tex_index += 4;
                    }
                },
            );
            let byte = |v: f64| Color::float_to_byte(v) as f64;
            let color = Cartesian4::new(byte(pick.red), byte(pick.green), byte(pick.blue), byte(pick.alpha));
            let bv = if bucket.mode == SceneMode::Scene2D { p.bounding_volume_2d() } else { p.bounding_volume_wc() };
            let encoded = EncodedCartesian3::from_cartesian(bv.center);
            let low = Cartesian4::new(encoded.low.x, encoded.low.y, encoded.low.z, bv.radius);
            let near_far = ddc.map_or(Cartesian2::new(0.0, f64::MAX), |d| Cartesian2::new(d.near, d.far));
            batch_table.set_batched_attribute(index, 0, Cartesian2::new(width, if show { 1.0 } else { 0.0 }))?;
            batch_table.set_batched_attribute(index, 1, color)?;
            batch_table.set_batched_attribute(index, 2, encoded.high)?;
            batch_table.set_batched_attribute(index, 3, low)?;
            batch_table.set_batched_attribute(index, 4, near_far)?;
        }
        Ok(())
    }

    /// `PolylineBucket.writeForMorph`: the 3D positions while morphing.
    fn write_for_morph(bucket: &PolylineBucket<G>, out: &mut [f32], mut position_index: usize) {
        let m = bucket.model_matrix;
        for p in &bucket.polylines {
            let s = p.0.borrow();
            let positions = &s.segments.0;
            let lengths = &s.segments.1;
            walk(
                positions,
                lengths,
                s.r#loop,
                SceneMode::Scene3D,
                f64::INFINITY,
                |_, position, prev, next, start_k, end_k| {
                    let (position, prev, next) =
                        (m.multiply_by_point(position), m.multiply_by_point(prev), m.multiply_by_point(next));
                    for _ in start_k..end_k {
                        write_elements(position, out, position_index);
                        write_elements(prev, out, position_index + 6);
                        write_elements(next, out, position_index + 12);
                        position_index += 18;
                    }
                },
            );
        }
    }

    /// `PolylineBucket.updateIndices`.
    fn update_indices(
        bucket: &PolylineBucket<G>,
        total_indices: &mut Vec<Vec<u16>>,
        vertex_buffer_offset: &mut Vec<usize>,
        vertex_array_buckets: &mut Vec<Vec<Rc<RefCell<Locator>>>>,
        mut offset: usize,
    ) -> usize {
        let locator = |count, offset| Rc::new(RefCell::new(Locator { count, offset, bucket: bucket.id }));
        let mut bucket_locator = locator(0, offset);
        vertex_array_buckets.last_mut().unwrap().push(bucket_locator.clone());
        let mut count = 0;
        let mut indices_count = total_indices.last().unwrap().last().map_or(0, |&i| i as usize + 1);
        for p in &bucket.polylines {
            p.0.borrow_mut().locator_buckets.clear();
            let segments: Vec<usize> = if bucket.mode == SceneMode::Scene3D {
                let n = p.0.borrow().actual_positions.len();
                if n == 0 {
                    continue;
                }
                vec![n]
            } else {
                p.0.borrow().segments.1.clone()
            };
            if !segments.is_empty() {
                let mut segment_index_count = 0;
                for &length in &segments {
                    for _ in 0..length.saturating_sub(1) {
                        if indices_count + 4 > SIXTY_FOUR_KILOBYTES {
                            p.0.borrow_mut().locator_buckets.push((bucket_locator.clone(), segment_index_count));
                            segment_index_count = 0;
                            vertex_buffer_offset.push(4);
                            total_indices.push(Vec::new());
                            indices_count = 0;
                            bucket_locator.borrow_mut().count = count;
                            count = 0;
                            offset = 0;
                            bucket_locator = locator(0, 0);
                            vertex_array_buckets.push(vec![bucket_locator.clone()]);
                        }
                        let i = indices_count as u16;
                        total_indices.last_mut().unwrap().extend([i, i + 2, i + 1, i + 1, i + 2, i + 3]);
                        segment_index_count += 6;
                        count += 6;
                        offset += 6;
                        indices_count += 4;
                    }
                }
                p.0.borrow_mut().locator_buckets.push((bucket_locator.clone(), segment_index_count));
                if indices_count + 4 > SIXTY_FOUR_KILOBYTES {
                    vertex_buffer_offset.push(0);
                    total_indices.push(Vec::new());
                    indices_count = 0;
                    bucket_locator.borrow_mut().count = count;
                    offset = 0;
                    count = 0;
                    bucket_locator = locator(0, 0);
                    vertex_array_buckets.push(vec![bucket_locator.clone()]);
                }
            }
            p.clean();
        }
        bucket_locator.borrow_mut().count = count;
        offset
    }

    /// `createVertexArrays`.
    fn create_vertex_arrays(&mut self, context: &Context<G>, projection: &dyn MapProjection) -> Result<()> {
        self.create_vertex_array = false;
        self.release_shaders();
        for entry in self.vertex_arrays.drain(..) {
            entry.va.destroy()?;
        }
        self.sort_polylines_into_buckets();
        let mut total_indices: Vec<Vec<u16>> = vec![Vec::new()];
        let mut vertex_buffer_offset = vec![0usize];
        let mut vertex_array_buckets: Vec<Vec<Rc<RefCell<Locator>>>> = vec![Vec::new()];
        let mut offset = 0;
        let highlight = self.use_highlight_color;
        let batch_table = self.batch_table.as_mut().expect("the batch table exists");
        let mut total_length = 0;
        for bucket in &mut self.buckets {
            Self::update_shader(bucket, context, batch_table, highlight)?;
            total_length += bucket.length_of_positions;
        }
        if total_length == 0 {
            return Ok(());
        }
        let mode = self.mode.unwrap_or(SceneMode::Scene3D);
        let mut position_array = vec![0f32; 6 * total_length * 3];
        let mut tex_array = vec![0f32; total_length * 4];
        let mut position_3d_array: Option<Vec<f32>> = None;
        let (mut position_index, mut tex_index) = (0, 0);
        for bucket in &self.buckets {
            Self::write_bucket(
                bucket,
                &mut position_array,
                &mut tex_array,
                position_index,
                tex_index,
                batch_table,
                context,
                projection,
            )?;
            if mode == SceneMode::Morphing {
                let morph = position_3d_array.get_or_insert_with(|| vec![0f32; 6 * total_length * 3]);
                Self::write_for_morph(bucket, morph, position_index);
            }
            position_index += 6 * bucket.length_of_positions * 3;
            tex_index += bucket.length_of_positions * 4;
            offset = Self::update_indices(
                bucket,
                &mut total_indices,
                &mut vertex_buffer_offset,
                &mut vertex_array_buckets,
                offset,
            );
        }
        let _ = offset;
        let floats = |v: &[f32]| v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>();
        let usage = self.position_buffer_usage.0;
        let position_buffer =
            Buffer::new(context, BufferKind::Vertex, usage, BufferData::Bytes(&floats(&position_array)))?;
        let position_3d_buffer = position_3d_array
            .map(|a| Buffer::new(context, BufferKind::Vertex, usage, BufferData::Bytes(&floats(&a))))
            .transpose()?;
        let tex_buffer =
            Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&floats(&tex_array)))?;
        const POSITION_SIZE: usize = 3 * 4;
        const TEX_SIZE: usize = 4 * 4;
        let mut vbo = 0;
        for (k, indices) in total_indices.iter().enumerate() {
            if indices.is_empty() {
                continue;
            }
            let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let index_buffer = Buffer::new(
                context,
                BufferKind::Index(IndexDatatype::UnsignedShort),
                BufferUsage::StaticDraw,
                BufferData::Bytes(&bytes),
            )?;
            vbo += vertex_buffer_offset[k];
            let high = 6 * (k * (POSITION_SIZE * SIXTY_FOUR_KILOBYTES) - vbo * POSITION_SIZE);
            let low = POSITION_SIZE + high;
            let prev_high = POSITION_SIZE + low;
            let prev_low = POSITION_SIZE + prev_high;
            let next_high = POSITION_SIZE + prev_low;
            let next_low = POSITION_SIZE + next_high;
            let tex_offset = k * (TEX_SIZE * SIXTY_FOUR_KILOBYTES) - vbo * TEX_SIZE;
            // (location, offset, is 3D)
            let layout = [
                (POSITION_3D_HIGH, high, true),
                (POSITION_3D_LOW, low, true),
                (POSITION_2D_HIGH, high, false),
                (POSITION_2D_LOW, low, false),
                (PREV_POSITION_3D_HIGH, prev_high, true),
                (PREV_POSITION_3D_LOW, prev_low, true),
                (PREV_POSITION_2D_HIGH, prev_high, false),
                (PREV_POSITION_2D_LOW, prev_low, false),
                (NEXT_POSITION_3D_HIGH, next_high, true),
                (NEXT_POSITION_3D_LOW, next_low, true),
                (NEXT_POSITION_2D_HIGH, next_high, false),
                (NEXT_POSITION_2D_LOW, next_low, false),
            ];
            let mut attributes = Vec::with_capacity(13);
            for (location, attribute_offset, is_3d) in layout {
                let buffer = match (mode, is_3d) {
                    (SceneMode::Scene3D, true) => Some(position_buffer.clone()),
                    (SceneMode::Scene3D, false) => None,
                    (SceneMode::Scene2D | SceneMode::ColumbusView, true) => None,
                    (SceneMode::Scene2D | SceneMode::ColumbusView, false) => Some(position_buffer.clone()),
                    (_, true) => position_3d_buffer.clone(),
                    (_, false) => Some(position_buffer.clone()),
                };
                let mut attribute = match buffer {
                    Some(buffer) => VertexAttribute::buffer(location, buffer, 3),
                    None => VertexAttribute::constant(location, vec![0.0, 0.0, 0.0]),
                };
                if matches!(attribute.source, AttributeSource::Buffer(_)) {
                    attribute.offset = attribute_offset;
                    attribute.stride = 6 * POSITION_SIZE;
                }
                attributes.push(attribute);
            }
            let mut tex = VertexAttribute::buffer(TEX_COORD_EXPAND_AND_BATCH_INDEX, tex_buffer.clone(), 4);
            tex.offset = tex_offset;
            attributes.push(tex);
            let va = Rc::new(VertexArray::new(context, attributes, Some(index_buffer))?);
            self.vertex_arrays.push(VertexArrayEntry { va, buckets: vertex_array_buckets[k].clone() });
        }
        self.position_buffer = Some(position_buffer);
        Ok(())
    }

    /// `PolylineBucket.writeUpdate`: rewrite one polyline's positions.
    fn write_update(
        bucket: &PolylineBucket<G>,
        index: usize,
        p: &Polyline,
        buffer: &Buffer<G>,
        projection: &dyn MapProjection,
    ) -> Result<()> {
        let Some(length) = p.actual_length().filter(|&l| l > 0) else { return Ok(()) };
        let start = index
            + bucket
                .polylines
                .iter()
                .take_while(|q| !q.ptr_eq(p))
                .map(|q| q.actual_length().unwrap_or(0))
                .sum::<usize>();
        let max_lon = projection.ellipsoid().maximum_radius * std::f64::consts::PI;
        let mut out = vec![0f32; 6 * length * 3];
        let segments = Self::segments(bucket, p, projection)?;
        let r#loop = p.r#loop();
        let mut position_index = 0;
        walk(
            &segments.positions,
            &segments.lengths,
            r#loop,
            bucket.mode,
            max_lon,
            |_, position, prev, next, start_k, end_k| {
                for _ in start_k..end_k {
                    if position_index + 18 > out.len() {
                        return;
                    }
                    write_elements(position, &mut out, position_index);
                    write_elements(prev, &mut out, position_index + 6);
                    write_elements(next, &mut out, position_index + 12);
                    position_index += 18;
                }
            },
        );
        let bytes: Vec<u8> = out.iter().flat_map(|f| f.to_le_bytes()).collect();
        buffer.copy_from_array_view(&bytes, 6 * 3 * 4 * start)
    }

    fn update_frame(&mut self, frame: &mut FrameState) -> Result<()> {
        self.check()?;
        self.compact();
        self.commands.clear();
        if self.polylines.is_empty() || !self.show {
            return Ok(());
        }
        // updateMode
        let model_matrix = self.shared.model_matrix.get();
        if self.mode != Some(frame.mode) || self.applied_model_matrix != model_matrix {
            self.mode = Some(frame.mode);
            self.applied_model_matrix = model_matrix;
            self.create_vertex_array = true;
        }
        let context = frame
            .context
            .as_ref()
            .and_then(|c| c.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("frameState.context is required."))?;
        let projection = frame.uniform.map_projection.clone();
        if self.create_batch_table {
            if context.limits()?.maximum_vertex_texture_image_units == 0 {
                return Err(RuntimeError::new(
                    "Vertex texture fetch support is required to render polylines. The maximum number of vertex texture image units must be greater than zero.",
                ));
            }
            self.create_batch_table(&context)?;
            self.create_batch_table = false;
        }

        if self.create_vertex_array || self.compute_new_buffers_usage() {
            self.create_vertex_arrays(&context, projection.as_ref())?;
        } else if self.shared.updated.get() {
            let to_update = self.shared.to_update.borrow().clone();
            if self.mode != Some(SceneMode::Scene3D) {
                for p in &to_update {
                    p.update();
                }
            }
            let changed = *self.shared.properties_changed.borrow();
            if changed[POSITION_SIZE_INDEX] > 0 || changed[MATERIAL_INDEX] > 0 {
                self.create_vertex_arrays(&context, projection.as_ref())?;
            } else {
                for p in &to_update {
                    let properties = p.properties_changed();
                    let bucket_id = p.0.borrow().bucket;
                    let mut index = 0;
                    for bucket in &self.buckets {
                        if Some(bucket.id) == bucket_id {
                            if properties[POSITION_INDEX] > 0 {
                                let buffer = self.position_buffer.as_ref().expect("positions were written");
                                Self::write_update(bucket, index, p, buffer, projection.as_ref())?;
                            }
                            break;
                        }
                        index += bucket.length_of_positions;
                    }
                    let table = self.batch_table.as_mut().expect("the batch table exists");
                    let s = p.0.borrow();
                    if properties[SHOW_INDEX] > 0 || properties[WIDTH_INDEX] > 0 {
                        // Cesium passes `show` as a boolean, which never
                        // equals the stored byte, so this always writes.
                        table.set_batched_attribute_unchecked(
                            s.index,
                            0,
                            Cartesian2::new(s.width, if s.show { 1.0 } else { 0.0 }),
                        )?;
                    }
                    if properties[POSITION_INDEX] > 0 || properties[POSITION_SIZE_INDEX] > 0 {
                        let bv =
                            if frame.mode == SceneMode::Scene2D { s.bounding_volume_2d } else { s.bounding_volume_wc };
                        let encoded = EncodedCartesian3::from_cartesian(bv.center);
                        table.set_batched_attribute(s.index, 2, encoded.high)?;
                        table.set_batched_attribute(
                            s.index,
                            3,
                            Cartesian4::new(encoded.low.x, encoded.low.y, encoded.low.z, bv.radius),
                        )?;
                    }
                    if properties[DISTANCE_DISPLAY_CONDITION_INDEX] > 0 {
                        let near_far = s
                            .distance_display_condition
                            .map_or(Cartesian2::new(0.0, f64::MAX), |d| Cartesian2::new(d.near, d.far));
                        table.set_batched_attribute(s.index, 4, near_far)?;
                    }
                    drop(s);
                    p.clean();
                }
            }
            self.shared.to_update.borrow_mut().clear();
            self.shared.updated.set(false);
        }
        *self.shared.properties_changed.borrow_mut() = [0; NUMBER_OF_PROPERTIES];

        let model_matrix = if frame.mode == SceneMode::Scene3D { model_matrix } else { Matrix4::IDENTITY };
        let use_depth_test = frame.morph_time != 0.0;
        if self.opaque_rs.as_ref().is_none_or(|rs| rs.depth_test.enabled != use_depth_test) {
            self.opaque_rs = Some(RenderState {
                depth_mask: use_depth_test,
                depth_test: DepthTest { enabled: use_depth_test, ..Default::default() },
                ..Default::default()
            });
        }
        if self.translucent_rs.as_ref().is_none_or(|rs| rs.depth_test.enabled != use_depth_test) {
            self.translucent_rs = Some(RenderState {
                blending: blending_state::alpha_blend(),
                depth_mask: !use_depth_test,
                depth_test: DepthTest { enabled: use_depth_test, ..Default::default() },
                ..Default::default()
            });
        }
        self.batch_table.as_mut().expect("the batch table exists").update(&context)?;
        if frame.passes.render || frame.passes.pick {
            self.create_command_lists(frame, &context, model_matrix)?;
        }
        Ok(())
    }

    /// The bounding volume a polyline contributes in `mode`.
    fn polyline_volume(mode: SceneMode, p: &Polyline) -> BoundingSphere {
        let s = p.0.borrow();
        match mode {
            SceneMode::Scene3D => s.bounding_volume_wc,
            SceneMode::ColumbusView => s.bounding_volume_2d,
            SceneMode::Scene2D => {
                let mut bv = s.bounding_volume_2d;
                bv.center.x = 0.0;
                bv
            }
            SceneMode::Morphing => s.bounding_volume_wc.union(&s.bounding_volume_2d),
        }
    }

    /// `createCommandLists`: one command per run of polylines with the same
    /// material id, per bucket range, per vertex array. As in Cesium, the
    /// bounding sphere accumulates in one scratch sphere whose "clone next"
    /// flag is reset only when a command is emitted.
    fn create_command_lists(
        &mut self,
        frame: &mut FrameState,
        context: &Context<G>,
        model_matrix: Matrix4,
    ) -> Result<()> {
        let table = self.batch_table.as_ref().expect("the batch table exists");
        let batch_uniforms = table.uniform_values();
        let batch_texture = table.texture().cloned();
        let batch_names = table.uniform_names();
        let mut scratch = BoundingSphere::default();
        let mut clone_bounding_sphere = true;
        let mut out: Vec<PolylineCommand> = Vec::new();
        for (va_index, entry) in self.vertex_arrays.iter().enumerate() {
            for locator in &entry.buckets {
                let bucket_id = locator.borrow().bucket;
                let Some(bucket) = self.buckets.iter().find(|b| b.id == bucket_id) else { continue };
                let program = bucket
                    .shader_program
                    .clone()
                    .ok_or_else(|| RuntimeError::new("Polyline shader is not compiled"))?;
                let mut offset = locator.borrow().offset;
                let mut current: Option<(String, SharedMaterial)> = None;
                let mut count = 0;
                let emit = |material: &SharedMaterial,
                            count: usize,
                            offset: usize,
                            volume: BoundingSphere,
                            list: &mut Vec<SceneCommand>,
                            out: &mut Vec<PolylineCommand>| {
                    let translucent = material.borrow().is_translucent();
                    let state = if translucent { self.translucent_rs.clone() } else { self.opaque_rs.clone() }.unwrap();
                    let pass = if translucent { Pass::Translucent } else { Pass::Opaque };
                    let material_uniforms = material_uniform_values(&material.borrow());
                    let mut names: Vec<String> = material_uniforms.iter().map(|(n, _)| n.clone()).collect();
                    names.extend(batch_names.iter().map(|n| n.to_string()));
                    names.push("u_highlightColor".into());
                    names.sort();
                    out.push(PolylineCommand {
                        pass,
                        count,
                        offset,
                        vertex_array: va_index,
                        bucket: bucket.material_type.clone(),
                        render_state: state.clone(),
                        bounding_volume: volume,
                        model_matrix,
                        pick_id: "v_pickColor".into(),
                        debug_show_bounding_volume: self.debug_show_bounding_volume,
                        uniforms: names,
                    });
                    let mut options = SceneDrawOptions::new(entry.va.clone(), program.clone());
                    options.state = state;
                    options.primitive_type = PrimitiveType::Triangles;
                    options.count = Some(count);
                    options.offset = offset;
                    let h = self.highlight_color;
                    options.uniforms = material_uniforms;
                    options.uniforms.extend(batch_uniforms.iter().cloned());
                    options.uniforms.push((
                        "u_highlightColor".into(),
                        UniformValue::Float(vec![h.red as f32, h.green as f32, h.blue as f32, h.alpha as f32]),
                    ));
                    if let Some(texture) = &batch_texture {
                        options.textures.push(("batchTexture".into(), texture.clone() as Rc<dyn SampledTexture<G>>));
                    }
                    let metadata = DrawMetadata {
                        bounding_volume: Some(volume),
                        model_matrix: Some(model_matrix),
                        owner: Some(format!("PolylineCollection#{}", self.shared.id)),
                        pick_id: Some("v_pickColor".into()),
                        debug_show_bounding_volume: self.debug_show_bounding_volume,
                        ..Default::default()
                    };
                    list.push(SceneCommand::from_draw(
                        format!("polyline-collection-{}-{}", self.shared.id, out.len()),
                        context.clone(),
                        pass,
                        metadata,
                        options,
                    ));
                };
                for p in &bucket.polylines {
                    let material = p.material();
                    let id = material_id(&material.borrow());
                    if current.as_ref().map(|(c, _)| c) != Some(&id) {
                        if let Some((_, m)) = &current {
                            if count > 0 {
                                emit(m, count, offset, scratch, &mut frame.command_list, &mut out);
                                offset += count;
                                count = 0;
                                clone_bounding_sphere = true;
                            }
                        }
                        // `Material.update` reads `context.defaultTexture`,
                        // creating it on first use.
                        context.default_texture()?;
                        material.borrow_mut().update_textures()?;
                        current = Some((id, material));
                    }
                    for (l, c) in &p.0.borrow().locator_buckets {
                        if Rc::ptr_eq(l, locator) {
                            count += c;
                        }
                    }
                    let volume = Self::polyline_volume(frame.mode, p);
                    if clone_bounding_sphere {
                        clone_bounding_sphere = false;
                        scratch = volume;
                    } else {
                        scratch = volume.union(&scratch);
                    }
                }
                if let Some((_, m)) = &current {
                    if count > 0 {
                        emit(m, count, offset, scratch, &mut frame.command_list, &mut out);
                        clone_bounding_sphere = true;
                    }
                }
            }
        }
        self.commands = out;
        Ok(())
    }

    /// The commands queued by the last update.
    pub fn commands(&self) -> &[PolylineCommand] {
        &self.commands
    }
    pub fn buckets(&self) -> &[PolylineBucket<G>] {
        &self.buckets
    }
    /// The vertex arrays, in command order.
    pub fn vertex_arrays(&self) -> Vec<Rc<VertexArray<G>>> {
        self.vertex_arrays.iter().map(|e| e.va.clone()).collect()
    }
    /// The vertex arrays and, for each, its bucket ranges as
    /// `(count, offset, material type)`.
    pub fn vertex_array_ranges(&self) -> Vec<Vec<(usize, usize, Option<String>)>> {
        self.vertex_arrays
            .iter()
            .map(|e| {
                e.buckets
                    .iter()
                    .map(|l| {
                        let l = l.borrow();
                        let t = self.buckets.iter().find(|b| b.id == l.bucket).map(|b| b.material_type.clone());
                        (l.count, l.offset, t)
                    })
                    .collect()
            })
            .collect()
    }
    /// `_positionBufferUsage`: the usage and the frames left before it
    /// returns to static.
    pub fn position_buffer_usage(&self) -> (BufferUsage, u32) {
        self.position_buffer_usage
    }
    pub fn batch_table(&self) -> Option<&BatchTable<G>> {
        self.batch_table.as_ref()
    }
    /// `_polylinesUpdated`.
    pub fn polylines_updated(&self) -> bool {
        self.shared.updated.get()
    }
    /// `_polylinesToUpdate`.
    pub fn polylines_to_update(&self) -> Vec<Polyline> {
        self.shared.to_update.borrow().clone()
    }
    /// The material type of a polyline's bucket, `"stale"` for a bucket of an
    /// earlier rebuild, `None` before any.
    pub fn bucket_of(&self, polyline: &Polyline) -> Option<String> {
        let id = polyline.0.borrow().bucket?;
        Some(self.buckets.iter().find(|b| b.id == id).map_or("stale".to_string(), |b| b.material_type.clone()))
    }
}

/// `/in\s+float\s+v_polylineAngle;/`.
fn has_polyline_angle(source: &str) -> bool {
    let mut rest = source;
    while let Some(i) = rest.find("in") {
        let after = &rest[i + 2..];
        let trimmed = after.trim_start();
        if trimmed.len() < after.len() && trimmed.starts_with("float") {
            let after_float = &trimmed[5..];
            let name = after_float.trim_start();
            if name.len() < after_float.len() && name.starts_with("v_polylineAngle;") {
                return true;
            }
        }
        rest = &rest[i + 2..];
    }
    false
}

/// `createMaterialId`: the type and the uniform values. Materials with the
/// same id share a command.
fn material_id(material: &crate::scene::material::Material) -> String {
    format!("{}:{:?}", material.type_name, material.uniform_bindings())
}

/// The material's numeric uniforms by generated name.
fn material_uniform_values(material: &crate::scene::material::Material) -> Vec<(String, UniformValue)> {
    material
        .uniform_bindings()
        .into_iter()
        .filter_map(|(name, value)| {
            let numbers = || value.numbers().unwrap_or_default().into_iter().map(|v| v as f32).collect();
            let uniform = match &value {
                MaterialValue::Bool(v) => UniformValue::Int(vec![i32::from(*v)]),
                MaterialValue::IntVec3(v) => UniformValue::Int(vec![v.x as i32, v.y as i32, v.z as i32]),
                MaterialValue::Mat2(_) => UniformValue::Matrix { dimension: 2, values: numbers() },
                MaterialValue::Mat3(_) => UniformValue::Matrix { dimension: 3, values: numbers() },
                MaterialValue::Mat4(_) => UniformValue::Matrix { dimension: 4, values: numbers() },
                MaterialValue::Channels(_) | MaterialValue::Sampler2D(_) | MaterialValue::SamplerCube(_) => {
                    return None
                }
                _ => UniformValue::Float(numbers()),
            };
            Some((name, uniform))
        })
        .collect()
}

impl<G: Gl + 'static> ScenePrimitive for PolylineCollection<G> {
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
    fn destroy(&mut self) -> Result<()> {
        self.check()?;
        for entry in self.vertex_arrays.drain(..) {
            entry.va.destroy()?;
        }
        self.release_shaders();
        for p in &self.polylines {
            if !p.is_destroyed() {
                p.destroy();
            }
        }
        if let Some(mut table) = self.batch_table.take() {
            table.destroy()?;
        }
        self.commands.clear();
        self.destroyed = true;
        Ok(())
    }
}
