//! Frame-driven glTF component loading. The JSON, geometry, image and metadata
//! loaders share the caller's ResourceCache. Call `load` once, then `process`
//! each frame; resource futures are polled by the frame loop.
use super::gltf_pipeline::{get_accessor_byte_stride, BufferSource};
use super::model::extensions::gpm::{self, GltfMeshPrimitiveGpmLoader, GltfMeshPrimitiveGpmLoaderOptions};
use super::{model_components as mc, resource_loader::ManagedLoader, *};
use crate::{
    core::*,
    renderer::{Context, Gl},
};
use futures_util::FutureExt;
use serde_json::Value;
use std::{
    cell::{Cell, Ref, RefCell},
    rc::Rc,
};
type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Clone, Debug)]
pub struct GltfLoaderOptions {
    pub release_gltf_json: bool,
    pub asynchronous: bool,
    pub incrementally_load_textures: bool,
    pub up_axis: Axis,
    pub forward_axis: Axis,
    pub load_attributes_as_typed_array: bool,
    pub load_attributes_for_2d: bool,
    pub enable_pick: bool,
    pub load_indices_for_wireframe: bool,
    pub load_primitive_outline: bool,
    pub load_for_classification: bool,
    pub rename_batch_id_semantic: bool,
    pub texture_options: GltfTextureOptions,
}
impl Default for GltfLoaderOptions {
    fn default() -> Self {
        Self {
            release_gltf_json: false,
            asynchronous: true,
            incrementally_load_textures: true,
            up_axis: Axis::Y,
            forward_axis: Axis::Z,
            load_attributes_as_typed_array: false,
            load_attributes_for_2d: false,
            enable_pick: false,
            load_indices_for_wireframe: false,
            load_primitive_outline: true,
            load_for_classification: false,
            rename_batch_id_semantic: false,
            texture_options: GltfTextureOptions::default(),
        }
    }
}

#[derive(Clone, Copy)]
struct AttributeLocation {
    node: usize,
    primitive: Option<usize>,
    attribute: PlannedAttribute,
}
struct VertexJob<G: Gl + 'static> {
    lease: CacheLease<GltfVertexBufferLoader<G>>,
    location: AttributeLocation,
    accessor: Value,
    compressed: bool,
    spz: bool,
    options: GpuBufferOptions,
    done: bool,
}
struct IndexJob<G: Gl + 'static> {
    lease: CacheLease<GltfIndexBufferLoader<G>>,
    node: usize,
    primitive: usize,
    done: bool,
}
struct ViewJob {
    lease: CacheLease<GltfBufferViewLoader>,
    accessor: Value,
    target: ValueTarget,
    done: bool,
}
enum ValueTarget {
    Outline(usize),
    EdgeVisibility(usize, usize),
    SilhouetteNormals(usize, usize),
    Skin(usize),
    AnimationInput(usize, usize),
    AnimationOutput(usize, usize),
}
struct TextureJob<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    lease: CacheLease<GltfTextureLoader<G, H>>,
    index: f64,
    done: bool,
}
struct Runtime<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    request: Option<GltfResource>,
    components: mc::Components<G>,
    vertices: Vec<VertexJob<G>>,
    indices: Vec<IndexJob<G>>,
    views: Vec<ViewJob>,
    textures: Vec<TextureJob<G, H>>,
    promises: Vec<ResourceLoadFuture>,
    texture_promises: Vec<ResourceLoadFuture>,
    plans: Vec<(usize, usize, PrimitiveLoadPlan)>,
    metadata: Option<GltfStructuralMetadataLoader<G, H>>,
    ready: bool,
    textures_ready: bool,
    textures_failed: bool,
    gpm: Vec<GltfMeshPrimitiveGpmLoader<G, H>>,
}

pub struct GltfLoader<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    data: ManagedLoader<GltfResource>,
    cache: ResourceCache,
    json: Rc<RefCell<Option<CacheLease<GltfJsonLoader>>>>,
    resource: Resource,
    base: Resource,
    input: Option<GltfJsonInput>,
    context: Context<G>,
    host: Rc<H>,
    options: GltfLoaderOptions,
    runtime: RefCell<Option<Runtime<G, H>>>,
    unloaded: Cell<bool>,
}
impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> GltfLoader<G, H>
where
    H::Image: 'static,
{
    pub fn new(
        cache: ResourceCache,
        resource: Resource,
        base: Option<Resource>,
        input: Option<GltfJsonInput>,
        context: Context<G>,
        host: Rc<H>,
        options: GltfLoaderOptions,
    ) -> Self {
        Self {
            data: ManagedLoader::new(None, None),
            cache,
            json: Rc::new(RefCell::new(None)),
            base: base.unwrap_or_else(|| resource.clone()),
            resource,
            input,
            context,
            host,
            options,
            runtime: RefCell::new(None),
            unloaded: Cell::new(false),
        }
    }
    pub fn components(&self) -> Option<Ref<'_, mc::Components<G>>> {
        Ref::filter_map(self.runtime.borrow(), |r| r.as_ref().map(|r| &r.components)).ok()
    }
    pub fn gltf_json(&self) -> Option<std::sync::Arc<LoadedGltfJson>> {
        self.json.borrow().as_ref().and_then(|l| l.loader().gltf())
    }
    pub fn textures_loaded(&self) -> bool {
        self.runtime.borrow().as_ref().is_some_and(|r| r.textures_ready)
    }
    pub fn incrementally_load_textures(&self) -> bool {
        self.options.incrementally_load_textures
    }
    pub fn is_unloaded(&self) -> bool {
        self.unloaded.get()
    }
    fn process_inner(&self, frame: &FrameState) -> Result<bool> {
        if self.unloaded.get() || self.data.is_destroyed() {
            return Ok(false);
        }
        if self.runtime.borrow().is_none() {
            let Some(request) = self.data.value() else { return Ok(false) };
            let mut runtime = Runtime::new(request, &self.options);
            runtime.parse(self, frame)?;
            *self.runtime.borrow_mut() = Some(runtime);
            self.data.unload();
            self.data.set_state(ResourceLoaderState::Processing);
            if self.options.release_gltf_json {
                self.json.borrow_mut().take();
            }
        }
        let mut runtime = self.runtime.borrow_mut();
        let r = runtime.as_mut().expect("initialized");
        r.process(self, frame)
    }
}
impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> ResourceLoader for GltfLoader<G, H>
where
    H::Image: 'static,
{
    fn cache_key(&self) -> Option<&str> {
        None
    }
    fn load(&self) -> ResourceLoadFuture {
        let cache = self.cache.clone();
        let resource = self.resource.clone();
        let base = self.base.clone();
        let input = self.input.clone();
        let json = self.json.clone();
        self.data.load_with_state("Failed to load glTF".into(), ResourceLoaderState::Loaded, move || {
            async move {
                let lease = cache.gltf_json_loader(resource.clone(), base.clone(), input)?;
                let child = lease.loader();
                *json.borrow_mut() = Some(lease);
                child.load().await?;
                let document = child.gltf().ok_or_else(|| RuntimeError::new("glTF JSON was unloaded"))?;
                Ok(GltfResource { document, resource, base })
            }
            .boxed_local()
        })
    }
    fn state(&self) -> ResourceLoaderState {
        self.data.state()
    }
    fn is_destroyed(&self) -> bool {
        self.data.is_destroyed()
    }
    fn unload(&self) {
        self.runtime.borrow_mut().take();
        self.json.borrow_mut().take();
        self.data.unload();
        self.unloaded.set(true);
        self.data.set_state(ResourceLoaderState::Unloaded);
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
    fn process(&self, frame: &FrameState) -> Result<bool> {
        match self.process_inner(frame) {
            Ok(v) => Ok(v),
            Err(e) if e.message.starts_with("Failed to load glTF texture\n") => Err(e),
            Err(e) => {
                self.unload();
                Err(resource_loader::loader_error("Failed to load glTF", Some(&e)))
            }
        }
    }
}
fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}
fn index(v: &Value) -> Option<usize> {
    v.as_u64().map(|v| v as usize)
}
fn string(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}
fn number(v: &Value, default: f64) -> f64 {
    v.as_f64().unwrap_or(default)
}
fn values(v: &Value) -> Vec<f64> {
    list(v).iter().map(|v| number(v, f64::NAN)).collect()
}
fn error(e: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(e.to_string())
}
fn shape(a: &Value) -> Result<AttributeType> {
    AttributeType::from_str_value(a["type"].as_str().unwrap_or("")).ok_or_else(|| error("Invalid accessor type"))
}
fn datatype(a: &Value) -> Result<ComponentDatatype> {
    ComponentDatatype::from_gl(a["componentType"].as_u64().unwrap_or(0) as u32)
        .ok_or_else(|| error("Invalid accessor component type"))
}
fn math(kind: AttributeType, v: &[f64], quaternion: bool) -> mc::MathValue {
    use mc::MathValue as M;
    let get = |i| v.get(i).copied().unwrap_or(f64::NAN);
    match kind {
        AttributeType::Scalar => M::Number(get(0)),
        AttributeType::Vec2 => M::Cartesian2(Cartesian2::new(get(0), get(1))),
        AttributeType::Vec3 => M::Cartesian3(Cartesian3::new(get(0), get(1), get(2))),
        AttributeType::Vec4 if quaternion => M::Quaternion(Quaternion::new(get(0), get(1), get(2), get(3))),
        AttributeType::Vec4 => M::Cartesian4(Cartesian4::new(get(0), get(1), get(2), get(3))),
        AttributeType::Mat2 => M::Matrix2(Matrix2(std::array::from_fn(get))),
        AttributeType::Mat3 => M::Matrix3(Matrix3(std::array::from_fn(get))),
        AttributeType::Mat4 => M::Matrix4(Matrix4(std::array::from_fn(get))),
    }
}
/// Copy and deinterleave an accessor, preserving its original component type.
fn packed(gltf: &Value, a: &Value, source: &BufferSource) -> Result<TypedArray> {
    let datatype = datatype(a)?;
    let n = shape(a)?.get_number_of_components();
    let count = index(&a["count"]).unwrap_or(0);
    let stride = get_accessor_byte_stride(gltf, a).map_err(error)? as usize;
    let offset = index(&a["byteOffset"]).unwrap_or(0);
    let length = count.checked_mul(n).ok_or_else(|| error("Accessor size overflow"))?;
    let mut result = TypedArray::zeros(datatype, length);
    for i in 0..count {
        let start = offset
            .checked_add(i.checked_mul(stride).ok_or_else(|| error("Accessor offset overflow"))?)
            .ok_or_else(|| error("Accessor offset overflow"))?;
        let row = datatype
            .create_array_buffer_view(source.bytes(), start, Some(n))
            .ok_or_else(|| error("Accessor exceeds buffer view"))?;
        for j in 0..n {
            result.set(i * n + j, row.get(j).unwrap_or(f64::NAN));
        }
    }
    Ok(result)
}
fn attribute_mut<G: Gl>(c: &mut mc::Components<G>, l: AttributeLocation) -> &mut mc::Attribute<G> {
    let node = &mut c.nodes[l.node];
    if let Some(p) = l.primitive {
        let p = &mut node.primitives[p];
        match l.attribute {
            PlannedAttribute::Attribute(a) => &mut p.attributes[a],
            PlannedAttribute::MorphTarget { target, attribute } => &mut p.morph_targets[target].attributes[attribute],
            PlannedAttribute::OutlineCoordinates => p.outline_coordinates.as_mut().unwrap(),
        }
    } else {
        let PlannedAttribute::Attribute(a) = l.attribute else { unreachable!() };
        &mut node.instances.as_mut().unwrap().attributes[a]
    }
}
impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> Runtime<G, H>
where
    H::Image: 'static,
{
    fn new(request: GltfResource, o: &GltfLoaderOptions) -> Self {
        Self {
            request: Some(request),
            components: mc::Components {
                up_axis: Some(o.up_axis),
                forward_axis: Some(o.forward_axis),
                ..Default::default()
            },
            vertices: vec![],
            indices: vec![],
            views: vec![],
            textures: vec![],
            promises: vec![],
            texture_promises: vec![],
            plans: vec![],
            metadata: None,
            ready: false,
            textures_ready: false,
            textures_failed: false,
            gpm: vec![],
        }
    }
    fn request(&self) -> &GltfResource {
        self.request.as_ref().expect("glTF request retained while processing")
    }
    fn accessor(&self, id: usize) -> Result<Value> {
        self.request().document.json["accessors"].get(id).cloned().ok_or_else(|| error("Invalid accessor ID"))
    }
    fn load_values(&mut self, loader: &GltfLoader<G, H>, id: usize, target: ValueTarget) -> Result<Vec<mc::MathValue>> {
        let a = self.accessor(id)?;
        let kind = shape(&a)?;
        let count = index(&a["count"]).unwrap_or(0);
        let result = vec![math(kind, &vec![0.; kind.get_number_of_components()], false); count];
        if let Some(view) = index(&a["bufferView"]) {
            let lease = loader.cache.buffer_view_loader(
                self.request().document.clone(),
                view,
                &self.request().resource,
                &self.request().base,
            )?;
            self.promises.push(lease.loader().load());
            self.views.push(ViewJob { lease, accessor: a, target, done: false });
        }
        Ok(result)
    }
    fn parse(&mut self, loader: &GltfLoader<G, H>, frame: &FrameState) -> Result<()> {
        let gltf = self.request().document.json.clone();
        let o = &loader.options;
        let mut texture_options = o.texture_options;
        texture_options.asynchronous = o.asynchronous;
        if let Some(copyright) = gltf["asset"]["copyright"].as_str() {
            self.components.asset.credits = copyright.split(';').map(|s| Credit::new(s.trim(), false)).collect();
        }
        if let Some(center) = gltf.pointer("/extensions/CESIUM_RTC/center") {
            self.components.transform = Matrix4::from_translation(Cartesian3::new(
                number(&center[0], 0.),
                number(&center[1], 0.),
                number(&center[2], 0.),
            ));
        }
        for (n, j) in list(&gltf["nodes"]).iter().enumerate() {
            let mut node = mc::Node {
                name: string(&j["name"]),
                index: Some(n),
                children: list(&j["children"]).iter().filter_map(index).collect(),
                ..Default::default()
            };
            if j["matrix"].is_array() {
                node.matrix = Some(Matrix4(std::array::from_fn(|i| number(&j["matrix"][i], f64::NAN))));
            }
            if j["translation"].is_array() {
                node.translation = Some(Cartesian3::new(
                    number(&j["translation"][0], 0.),
                    number(&j["translation"][1], 0.),
                    number(&j["translation"][2], 0.),
                ));
            }
            if j["rotation"].is_array() {
                node.rotation = Some(Quaternion::new(
                    number(&j["rotation"][0], 0.),
                    number(&j["rotation"][1], 0.),
                    number(&j["rotation"][2], 0.),
                    number(&j["rotation"][3], 0.),
                ));
            }
            if j["scale"].is_array() {
                node.scale = Some(Cartesian3::new(
                    number(&j["scale"][0], 0.),
                    number(&j["scale"][1], 0.),
                    number(&j["scale"][2], 0.),
                ));
            }
            node.articulation_name = string(&j["extensions"]["AGI_articulations"]["articulationName"]);
            if let Some(instancing) = j.pointer("/extensions/EXT_mesh_gpu_instancing") {
                if o.load_for_classification {
                    return Err(error(
                        "Models with the EXT_mesh_gpu_instancing extension cannot be used for classification.",
                    ));
                }
                let mut instances = mc::Instances::default();
                let attributes = &instancing["attributes"];
                if let Some(attrs) = attributes.as_object() {
                    for (name, id) in attrs {
                        let semantic = InstanceAttributeSemantic::from_gltf_semantic(name);
                        let transform = matches!(
                            semantic,
                            Some(
                                InstanceAttributeSemantic::Translation
                                    | InstanceAttributeSemantic::Rotation
                                    | InstanceAttributeSemantic::Scale
                            )
                        );
                        let only = o.load_attributes_as_typed_array
                            || (!attributes["ROTATION"].is_null() && transform)
                            || !loader.context.capabilities()?.instanced_arrays;
                        let missing_bounds = index(&attributes["TRANSLATION"])
                            .and_then(|i| gltf["accessors"].get(i))
                            .is_none_or(|a| a["min"].is_null() || a["max"].is_null());
                        let cpu = only
                            || (semantic == Some(InstanceAttributeSemantic::Translation)
                                && (missing_bounds
                                    || (o.load_attributes_for_2d && !frame.scene_3d_only)
                                    || (o.enable_pick && !loader.context.webgl2())));
                        let location = AttributeLocation {
                            node: n,
                            primitive: None,
                            attribute: PlannedAttribute::Attribute(instances.attributes.len()),
                        };
                        instances.attributes.push(self.load_attribute(
                            loader,
                            index(id).ok_or_else(|| error("Invalid instance accessor"))?,
                            name,
                            &Value::Null,
                            location,
                            GpuBufferOptions {
                                asynchronous: o.asynchronous,
                                load_buffer: !only,
                                load_typed_array: cpu,
                            },
                        )?);
                    }
                }
                instances.feature_ids = if j["extensions"]["EXT_instance_features"].is_null() {
                    self.legacy_features(
                        loader,
                        &instancing["extensions"]["EXT_feature_metadata"],
                        "instanceFeatureId",
                    )?
                } else {
                    self.features(loader, &j["extensions"]["EXT_instance_features"]["featureIds"], "instanceFeatureId")?
                };
                node.instances = Some(instances);
            }
            if let Some(mesh_id) = index(&j["mesh"]) {
                let mesh = &gltf["meshes"][mesh_id];
                let primitives = get_mesh_primitives(mesh).map_err(error)?.unwrap_or(Value::Array(vec![]));
                for (p, primitive) in list(&primitives).iter().enumerate() {
                    node.primitives.push(self.load_primitive(
                        loader,
                        primitive,
                        n,
                        p,
                        node.instances.is_some(),
                        frame,
                    )?);
                }
                let weights = j.get("weights").or_else(|| mesh.get("weights"));
                node.morph_weights = weights
                    .map(values)
                    .unwrap_or_else(|| vec![0.; node.primitives.first().map_or(0, |p| p.morph_targets.len())]);
            }
            self.components.nodes.push(node);
        }
        if !o.load_for_classification {
            for (i, j) in list(&gltf["skins"]).iter().enumerate() {
                let joints = list(&j["joints"]).iter().filter_map(index).collect::<Vec<_>>();
                let matrices = if let Some(id) = index(&j["inverseBindMatrices"]) {
                    self.load_values(loader, id, ValueTarget::Skin(i))?
                        .into_iter()
                        .map(|v| if let mc::MathValue::Matrix4(v) = v { v } else { Matrix4::IDENTITY })
                        .collect()
                } else {
                    vec![Matrix4::IDENTITY; joints.len()]
                };
                self.components.skins.push(mc::Skin { index: Some(i), joints, inverse_bind_matrices: matrices });
            }
            for (i, j) in list(&gltf["nodes"]).iter().enumerate() {
                self.components.nodes[i].skin = index(&j["skin"]);
            }
            for (i, j) in list(&gltf["animations"]).iter().enumerate() {
                let mut animation = mc::Animation { name: string(&j["name"]), ..Default::default() };
                for (s, sampler) in list(&j["samplers"]).iter().enumerate() {
                    let input = self
                        .load_values(
                            loader,
                            index(&sampler["input"]).ok_or_else(|| error("Missing animation input"))?,
                            ValueTarget::AnimationInput(i, s),
                        )?
                        .into_iter()
                        .map(|v| if let mc::MathValue::Number(v) = v { v } else { f64::NAN })
                        .collect();
                    let output = self.load_values(
                        loader,
                        index(&sampler["output"]).ok_or_else(|| error("Missing animation output"))?,
                        ValueTarget::AnimationOutput(i, s),
                    )?;
                    animation.samplers.push(mc::AnimationSampler {
                        input,
                        output,
                        interpolation: Some(match sampler["interpolation"].as_str() {
                            Some("STEP") => InterpolationType::Step,
                            Some("CUBICSPLINE") => InterpolationType::Cubicspline,
                            _ => InterpolationType::Linear,
                        }),
                    });
                }
                for channel in list(&j["channels"]) {
                    animation.channels.push(mc::AnimationChannel {
                        sampler: index(&channel["sampler"]),
                        target: index(&channel["target"]["node"]).map(|node| mc::AnimationTarget {
                            node: Some(node),
                            path: mc::AnimatedPropertyType::from_str_value(
                                channel["target"]["path"].as_str().unwrap_or(""),
                            ),
                        }),
                    });
                }
                self.components.animations.push(animation);
            }
        }
        for j in list(&gltf["extensions"]["AGI_articulations"]["articulations"]) {
            self.components.articulations.push(mc::Articulation {
                name: string(&j["name"]),
                stages: list(&j["stages"])
                    .iter()
                    .map(|s| mc::ArticulationStage {
                        name: string(&s["name"]),
                        stage_type: ArticulationStageType::from_str_value(s["type"].as_str().unwrap_or("")),
                        minimum_value: s["minimumValue"].as_f64(),
                        maximum_value: s["maximumValue"].as_f64(),
                        initial_value: s["initialValue"].as_f64(),
                    })
                    .collect(),
            });
        }
        let roots = index(&gltf["scene"])
            .and_then(|i| gltf["scenes"].get(i))
            .and_then(|s| s["nodes"].as_array())
            .map(|a| a.iter().filter_map(index).collect())
            .unwrap_or_default();
        self.components.scene = Some(mc::Scene { nodes: roots });
        let ext = &gltf["extensions"];
        if !ext["EXT_structural_metadata"].is_null() || !ext["EXT_feature_metadata"].is_null() {
            let metadata = GltfStructuralMetadataLoader::new(GltfStructuralMetadataLoaderOptions {
                cache: loader.cache.clone(),
                request: self.request().clone(),
                extension: ext.get("EXT_structural_metadata").cloned(),
                extension_legacy: ext.get("EXT_feature_metadata").cloned(),
                context: loader.context.clone(),
                host: loader.host.clone(),
                texture_options,
                cache_key: None,
            })?;
            self.promises.push(metadata.load());
            self.metadata = Some(metadata);
        }
        if let Some(ext) = ext.get("NGA_gpm_local") {
            self.components.gpm_local = Some(Rc::new(gpm::gltf_gpm_loader::load(ext)?));
        }
        for mesh in list(&gltf["meshes"]) {
            for primitive in list(&mesh["primitives"]) {
                if let Some(extension) = primitive.pointer("/extensions/NGA_gpm_local") {
                    let gpm = GltfMeshPrimitiveGpmLoader::new(GltfMeshPrimitiveGpmLoaderOptions {
                        cache: loader.cache.clone(),
                        request: self.request().clone(),
                        extension: extension.clone(),
                        context: loader.context.clone(),
                        host: loader.host.clone(),
                        texture_options,
                        cache_key: None,
                    });
                    self.promises.push(gpm.load());
                    self.gpm.push(gpm);
                }
            }
        }
        Ok(())
    }
    fn load_attribute(
        &mut self,
        loader: &GltfLoader<G, H>,
        id: usize,
        name: &str,
        primitive: &Value,
        location: AttributeLocation,
        options: GpuBufferOptions,
    ) -> Result<mc::Attribute<G>> {
        let a = self.accessor(id)?;
        let kind = shape(&a)?;
        let datatype = datatype(&a)?;
        let renamed = if loader.options.rename_batch_id_semantic && matches!(name, "_BATCHID" | "BATCHID") {
            "_FEATURE_ID_0"
        } else {
            name
        };
        let semantic = if location.primitive.is_some() {
            VertexAttributeSemantic::from_gltf_semantic(renamed).map(mc::AttributeSemantic::Vertex)
        } else {
            InstanceAttributeSemantic::from_gltf_semantic(renamed).map(mc::AttributeSemantic::Instance)
        };
        let set_index = semantic.and_then(|_| renamed.rsplit_once('_').and_then(|(_, n)| n.parse().ok()));
        let mut attribute = mc::Attribute {
            name: Some(name.into()),
            semantic,
            set_index,
            component_datatype: Some(datatype),
            attribute_type: Some(kind),
            normalized: a["normalized"].as_bool().unwrap_or(false),
            count: index(&a["count"]),
            constant: Some(math(kind, &vec![0.; kind.get_number_of_components()], false)),
            min: a.get("min").map(|v| math(kind, &values(v), false)),
            max: a.get("max").map(|v| math(kind, &values(v), false)),
            byte_offset: index(&a["byteOffset"]),
            byte_stride: Some(get_accessor_byte_stride(&self.request().document.json, &a).map_err(error)? as usize),
            ..Default::default()
        };
        if self.request().document.json["extensionsRequired"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v == "KHR_mesh_quantization"))
            && attribute.normalized
            && matches!(
                semantic,
                Some(mc::AttributeSemantic::Vertex(
                    VertexAttributeSemantic::Position
                        | VertexAttributeSemantic::Normal
                        | VertexAttributeSemantic::Tangent
                        | VertexAttributeSemantic::Texcoord
                        | VertexAttributeSemantic::FeatureId
                        | VertexAttributeSemantic::Scale
                        | VertexAttributeSemantic::Rotation
                ))
            )
        {
            let divisor = match datatype {
                ComponentDatatype::Byte => 127.,
                ComponentDatatype::UnsignedByte => 255.,
                ComponentDatatype::Short => 32767.,
                ComponentDatatype::UnsignedShort => 65535.,
                _ => 1.,
            };
            attribute.min = a
                .get("min")
                .map(|v| math(kind, &values(v).iter().map(|v| (v / divisor).max(-1.)).collect::<Vec<_>>(), false));
            attribute.max = a
                .get("max")
                .map(|v| math(kind, &values(v).iter().map(|v| (v / divisor).max(-1.)).collect::<Vec<_>>(), false));
        }
        if let Some(q) = a.pointer("/extensions/WEB3D_quantized_attributes") {
            if q["decodedMin"].is_array() && q["decodedMax"].is_array() {
                attribute.min = Some(math(kind, &values(&q["decodedMin"]), false));
                attribute.max = Some(math(kind, &values(&q["decodedMax"]), false));
            }
            let m = values(&q["decodeMatrix"]);
            let dim = (m.len() as f64).sqrt() as usize;
            let (offset, step) = if (2..=5).contains(&dim) {
                let t = match dim {
                    2 => AttributeType::Scalar,
                    3 => AttributeType::Vec2,
                    4 => AttributeType::Vec3,
                    _ => AttributeType::Vec4,
                };
                (
                    Some(math(t, &m[dim * (dim - 1)..], false)),
                    Some(math(t, &(0..dim - 1).map(|i| m[i * (dim + 1)]).collect::<Vec<_>>(), false)),
                )
            } else {
                (None, None)
            };
            attribute.quantization = Some(mc::Quantization {
                component_datatype: Some(datatype),
                attribute_type: Some(kind),
                quantized_volume_offset: offset,
                quantized_volume_step_size: step,
                ..Default::default()
            });
        }
        let draco =
            primitive.pointer("/extensions/KHR_draco_mesh_compression").filter(|e| !e["attributes"][name].is_null());
        let spz =
            primitive.pointer("/extensions/KHR_gaussian_splatting/extensions/KHR_gaussian_splatting_compression_spz_2");
        let source = if let Some(extension) = draco {
            Some(VertexBufferSource::Draco {
                primitive: primitive.clone(),
                extension: extension.clone(),
                semantic: name.into(),
                accessor_id: id,
            })
        } else if let Some(extension) = spz {
            Some(VertexBufferSource::Spz {
                primitive: primitive.clone(),
                extension: extension.clone(),
                semantic: name.into(),
            })
        } else {
            index(&a["bufferView"]).map(VertexBufferSource::BufferView)
        };
        if let Some(source) = source {
            let lease =
                loader.cache.vertex_buffer_loader(self.request(), source, Some(loader.context.clone()), options)?;
            self.promises.push(lease.loader().load());
            self.vertices.push(VertexJob {
                lease,
                location,
                accessor: a,
                compressed: draco.is_some(),
                spz: spz.is_some(),
                options,
                done: false,
            });
        }
        Ok(attribute)
    }
    fn load_primitive(
        &mut self,
        loader: &GltfLoader<G, H>,
        j: &Value,
        node: usize,
        p: usize,
        instances: bool,
        frame: &FrameState,
    ) -> Result<mc::Primitive<G>> {
        let o = &loader.options;
        let mut primitive = mc::Primitive::default();
        let mut plan = PrimitiveLoadPlan::default();
        if let Some(id) = index(&j["material"]) {
            let material = self.request().document.json["materials"][id].clone();
            primitive.material = Some(self.material(loader, &material)?);
        }
        let ext = &j["extensions"];
        plan.needs_outlines = o.load_primitive_outline && !ext["CESIUM_primitive_outline"].is_null();
        plan.needs_gaussian_splats =
            ext.pointer("/KHR_gaussian_splatting/extensions/KHR_gaussian_splatting_compression_spz_2").is_some();
        let post = plan.needs_outlines || plan.needs_gaussian_splats;
        if let Some(extension) = ext.get("EXT_mesh_primitive_edge_visibility") {
            let accessor =
                index(&extension["visibility"]).ok_or_else(|| error("Edge visibility accessor not found!"))?;
            if self.accessor(accessor).is_err() {
                return Err(error("Edge visibility accessor not found!"));
            }
            let visibility = self.load_values(loader, accessor, ValueTarget::EdgeVisibility(node, p))?;
            let silhouette_normals =
                if let Some(id) = index(&extension["silhouetteNormals"]).filter(|&id| self.accessor(id).is_ok()) {
                    Some(self.load_values(loader, id, ValueTarget::SilhouetteNormals(node, p))?)
                } else {
                    None
                };
            if !extension["lineStrings"].is_null() {
                return Err(error("Cannot set properties of undefined (setting 'lineStrings')"));
            }
            primitive.edge_visibility =
                Some(mc::EdgeVisibility { visibility, material: index(&extension["material"]), silhouette_normals });
        }

        if plan.needs_outlines {
            let id =
                index(&ext["CESIUM_primitive_outline"]["indices"]).ok_or_else(|| error("Missing outline accessor"))?;
            let v = self.load_values(loader, id, ValueTarget::Outline(self.plans.len()))?;
            plan.outline_indices =
                Some(v.into_iter().map(|v| if let mc::MathValue::Number(v) = v { v } else { f64::NAN }).collect());
        }
        let mut feature_ids = false;
        if let Some(attrs) = j["attributes"].as_object() {
            for (name, id) in attrs {
                let renamed = if o.rename_batch_id_semantic && matches!(name.as_str(), "_BATCHID" | "BATCHID") {
                    "_FEATURE_ID_0"
                } else {
                    name
                };
                let semantic = VertexAttributeSemantic::from_gltf_semantic(renamed);
                if o.load_for_classification
                    && !matches!(
                        semantic,
                        Some(
                            VertexAttributeSemantic::Position
                                | VertexAttributeSemantic::Texcoord
                                | VertexAttributeSemantic::FeatureId
                        )
                    )
                {
                    continue;
                }
                feature_ids |= semantic == Some(VertexAttributeSemantic::FeatureId);
                let cpu = o.load_attributes_as_typed_array
                    || (semantic == Some(VertexAttributeSemantic::Position)
                        && ((!instances && o.load_attributes_for_2d && !frame.scene_3d_only)
                            || (o.enable_pick && !loader.context.webgl2())))
                    || (o.load_for_classification && semantic == Some(VertexAttributeSemantic::FeatureId));
                let planned = PlannedAttribute::Attribute(primitive.attributes.len());
                let location = AttributeLocation { node, primitive: Some(p), attribute: planned };
                primitive.attributes.push(self.load_attribute(
                    loader,
                    index(id).ok_or_else(|| error("Invalid vertex accessor"))?,
                    name,
                    j,
                    location,
                    GpuBufferOptions {
                        asynchronous: o.asynchronous,
                        load_buffer: !post && !o.load_attributes_as_typed_array,
                        load_typed_array: post || cpu,
                    },
                )?);
                plan.attribute_plans.push(AttributeLoadPlan {
                    attribute: planned,
                    load_buffer: !o.load_attributes_as_typed_array,
                    load_typed_array: cpu,
                });
            }
        }
        if !o.load_for_classification {
            for (target, t) in list(&j["targets"]).iter().enumerate() {
                let mut morph = mc::MorphTarget::default();
                if let Some(attrs) = t.as_object() {
                    for (name, id) in attrs {
                        let semantic = VertexAttributeSemantic::from_gltf_semantic(name);
                        let cpu = o.load_attributes_as_typed_array
                            || (semantic == Some(VertexAttributeSemantic::Position)
                                && ((o.load_attributes_for_2d && !frame.scene_3d_only)
                                    || (o.enable_pick && !loader.context.webgl2())));
                        let planned = PlannedAttribute::MorphTarget { target, attribute: morph.attributes.len() };
                        let location = AttributeLocation { node, primitive: Some(p), attribute: planned };
                        morph.attributes.push(self.load_attribute(
                            loader,
                            index(id).ok_or_else(|| error("Invalid morph accessor"))?,
                            name,
                            &Value::Null,
                            location,
                            GpuBufferOptions {
                                asynchronous: o.asynchronous,
                                load_buffer: !post && !o.load_attributes_as_typed_array,
                                load_typed_array: post || cpu,
                            },
                        )?);
                        plan.attribute_plans.push(AttributeLoadPlan {
                            attribute: planned,
                            load_buffer: !o.load_attributes_as_typed_array,
                            load_typed_array: cpu,
                        });
                    }
                }
                primitive.morph_targets.push(morph);
            }
        }
        if let Some(id) = index(&j["indices"]) {
            let a = self.accessor(id)?;
            let draco = ext.get("KHR_draco_mesh_compression");
            if !a["bufferView"].is_null() || draco.is_some() {
                let cpu = o.load_attributes_as_typed_array
                    || ((o.load_indices_for_wireframe || o.enable_pick) && !loader.context.webgl2())
                    || (o.load_for_classification && feature_ids)
                    || !ext["EXT_mesh_primitive_edge_visibility"].is_null();
                let lease = loader.cache.index_buffer_loader(
                    self.request(),
                    id,
                    draco.map(|d| (j, d)),
                    Some(loader.context.clone()),
                    GpuBufferOptions {
                        asynchronous: o.asynchronous,
                        load_buffer: !post && !o.load_attributes_as_typed_array,
                        load_typed_array: post || cpu,
                    },
                )?;
                self.promises.push(lease.loader().load());
                self.indices.push(IndexJob { lease, node, primitive: p, done: false });
                primitive.indices = Some(mc::Indices { count: index(&a["count"]), ..Default::default() });
                plan.indices_plan =
                    Some(IndicesLoadPlan { load_buffer: !o.load_attributes_as_typed_array, load_typed_array: cpu });
            }
        }
        primitive.feature_ids = if ext["EXT_mesh_features"].is_null() {
            self.legacy_features(loader, &ext["EXT_feature_metadata"], "featureId")?
        } else {
            self.features(loader, &ext["EXT_mesh_features"]["featureIds"], "featureId")?
        };
        primitive.property_texture_ids =
            list(&ext["EXT_structural_metadata"]["propertyTextures"]).iter().filter_map(index).collect();
        primitive.property_attribute_ids =
            list(&ext["EXT_structural_metadata"]["propertyAttributes"]).iter().filter_map(index).collect();
        if ext["EXT_structural_metadata"].is_null() {
            let root = &self.request().document.json["extensions"]["EXT_feature_metadata"]["featureTextures"];
            let mut keys = root.as_object().map(|m| m.keys().collect::<Vec<_>>()).unwrap_or_default();
            keys.sort();
            primitive.property_texture_ids = list(&ext["EXT_feature_metadata"]["featureTextures"])
                .iter()
                .filter_map(|v| v.as_str().and_then(|id| keys.iter().position(|k| k.as_str() == id)))
                .collect();
        }
        primitive.primitive_type = PrimitiveType::from_gl(j["mode"].as_u64().unwrap_or(4) as u32);
        if o.load_for_classification && primitive.primitive_type != Some(PrimitiveType::Triangles) {
            return Err(error("Only triangle meshes can be used for classification."));
        }
        self.plans.push((node, p, plan));
        Ok(primitive)
    }
    fn texture(&mut self, loader: &GltfLoader<G, H>, info: &Value) -> Result<Option<mc::TextureReader<G>>> {
        if info.is_null() {
            return Ok(None);
        }
        let id = info["index"].as_f64().ok_or_else(|| error("Missing texture index"))?;
        let supported = loader.options.texture_options.supported_image_formats;
        if gltf_loader_util::get_image_id_from_texture(&self.request().document.json, &info["index"], supported)
            .map_err(error)?
            .is_none()
        {
            return Ok(None);
        }
        let mut options = loader.options.texture_options;
        options.asynchronous = loader.options.asynchronous;
        let lease = loader.cache.texture_loader(
            self.request(),
            info.clone(),
            loader.context.clone(),
            loader.host.clone(),
            options,
        )?;
        self.texture_promises.push(lease.loader().load());
        self.textures.push(TextureJob { lease, index: id, done: false });
        Ok(Some(gltf_loader_util::create_model_texture_reader(info, None, None)))
    }
    fn features(&mut self, loader: &GltfLoader<G, H>, ids: &Value, prefix: &str) -> Result<Vec<mc::FeatureId<G>>> {
        let mut result = vec![];
        for (i, j) in list(ids).iter().enumerate() {
            let label = string(&j["label"]);
            let positional_label = Some(format!("{prefix}_{i}"));
            let feature_count = index(&j["featureCount"]).map(|v| v as u32);
            let null_feature_id = index(&j["nullFeatureId"]).map(|v| v as u32);
            let property_table_id = index(&j["propertyTable"]);
            result.push(if let Some(set) = index(&j["attribute"]).filter(|_| j["texture"].is_null()) {
                mc::FeatureId::Attribute(mc::FeatureIdAttribute {
                    feature_count,
                    null_feature_id,
                    property_table_id,
                    set_index: Some(set as u32),
                    label,
                    positional_label,
                })
            } else if !j["texture"].is_null() {
                let mut reader = self.texture(loader, &j["texture"])?;
                if let Some(r) = &mut reader {
                    r.channels = Some(
                        j["texture"]["channels"]
                            .as_array()
                            .map(|v| {
                                v.iter()
                                    .filter_map(|v| index(v).and_then(|i| "rgba".chars().nth(i)))
                                    .collect::<String>()
                            })
                            .unwrap_or_else(|| "r".into()),
                    );
                }
                mc::FeatureId::Texture(mc::FeatureIdTexture {
                    feature_count,
                    null_feature_id,
                    property_table_id,
                    label,
                    positional_label,
                    texture_reader: reader,
                })
            } else {
                mc::FeatureId::ImplicitRange(mc::FeatureIdImplicitRange {
                    feature_count,
                    null_feature_id,
                    property_table_id,
                    label,
                    positional_label,
                    offset: 0.,
                    repeat: Some(1.),
                })
            });
        }
        Ok(result)
    }
    fn legacy_features(
        &mut self,
        loader: &GltfLoader<G, H>,
        ext: &Value,
        prefix: &str,
    ) -> Result<Vec<mc::FeatureId<G>>> {
        let tables = self.request().document.json["extensions"]["EXT_feature_metadata"]["featureTables"].clone();
        let mut keys = tables.as_object().map(|m| m.keys().collect::<Vec<_>>()).unwrap_or_default();
        keys.sort();
        let mut result = vec![];
        for j in list(&ext["featureIdAttributes"]) {
            let table = j["featureTable"].as_str().unwrap_or("");
            let property_table_id = keys.iter().position(|k| k.as_str() == table);
            let feature_count = index(&tables[table]["count"]).map(|v| v as u32);
            let ids = &j["featureIds"];
            let positional_label = Some(format!("{prefix}_{}", result.len()));
            result.push(if let Some(name) = ids["attribute"].as_str() {
                mc::FeatureId::Attribute(mc::FeatureIdAttribute {
                    feature_count,
                    property_table_id,
                    set_index: name.rsplit_once('_').and_then(|(_, v)| v.parse().ok()),
                    positional_label,
                    ..Default::default()
                })
            } else {
                mc::FeatureId::ImplicitRange(mc::FeatureIdImplicitRange {
                    feature_count,
                    property_table_id,
                    positional_label,
                    offset: number(&ids["constant"], 0.),
                    repeat: ids["divisor"].as_f64().filter(|v| *v != 0.),
                    ..Default::default()
                })
            });
        }
        for j in list(&ext["featureIdTextures"]) {
            let table = j["featureTable"].as_str().unwrap_or("");
            let property_table_id = keys.iter().position(|k| k.as_str() == table);
            let feature_count = index(&tables[table]["count"]).map(|v| v as u32);
            let ids = &j["featureIds"];
            let mut texture_reader = self.texture(loader, &ids["texture"])?;
            if let Some(r) = &mut texture_reader {
                r.channels = string(&ids["channels"]);
            }
            result.push(mc::FeatureId::Texture(mc::FeatureIdTexture {
                feature_count,
                property_table_id,
                texture_reader,
                positional_label: Some(format!("{prefix}_{}", result.len())),
                ..Default::default()
            }));
        }
        Ok(result)
    }
    fn material(&mut self, loader: &GltfLoader<G, H>, j: &Value) -> Result<mc::Material<G>> {
        let c3 = |v: &Value, d: Cartesian3| {
            if v.is_array() {
                Cartesian3::new(number(&v[0], 0.), number(&v[1], 0.), number(&v[2], 0.))
            } else {
                d
            }
        };
        let c4 = |v: &Value, d: Cartesian4| {
            if v.is_array() {
                Cartesian4::new(number(&v[0], 0.), number(&v[1], 0.), number(&v[2], 0.), number(&v[3], 0.))
            } else {
                d
            }
        };
        let ext = &j["extensions"];
        let mut m = mc::Material { unlit: !ext["KHR_materials_unlit"].is_null(), ..Default::default() };
        if let Some(s) = ext.get("KHR_materials_pbrSpecularGlossiness") {
            m.specular_glossiness = Some(mc::SpecularGlossiness {
                diffuse_texture: self.texture(loader, &s["diffuseTexture"])?,
                specular_glossiness_texture: self.texture(loader, &s["specularGlossinessTexture"])?,
                diffuse_factor: c4(&s["diffuseFactor"], Cartesian4::ONE),
                specular_factor: c3(&s["specularFactor"], Cartesian3::ONE),
                glossiness_factor: number(&s["glossinessFactor"], 1.),
            });
        } else {
            let s = &j["pbrMetallicRoughness"];
            m.metallic_roughness = mc::MetallicRoughness {
                base_color_texture: self.texture(loader, &s["baseColorTexture"])?,
                metallic_roughness_texture: self.texture(loader, &s["metallicRoughnessTexture"])?,
                base_color_factor: c4(&s["baseColorFactor"], Cartesian4::ONE),
                metallic_factor: number(&s["metallicFactor"], 1.),
                roughness_factor: number(&s["roughnessFactor"], 1.),
            };
            if !m.unlit {
                if let Some(s) = ext.get("KHR_materials_specular") {
                    m.specular = Some(mc::Specular {
                        specular_factor: number(&s["specularFactor"], 1.),
                        specular_color_factor: c3(&s["specularColorFactor"], Cartesian3::ONE),
                        specular_texture: self.texture(loader, &s["specularTexture"])?,
                        specular_color_texture: self.texture(loader, &s["specularColorTexture"])?,
                    });
                }
                if let Some(s) = ext.get("KHR_materials_anisotropy") {
                    m.anisotropy = Some(mc::Anisotropy {
                        anisotropy_strength: number(&s["anisotropyStrength"], 0.),
                        anisotropy_rotation: number(&s["anisotropyRotation"], 0.),
                        anisotropy_texture: self.texture(loader, &s["anisotropyTexture"])?,
                    });
                }
                if let Some(s) = ext.get("KHR_materials_clearcoat") {
                    m.clearcoat = Some(mc::Clearcoat {
                        clearcoat_factor: number(&s["clearcoatFactor"], 0.),
                        clearcoat_roughness_factor: number(&s["clearcoatRoughnessFactor"], 0.),
                        clearcoat_texture: self.texture(loader, &s["clearcoatTexture"])?,
                        clearcoat_roughness_texture: self.texture(loader, &s["clearcoatRoughnessTexture"])?,
                        clearcoat_normal_texture: self.texture(loader, &s["clearcoatNormalTexture"])?,
                    });
                }
            }
        }
        m.emissive_texture = self.texture(loader, &j["emissiveTexture"])?;
        m.occlusion_texture = self.texture(loader, &j["occlusionTexture"])?;
        if !loader.options.load_for_classification {
            m.normal_texture = self.texture(loader, &j["normalTexture"])?;
        }
        m.emissive_factor = c3(&j["emissiveFactor"], Cartesian3::ZERO);
        m.alpha_mode =
            AlphaMode::from_str_value(j["alphaMode"].as_str().unwrap_or("OPAQUE")).unwrap_or(AlphaMode::Opaque);
        m.alpha_cutoff = j["alphaCutoff"].as_f64();
        m.double_sided = j["doubleSided"].as_bool().unwrap_or(false);
        m.point_diameter =
            ext["BENTLEY_materials_point_style"]["diameter"].as_f64().filter(|v| *v >= 1.).map(f64::floor);
        Ok(m)
    }
    fn process(&mut self, loader: &GltfLoader<G, H>, frame: &FrameState) -> Result<bool> {
        fn poll(promises: &mut Vec<ResourceLoadFuture>) -> Result<bool> {
            let mut pending = vec![];
            let mut failure = None;
            for future in promises.drain(..) {
                match future.clone().now_or_never() {
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        failure.get_or_insert(e);
                    }
                    None => pending.push(future),
                }
            }
            *promises = pending;
            failure.map_or_else(|| Ok(promises.is_empty()), Err)
        }
        let resources_loaded = poll(&mut self.promises)?;
        let textures_loaded = match poll(&mut self.texture_promises) {
            Ok(ready) => ready,
            Err(e) if loader.options.incrementally_load_textures => {
                self.textures_failed = true;
                return Err(RuntimeError::new(format!("Failed to load glTF texture\n{}", e.message)));
            }
            Err(e) => return Err(e),
        };
        if !self.ready {
            let document = self.request().document.clone();
            let mut ready = resources_loaded;
            for job in &mut self.vertices {
                if job.done {
                    continue;
                }
                let child = job.lease.loader();
                if !child.process(frame)? {
                    ready = false;
                    continue;
                }
                let a = attribute_mut(&mut self.components, job.location);
                if job.options.load_buffer {
                    a.buffer = child.buffer();
                }
                if job.compressed || job.spz {
                    a.byte_offset = Some(0);
                    a.byte_stride = None;
                    if let Some(q) = child.quantization() {
                        let convert = |v: Option<MetadataValue>| {
                            v.map(|v| {
                                let v = v
                                    .as_array()
                                    .map(|a| a.iter().map(MetadataValue::to_number).collect::<Vec<_>>())
                                    .unwrap_or_else(|| vec![v.to_number()]);
                                let kind = match v.len() {
                                    2 => AttributeType::Vec2,
                                    3 => AttributeType::Vec3,
                                    4 => AttributeType::Vec4,
                                    _ => AttributeType::Scalar,
                                };
                                math(kind, &v, false)
                            })
                        };
                        a.quantization = Some(mc::Quantization {
                            oct_encoded: q.oct_encoded,
                            oct_encoded_zxy: q.oct_encoded_zxy,
                            component_datatype: Some(q.component_datatype),
                            attribute_type: Some(q.attribute_type),
                            normalization_range: convert(q.normalization_range),
                            quantized_volume_offset: convert(q.quantized_volume_offset),
                            quantized_volume_dimensions: convert(q.quantized_volume_dimensions),
                            quantized_volume_step_size: convert(q.quantized_volume_step_size),
                        });
                    }
                    if job.options.load_typed_array {
                        if let Some(bytes) = child.typed_array() {
                            let dt = a
                                .quantization
                                .as_ref()
                                .and_then(|q| q.component_datatype)
                                .or(a.component_datatype)
                                .ok_or_else(|| error("Missing component datatype"))?;
                            a.typed_array = dt.create_array_buffer_view(bytes.bytes(), 0, None);
                        }
                    }
                    if job.spz && a.semantic == Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)) {
                        let data = a.typed_array.as_ref().ok_or_else(|| error("Missing SPZ positions"))?.to_f64_vec();
                        let mut min = [f64::INFINITY; 3];
                        let mut max = [f64::NEG_INFINITY; 3];
                        for row in data.chunks(3) {
                            for (i, &v) in row.iter().enumerate() {
                                min[i] = min[i].min(v);
                                max[i] = max[i].max(v);
                            }
                        }
                        a.min = Some(math(AttributeType::Vec3, &min, false));
                        a.max = Some(math(AttributeType::Vec3, &max, false));
                    }
                } else if job.options.load_typed_array {
                    if let Some(source) = child.typed_array() {
                        a.typed_array = Some(packed(&document.json, &job.accessor, &source)?);
                    }
                    if !job.options.load_buffer {
                        a.byte_offset = Some(0);
                        a.byte_stride = None;
                    }
                }
                job.done = true;
            }
            for job in &mut self.indices {
                if job.done {
                    continue;
                }
                let child = job.lease.loader();
                if !child.process(frame)? {
                    ready = false;
                    continue;
                }
                let i = self.components.nodes[job.node].primitives[job.primitive].indices.as_mut().unwrap();
                i.index_datatype = Some(child.index_datatype());
                i.buffer = child.buffer();
                i.typed_array = child.typed_array();
                job.done = true;
            }
            for job in &mut self.views {
                if job.done {
                    continue;
                }
                let child = job.lease.loader();
                let Some(source) = child.typed_array() else {
                    ready = false;
                    continue;
                };
                let array = packed(&document.json, &job.accessor, &source)?;
                let data = array.to_f64_vec();
                let kind = shape(&job.accessor)?;
                let n = kind.get_number_of_components();
                let output = data
                    .chunks(n)
                    .map(|v| math(kind, v, matches!(job.target, ValueTarget::AnimationOutput(..))))
                    .collect::<Vec<_>>();
                match job.target {
                    ValueTarget::Outline(i) => self.plans[i].2.outline_indices = Some(data),
                    ValueTarget::EdgeVisibility(n, p) => {
                        self.components.nodes[n].primitives[p].edge_visibility.as_mut().unwrap().visibility = output
                    }
                    ValueTarget::SilhouetteNormals(n, p) => {
                        self.components.nodes[n].primitives[p].edge_visibility.as_mut().unwrap().silhouette_normals =
                            Some(output)
                    }
                    ValueTarget::Skin(i) => {
                        self.components.skins[i].inverse_bind_matrices = output
                            .into_iter()
                            .map(|v| if let mc::MathValue::Matrix4(v) = v { v } else { Matrix4::ZERO })
                            .collect()
                    }
                    ValueTarget::AnimationInput(a, s) => self.components.animations[a].samplers[s].input = data,
                    ValueTarget::AnimationOutput(a, s) => self.components.animations[a].samplers[s].output = output,
                }
                job.done = true;
            }
            if let Some(metadata) = &self.metadata {
                let done = metadata.process(frame)?;
                ready &= done;
                if done {
                    self.components.structural_metadata = metadata.structural_metadata().map(FrameResource::new);
                }
            }
            for gpm in &self.gpm {
                let done = gpm.process(frame)?;
                ready &= done;
                if done {
                    self.components.structural_metadata = gpm.structural_metadata().map(FrameResource::new);
                }
            }
            if ready {
                for (node, primitive, plan) in &mut self.plans {
                    plan.post_process(&mut self.components.nodes[*node].primitives[*primitive], &loader.context)?;
                }
                self.views.clear();
                self.request.take();
                self.ready = true;
                loader.data.set_state(ResourceLoaderState::Ready);
            }
        }
        let mut ready = textures_loaded;
        for job in &mut self.textures {
            if job.done {
                continue;
            }
            let child = job.lease.loader();
            if !child.process(frame)? {
                ready = false;
                continue;
            }
            if let Some(texture) = child.texture() {
                attach_texture(&mut self.components, job.index, &texture)?;
            }
            job.done = true;
        }
        self.textures_ready = ready && !self.textures_failed;
        Ok(self.ready && (loader.options.incrementally_load_textures || self.textures_ready))
    }
}
fn attach_texture<G: Gl + 'static>(
    components: &mut mc::Components<G>,
    id: f64,
    texture: &Rc<crate::renderer::Texture<G>>,
) -> Result<()> {
    fn reader<G: Gl>(r: &mut Option<mc::TextureReader<G>>, id: f64, texture: &Rc<crate::renderer::Texture<G>>) {
        if let Some(r) = r {
            if r.index == Some(id) {
                r.texture = Some(texture.clone());
            }
        }
    }
    fn features<G: Gl>(ids: &mut [mc::FeatureId<G>], id: f64, texture: &Rc<crate::renderer::Texture<G>>) -> Result<()> {
        for f in ids {
            if let mc::FeatureId::Texture(f) = f {
                if f.texture_reader.as_ref().is_some_and(|r| r.index == Some(id)) {
                    let mut sampler = texture.sampler();
                    sampler.wrap_s = crate::renderer::TextureWrap::ClampToEdge;
                    sampler.wrap_t = crate::renderer::TextureWrap::ClampToEdge;
                    sampler.wrap_r = crate::renderer::TextureWrap::ClampToEdge;
                    sampler.minification_filter = crate::renderer::TextureMinificationFilter::Nearest;
                    sampler.magnification_filter = crate::renderer::TextureMagnificationFilter::Nearest;
                    texture.set_sampler(sampler)?;
                    reader(&mut f.texture_reader, id, texture);
                }
            }
        }
        Ok(())
    }
    for node in &mut components.nodes {
        if let Some(instances) = &mut node.instances {
            features(&mut instances.feature_ids, id, texture)?;
        }
        for p in &mut node.primitives {
            features(&mut p.feature_ids, id, texture)?;
            if let Some(m) = &mut p.material {
                reader(&mut m.metallic_roughness.base_color_texture, id, texture);
                reader(&mut m.metallic_roughness.metallic_roughness_texture, id, texture);
                reader(&mut m.emissive_texture, id, texture);
                reader(&mut m.normal_texture, id, texture);
                reader(&mut m.occlusion_texture, id, texture);
                if let Some(s) = &mut m.specular_glossiness {
                    reader(&mut s.diffuse_texture, id, texture);
                    reader(&mut s.specular_glossiness_texture, id, texture);
                }
                if let Some(s) = &mut m.specular {
                    reader(&mut s.specular_texture, id, texture);
                    reader(&mut s.specular_color_texture, id, texture);
                }
                if let Some(s) = &mut m.anisotropy {
                    reader(&mut s.anisotropy_texture, id, texture);
                }
                if let Some(s) = &mut m.clearcoat {
                    reader(&mut s.clearcoat_texture, id, texture);
                    reader(&mut s.clearcoat_roughness_texture, id, texture);
                    reader(&mut s.clearcoat_normal_texture, id, texture);
                }
            }
        }
    }
    Ok(())
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> Drop for Runtime<G, H>
where
    H::Image: 'static,
{
    fn drop(&mut self) {
        for (node, primitive, plan) in &self.plans {
            if !plan.needs_outlines {
                continue;
            }
            let Some(p) = self.components.nodes.get(*node).and_then(|n| n.primitives.get(*primitive)) else { continue };
            for a in &p.attributes {
                if let Some(b) = &a.buffer {
                    let _ = b.destroy();
                }
            }
            for t in &p.morph_targets {
                for a in &t.attributes {
                    if let Some(b) = &a.buffer {
                        let _ = b.destroy();
                    }
                }
            }
            if let Some(a) = &p.outline_coordinates {
                if let Some(b) = &a.buffer {
                    let _ = b.destroy();
                }
            }
            if let Some(i) = &p.indices {
                if let Some(b) = &i.buffer {
                    let _ = b.destroy();
                }
            }
        }
    }
}
