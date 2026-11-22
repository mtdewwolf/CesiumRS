//! CesiumJS Primitive geometry preparation, instance attributes, and drawing.

use super::{
    appearance::Appearance,
    batch_table::{BatchTable, BatchTableAttribute, BatchValue},
    material::{material_uniform_values, Material},
    primitive_pipeline::{CombineGeometryParameters, CombineGeometryResults, PickOffset, PrimitivePipeline},
    primitive_shader::{self, ShaderRewrites},
    primitive_state::PrimitiveState,
    scene::{Result, SceneDrawOptions, ScenePrimitive},
    CullFace, DepthFunction, FrameState, SceneCommand, SceneMode, ShadowMode,
};
use crate::{
    core::{
        task_processor::TaskFuture, BoundingSphere, Cartesian2, Cartesian3, Cartesian4, ComponentDatatype,
        EncodedCartesian3, Geometry, GeometryInstance, GeometryOffsetAttribute, Intersect, Matrix4, Plane,
        PrimitiveType, RuntimeError, TypedArray,
    },
    renderer::{
        BufferUsage, Context, DrawMetadata, GeometryUploadOptions, Gl, Pass, PickId, RenderState, SampledTexture,
        ShaderProgram, ShaderProgramOptions, ShaderSource, UniformValue, VertexArray,
    },
    workers::GeometryDescription,
};
use std::{
    cell::Cell,
    collections::BTreeMap,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context as TaskContext, Poll, Waker},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Options for creating a Primitive from created geometry or worker descriptions.
pub struct PrimitiveOptions {
    pub geometry_instances: Vec<GeometryInstance>,
    /// Geometry to create for the listed instance indices.
    pub geometry_descriptions: BTreeMap<usize, GeometryDescription>,
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
    pub rtc_center: Option<Cartesian3>,
    pub create_pick_offsets: bool,
    /// Override the picked primitive's identity for wrapper primitives.
    pub pick_primitive: Option<String>,
}

impl Default for PrimitiveOptions {
    fn default() -> Self {
        Self {
            geometry_instances: Vec::new(),
            geometry_descriptions: BTreeMap::new(),
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
            create_pick_offsets: false,
            pick_primitive: None,
        }
    }
}

/// The object stored in Context's pick registry for each geometry instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrimitivePickObject {
    pub primitive: String,
    pub id: Option<String>,
}

/// The state and binding names of a queued Primitive draw.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveCommand {
    pub vertex_array: usize,
    pub primitive_type: PrimitiveType,
    pub pass: Pass,
    pub render_state: RenderState,
    pub depth_fail: bool,
    pub metadata: DrawMetadata,
    pub uniforms: Vec<String>,
    pub uniform_values: Vec<(String, UniformValue)>,
}

#[derive(PartialEq, Eq)]
struct ShaderInputs {
    vertex: String,
    fragment: String,
    compress_vertices: bool,
    scene_3d_only: bool,
    rtc: bool,
}

struct PrimitiveShader<G: Gl> {
    inputs: ShaderInputs,
    program: Rc<ShaderProgram<G>>,
}

struct PrimitiveBatchTable<G: Gl + 'static> {
    table: BatchTable<G>,
    indices: BTreeMap<String, usize>,
    sphere_indices: Option<[usize; 5]>,
    offset_2d_index: Option<usize>,
    pick_ids: Vec<PickId>,
}

#[derive(Clone, Copy)]
struct GeometryBounds {
    local: Option<BoundingSphere>,
    cv: Option<BoundingSphere>,
}

enum GeometryWork {
    Creating(TaskFuture<Vec<GeometryInstance>>),
    Created(Vec<GeometryInstance>),
    Combining(TaskFuture<CombineGeometryResults>),
}

pub(crate) type CreatePrimitive = fn(PrimitiveOptions) -> Box<dyn RenderPrimitive>;

/// The part of a primitive's interface used by entity geometry batches.
pub(crate) trait RenderPrimitive: ScenePrimitive {
    fn ready(&self) -> bool;
    fn set_properties(&mut self, show: bool, shadows: ShadowMode);
    fn set_model_matrix(&mut self, model_matrix: Matrix4);
    fn set_material(&mut self, material: Material, depth_fail: bool);
    fn set_instance_attribute(&mut self, id: &str, name: &str, value: TypedArray) -> Result<()>;
}

impl<G: Gl + 'static> RenderPrimitive for Primitive<G> {
    fn ready(&self) -> bool {
        self.is_ready()
    }

    fn set_properties(&mut self, show: bool, shadows: ShadowMode) {
        self.show = show;
        self.shadows = shadows;
    }

    fn set_model_matrix(&mut self, model_matrix: Matrix4) {
        self.model_matrix = model_matrix;
    }

    fn set_material(&mut self, material: Material, depth_fail: bool) {
        let appearance = if depth_fail { &mut self.depth_fail_appearance } else { &mut self.appearance };
        if let Some(slot) = appearance.as_deref_mut().and_then(Appearance::material_mut) {
            *slot = material;
        }
    }

    fn set_instance_attribute(&mut self, id: &str, name: &str, value: TypedArray) -> Result<()> {
        if let Some(mut attributes) = self.get_geometry_instance_attributes(id)? {
            if attributes.get(name)?.is_some_and(|previous| previous != value) {
                attributes.set(name, &value)?;
            }
        }
        Ok(())
    }
}

pub struct Primitive<G: Gl + 'static> {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub appearance: Option<Box<dyn Appearance>>,
    pub depth_fail_appearance: Option<Box<dyn Appearance>>,
    pub vertex_cache_optimize: bool,
    pub interleave: bool,
    pub compress_vertices: bool,
    pub release_geometry_instances: bool,
    pub allow_picking: bool,
    pub cull: bool,
    pub asynchronous: bool,
    pub debug_show_bounding_volume: bool,
    pub shadows: ShadowMode,

    owner: String,
    pick_primitive: Option<String>,
    rtc_center: Option<Cartesian3>,
    create_pick_offsets: bool,
    geometry_instances: Option<Vec<GeometryInstance>>,
    geometry_descriptions: BTreeMap<usize, GeometryDescription>,
    instance_ids: Vec<Option<String>>,
    geometries: Vec<Geometry>,
    bounding_spheres: Vec<Option<BoundingSphere>>,
    bounding_spheres_cv: Vec<Option<BoundingSphere>>,
    offset_instance_extend: Vec<Option<GeometryOffsetAttribute>>,
    pick_offsets: Vec<PickOffset>,
    recompute_bounds: bool,
    offsets_dirty: bool,
    batch: Option<PrimitiveBatchTable<G>>,
    context: Option<Context<G>>,
    attribute_locations: BTreeMap<String, u32>,
    vertex_arrays: Vec<Rc<VertexArray<G>>>,
    primitive_type: PrimitiveType,
    bounds: Vec<GeometryBounds>,
    shader: Option<PrimitiveShader<G>>,
    depth_fail_shader: Option<PrimitiveShader<G>>,
    commands: Vec<PrimitiveCommand>,
    state: PrimitiveState,
    work: Option<GeometryWork>,
    ready: Rc<Cell<bool>>,
    error: Option<RuntimeError>,
    destroyed: bool,
}

impl<G: Gl + 'static> Primitive<G> {
    pub fn new(options: PrimitiveOptions) -> Self {
        Self {
            show: options.show.unwrap_or(true),
            model_matrix: options.model_matrix.unwrap_or(Matrix4::IDENTITY),
            appearance: options.appearance,
            depth_fail_appearance: options.depth_fail_appearance,
            vertex_cache_optimize: options.vertex_cache_optimize.unwrap_or(false),
            interleave: options.interleave.unwrap_or(false),
            compress_vertices: options.compress_vertices.unwrap_or(true),
            release_geometry_instances: options.release_geometry_instances.unwrap_or(true),
            allow_picking: options.allow_picking.unwrap_or(true),
            cull: options.cull.unwrap_or(true),
            asynchronous: options.asynchronous.unwrap_or(true),
            debug_show_bounding_volume: options.debug_show_bounding_volume.unwrap_or(false),
            shadows: options.shadows.unwrap_or(ShadowMode::Disabled),
            owner: format!("Primitive#{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)),
            pick_primitive: options.pick_primitive,
            rtc_center: options.rtc_center,
            create_pick_offsets: options.create_pick_offsets,
            geometry_instances: Some(options.geometry_instances),
            geometry_descriptions: options.geometry_descriptions,
            instance_ids: Vec::new(),
            geometries: Vec::new(),
            bounding_spheres: Vec::new(),
            bounding_spheres_cv: Vec::new(),
            offset_instance_extend: Vec::new(),
            pick_offsets: Vec::new(),
            recompute_bounds: false,
            offsets_dirty: true,
            batch: None,
            context: None,
            attribute_locations: BTreeMap::new(),
            vertex_arrays: Vec::new(),
            primitive_type: PrimitiveType::Triangles,
            bounds: Vec::new(),
            shader: None,
            depth_fail_shader: None,
            commands: Vec::new(),
            state: PrimitiveState::Ready,
            work: None,
            ready: Rc::new(Cell::new(false)),
            error: None,
            destroyed: false,
        }
    }

    pub fn state(&self) -> PrimitiveState {
        self.state
    }

    pub fn is_ready(&self) -> bool {
        self.ready.get()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn geometries(&self) -> &[Geometry] {
        &self.geometries
    }

    pub fn geometry_instances(&self) -> Option<&[GeometryInstance]> {
        self.geometry_instances.as_deref()
    }

    pub fn bounding_spheres(&self) -> &[Option<BoundingSphere>] {
        &self.bounding_spheres
    }

    pub fn pick_offsets(&self) -> &[PickOffset] {
        &self.pick_offsets
    }

    pub fn commands(&self) -> &[PrimitiveCommand] {
        &self.commands
    }

    pub fn shader_program(&self, depth_fail: bool) -> Option<&ShaderProgram<G>> {
        let shader = if depth_fail { &self.depth_fail_shader } else { &self.shader };
        shader.as_ref().map(|shader| shader.program.as_ref())
    }

    /// Replace a per-vertex pickColor input with the batched instance color.
    pub fn update_pick_color_attribute(source: &str) -> String {
        primitive_shader::update_pick_color_attribute(source)
    }

    pub fn get_geometry_instance_attributes(&mut self, id: &str) -> Result<Option<GeometryInstanceAttributes<'_, G>>> {
        self.check_destroyed()?;
        if self.batch.is_none() {
            return Err(RuntimeError::new("must call update before calling getGeometryInstanceAttributes"));
        }
        Ok(self
            .instance_ids
            .iter()
            .position(|item| item.as_deref() == Some(id))
            .map(|index| GeometryInstanceAttributes { primitive: self, index }))
    }

    fn check_destroyed(&self) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("Primitive has been destroyed"));
        }
        Ok(())
    }

    fn create_batch_table(&mut self, context: &Context<G>) -> Result<()> {
        if context.limits()?.maximum_vertex_texture_image_units == 0 {
            return Err(RuntimeError::new(
                "Vertex texture fetch support is required to render primitives with per-instance attributes. The maximum number of vertex texture image units must be greater than zero.",
            ));
        }
        let instances = self.geometry_instances.as_ref().expect("Primitive has instances before creating its table");
        let mut attributes = Vec::new();
        let mut indices = BTreeMap::new();
        for (name, attribute) in &instances[0].attributes {
            let is_common = instances.iter().skip(1).all(|instance| {
                instance.attributes.iter().any(|(other_name, other)| {
                    name == other_name
                        && attribute.component_datatype == other.component_datatype
                        && attribute.components_per_attribute == other.components_per_attribute
                        && attribute.normalize == other.normalize
                })
            });
            if is_common {
                indices.insert(name.clone(), attributes.len());
                attributes.push(BatchTableAttribute {
                    function_name: format!("czm_batchTable_{name}"),
                    component_datatype: attribute.component_datatype,
                    components_per_attribute: u32::from(attribute.components_per_attribute),
                    normalize: attribute.normalize,
                });
            }
        }

        let mut add_float = |name: &str, count| {
            let index = attributes.len();
            attributes.push(BatchTableAttribute {
                function_name: format!("czm_batchTable_{name}"),
                component_datatype: ComponentDatatype::Float,
                components_per_attribute: count,
                normalize: false,
            });
            index
        };
        let sphere_indices = indices.contains_key("distanceDisplayCondition").then(|| {
            [
                add_float("boundingSphereCenter3DHigh", 3),
                add_float("boundingSphereCenter3DLow", 3),
                add_float("boundingSphereCenter2DHigh", 3),
                add_float("boundingSphereCenter2DLow", 3),
                add_float("boundingSphereRadius", 1),
            ]
        });
        let offset_2d_index = indices.contains_key("offset").then(|| add_float("offset2D", 3));
        let pick_index = attributes.len();
        attributes.push(BatchTableAttribute {
            function_name: "czm_batchTable_pickColor".into(),
            component_datatype: ComponentDatatype::UnsignedByte,
            components_per_attribute: 4,
            normalize: true,
        });

        let mut table = BatchTable::new(context, attributes, instances.len())?;
        let mut pick_ids = Vec::with_capacity(instances.len());
        for (i, instance) in instances.iter().enumerate() {
            for (name, attribute) in &instance.attributes {
                if let Some(&index) = indices.get(name) {
                    table.set_batched_attribute(i, index, batch_value(&attribute.value.to_f64_vec())?)?;
                }
            }
            let id = context.create_pick_id(Rc::new(PrimitivePickObject {
                primitive: self.pick_primitive.as_ref().unwrap_or(&self.owner).clone(),
                id: instance.id.clone(),
            }))?;
            let [r, g, b, a] = id.color.to_bytes().map(f64::from);
            table.set_batched_attribute(i, pick_index, Cartesian4::new(r, g, b, a))?;
            pick_ids.push(id);
        }

        self.instance_ids = instances.iter().map(|instance| instance.id.clone()).collect();
        self.batch = Some(PrimitiveBatchTable { table, indices, sphere_indices, offset_2d_index, pick_ids });
        self.context = Some(context.clone());
        Ok(())
    }

    fn set_ready(&mut self, frame: &mut FrameState, state: PrimitiveState, error: Option<RuntimeError>) {
        self.state = state;
        self.error = error;
        let ready = Rc::downgrade(&self.ready);
        frame.after_render.push(Box::new(move || {
            let Some(ready) = ready.upgrade() else { return Ok(false) };
            ready.set(true);
            Ok(true)
        }));
    }

    fn combine_parameters(&self, frame: &FrameState, context: &Context<G>) -> Result<CombineGeometryParameters> {
        let projection = &frame.uniform.map_projection;
        Ok(CombineGeometryParameters {
            instances: Vec::new(),
            ellipsoid_radii: projection.ellipsoid().radii,
            is_geographic: projection.is_geographic(),
            uint_index_support: context.capabilities()?.element_index_uint,
            scene_3d_only: frame.scene_3d_only,
            vertex_cache_optimize: self.vertex_cache_optimize,
            compress_vertices: self.compress_vertices,
            model_matrix: self.model_matrix,
            create_pick_offsets: self.create_pick_offsets,
        })
    }

    fn store_combined(&mut self, frame: &mut FrameState, result: CombineGeometryResults) {
        self.model_matrix = result.model_matrix;
        self.geometries = result.geometries.unwrap_or_default();
        self.attribute_locations = result.attribute_locations.unwrap_or_default().into_iter().collect();
        self.pick_offsets = result.pick_offsets.unwrap_or_default();
        self.bounding_spheres = result.bounding_spheres;
        self.bounding_spheres_cv = result.bounding_spheres_cv;
        self.offset_instance_extend = result.offset_instance_extend.unwrap_or_default();
        if self.geometries.is_empty() {
            self.set_ready(frame, PrimitiveState::Failed, None);
        } else {
            self.recompute_bounds = true;
            self.state = PrimitiveState::Combined;
        }
    }

    fn load_geometry(&mut self, frame: &mut FrameState, context: &Context<G>) -> Result<()> {
        if !self.asynchronous && self.work.is_none() {
            let mut parameters = self.combine_parameters(frame, context)?;
            parameters.instances = create_geometry_instances(
                self.geometry_instances.clone().unwrap_or_default(),
                &self.geometry_descriptions,
            )?;
            self.store_combined(frame, PrimitivePipeline::combine_geometry(parameters)?);
            return Ok(());
        }

        let waker = frame.render_request.clone().map(|request| Waker::from(Arc::new(request)));
        let mut task_context = TaskContext::from_waker(waker.as_ref().unwrap_or(Waker::noop()));
        match self.work.take() {
            None => {
                let instances = self.geometry_instances.clone().unwrap_or_default();
                let descriptions = self.geometry_descriptions.clone();
                self.work = Some(GeometryWork::Creating(background_task(move || {
                    let mut instances = create_geometry_instances(instances, &descriptions)?;
                    let geometries: Vec<_> = instances.iter_mut().map(|instance| instance.geometry.take()).collect();
                    let packed = PrimitivePipeline::pack_create_geometry_results(&geometries);
                    for (instance, geometry) in instances.iter_mut().zip(packed.unpack()?) {
                        instance.geometry = geometry;
                    }
                    Ok(instances)
                })));
                self.state = PrimitiveState::Creating;
            }
            Some(GeometryWork::Creating(mut task)) => match task.as_mut().poll(&mut task_context) {
                Poll::Ready(result) => {
                    self.work = Some(GeometryWork::Created(result?));
                    self.state = PrimitiveState::Created;
                }
                Poll::Pending => {
                    self.work = Some(GeometryWork::Creating(task));
                    return Ok(());
                }
            },
            Some(GeometryWork::Created(instances)) => {
                let mut parameters = self.combine_parameters(frame, context)?;
                parameters.instances = instances;
                self.work = Some(GeometryWork::Combining(background_task(move || {
                    let combined = PrimitivePipeline::combine_geometry(parameters)?;
                    PrimitivePipeline::unpack_combine_geometry_results(
                        PrimitivePipeline::pack_combine_geometry_results(combined),
                    )
                })));
                self.state = PrimitiveState::Combining;
            }
            Some(GeometryWork::Combining(mut task)) => match task.as_mut().poll(&mut task_context) {
                Poll::Ready(result) => self.store_combined(frame, result?),
                Poll::Pending => {
                    self.work = Some(GeometryWork::Combining(task));
                    return Ok(());
                }
            },
        }
        if let Some(request) = &frame.render_request {
            request.request_render();
        }
        Ok(())
    }

    fn update_batch_table_bounding_spheres(&mut self, frame: &FrameState) -> Result<()> {
        let batch = self.batch.as_mut().unwrap();
        let Some([high_3d, low_3d, high_2d, low_2d, radius]) = batch.sphere_indices else { return Ok(()) };
        let projection = &frame.uniform.map_projection;
        for (i, sphere) in self.bounding_spheres.iter().enumerate() {
            let Some(sphere) = sphere else { continue };
            let sphere = sphere.transform(&self.model_matrix);
            let encoded = EncodedCartesian3::from_cartesian(sphere.center);
            batch.table.set_batched_attribute(i, high_3d, encoded.high)?;
            batch.table.set_batched_attribute(i, low_3d, encoded.low)?;
            if !frame.scene_3d_only {
                if let Some(cartographic) = projection.ellipsoid().cartesian_to_cartographic(sphere.center) {
                    let encoded = EncodedCartesian3::from_cartesian(projection.project(&cartographic));
                    batch.table.set_batched_attribute(i, high_2d, encoded.high)?;
                    batch.table.set_batched_attribute(i, low_2d, encoded.low)?;
                }
            }
            batch.table.set_batched_attribute(i, radius, sphere.radius)?;
        }
        Ok(())
    }

    fn update_batch_table_offsets(&mut self, frame: &FrameState) -> Result<()> {
        if !self.offsets_dirty || frame.scene_3d_only {
            return Ok(());
        }
        let batch = self.batch.as_mut().unwrap();
        let (Some(&offset), Some(offset_2d)) = (batch.indices.get("offset"), batch.offset_2d_index) else {
            return Ok(());
        };
        let projection = &frame.uniform.map_projection;
        let ellipsoid = projection.ellipsoid();
        for (i, sphere) in self.bounding_spheres.iter().enumerate() {
            let Some(sphere) = sphere else { continue };
            let value = batch.table.get_batched_attribute(i, offset)?;
            let BatchValue::Vec3(value) = value else { continue };
            let mut projected = Cartesian3::ZERO;
            if value != Cartesian3::ZERO {
                let center = self.model_matrix.multiply_by_point(sphere.center);
                if let Some(center) = ellipsoid.scale_to_geodetic_surface(center) {
                    if let (Some(old), Some(new)) = (
                        ellipsoid.cartesian_to_cartographic(center),
                        ellipsoid.cartesian_to_cartographic(center + value),
                    ) {
                        projected = swizzle(projection.project(&new) - projection.project(&old));
                    }
                }
            }
            batch.table.set_batched_attribute(i, offset_2d, projected)?;
        }
        self.offsets_dirty = false;
        Ok(())
    }

    fn create_vertex_arrays(&mut self, frame: &mut FrameState, context: &Context<G>) -> Result<()> {
        let mut arrays = Vec::with_capacity(self.geometries.len());
        let mut bounds = Vec::with_capacity(self.geometries.len());
        for geometry in &self.geometries {
            let options = GeometryUploadOptions {
                buffer_usage: BufferUsage::StaticDraw,
                attribute_locations: self.attribute_locations.iter().map(|(n, i)| (n.clone(), *i)).collect(),
                interleave: self.interleave,
                ..Default::default()
            };
            arrays.push(Rc::new(VertexArray::from_geometry(context, geometry, options)?));
            bounds.push(GeometryBounds {
                local: geometry.bounding_sphere,
                cv: geometry
                    .bounding_sphere_cv
                    .map(|sphere| BoundingSphere::new(swizzle(sphere.center), sphere.radius)),
            });
        }
        self.primitive_type = self.geometries[0].primitive_type;
        self.vertex_arrays = arrays;
        self.bounds = bounds;
        if self.release_geometry_instances {
            self.geometry_instances = None;
            self.geometry_descriptions.clear();
        }
        self.geometries.clear();
        self.set_ready(frame, PrimitiveState::Complete, None);
        Ok(())
    }

    fn recompute_bounding_spheres(&mut self, frame: &FrameState) -> Result<()> {
        if !self.recompute_bounds {
            return Ok(());
        }
        let batch = self.batch.as_ref().unwrap();
        if let Some(&index) = batch.indices.get("offset") {
            let mut unsplit: Option<BoundingSphere> = None;
            let mut split: Option<BoundingSphere> = None;
            for (i, sphere) in self.bounding_spheres.iter().enumerate() {
                let Some(sphere) = sphere else { continue };
                let offset = batch.table.get_batched_attribute(i, index)?;
                let sphere = offset_sphere(*sphere, offset, self.offset_instance_extend.get(i).copied().flatten());
                let target = if sphere.center.x - sphere.radius > 0.0
                    || sphere.intersect_plane(&Plane::ORIGIN_ZX_PLANE) != Intersect::Intersecting
                {
                    &mut unsplit
                } else {
                    &mut split
                };
                *target = Some(target.map_or(sphere, |old| old.union(&sphere)));
            }
            let projection = frame.uniform.map_projection.as_ref();
            let mut spheres = [unsplit, split, split].into_iter().flatten();
            for bounds in &mut self.bounds {
                if let Some(sphere) = spheres.next() {
                    bounds.local = Some(sphere);
                    bounds.cv = (!frame.scene_3d_only).then(|| sphere.project_to_2d(Some(projection)));
                }
            }
        }
        self.recompute_bounds = false;
        Ok(())
    }

    fn update_shaders(&mut self, context: &Context<G>, frame: &FrameState) -> Result<()> {
        let batch = self.batch.as_ref().unwrap();
        let rewrites = ShaderRewrites {
            offset: batch.indices.contains_key("offset"),
            show: batch.indices.contains_key("show"),
            distance_display_condition: batch.sphere_indices.is_some(),
            color: batch.indices.contains_key("color"),
            depth_fail_color: batch.indices.contains_key("depthFailColor"),
            compress_vertices: self.compress_vertices,
            scene_3d_only: frame.scene_3d_only,
            rtc: self.rtc_center.is_some(),
        };
        let shader_for = |cached: &mut Option<PrimitiveShader<G>>, appearance: &dyn Appearance, depth_fail| {
            let inputs = ShaderInputs {
                vertex: appearance.vertex_shader_source(),
                fragment: appearance.get_fragment_shader_source(),
                compress_vertices: rewrites.compress_vertices,
                scene_3d_only: rewrites.scene_3d_only,
                rtc: rewrites.rtc,
            };
            if cached.as_ref().is_some_and(|shader| shader.inputs == inputs) {
                return Ok::<_, RuntimeError>(());
            }
            let vertex = batch.table.vertex_shader_callback()(&inputs.vertex);
            let (vertex, fragment) = rewrites.apply(vertex, inputs.fragment.clone(), depth_fail)?;
            let options = ShaderProgramOptions {
                vertex_shader_source: ShaderSource::new(vertex),
                fragment_shader_source: ShaderSource::new(fragment),
                attribute_locations: self.attribute_locations.clone(),
            };
            let program = context.with_shader_cache(|cache| cache.get_shader_program(&options))?;
            for attribute in program.vertex_attributes() {
                if !self.attribute_locations.contains_key(&attribute.name) {
                    return Err(RuntimeError::new(format!(
                        "The appearance vertex shader requires input '{}', which was not computed as part of the Geometry. Use the appearance's vertexFormat property when constructing the geometry.",
                        attribute.name
                    )));
                }
            }
            *cached = Some(PrimitiveShader { inputs, program });
            Ok(())
        };
        shader_for(&mut self.shader, self.appearance.as_deref().unwrap(), false)?;
        if let Some(appearance) = self.depth_fail_appearance.as_deref() {
            shader_for(&mut self.depth_fail_shader, appearance, true)?;
        } else {
            self.depth_fail_shader = None;
        }
        Ok(())
    }

    fn uniforms(&self, appearance: &dyn Appearance) -> Result<Vec<(String, UniformValue)>> {
        let mut uniforms = appearance.uniforms();
        if let Some(material) = appearance.material() {
            for (name, value) in material_uniform_values(material) {
                if uniforms.iter().any(|(n, _)| *n == name) {
                    return Err(RuntimeError::new(format!(
                        "Appearance and material have a uniform with the same name: {name}"
                    )));
                }
                uniforms.push((name, value));
            }
        }
        uniforms.extend(self.batch.as_ref().unwrap().table.uniform_values());
        Ok(uniforms)
    }

    fn queue_commands(&mut self, frame: &mut FrameState, context: &Context<G>) -> Result<()> {
        if frame.mode != SceneMode::Scene3D && self.model_matrix != Matrix4::IDENTITY {
            return Err(RuntimeError::new("Primitive.modelMatrix is only supported in 3D mode."));
        }

        let appearance = self.appearance.as_deref().unwrap();
        let translucent = appearance.is_translucent();
        let two_passes = appearance.closed() && translucent;
        let pass = if translucent { Pass::Translucent } else { Pass::Opaque };
        let depth_fail_appearance = self.depth_fail_appearance.as_deref();
        let mut draws = Vec::with_capacity(4);
        draws.extend(render_states(appearance, two_passes, false).into_iter().map(|s| (s, false)));
        if let Some(appearance) = depth_fail_appearance {
            draws.extend(render_states(appearance, two_passes, true).into_iter().map(|s| (s, true)));
        }
        let uniforms = self.uniforms(appearance)?;
        let depth_fail_uniforms = depth_fail_appearance.map(|appearance| self.uniforms(appearance)).transpose()?;
        let batch_texture = self.batch.as_ref().unwrap().table.texture();
        let pixel_size = appearance.pixel_size();
        let uniform_map = self.rtc_center.map(|center| {
            let context = context.clone();
            let model = self.model_matrix;
            Rc::new(move || Ok(vec![rtc_uniform(&context, model, center)])) as crate::renderer::UniformMap
        });

        for (i, vertices) in self.vertex_arrays.iter().enumerate() {
            let bounds = self.bounds[i];
            let world = bounds.local.map(|sphere| sphere.transform(&self.model_matrix));
            let bounding_volume = match frame.mode {
                SceneMode::Scene3D => world.map(|mut sphere| {
                    if let (Some(size), Some(local)) = (pixel_size, bounds.local) {
                        sphere.radius = local.radius + size * meters_per_pixel(frame, &local);
                    }
                    sphere
                }),
                SceneMode::ColumbusView | SceneMode::Scene2D => bounds.cv,
                SceneMode::Morphing => match (world, bounds.cv) {
                    (Some(world), Some(cv)) => Some(world.union(&cv)),
                    (world, cv) => world.or(cv),
                },
            };

            let metadata = DrawMetadata {
                bounding_volume,
                model_matrix: Some(self.model_matrix),
                owner: Some(self.owner.clone()),
                cull: self.cull,
                debug_show_bounding_volume: self.debug_show_bounding_volume,
                cast_shadows: self.shadows.cast_shadows(),
                receive_shadows: self.shadows.receive_shadows(),
                pick_id: self.allow_picking.then(|| "v_pickColor".into()),
                ..Default::default()
            };
            for (state, depth_fail) in &draws {
                let (shader, uniforms) = if *depth_fail {
                    (self.depth_fail_shader.as_ref().unwrap(), depth_fail_uniforms.as_ref().unwrap())
                } else {
                    (self.shader.as_ref().unwrap(), &uniforms)
                };
                let mut options = SceneDrawOptions::new(vertices.clone(), shader.program.clone());
                options.state = state.clone();
                options.primitive_type = self.primitive_type;
                options.uniforms = uniforms.clone();
                let mut values = uniforms.clone();
                options.uniform_map = uniform_map.clone();
                let mut names: Vec<_> = uniforms.iter().map(|(n, _)| n.clone()).collect();
                if let Some(texture) = batch_texture {
                    options.textures.push(("batchTexture".into(), texture.clone() as Rc<dyn SampledTexture<G>>));
                    names.push("batchTexture".into());
                }
                if let Some(center) = self.rtc_center {
                    let uniform = rtc_uniform(context, self.model_matrix, center);
                    names.push(uniform.0.clone());
                    values.push(uniform);
                }
                names.sort();
                frame.command_list.push(SceneCommand::from_draw(
                    format!("{}-{}", self.owner, self.commands.len()),
                    context.clone(),
                    pass,
                    metadata.clone(),
                    options,
                ));
                self.commands.push(PrimitiveCommand {
                    vertex_array: i,
                    primitive_type: self.primitive_type,
                    pass,
                    render_state: state.clone(),
                    depth_fail: *depth_fail,
                    metadata: metadata.clone(),
                    uniforms: names,
                    uniform_values: values,
                });
            }
        }
        Ok(())
    }

    fn update_frame(&mut self, frame: &mut FrameState) -> Result<()> {
        self.check_destroyed()?;
        self.commands.clear();
        if self.appearance.is_none()
            || (!frame.passes.render && !frame.passes.pick)
            || (frame.scene_3d_only && frame.mode != SceneMode::Scene3D)
            || (self.vertex_arrays.is_empty() && self.geometry_instances.as_ref().is_none_or(Vec::is_empty))
        {
            return Ok(());
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.state == PrimitiveState::Failed {
            return Ok(());
        }
        if self.rtc_center.is_some() {
            if !frame.scene_3d_only {
                return Err(RuntimeError::new("RTC rendering is only available for 3D only scenes."));
            }
            if self.geometry_instances.as_ref().is_some_and(|instances| instances.len() != 1) {
                return Err(RuntimeError::new("Relative-to-center rendering only supports one geometry instance."));
            }
        }
        let context = frame
            .context
            .as_ref()
            .and_then(|resource| resource.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("frameState.context is required."))?;
        if let Some(previous) = &self.context {
            previous.same(&context)?;
        }
        if self.batch.is_none() {
            self.create_batch_table(&context)?;
        }
        self.batch.as_mut().unwrap().table.update(&context)?;

        if !matches!(self.state, PrimitiveState::Complete | PrimitiveState::Combined) {
            let async_load = self.asynchronous || self.work.is_some();
            if let Err(error) = self.load_geometry(frame, &context) {
                self.work = None;
                self.set_ready(frame, PrimitiveState::Failed, Some(error.clone()));
                return if async_load { Ok(()) } else { Err(error) };
            }
            // A worker reply is observed on one frame and uploaded on the next.
            if async_load && self.state == PrimitiveState::Combined {
                return Ok(());
            }
        }
        if self.state == PrimitiveState::Combined {
            self.update_batch_table_bounding_spheres(frame)?;
            self.update_batch_table_offsets(frame)?;
            self.create_vertex_arrays(frame, &context)?;
        }
        if !self.show || self.state != PrimitiveState::Complete {
            return Ok(());
        }
        self.update_batch_table_offsets(frame)?;
        self.recompute_bounding_spheres(frame)?;
        if self.appearance.as_ref().is_some_and(|appearance| appearance.material().is_some()) {
            context.default_texture()?;
        }
        self.update_shaders(&context, frame)?;
        self.queue_commands(frame, &context)
    }
}

/// A borrowed view of one instance's attributes and pick data. Setters update
/// the CPU batch table and the next frame uploads the changed values.
pub struct GeometryInstanceAttributes<'a, G: Gl + 'static> {
    primitive: &'a mut Primitive<G>,
    index: usize,
}

impl<G: Gl + 'static> GeometryInstanceAttributes<'_, G> {
    pub fn get(&self, name: &str) -> Result<Option<TypedArray>> {
        let batch = self.primitive.batch.as_ref().unwrap();
        let Some(&attribute) = batch.indices.get(name) else { return Ok(None) };
        let values = match batch.table.get_batched_attribute(self.index, attribute)? {
            BatchValue::Scalar(v) => vec![v],
            BatchValue::Vec2(v) => vec![v.x, v.y],
            BatchValue::Vec3(v) => vec![v.x, v.y, v.z],
            BatchValue::Vec4(v) => vec![v.x, v.y, v.z, v.w],
        };
        Ok(Some(TypedArray::from_values(batch.table.attributes()[attribute].component_datatype, &values)))
    }

    pub fn set(&mut self, name: &str, value: &TypedArray) -> Result<()> {
        let batch = self.primitive.batch.as_mut().unwrap();
        let Some(&attribute) = batch.indices.get(name) else {
            return Err(RuntimeError::new(format!("Unknown per-instance attribute: {name}")));
        };
        if value.len() != batch.table.attributes()[attribute].components_per_attribute as usize {
            return Err(RuntimeError::new(format!(
                "Invalid number of components for per-instance attribute '{name}'."
            )));
        }
        batch.table.set_batched_attribute(self.index, attribute, batch_value(&value.to_f64_vec())?)?;
        if name == "offset" {
            self.primitive.recompute_bounds = true;
            self.primitive.offsets_dirty = true;
        }
        Ok(())
    }

    pub fn bounding_sphere(&self) -> Option<BoundingSphere> {
        let primitive = &self.primitive;
        let mut sphere = primitive.bounding_spheres.get(self.index).copied().flatten()?;
        let batch = primitive.batch.as_ref().unwrap();
        if let Some(offset) =
            batch.indices.get("offset").and_then(|&i| batch.table.get_batched_attribute(self.index, i).ok())
        {
            sphere = offset_sphere(sphere, offset, primitive.offset_instance_extend.get(self.index).copied().flatten());
        }
        Some(sphere.transform(&primitive.model_matrix))
    }

    pub fn bounding_sphere_cv(&self) -> Option<BoundingSphere> {
        self.primitive.bounding_spheres_cv.get(self.index).copied().flatten()
    }

    pub fn pick_id(&self) -> &PickId {
        &self.primitive.batch.as_ref().unwrap().pick_ids[self.index]
    }
}

fn create_geometry_instances(
    mut instances: Vec<GeometryInstance>,
    descriptions: &BTreeMap<usize, GeometryDescription>,
) -> Result<Vec<GeometryInstance>> {
    for (&index, description) in descriptions {
        let instance = instances
            .get_mut(index)
            .ok_or_else(|| RuntimeError::new(format!("Geometry description references missing instance {index}.")))?;
        instance.geometry = description.create_geometry();
    }
    Ok(instances)
}

fn rtc_uniform<G: Gl>(context: &Context<G>, model: Matrix4, center: Cartesian3) -> (String, UniformValue) {
    let modified = context.uniform_state().view.multiply(&model);
    let modified = modified.set_translation(modified.multiply_by_point(center));
    ("u_modifiedModelView".into(), UniformValue::Matrix { dimension: 4, values: modified.0.map(|v| v as f32).to_vec() })
}

fn batch_value(values: &[f64]) -> Result<BatchValue> {
    Ok(match *values {
        [x] => x.into(),
        [x, y] => Cartesian2::new(x, y).into(),
        [x, y, z] => Cartesian3::new(x, y, z).into(),
        [x, y, z, w] => Cartesian4::new(x, y, z, w).into(),
        _ => return Err(RuntimeError::new("value must be an array with length between 1 and 4.")),
    })
}

fn offset_sphere(sphere: BoundingSphere, value: BatchValue, kind: Option<GeometryOffsetAttribute>) -> BoundingSphere {
    let BatchValue::Vec3(offset) = value else { return sphere };
    let shifted = BoundingSphere::new(sphere.center + offset, sphere.radius);
    match kind {
        Some(GeometryOffsetAttribute::Top) => sphere.union(&shifted),
        Some(GeometryOffsetAttribute::All) => shifted,
        _ => sphere,
    }
}

fn swizzle(v: Cartesian3) -> Cartesian3 {
    Cartesian3::new(v.z, v.x, v.y)
}

fn meters_per_pixel(frame: &FrameState, sphere: &BoundingSphere) -> f64 {
    use crate::renderer::UniformFrustum;
    let camera = &frame.uniform.camera;
    let distance = ((camera.position - sphere.center).dot(camera.direction).abs() - sphere.radius).max(0.0);
    let width = f64::from(frame.drawing_buffer_width);
    let height = f64::from(frame.drawing_buffer_height);
    let ratio = frame.pixel_ratio;
    let dimensions = match camera.frustum {
        UniformFrustum::Perspective(f) => f.get_pixel_dimensions(width, height, distance, ratio),
        UniformFrustum::PerspectiveOffCenter(f) => f.get_pixel_dimensions(width, height, distance, ratio),
        UniformFrustum::Orthographic(f) => f.get_pixel_dimensions(width, height, distance, ratio),
        UniformFrustum::OrthographicOffCenter(f) => f.get_pixel_dimensions(width, height, distance, ratio),
    };
    dimensions.x.max(dimensions.y)
}

fn render_states(appearance: &dyn Appearance, two_passes: bool, depth_fail: bool) -> Vec<RenderState> {
    let mut front = appearance.get_render_state();
    if depth_fail {
        front.depth_test.func = DepthFunction::Greater.value();
    }
    if two_passes {
        front.cull.enabled = true;
        front.cull.face = CullFace::Back.value();
        let mut back = front.clone();
        back.cull.face = CullFace::Front.value();
        vec![back, front]
    } else {
        vec![front]
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn background_task<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> TaskFuture<T> {
    let (sender, receiver) = futures_channel::oneshot::channel();
    let spawned = std::thread::Builder::new().name("cesium-primitive".into()).spawn(move || {
        if !sender.is_canceled() {
            let _ = sender.send(work());
        }
    });
    Box::pin(async move {
        spawned.map_err(|error| RuntimeError::new(error.to_string()))?;
        receiver.await.map_err(|_| RuntimeError::new("Primitive geometry worker terminated."))?
    })
}

#[cfg(target_arch = "wasm32")]
fn background_task<T: 'static>(work: impl FnOnce() -> Result<T> + 'static) -> TaskFuture<T> {
    Box::pin(async move { work() })
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

    fn destroy(&mut self) -> Result<()> {
        self.check_destroyed()?;
        self.destroyed = true;
        self.ready = Rc::new(Cell::new(false));
        self.work = None;
        self.error = None;
        self.geometry_instances = None;
        self.geometry_descriptions.clear();
        self.appearance = None;
        self.depth_fail_appearance = None;
        self.geometries.clear();
        self.bounding_spheres.clear();
        self.bounding_spheres_cv.clear();
        self.offset_instance_extend.clear();
        self.bounds.clear();
        self.pick_offsets.clear();
        self.instance_ids.clear();
        self.attribute_locations.clear();
        self.commands.clear();
        self.shader = None;
        self.depth_fail_shader = None;
        self.context = None;

        let mut result = Ok(());
        for vertices in self.vertex_arrays.drain(..) {
            result = result.and(vertices.destroy());
        }
        if let Some(mut batch) = self.batch.take() {
            result = result.and(batch.table.destroy());
        }
        result
    }
}

#[cfg(test)]
pub(super) mod tests {
    use crate::{
        core::RuntimeError,
        renderer::{Gl, GlValue},
    };

    pub(in crate::scene) struct TestGl;

    impl Gl for TestGl {
        type Object = u32;

        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([1, 1])
        }

        fn invoke(&self, _: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            Ok(GlValue::Null)
        }
    }
}
