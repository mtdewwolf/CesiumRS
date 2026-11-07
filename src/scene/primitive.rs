//! CesiumJS `Primitive`: geometry instances combined into vertex arrays and
//! drawn with an [`Appearance`], with per-instance attributes (color, show,
//! distance display condition, offset, pick color and any others the
//! instances share) in a [`BatchTable`].
//!
//! The update follows Cesium: create the batch table, create and combine the
//! geometry (synchronously, or over three updates when `asynchronous`, where
//! Cesium waits for its workers), fill the batch bounding spheres and 2D
//! offsets, create the vertex arrays, then build render states, shader
//! programs and commands whenever the appearance, material or translucency
//! changes, and queue the commands with the bounding volumes of the scene
//! mode. The shader text, uploads, render states, commands, bounding volumes
//! and per-instance attributes are checked against a SwiftShader capture of
//! the pinned engine in `tests/scene_primitive.rs`.
//!
//! Without workers, `asynchronous` resolves Cesium's promises at the next
//! update: the geometry is created on the first update, combined on the
//! second and drawn from the third.
use std::{collections::BTreeMap, rc::Rc};

use regex::Regex;

use super::{
    appearance::Appearance,
    batch_table::{BatchTable, BatchTableAttribute, BatchValue},
    primitive_pipeline::{CombineGeometryParameters, PrimitivePipeline},
    primitive_state::PrimitiveState,
    scene::{Result, SceneDrawOptions, ScenePrimitive},
    CullFace, DepthFunction, FrameState, MetadataValue, SceneCommand, SceneMode, ScenePickObject, ShadowMode,
};
use crate::{
    core::{
        BoundingSphere, Cartesian2, Cartesian3, Cartesian4, Color, ComponentDatatype, EncodedCartesian3, Geometry,
        GeometryInstance, GeometryOffsetAttribute, Intersect, Matrix4, Plane, RuntimeError, TypedArray,
    },
    renderer::{
        BufferUsage, Context, DrawMetadata, GeometryUploadOptions, Gl, Pass, PickId, RenderState, SampledTexture,
        ShaderProgram, ShaderProgramOptions, ShaderSource, UniformFrustum, UniformValue, VertexArray,
    },
};

/// Options for creating a [`Primitive`].
pub struct PrimitiveOptions {
    pub geometry_instances: Vec<GeometryInstance>,
    pub appearance: Option<Box<dyn Appearance>>,
    pub depth_fail_appearance: Option<Box<dyn Appearance>>,
    pub show: Option<bool>,
    pub model_matrix: Option<Matrix4>,
    pub vertex_cache_optimize: Option<bool>,
    pub interleave: Option<bool>,
    pub compress_vertices: Option<bool>,
    pub release_geometry_instances: Option<bool>,
    pub allow_picking: Option<bool>,
    pub cull: Option<bool>,
    pub asynchronous: Option<bool>,
    pub debug_show_bounding_volume: Option<bool>,
    pub shadows: Option<ShadowMode>,
    /// Draw relative to this center (one instance, 3D-only scenes).
    pub rtc_center: Option<Cartesian3>,
}

impl Default for PrimitiveOptions {
    fn default() -> Self {
        Self {
            geometry_instances: Vec::new(),
            appearance: None,
            depth_fail_appearance: None,
            show: Some(true),
            model_matrix: Some(Matrix4::IDENTITY),
            vertex_cache_optimize: Some(false),
            interleave: Some(false),
            compress_vertices: Some(true),
            release_geometry_instances: Some(true),
            allow_picking: Some(true),
            cull: Some(true),
            asynchronous: Some(true),
            debug_show_bounding_volume: Some(false),
            shadows: Some(ShadowMode::Disabled),
            rtc_center: None,
        }
    }
}

/// Which render state a command draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveRenderState {
    Front,
    Back,
    FrontDepthFail,
    BackDepthFail,
}

/// What the primitive put in one color command.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveCommand {
    pub pass: Pass,
    pub vertex_array: usize,
    pub render_state: PrimitiveRenderState,
    /// Drawn with the depth-fail program rather than the color program.
    pub depth_fail: bool,
    pub bounding_volume: BoundingSphere,
    pub model_matrix: Matrix4,
    pub cull: bool,
    pub debug_show_bounding_volume: bool,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    pub pick_id: Option<String>,
    pub uniforms: Vec<String>,
}

/// The color and depth-fail shader programs, when compiled.
pub type PrimitiveShaderPrograms<G> = (Option<Rc<ShaderProgram<G>>>, Option<Rc<ShaderProgram<G>>>);

/// The vertex and fragment shader text of a program.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveShader {
    pub vertex: String,
    pub fragment: String,
}

/// The object a primitive registers with `Context.createPickId` for each
/// instance.
pub struct PrimitivePick {
    pub primitive: u64,
    pub id: Option<String>,
}

/// The bounding spheres the primitive keeps per vertex array.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrimitiveBoundingSpheres {
    pub local: Vec<BoundingSphere>,
    pub wc: Vec<BoundingSphere>,
    pub cv: Vec<BoundingSphere>,
    pub two_d: Vec<BoundingSphere>,
    pub morph: Vec<BoundingSphere>,
}

#[derive(Clone, Copy, Debug, Default)]
struct BoundingSphereIndices {
    center_3d_high: usize,
    center_3d_low: usize,
    center_2d_high: usize,
    center_2d_low: usize,
    radius: usize,
}

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct Primitive<G: Gl + 'static> {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub cull: bool,
    pub debug_show_bounding_volume: bool,
    pub shadows: ShadowMode,
    pub rtc_center: Option<Cartesian3>,
    appearance: Option<Box<dyn Appearance>>,
    appearance_version: u64,
    applied_appearance: Option<u64>,
    depth_fail_appearance: Option<Box<dyn Appearance>>,
    depth_fail_version: u64,
    applied_depth_fail: Option<Option<u64>>,
    vertex_cache_optimize: bool,
    interleave: bool,
    compress_vertices: bool,
    release_geometry_instances: bool,
    allow_picking: bool,
    asynchronous: bool,
    id: u64,
    geometry_instances: Option<Vec<GeometryInstance>>,
    created_instances: Option<Vec<GeometryInstance>>,
    combined: Option<super::primitive_pipeline::CombineGeometryResults>,
    translucent: Option<bool>,
    state: PrimitiveState,
    ready: bool,
    ready_flag: Rc<std::cell::Cell<Option<bool>>>,
    error: Option<RuntimeError>,
    number_of_instances: usize,
    spheres: PrimitiveBoundingSpheres,
    applied_model_matrix: Option<Matrix4>,
    instance_ids: Vec<Option<String>>,
    va: Vec<Rc<VertexArray<G>>>,
    primitive_type: Option<crate::core::PrimitiveType>,
    attribute_locations: BTreeMap<String, u32>,
    front_rs: Option<RenderState>,
    back_rs: Option<RenderState>,
    front_depth_fail_rs: Option<RenderState>,
    back_depth_fail_rs: Option<RenderState>,
    sp: Option<(Rc<ShaderProgram<G>>, PrimitiveShader)>,
    sp_depth_fail: Option<(Rc<ShaderProgram<G>>, PrimitiveShader)>,
    pick_ids: Vec<PickId>,
    color_commands: Vec<(usize, PrimitiveRenderState, bool, Pass)>,
    uniforms: Vec<(String, UniformValue)>,
    depth_fail_uniforms: Vec<(String, UniformValue)>,
    commands: Vec<PrimitiveCommand>,
    batch_table: Option<BatchTable<G>>,
    batch_indices: Vec<(String, usize)>,
    bounding_sphere_indices: Option<BoundingSphereIndices>,
    offset_2d_index: Option<usize>,
    offsets_updated: bool,
    offset_instance_extend: Vec<Option<GeometryOffsetAttribute>>,
    instance_bounding_spheres: Vec<Option<BoundingSphere>>,
    instance_bounding_spheres_cv: Vec<Option<BoundingSphere>>,
    recompute_bounding_spheres: bool,
    bounding_spheres_updated: bool,
    destroyed: bool,
}

/// `(\b)name(\b)` replaced by `$1replacement$2` everywhere.
fn replace_word(source: &str, word: &str, replacement: &str) -> String {
    Regex::new(&format!(r"\b{word}\b")).unwrap().replace_all(source, regex::NoExpand(replacement)).into_owned()
}

fn remove_all(source: &str, pattern: &str) -> String {
    Regex::new(pattern).unwrap().replace_all(source, "").into_owned()
}

fn contains(source: &str, pattern: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(source)
}

impl<G: Gl + 'static> Primitive<G> {
    pub fn new(options: PrimitiveOptions) -> Self {
        Self {
            show: options.show.unwrap_or(true),
            model_matrix: options.model_matrix.unwrap_or(Matrix4::IDENTITY),
            cull: options.cull.unwrap_or(true),
            debug_show_bounding_volume: options.debug_show_bounding_volume.unwrap_or(false),
            shadows: options.shadows.unwrap_or(ShadowMode::Disabled),
            rtc_center: options.rtc_center,
            appearance: options.appearance,
            appearance_version: 1,
            applied_appearance: None,
            depth_fail_appearance: options.depth_fail_appearance,
            depth_fail_version: 1,
            applied_depth_fail: None,
            vertex_cache_optimize: options.vertex_cache_optimize.unwrap_or(false),
            interleave: options.interleave.unwrap_or(false),
            compress_vertices: options.compress_vertices.unwrap_or(true),
            release_geometry_instances: options.release_geometry_instances.unwrap_or(true),
            allow_picking: options.allow_picking.unwrap_or(true),
            asynchronous: options.asynchronous.unwrap_or(true),
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            geometry_instances: Some(options.geometry_instances),
            created_instances: None,
            combined: None,
            translucent: None,
            state: PrimitiveState::Ready,
            ready: false,
            ready_flag: Rc::default(),
            error: None,
            number_of_instances: 0,
            spheres: PrimitiveBoundingSpheres::default(),
            applied_model_matrix: None,
            instance_ids: Vec::new(),
            va: Vec::new(),
            primitive_type: None,
            attribute_locations: BTreeMap::new(),
            front_rs: None,
            back_rs: None,
            front_depth_fail_rs: None,
            back_depth_fail_rs: None,
            sp: None,
            sp_depth_fail: None,
            pick_ids: Vec::new(),
            color_commands: Vec::new(),
            uniforms: Vec::new(),
            depth_fail_uniforms: Vec::new(),
            commands: Vec::new(),
            batch_table: None,
            batch_indices: Vec::new(),
            bounding_sphere_indices: None,
            offset_2d_index: None,
            offsets_updated: false,
            offset_instance_extend: Vec::new(),
            instance_bounding_spheres: Vec::new(),
            instance_bounding_spheres_cv: Vec::new(),
            recompute_bounding_spheres: false,
            bounding_spheres_updated: false,
            destroyed: false,
        }
    }

    pub fn state(&self) -> PrimitiveState {
        self.state
    }
    /// `ready`: set after the frame in which the primitive completed or failed.
    pub fn ready(&self) -> bool {
        self.ready_flag.get().unwrap_or(self.ready)
    }
    pub fn is_ready(&self) -> bool {
        self.ready()
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn vertex_cache_optimize(&self) -> bool {
        self.vertex_cache_optimize
    }
    pub fn interleave(&self) -> bool {
        self.interleave
    }
    pub fn release_geometry_instances(&self) -> bool {
        self.release_geometry_instances
    }
    pub fn allow_picking(&self) -> bool {
        self.allow_picking
    }
    pub fn asynchronous(&self) -> bool {
        self.asynchronous
    }
    pub fn compress_vertices(&self) -> bool {
        self.compress_vertices
    }
    /// The geometry instances, until released after the vertex arrays exist.
    pub fn geometry_instances(&self) -> Option<&[GeometryInstance]> {
        self.geometry_instances.as_deref()
    }
    pub fn appearance(&self) -> Option<&dyn Appearance> {
        self.appearance.as_deref()
    }
    /// Replaces the appearance; render states, shaders and commands are
    /// rebuilt on the next update.
    pub fn set_appearance(&mut self, appearance: Option<Box<dyn Appearance>>) {
        self.appearance = appearance;
        self.appearance_version += 1;
    }
    pub fn depth_fail_appearance(&self) -> Option<&dyn Appearance> {
        self.depth_fail_appearance.as_deref()
    }
    pub fn set_depth_fail_appearance(&mut self, appearance: Option<Box<dyn Appearance>>) {
        self.depth_fail_appearance = appearance;
        self.depth_fail_version += 1;
    }

    /// `setReady`: the state now; `ready` after the frame.
    fn set_ready(&mut self, frame: &mut FrameState, state: PrimitiveState, error: Option<RuntimeError>) {
        self.error = error;
        self.state = state;
        let flag = self.ready_flag.clone();
        let ready = matches!(state, PrimitiveState::Complete | PrimitiveState::Failed);
        frame.after_render.push(Box::new(move || {
            flag.set(Some(ready));
            Ok(true)
        }));
    }

    /// `getCommonPerInstanceAttributeNames`.
    fn common_attribute_names(instances: &[GeometryInstance]) -> Vec<String> {
        let first = &instances[0].attributes;
        first
            .iter()
            .filter(|(name, a)| {
                instances[1..].iter().all(|i| {
                    i.attributes.iter().find(|(n, _)| n == name).is_some_and(|(_, o)| {
                        o.component_datatype == a.component_datatype
                            && o.components_per_attribute == a.components_per_attribute
                            && o.normalize == a.normalize
                    })
                })
            })
            .map(|(n, _)| n.clone())
            .collect()
    }

    fn attribute_value(value: &TypedArray) -> BatchValue {
        let v = value.to_f64_vec();
        match v.len() {
            1 => BatchValue::Scalar(v[0]),
            2 => BatchValue::Vec2(Cartesian2::new(v[0], v[1])),
            3 => BatchValue::Vec3(Cartesian3::new(v[0], v[1], v[2])),
            _ => BatchValue::Vec4(Cartesian4::new(v[0], v[1], v[2], v[3])),
        }
    }

    fn batch_index(&self, name: &str) -> Option<usize> {
        self.batch_indices.iter().find(|(n, _)| n == name).map(|(_, i)| *i)
    }

    /// `createBatchTable`.
    fn create_batch_table(&mut self, context: &Context<G>) -> Result<()> {
        let instances = self.geometry_instances.as_deref().unwrap_or(&[]);
        if instances.is_empty() {
            return Ok(());
        }
        let names = Self::common_attribute_names(instances);
        let mut attributes = Vec::new();
        let mut indices = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let a = &instances[0].attributes.iter().find(|(n, _)| n == name).unwrap().1;
            indices.push((name.clone(), i));
            attributes.push(BatchTableAttribute {
                function_name: format!("czm_batchTable_{name}"),
                component_datatype: a.component_datatype,
                components_per_attribute: a.components_per_attribute as u32,
                normalize: a.normalize,
            });
        }
        let float = |name: &str, components| BatchTableAttribute {
            function_name: name.into(),
            component_datatype: ComponentDatatype::Float,
            components_per_attribute: components,
            normalize: false,
        };
        if names.iter().any(|n| n == "distanceDisplayCondition") {
            attributes.push(float("czm_batchTable_boundingSphereCenter3DHigh", 3));
            attributes.push(float("czm_batchTable_boundingSphereCenter3DLow", 3));
            attributes.push(float("czm_batchTable_boundingSphereCenter2DHigh", 3));
            attributes.push(float("czm_batchTable_boundingSphereCenter2DLow", 3));
            attributes.push(float("czm_batchTable_boundingSphereRadius", 1));
            let n = attributes.len();
            self.bounding_sphere_indices = Some(BoundingSphereIndices {
                center_3d_high: n - 5,
                center_3d_low: n - 4,
                center_2d_high: n - 3,
                center_2d_low: n - 2,
                radius: n - 1,
            });
        }
        if names.iter().any(|n| n == "offset") {
            attributes.push(float("czm_batchTable_offset2D", 3));
            self.offset_2d_index = Some(attributes.len() - 1);
        }
        attributes.push(BatchTableAttribute {
            function_name: "czm_batchTable_pickColor".into(),
            component_datatype: ComponentDatatype::UnsignedByte,
            components_per_attribute: 4,
            normalize: true,
        });
        let length = attributes.len();
        let mut table = BatchTable::new(context, attributes, instances.len())?;
        for (i, instance) in instances.iter().enumerate() {
            for (j, name) in names.iter().enumerate() {
                let a = &instance.attributes.iter().find(|(n, _)| n == name).unwrap().1;
                table.set_batched_attribute(i, j, Self::attribute_value(&a.value))?;
            }
            let primitive: Rc<dyn std::any::Any> =
                Rc::new(PrimitivePick { primitive: self.id, id: instance.id.clone() });
            let mut object = ScenePickObject::new(primitive, Rc::new(std::cell::Cell::new(true)));
            object.id = instance.id.clone().map(MetadataValue::String);
            let pick_id = context.create_pick_id(Rc::new(object))?;
            let c = pick_id.color;
            let byte = |v: f64| Color::float_to_byte(v) as f64;
            table.set_batched_attribute(
                i,
                length - 1,
                Cartesian4::new(byte(c.red), byte(c.green), byte(c.blue), byte(c.alpha)),
            )?;
            self.pick_ids.push(pick_id);
        }
        self.batch_table = Some(table);
        self.batch_indices = indices;
        Ok(())
    }

    fn combine_parameters(&self, frame: &FrameState, instances: Vec<GeometryInstance>) -> CombineGeometryParameters {
        let projection = &frame.uniform.map_projection;
        CombineGeometryParameters {
            instances,
            ellipsoid_radii: projection.ellipsoid().radii,
            is_geographic: projection.is_geographic(),
            uint_index_support: frame.context_capabilities.element_index_uint,
            scene_3d_only: frame.scene_3d_only,
            vertex_cache_optimize: self.vertex_cache_optimize,
            compress_vertices: self.compress_vertices,
            model_matrix: self.model_matrix,
            create_pick_offsets: false,
        }
    }

    /// `cloneInstance` of every instance, recording the ids.
    fn create_instances(&mut self) -> Vec<GeometryInstance> {
        let instances = self.geometry_instances.clone().unwrap_or_default();
        self.number_of_instances = instances.len();
        self.instance_ids.extend(instances.iter().map(|i| i.id.clone()));
        instances
    }

    fn apply_combined(
        &mut self,
        frame: &mut FrameState,
        result: Result<super::primitive_pipeline::CombineGeometryResults>,
    ) {
        match result {
            Err(error) => self.set_ready(frame, PrimitiveState::Failed, Some(error)),
            Ok(result) => {
                self.attribute_locations = result.attribute_locations.clone().unwrap_or_default().into_iter().collect();
                self.model_matrix = result.model_matrix;
                self.offset_instance_extend = result.offset_instance_extend.clone().unwrap_or_default();
                self.instance_bounding_spheres = result.bounding_spheres.clone();
                self.instance_bounding_spheres_cv = result.bounding_spheres_cv.clone();
                if result.geometries.as_ref().is_some_and(|g| !g.is_empty()) {
                    self.recompute_bounding_spheres = true;
                    self.state = PrimitiveState::Combined;
                    self.combined = Some(result);
                } else {
                    self.set_ready(frame, PrimitiveState::Failed, None);
                }
            }
        }
    }

    /// `loadSynchronous`.
    fn load_synchronous(&mut self, frame: &mut FrameState) {
        let instances = self.create_instances();
        let parameters = self.combine_parameters(frame, instances);
        self.apply_combined(frame, PrimitivePipeline::combine_geometry(parameters));
    }

    /// `loadAsynchronous`, with each promise resolved by the next update.
    fn load_asynchronous(&mut self, frame: &mut FrameState) {
        match self.state {
            PrimitiveState::Ready => {
                let instances = self.create_instances();
                self.created_instances = Some(instances);
                self.state = PrimitiveState::Creating;
            }
            PrimitiveState::Created => {
                let instances = self.created_instances.take().unwrap_or_default();
                let parameters = self.combine_parameters(frame, instances);
                self.state = PrimitiveState::Combining;
                match PrimitivePipeline::combine_geometry(parameters) {
                    Ok(result) => self.combined = Some(result),
                    Err(error) => self.set_ready(frame, PrimitiveState::Failed, Some(error)),
                }
            }
            _ => {}
        }
    }

    /// `recomputeBoundingSpheres`: the vertex-array spheres once offsets apply.
    fn recompute_bounding_spheres(&mut self, frame: &FrameState) -> Result<()> {
        let Some(offset_index) = self.batch_index("offset") else {
            self.recompute_bounding_spheres = false;
            return Ok(());
        };
        if !self.recompute_bounding_spheres {
            return Ok(());
        }
        let table = self.batch_table.as_ref().expect("the batch table exists");
        let mut spheres = Vec::new();
        for (i, bs) in self.instance_bounding_spheres.iter().enumerate() {
            let offset = match table.get_batched_attribute(i, offset_index)? {
                BatchValue::Vec3(v) => v,
                _ => Cartesian3::ZERO,
            };
            let bs = bs.unwrap_or_default();
            spheres.push(transform_bounding_sphere(bs, offset, self.offset_instance_extend.get(i).copied().flatten()));
        }
        let (mut combined, mut west, mut east) = (Vec::new(), Vec::new(), Vec::new());
        for bs in spheres {
            let min_x = bs.center.x - bs.radius;
            if min_x > 0.0 || bs.intersect_plane(&Plane::ORIGIN_ZX_PLANE) != Intersect::Intersecting {
                combined.push(bs);
            } else {
                west.push(bs);
                east.push(bs);
            }
        }
        let union = |list: &[BoundingSphere]| list.iter().copied().reduce(|a, b| a.union(&b));
        let result: Vec<BoundingSphere> =
            [union(&combined), union(&east), union(&west)].into_iter().flatten().collect();
        for (i, bs) in result.into_iter().enumerate() {
            let cv = bs.project_to_2d(Some(frame.uniform.map_projection.as_ref()));
            set_or_push(&mut self.spheres.local, i, bs);
            set_or_push(&mut self.spheres.cv, i, cv);
        }
        self.update_bounding_volumes(frame, self.model_matrix, true);
        self.recompute_bounding_spheres = false;
        Ok(())
    }

    /// `updateBatchTableBoundingSpheres`: centers (3D and 2D) and radii for
    /// the distance display condition.
    fn update_batch_table_bounding_spheres(&mut self, frame: &FrameState) -> Result<()> {
        let Some(indices) = self.bounding_sphere_indices else { return Ok(()) };
        if self.batch_index("distanceDisplayCondition").is_none() || self.bounding_spheres_updated {
            return Ok(());
        }
        let projection = &frame.uniform.map_projection;
        let table = self.batch_table.as_mut().expect("the batch table exists");
        for (i, bs) in self.instance_bounding_spheres.iter().enumerate() {
            let Some(bs) = bs else { continue };
            let bs = bs.transform(&self.model_matrix);
            let encoded = EncodedCartesian3::from_cartesian(bs.center);
            table.set_batched_attribute(i, indices.center_3d_high, encoded.high)?;
            table.set_batched_attribute(i, indices.center_3d_low, encoded.low)?;
            if !frame.scene_3d_only {
                let cartographic = projection
                    .ellipsoid()
                    .cartesian_to_cartographic(bs.center)
                    .ok_or_else(|| RuntimeError::new("Cannot project a bounding sphere centered at the origin."))?;
                let encoded = EncodedCartesian3::from_cartesian(projection.project(&cartographic));
                table.set_batched_attribute(i, indices.center_2d_high, encoded.high)?;
                table.set_batched_attribute(i, indices.center_2d_low, encoded.low)?;
            }
            table.set_batched_attribute(i, indices.radius, bs.radius)?;
        }
        self.bounding_spheres_updated = true;
        Ok(())
    }

    /// `updateBatchTableOffsets`: each offset as a 2D map vector.
    fn update_batch_table_offsets(&mut self, frame: &FrameState) -> Result<()> {
        let Some(offset_index) = self.batch_index("offset") else { return Ok(()) };
        if self.offsets_updated || frame.scene_3d_only {
            return Ok(());
        }
        let index_2d = self.offset_2d_index.expect("an offset has a 2D slot");
        let projection = &frame.uniform.map_projection;
        let ellipsoid = projection.ellipsoid();
        let table = self.batch_table.as_mut().expect("the batch table exists");
        for (i, bs) in self.instance_bounding_spheres.iter().enumerate() {
            let Some(bs) = bs else { continue };
            let offset = match table.get_batched_attribute(i, offset_index)? {
                BatchValue::Vec3(v) => v,
                _ => Cartesian3::ZERO,
            };
            if offset == Cartesian3::ZERO {
                table.set_batched_attribute(i, index_2d, Cartesian3::ZERO)?;
                continue;
            }
            let bs = bs.transform(&self.model_matrix);
            let center = ellipsoid.scale_to_geodetic_surface(bs.center).unwrap_or(bs.center);
            let carto = |p: Cartesian3| {
                ellipsoid
                    .cartesian_to_cartographic(p)
                    .ok_or_else(|| RuntimeError::new("Cannot project an offset at the origin."))
            };
            let center_2d = projection.project(&carto(center)?);
            let moved = projection.project(&carto(offset + center)?);
            let v = moved - center_2d;
            table.set_batched_attribute(i, index_2d, Cartesian3::new(v.z, v.x, v.y))?;
        }
        self.offsets_updated = true;
        Ok(())
    }

    /// `createVertexArray`.
    fn create_vertex_array(&mut self, frame: &mut FrameState, context: &Context<G>) -> Result<()> {
        let mut geometries: Vec<Geometry> = self.combined.take().and_then(|r| r.geometries).unwrap_or_default();
        let locations: std::collections::HashMap<String, u32> =
            self.attribute_locations.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for geometry in &mut geometries {
            let va = VertexArray::from_geometry(
                context,
                geometry,
                GeometryUploadOptions {
                    buffer_usage: BufferUsage::StaticDraw,
                    attribute_locations: locations.clone(),
                    interleave: self.interleave,
                    vertex_array_attributes: Vec::new(),
                },
            )?;
            self.va.push(Rc::new(va));
            self.spheres.local.push(geometry.bounding_sphere.unwrap_or_default());
            self.spheres.wc.push(BoundingSphere::default());
            if !frame.scene_3d_only {
                let cv = geometry
                    .bounding_sphere_cv
                    .as_mut()
                    .ok_or_else(|| RuntimeError::new("Geometry has no Columbus view bounding sphere."))?;
                let c = cv.center;
                cv.center = Cartesian3::new(c.z, c.x, c.y);
                self.spheres.cv.push(*cv);
                self.spheres.two_d.push(BoundingSphere::default());
                self.spheres.morph.push(BoundingSphere::default());
            }
        }
        if self.release_geometry_instances {
            self.geometry_instances = None;
        }
        self.set_ready(frame, PrimitiveState::Complete, None);
        Ok(())
    }

    /// `createRenderStates`.
    fn create_render_states(&mut self, two_passes: bool) {
        let appearance = self.appearance.as_ref().expect("an appearance");
        let rs = appearance.get_render_state();
        let culled = |face: CullFace| {
            let mut rs = rs.clone();
            rs.cull.enabled = true;
            rs.cull.face = face.value();
            rs
        };
        if two_passes {
            self.front_rs = Some(culled(CullFace::Back));
            self.back_rs = Some(culled(CullFace::Front));
        } else {
            self.front_rs = Some(rs.clone());
            self.back_rs = Some(rs.clone());
        }
        if let Some(depth_fail) = &self.depth_fail_appearance {
            let mut rs = depth_fail.get_render_state();
            rs.depth_test.func = DepthFunction::Greater.value();
            if two_passes {
                let mut front = rs.clone();
                front.cull.enabled = true;
                front.cull.face = CullFace::Back.value();
                let mut back = front.clone();
                back.cull.face = CullFace::Front.value();
                self.front_depth_fail_rs = Some(front);
                self.back_depth_fail_rs = Some(back);
            } else {
                self.front_depth_fail_rs = Some(rs);
                // Cesium's quirk: the back-face depth-fail state is the
                // color front-face state.
                self.back_depth_fail_rs = self.front_rs.clone();
            }
        }
    }

    fn has(&self, name: &str) -> bool {
        self.batch_index(name).is_some()
    }

    /// `Primitive._modifyShaderPosition`.
    fn modify_shader_position(&self, source: &str, scene_3d_only: bool) -> String {
        let re = Regex::new(r"in\s+vec(?:3|4)\s+(.*)3DHigh;").unwrap();
        let (mut forward, mut attributes, mut compute) = (String::new(), String::new(), String::new());
        let mut vs = source.to_string();
        let names: Vec<String> = re.captures_iter(source).map(|c| c[1].to_string()).collect();
        for name in names {
            let mut chars = name.chars();
            let capitalized: String =
                chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
            let function = format!("vec4 czm_compute{capitalized}()");
            if function != "vec4 czm_computePosition()" {
                forward += &format!("{function};\n");
            }
            if self.rtc_center.is_none() {
                if !scene_3d_only {
                    attributes += &format!("in vec3 {name}2DHigh;\nin vec3 {name}2DLow;\n");
                    compute += &format!(
                        "{function}\n{{\n    vec4 p;\n    if (czm_morphTime == 1.0)\n    {{\n        p = czm_translateRelativeToEye({name}3DHigh, {name}3DLow);\n    }}\n    else if (czm_morphTime == 0.0)\n    {{\n        p = czm_translateRelativeToEye({name}2DHigh.zxy, {name}2DLow.zxy);\n    }}\n    else\n    {{\n        p = czm_columbusViewMorph(\n                czm_translateRelativeToEye({name}2DHigh.zxy, {name}2DLow.zxy),\n                czm_translateRelativeToEye({name}3DHigh, {name}3DLow),\n                czm_morphTime);\n    }}\n    return p;\n}}\n\n"
                    );
                } else {
                    compute += &format!(
                        "{function}\n{{\n    return czm_translateRelativeToEye({name}3DHigh, {name}3DLow);\n}}\n\n"
                    );
                }
            } else {
                vs = remove_all(&vs, r"in\s+vec(?:3|4)\s+position3DHigh;");
                vs = remove_all(&vs, r"in\s+vec(?:3|4)\s+position3DLow;");
                forward += "uniform mat4 u_modifiedModelView;\n";
                attributes += "in vec4 position;\n";
                compute += &format!("{function}\n{{\n    return u_modifiedModelView * position;\n}}\n\n");
                vs = remove_all(&vs, r"czm_modelViewRelativeToEye\s+\*\s+");
                vs = vs.replace("czm_modelViewProjectionRelativeToEye", "czm_projection");
            }
        }
        [forward, attributes, vs, compute].join("\n")
    }

    /// `Primitive._appendShowToShader`.
    fn append_show(&self, vs: &str) -> String {
        if !self.has("show") {
            return vs.to_string();
        }
        let renamed = ShaderSource::replace_main(vs, "czm_non_show_main");
        format!("{renamed}\nvoid main() \n{{ \n    czm_non_show_main(); \n    gl_Position *= czm_batchTable_show(batchId); \n}}")
    }

    /// `Primitive._updateColorAttribute`.
    fn update_color_attribute(&self, vs: &str, depth_fail: bool) -> Result<String> {
        if !self.has("color") && !self.has("depthFailColor") {
            return Ok(vs.to_string());
        }
        if !contains(vs, r"in\s+vec4\s+color;") {
            return Ok(vs.to_string());
        }
        if depth_fail && !self.has("depthFailColor") {
            return Err(RuntimeError::new(
                "A depthFailColor per-instance attribute is required when using a depth fail appearance that uses a color attribute.",
            ));
        }
        let modified = remove_all(vs, r"in\s+vec4\s+color;");
        let replacement =
            if depth_fail { "czm_batchTable_depthFailColor(batchId)" } else { "czm_batchTable_color(batchId)" };
        Ok(replace_word(&modified, "color", replacement))
    }

    /// `Primitive._appendOffsetToShader`.
    fn append_offset(&self, vs: &str) -> String {
        if !self.has("offset") {
            return vs.to_string();
        }
        let modified = Regex::new(r"in\s+float\s+batchId;")
            .unwrap()
            .replace_all(vs, regex::NoExpand("in float batchId;\nin float applyOffset;"))
            .into_owned();
        let replacement = "vec4 ${1} = czm_computePosition();\n    if (czm_sceneMode == czm_sceneMode3D)\n    {\n        ${1} = ${1} + vec4(czm_batchTable_offset(batchId) * applyOffset, 0.0);    }\n    else\n    {\n        ${1} = ${1} + vec4(czm_batchTable_offset2D(batchId) * applyOffset, 0.0);    }\n";
        Regex::new(r"vec4\s+([A-Za-z0-9_]+)\s+=\s+czm_computePosition\(\);")
            .unwrap()
            .replace_all(&modified, replacement)
            .into_owned()
    }

    /// `Primitive._appendDistanceDisplayConditionToShader`.
    fn append_distance_display_condition(&self, vs: &str, scene_3d_only: bool) -> String {
        if !self.has("distanceDisplayCondition") {
            return vs.to_string();
        }
        let renamed = ShaderSource::replace_main(vs, "czm_non_distanceDisplayCondition_main");
        let mut main = String::from("void main() \n{ \n    czm_non_distanceDisplayCondition_main(); \n    vec2 distanceDisplayCondition = czm_batchTable_distanceDisplayCondition(batchId);\n    vec3 boundingSphereCenter3DHigh = czm_batchTable_boundingSphereCenter3DHigh(batchId);\n    vec3 boundingSphereCenter3DLow = czm_batchTable_boundingSphereCenter3DLow(batchId);\n    float boundingSphereRadius = czm_batchTable_boundingSphereRadius(batchId);\n");
        if !scene_3d_only {
            main += "    vec3 boundingSphereCenter2DHigh = czm_batchTable_boundingSphereCenter2DHigh(batchId);\n    vec3 boundingSphereCenter2DLow = czm_batchTable_boundingSphereCenter2DLow(batchId);\n    vec4 centerRTE;\n    if (czm_morphTime == 1.0)\n    {\n        centerRTE = czm_translateRelativeToEye(boundingSphereCenter3DHigh, boundingSphereCenter3DLow);\n    }\n    else if (czm_morphTime == 0.0)\n    {\n        centerRTE = czm_translateRelativeToEye(boundingSphereCenter2DHigh.zxy, boundingSphereCenter2DLow.zxy);\n    }\n    else\n    {\n        centerRTE = czm_columbusViewMorph(\n                czm_translateRelativeToEye(boundingSphereCenter2DHigh.zxy, boundingSphereCenter2DLow.zxy),\n                czm_translateRelativeToEye(boundingSphereCenter3DHigh, boundingSphereCenter3DLow),\n                czm_morphTime);\n    }\n";
        } else {
            main += "    vec4 centerRTE = czm_translateRelativeToEye(boundingSphereCenter3DHigh, boundingSphereCenter3DLow);\n";
        }
        main += "    float radiusSq = boundingSphereRadius * boundingSphereRadius; \n    float distanceSq; \n    if (czm_sceneMode == czm_sceneMode2D) \n    { \n        distanceSq = czm_eyeHeight2D.y - radiusSq; \n    } \n    else \n    { \n        distanceSq = dot(centerRTE.xyz, centerRTE.xyz) - radiusSq; \n    } \n    distanceSq = max(distanceSq, 0.0); \n    float nearSq = distanceDisplayCondition.x * distanceDisplayCondition.x; \n    float farSq = distanceDisplayCondition.y * distanceDisplayCondition.y; \n    float show = (distanceSq >= nearSq && distanceSq <= farSq) ? 1.0 : 0.0; \n    gl_Position *= show; \n}";
        format!("{renamed}\n{main}")
    }

    /// `modifyForEncodedNormals`.
    fn modify_for_encoded_normals(&self, vs: &str) -> String {
        if !self.compress_vertices {
            return vs.to_string();
        }
        let normal = contains(vs, r"in\s+vec3\s+normal;");
        let st = contains(vs, r"in\s+vec2\s+st;");
        if !normal && !st {
            return vs.to_string();
        }
        let tangent = contains(vs, r"in\s+vec3\s+tangent;");
        let bitangent = contains(vs, r"in\s+vec3\s+bitangent;");
        let mut components = if st && normal { 2 } else { 1 };
        components += if tangent || bitangent { 1 } else { 0 };
        let ty = if components > 1 { format!("vec{components}") } else { "float".to_string() };
        let name = "compressedAttributes";
        let declaration = format!("in {ty} {name};");
        let (mut global, mut decode) = (String::new(), String::new());
        if st {
            global += "vec2 st;\n";
            let component = if components > 1 { format!("{name}.x") } else { name.to_string() };
            decode += &format!("    st = czm_decompressTextureCoordinates({component});\n");
        }
        if normal && tangent && bitangent {
            global += "vec3 normal;\nvec3 tangent;\nvec3 bitangent;\n";
            decode +=
                &format!("    czm_octDecode({name}.{}, normal, tangent, bitangent);\n", if st { "yz" } else { "xy" });
        } else {
            if normal {
                global += "vec3 normal;\n";
                let swizzle = if components > 1 { format!(".{}", if st { "y" } else { "x" }) } else { String::new() };
                decode += &format!("    normal = czm_octDecode({name}{swizzle});\n");
            }
            if tangent {
                global += "vec3 tangent;\n";
                decode += &format!("    tangent = czm_octDecode({name}.{});\n", if st && normal { "z" } else { "y" });
            }
            if bitangent {
                global += "vec3 bitangent;\n";
                decode += &format!("    bitangent = czm_octDecode({name}.{});\n", if st && normal { "z" } else { "y" });
            }
        }
        let mut modified = vs.to_string();
        for pattern in [r"in\s+vec3\s+normal;", r"in\s+vec2\s+st;", r"in\s+vec3\s+tangent;", r"in\s+vec3\s+bitangent;"]
        {
            modified = remove_all(&modified, pattern);
        }
        let modified = ShaderSource::replace_main(&modified, "czm_non_compressed_main");
        let main = format!("void main() \n{{ \n{decode}    czm_non_compressed_main(); \n}}");
        [declaration, global, modified, main].join("\n")
    }

    /// `createShaderProgram`.
    fn create_shader_program(&mut self, context: &Context<G>, scene_3d_only: bool) -> Result<()> {
        let table = self.batch_table.as_ref().expect("the batch table exists");
        let callback = table.vertex_shader_callback();
        let appearance = self.appearance.as_ref().expect("an appearance");
        let mut vs = callback(&appearance.vertex_shader_source());
        vs = self.append_offset(&vs);
        vs = self.append_show(&vs);
        vs = self.append_distance_display_condition(&vs, scene_3d_only);
        vs = append_pick_to_vertex_shader(&vs);
        vs = self.update_color_attribute(&vs, false)?;
        vs = self.modify_for_encoded_normals(&vs);
        vs = self.modify_shader_position(&vs, scene_3d_only);
        let fs = format!("in vec4 v_pickColor;\n{}", appearance.get_fragment_shader_source());
        let previous = self.sp.take().map(|(p, _)| p);
        let program = self.compile(context, previous, &vs, &fs)?;
        self.sp = Some((program, PrimitiveShader { vertex: vs, fragment: fs }));
        if let Some(depth_fail) = &self.depth_fail_appearance {
            let mut vs = callback(&depth_fail.vertex_shader_source());
            vs = self.append_show(&vs);
            vs = self.append_distance_display_condition(&vs, scene_3d_only);
            vs = append_pick_to_vertex_shader(&vs);
            vs = self.update_color_attribute(&vs, true)?;
            vs = self.modify_for_encoded_normals(&vs);
            vs = self.modify_shader_position(&vs, scene_3d_only);
            vs = depth_clamp_vs(&vs);
            let fs = depth_clamp_fs(&format!("in vec4 v_pickColor;\n{}", depth_fail.get_fragment_shader_source()));
            let previous = self.sp_depth_fail.take().map(|(p, _)| p);
            let program = self.compile(context, previous, &vs, &fs)?;
            self.sp_depth_fail = Some((program, PrimitiveShader { vertex: vs, fragment: fs }));
        }
        Ok(())
    }

    fn compile(
        &self,
        context: &Context<G>,
        previous: Option<Rc<ShaderProgram<G>>>,
        vs: &str,
        fs: &str,
    ) -> Result<Rc<ShaderProgram<G>>> {
        let options = ShaderProgramOptions {
            vertex_shader_source: ShaderSource::new(vs),
            fragment_shader_source: ShaderSource::new(fs),
            attribute_locations: self.attribute_locations.clone(),
        };
        let program = context.with_shader_cache(|c| c.replace_shader_program(previous, &options))?;
        // `validateShaderMatching`.
        for attribute in program.vertex_attributes() {
            if !self.attribute_locations.contains_key(&attribute.name) {
                return Err(RuntimeError::new(format!(
                    "Appearance/Geometry mismatch.  The appearance requires vertex shader attribute input '{}', which was not computed as part of the Geometry.  Use the appearance's vertexFormat property when constructing the geometry.",
                    attribute.name
                )));
            }
        }
        Ok(program)
    }

    /// `getUniforms`: the appearance's, the material's and the batch table's.
    fn uniforms_for(&self, appearance: &dyn Appearance) -> Result<Vec<(String, UniformValue)>> {
        let material: Vec<(String, UniformValue)> =
            appearance.material().map(|m| m.numeric_uniform_values()).unwrap_or_default();
        let mut uniforms = Vec::new();
        for (name, value) in appearance.uniforms() {
            if appearance.material().is_some_and(|m| m.uniform_bindings().iter().any(|(n, _)| *n == name)) {
                return Err(RuntimeError::new(format!(
                    "Appearance and material have a uniform with the same name: {name}"
                )));
            }
            uniforms.push((name, value));
        }
        uniforms.extend(material);
        uniforms.extend(self.batch_table.as_ref().map(|t| t.uniform_values()).unwrap_or_default());
        Ok(uniforms)
    }

    /// `createCommands`: per vertex array, a back-face then front-face
    /// command for two passes, each followed by its depth-fail command.
    fn create_commands(&mut self, translucent: bool, two_passes: bool) -> Result<()> {
        let appearance = self.appearance.as_deref().expect("an appearance");
        self.uniforms = self.uniforms_for(appearance)?;
        if let Some(depth_fail) = self.depth_fail_appearance.as_deref() {
            self.depth_fail_uniforms = self.uniforms_for(depth_fail)?;
        }
        let pass = if translucent { Pass::Translucent } else { Pass::Opaque };
        let depth_fail = self.depth_fail_appearance.is_some();
        self.color_commands.clear();
        for va in 0..self.va.len() {
            if two_passes {
                self.color_commands.push((va, PrimitiveRenderState::Back, false, pass));
            }
            self.color_commands.push((va, PrimitiveRenderState::Front, false, pass));
            if depth_fail {
                if two_passes {
                    self.color_commands.push((va, PrimitiveRenderState::BackDepthFail, true, pass));
                }
                self.color_commands.push((va, PrimitiveRenderState::FrontDepthFail, true, pass));
            }
        }
        Ok(())
    }

    /// `Primitive._updateBoundingVolumes`.
    fn update_bounding_volumes(&mut self, frame: &FrameState, model_matrix: Matrix4, force: bool) {
        if force || self.applied_model_matrix != Some(model_matrix) {
            self.applied_model_matrix = Some(model_matrix);
            for i in 0..self.spheres.local.len() {
                let wc = self.spheres.local[i].transform(&model_matrix);
                set_or_push(&mut self.spheres.wc, i, wc);
                if !frame.scene_3d_only {
                    let cv = self.spheres.cv[i];
                    set_or_push(&mut self.spheres.two_d, i, cv);
                    set_or_push(&mut self.spheres.morph, i, wc.union(&cv));
                }
            }
        }
        let Some(pixel_size) = self.appearance.as_ref().and_then(|a| a.pixel_size()) else { return };
        let camera = &frame.uniform.camera;
        for i in 0..self.spheres.local.len() {
            let bs = self.spheres.local[i];
            let to_center = camera.position - bs.center;
            let projection = camera.direction.multiply_by_scalar(to_center.dot(camera.direction));
            let distance = (projection.magnitude() - bs.radius).max(0.0);
            let (w, h) = (frame.drawing_buffer_width as f64, frame.drawing_buffer_height as f64);
            let ratio = frame.pixel_ratio;
            let p = match &camera.frustum {
                UniformFrustum::Perspective(f) => f.get_pixel_dimensions(w, h, distance, ratio),
                UniformFrustum::PerspectiveOffCenter(f) => f.get_pixel_dimensions(w, h, distance, ratio),
                UniformFrustum::Orthographic(f) => f.get_pixel_dimensions(w, h, distance, ratio),
                UniformFrustum::OrthographicOffCenter(f) => f.get_pixel_dimensions(w, h, distance, ratio),
            };
            self.spheres.wc[i].radius = bs.radius + p.x.max(p.y) * pixel_size;
        }
    }

    /// `updateAndQueueCommands`.
    fn update_and_queue_commands(
        &mut self,
        frame: &mut FrameState,
        context: &Context<G>,
        two_passes: bool,
    ) -> Result<()> {
        let model_matrix = self.model_matrix;
        if frame.mode != SceneMode::Scene3D && model_matrix != Matrix4::IDENTITY {
            return Err(RuntimeError::new("Primitive.modelMatrix is only supported in 3D mode."));
        }
        self.update_bounding_volumes(frame, model_matrix, false);
        let spheres = match frame.mode {
            SceneMode::Scene3D => &self.spheres.wc,
            SceneMode::ColumbusView => &self.spheres.cv,
            SceneMode::Scene2D => &self.spheres.two_d,
            SceneMode::Morphing => &self.spheres.morph,
        };
        if !(frame.passes.render || frame.passes.pick) {
            return Ok(());
        }
        let cast = self.shadows.cast_shadows();
        let receive = self.shadows.receive_shadows();
        let mut factor = if two_passes { 2 } else { 1 };
        factor *= if self.depth_fail_appearance.is_some() { 2 } else { 1 };
        let pick_id = self.allow_picking.then(|| "v_pickColor".to_string());
        let texture = self.batch_table.as_ref().and_then(|t| t.texture().cloned());
        for (j, (va, state_kind, depth_fail, pass)) in self.color_commands.iter().enumerate() {
            let bounding_volume = spheres.get(j / factor).copied().unwrap_or_default();
            let state = match state_kind {
                PrimitiveRenderState::Front => &self.front_rs,
                PrimitiveRenderState::Back => &self.back_rs,
                PrimitiveRenderState::FrontDepthFail => &self.front_depth_fail_rs,
                PrimitiveRenderState::BackDepthFail => &self.back_depth_fail_rs,
            }
            .clone()
            .expect("render states exist");
            let (program, uniforms) = if *depth_fail {
                (self.sp_depth_fail.as_ref().map(|p| p.0.clone()), &self.depth_fail_uniforms)
            } else {
                (self.sp.as_ref().map(|p| p.0.clone()), &self.uniforms)
            };
            let program = program.ok_or_else(|| RuntimeError::new("Primitive shader program is not compiled"))?;
            let mut names: Vec<String> = uniforms.iter().map(|(n, _)| n.clone()).collect();
            if texture.is_some() || self.batch_table.as_ref().is_some_and(|t| !t.attributes().is_empty()) {
                names.push("batchTexture".into());
            }
            if self.rtc_center.is_some() {
                names.push("u_modifiedModelView".into());
            }
            names.sort();
            self.commands.push(PrimitiveCommand {
                pass: *pass,
                vertex_array: *va,
                render_state: *state_kind,
                depth_fail: *depth_fail,
                bounding_volume,
                model_matrix,
                cull: self.cull,
                debug_show_bounding_volume: self.debug_show_bounding_volume,
                cast_shadows: cast,
                receive_shadows: receive,
                pick_id: pick_id.clone(),
                uniforms: names,
            });
            let mut options = SceneDrawOptions::new(self.va[*va].clone(), program);
            options.state = state;
            options.primitive_type = self.va_primitive_type();
            options.uniforms = uniforms.clone();
            if let Some(texture) = &texture {
                options.textures.push(("batchTexture".into(), texture.clone() as Rc<dyn SampledTexture<G>>));
            }
            if let Some(center) = self.rtc_center {
                let context = context.clone();
                let model = self.applied_model_matrix.unwrap_or(model_matrix);
                let base = uniforms.clone();
                options.uniforms.clear();
                options.uniform_map = Some(Rc::new(move || {
                    let view = context.uniform_state.borrow().view;
                    let mut m = view.multiply(&model);
                    let rtc = m.multiply_by_point(center);
                    m = m.set_translation(rtc);
                    let mut values = base.clone();
                    values.push((
                        "u_modifiedModelView".into(),
                        UniformValue::Matrix { dimension: 4, values: m.0.iter().map(|v| *v as f32).collect() },
                    ));
                    Ok(values)
                }));
            }
            let metadata = DrawMetadata {
                bounding_volume: Some(bounding_volume),
                model_matrix: Some(model_matrix),
                owner: Some(format!("Primitive#{}", self.id)),
                pick_id: pick_id.clone(),
                cull: self.cull,
                debug_show_bounding_volume: self.debug_show_bounding_volume,
                cast_shadows: cast,
                receive_shadows: receive,
                ..Default::default()
            };
            frame.command_list.push(SceneCommand::from_draw(
                format!("primitive-{}-{j}", self.id),
                context.clone(),
                *pass,
                metadata,
                options,
            ));
        }
        Ok(())
    }

    fn va_primitive_type(&self) -> crate::core::PrimitiveType {
        self.primitive_type.unwrap_or(crate::core::PrimitiveType::Triangles)
    }

    fn update_frame(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.commands.clear();
        let no_instances = self.geometry_instances.is_none() && self.va.is_empty();
        let empty = self.geometry_instances.as_ref().is_some_and(|i| i.is_empty());
        if no_instances
            || empty
            || self.appearance.is_none()
            || (frame.mode != SceneMode::Scene3D && frame.scene_3d_only)
            || (!frame.passes.render && !frame.passes.pick)
        {
            return Ok(());
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.rtc_center.is_some() && !frame.scene_3d_only {
            return Err(RuntimeError::new("RTC rendering is only available for 3D only scenes."));
        }
        if self.state == PrimitiveState::Failed {
            return Ok(());
        }
        let context = frame
            .context
            .as_ref()
            .and_then(|c| c.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("frameState.context is required."))?;
        if self.batch_table.is_none() {
            self.create_batch_table(&context)?;
        }
        if self.batch_table.as_ref().is_some_and(|t| !t.attributes().is_empty()) {
            if context.limits()?.maximum_vertex_texture_image_units == 0 {
                return Err(RuntimeError::new(
                    "Vertex texture fetch support is required to render primitives with per-instance attributes. The maximum number of vertex texture image units must be greater than zero.",
                ));
            }
            self.batch_table.as_mut().unwrap().update(&context)?;
        }
        // Without workers, the asynchronous promises resolve by the next update.
        match self.state {
            PrimitiveState::Creating => self.state = PrimitiveState::Created,
            PrimitiveState::Combining => {
                let result = self.combined.take().ok_or_else(|| RuntimeError::new("Combine failed"));
                self.apply_combined(frame, result);
            }
            _ => {}
        }
        if !matches!(self.state, PrimitiveState::Complete | PrimitiveState::Combined | PrimitiveState::Failed) {
            if self.asynchronous {
                self.load_asynchronous(frame);
            } else {
                self.load_synchronous(frame);
            }
        }
        if self.state == PrimitiveState::Combined {
            self.update_batch_table_bounding_spheres(frame)?;
            self.update_batch_table_offsets(frame)?;
            self.primitive_type = self
                .combined
                .as_ref()
                .and_then(|r| r.geometries.as_ref())
                .and_then(|g| g.first())
                .map(|g| g.primitive_type);
            self.create_vertex_array(frame, &context)?;
        }
        if !self.show || self.state != PrimitiveState::Complete {
            return Ok(());
        }
        if !self.offsets_updated {
            self.update_batch_table_offsets(frame)?;
        }
        if self.recompute_bounding_spheres {
            self.recompute_bounding_spheres(frame)?;
        }

        let (mut create_rs, mut create_sp) = (false, false);
        if self.applied_appearance != Some(self.appearance_version) {
            self.applied_appearance = Some(self.appearance_version);
            create_rs = true;
            create_sp = true;
        }
        let depth_fail = self.depth_fail_appearance.as_ref().map(|_| self.depth_fail_version);
        if self.applied_depth_fail != Some(depth_fail) {
            self.applied_depth_fail = Some(depth_fail);
            create_rs = true;
            create_sp = true;
        }
        let appearance = self.appearance.as_mut().expect("an appearance");
        let translucent = appearance.is_translucent();
        if self.translucent != Some(translucent) {
            self.translucent = Some(translucent);
            create_rs = true;
        }
        if let Some(material) = appearance.material_mut() {
            // `Material.update` reads `context.defaultTexture`, creating it on first use.
            context.default_texture()?;
            material.update_textures()?;
        }
        let two_passes = appearance.closed() && translucent;
        if create_rs {
            self.create_render_states(two_passes);
        }
        if create_sp {
            self.create_shader_program(&context, frame.scene_3d_only)?;
        }
        if create_rs || create_sp {
            self.create_commands(translucent, two_passes)?;
        }
        self.update_and_queue_commands(frame, &context, two_passes)
    }

    /// The commands queued by the last update.
    pub fn commands(&self) -> &[PrimitiveCommand] {
        &self.commands
    }
    pub fn vertex_arrays(&self) -> &[Rc<VertexArray<G>>] {
        &self.va
    }
    /// `_attributeLocations`.
    pub fn attribute_locations(&self) -> &BTreeMap<String, u32> {
        &self.attribute_locations
    }
    /// The color and depth-fail programs' shader text.
    pub fn shaders(&self) -> (Option<&PrimitiveShader>, Option<&PrimitiveShader>) {
        (self.sp.as_ref().map(|p| &p.1), self.sp_depth_fail.as_ref().map(|p| &p.1))
    }
    pub fn shader_programs(&self) -> PrimitiveShaderPrograms<G> {
        (self.sp.as_ref().map(|p| p.0.clone()), self.sp_depth_fail.as_ref().map(|p| p.0.clone()))
    }
    pub fn render_state(&self, kind: PrimitiveRenderState) -> Option<&RenderState> {
        match kind {
            PrimitiveRenderState::Front => self.front_rs.as_ref(),
            PrimitiveRenderState::Back => self.back_rs.as_ref(),
            PrimitiveRenderState::FrontDepthFail => self.front_depth_fail_rs.as_ref(),
            PrimitiveRenderState::BackDepthFail => self.back_depth_fail_rs.as_ref(),
        }
    }
    pub fn bounding_spheres(&self) -> &PrimitiveBoundingSpheres {
        &self.spheres
    }
    /// The combined per-instance bounding spheres.
    pub fn instance_bounding_spheres(&self) -> (&[Option<BoundingSphere>], &[Option<BoundingSphere>]) {
        (&self.instance_bounding_spheres, &self.instance_bounding_spheres_cv)
    }
    pub fn batch_table(&self) -> Option<&BatchTable<G>> {
        self.batch_table.as_ref()
    }
    /// The per-instance attribute names and their batch-table indices.
    pub fn batch_attribute_indices(&self) -> &[(String, usize)] {
        &self.batch_indices
    }
    /// `_batchTableOffsetsUpdated`, `_batchTableBoundingSpheresUpdated` and
    /// `_recomputeBoundingSpheres`.
    pub fn batch_flags(&self) -> (bool, bool, bool) {
        (self.offsets_updated, self.bounding_spheres_updated, self.recompute_bounding_spheres)
    }

    /// The index of the instance with `id` (`getGeometryInstanceAttributes`).
    pub fn instance_index(&self, id: &str) -> Result<Option<usize>> {
        if self.batch_table.is_none() {
            return Err(RuntimeError::new("must call update before calling getGeometryInstanceAttributes"));
        }
        Ok(self.instance_ids.iter().position(|i| i.as_deref() == Some(id)))
    }

    /// A per-instance attribute value as its typed array holds it.
    pub fn get_instance_attribute(&self, index: usize, name: &str) -> Result<Option<Vec<f64>>> {
        let Some(attribute) = self.batch_index(name) else { return Ok(None) };
        let table = self.batch_table.as_ref().expect("the batch table exists");
        let a = &table.attributes()[attribute];
        let values: Vec<f64> = match table.get_batched_attribute(index, attribute)? {
            BatchValue::Scalar(v) => vec![v],
            BatchValue::Vec2(v) => vec![v.x, v.y],
            BatchValue::Vec3(v) => vec![v.x, v.y, v.z],
            BatchValue::Vec4(v) => vec![v.x, v.y, v.z, v.w],
        };
        let typed = TypedArray::from_values(a.component_datatype, &values[..a.components_per_attribute as usize]);
        Ok(Some(typed.to_f64_vec()))
    }

    /// Sets a per-instance attribute; an offset change recomputes the
    /// bounding spheres.
    pub fn set_instance_attribute(&mut self, index: usize, name: &str, value: &[f64]) -> Result<()> {
        if value.is_empty() || value.len() > 4 {
            return Err(RuntimeError::new("value must be and array with length between 1 and 4."));
        }
        let Some(attribute) = self.batch_index(name) else { return Ok(()) };
        let v = match value.len() {
            1 => BatchValue::Scalar(value[0]),
            2 => BatchValue::Vec2(Cartesian2::new(value[0], value[1])),
            3 => BatchValue::Vec3(Cartesian3::new(value[0], value[1], value[2])),
            _ => BatchValue::Vec4(Cartesian4::new(value[0], value[1], value[2], value[3])),
        };
        self.batch_table.as_mut().expect("the batch table exists").set_batched_attribute(index, attribute, v)?;
        if name == "offset" {
            self.recompute_bounding_spheres = true;
            self.offsets_updated = false;
        }
        Ok(())
    }

    /// The instance's bounding sphere with its offset and the model matrix.
    pub fn instance_bounding_sphere(&self, index: usize) -> Result<Option<BoundingSphere>> {
        let Some(bs) = self.instance_bounding_spheres.get(index).copied().flatten() else { return Ok(None) };
        let mut bs = bs;
        if let Some(offset) = self.get_instance_attribute(index, "offset")? {
            let offset = Cartesian3::new(offset[0], offset[1], offset[2]);
            bs = transform_bounding_sphere(bs, offset, self.offset_instance_extend.get(index).copied().flatten());
        }
        Ok(Some(bs.transform(&self.model_matrix)))
    }
    pub fn instance_bounding_sphere_cv(&self, index: usize) -> Option<BoundingSphere> {
        self.instance_bounding_spheres_cv.get(index).copied().flatten()
    }
    pub fn instance_pick_color(&self, index: usize) -> Option<Color> {
        self.pick_ids.get(index).map(|p| p.color)
    }
}

fn set_or_push(list: &mut Vec<BoundingSphere>, i: usize, value: BoundingSphere) {
    if i < list.len() {
        list[i] = value;
    } else {
        list.push(value);
    }
}

/// `transformBoundingSphere`.
fn transform_bounding_sphere(
    bs: BoundingSphere,
    offset: Cartesian3,
    extend: Option<GeometryOffsetAttribute>,
) -> BoundingSphere {
    match extend {
        Some(GeometryOffsetAttribute::Top) => {
            let moved = BoundingSphere::new(bs.center + offset, bs.radius);
            bs.union(&moved)
        }
        Some(GeometryOffsetAttribute::All) => BoundingSphere::new(bs.center + offset, bs.radius),
        _ => bs,
    }
}

/// `appendPickToVertexShader`.
fn append_pick_to_vertex_shader(source: &str) -> String {
    let renamed = ShaderSource::replace_main(source, "czm_non_pick_main");
    format!("{renamed}\nout vec4 v_pickColor; \nvoid main() \n{{ \n    czm_non_pick_main(); \n    v_pickColor = czm_batchTable_pickColor(batchId); \n}}")
}

/// `depthClampVS`.
fn depth_clamp_vs(source: &str) -> String {
    ShaderSource::replace_main(source, "czm_non_depth_clamp_main")
        + "void main() {\n    czm_non_depth_clamp_main();\n    gl_Position = czm_depthClamp(gl_Position);}\n"
}

/// `depthClampFS`.
fn depth_clamp_fs(source: &str) -> String {
    ShaderSource::replace_main(source, "czm_non_depth_clamp_main")
        + "void main() {\n    czm_non_depth_clamp_main();\n    #if defined(LOG_DEPTH)\n        czm_writeLogDepth();\n    #else\n        czm_writeDepthClamp();\n    #endif\n}\n"
}

impl<G: Gl + 'static> ScenePrimitive for Primitive<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.update_frame(frame)
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: super::Cesium3DTilePass) -> Result<()> {
        Ok(())
    }
    /// Destroys the programs, vertex arrays, pick ids and batch table.
    fn destroy(&mut self) -> Result<()> {
        self.sp = None;
        self.sp_depth_fail = None;
        for va in self.va.drain(..) {
            va.destroy()?;
        }
        for pick in self.pick_ids.drain(..) {
            pick.destroy();
        }
        if let Some(mut table) = self.batch_table.take() {
            table.destroy()?;
        }
        self.commands.clear();
        self.geometry_instances = None;
        self.destroyed = true;
        Ok(())
    }
}
