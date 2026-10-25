mod common;
use cesium_rs::core::image_helpers::ImageFuture;
use cesium_rs::{core::*, renderer::*, scene::model_components as mc, scene::*};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};
#[derive(Default, Clone)]
struct Host {
    next: Rc<Cell<u32>>,
    bound: Rc<Cell<u32>>,
    buffers: Rc<RefCell<HashMap<u32, Vec<u8>>>>,
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        if name == "bindBuffer" {
            if let Some(GlValue::Object(id)) = args.get(1) {
                self.bound.set(*id);
            }
        }
        if name == "bufferData" {
            if let Some(GlValue::Bytes(b)) = args.get(1) {
                self.buffers.borrow_mut().insert(self.bound.get(), b.clone());
            }
        }
        if name == "getBufferSubData" {
            let GlValue::Number(offset) = args[1] else { panic!() };
            let GlValue::Bytes(b) = &args[2] else { panic!() };
            return Ok(GlValue::Bytes(
                self.buffers.borrow()[&self.bound.get()][offset as usize..offset as usize + b.len()].to_vec(),
            ));
        }
        Ok(if name == "getParameter" {
            match args.first() {
                Some(GlValue::Number(v))
                    if [
                        webgl_constants::ALIASED_LINE_WIDTH_RANGE,
                        webgl_constants::ALIASED_POINT_SIZE_RANGE,
                        webgl_constants::MAX_VIEWPORT_DIMS,
                    ]
                    .contains(&(*v as u32)) =>
                {
                    GlValue::Numbers(vec![1., 16384.])
                }
                _ => GlValue::Number(16384.),
            }
        } else {
            GlValue::Null
        })
    }
}
struct Images;
impl ImageHost for Images {
    type Image = RgbaImage;
    fn dimensions(&self, i: &RgbaImage) -> (u32, u32) {
        (i.width, i.height)
    }
    fn image_from_rgba(&self, i: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(i)
    }
    fn image_pixels(&self, i: &RgbaImage, _: u32, _: u32) -> Result<Vec<u8>, RuntimeError> {
        Ok(i.pixels.clone())
    }
    fn resize_image(&self, _: &RgbaImage, w: u32, h: u32) -> Result<RgbaImage, RuntimeError> {
        Ok(RgbaImage::new(w, h, vec![255; (w * h * 4) as usize]).unwrap())
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        Some(Box::pin(async { Ok(RgbaImage::new(1, 1, vec![255; 4]).unwrap()) }))
    }
    fn load_resource_url<'a>(
        &'a self,
        _: &'a str,
        _: bool,
        _: FetchImageOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        self.decode_image(&[], "", ImageDecodeOptions { flip_y: false, skip_color_space_conversion: true })
    }
}
impl ModelImageHost<Host> for Images {}
struct Transport;
impl ResourceTransport for Transport {
    fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
        Box::pin(async { Err(RequestErrorEvent::new(Some(404), None, None)) })
    }
}
fn frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(8., 8.));
    let s = camera.uniform_camera();
    let c = camera.frustum.compute_culling_volume(s.position, s.direction, s.up);
    FrameState::new(UniformFrame::new(s, JulianDate::new(2451545., 0., TimeStandard::Tai)), c, [8, 8])
}
fn ready<T>(future: impl Future<Output = T>) -> T {
    match std::pin::pin!(future).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("unexpected pending request"),
    }
}
fn math(v: mc::MathValue) -> Value {
    use mc::MathValue as M;
    match v {
        M::Number(v) => num(v),
        M::Cartesian2(v) => v.enc(),
        M::Cartesian3(v) => v.enc(),
        M::Cartesian4(v) => v.enc(),
        M::Quaternion(v) => v.enc(),
        M::Matrix2(v) => v.enc(),
        M::Matrix3(v) => v.enc(),
        M::Matrix4(v) => v.enc(),
    }
}
fn array(a: &TypedArray) -> Value {
    a.to_f64_vec().into_iter().map(num).collect()
}
fn buffer<G: Gl>(b: &Option<Buffer<G>>) -> Value {
    b.as_ref().map(|b| json!(b.get_buffer_data(0, b.size_in_bytes()).unwrap())).unwrap_or(Value::Null)
}
fn attr<G: Gl>(a: &mc::Attribute<G>) -> Value {
    json!({"name":a.name,"semantic":a.semantic.map(|s|s.as_str()),"setIndex":a.set_index,"componentDatatype":a.component_datatype.map(|v|v as u32),"type":a.attribute_type.map(|v|v.as_str()),"normalized":a.normalized,"count":a.count,"min":a.min.map(math),"max":a.max.map(math),"constant":a.constant.map(math),"byteOffset":a.byte_offset,"byteStride":a.byte_stride,"array":a.typed_array.as_ref().map(array),"buffer":buffer(&a.buffer)})
}
fn feature<G: Gl>(f: &mc::FeatureId<G>) -> Value {
    match f {
        mc::FeatureId::Attribute(f) => {
            json!({"featureCount":f.feature_count,"nullFeatureId":f.null_feature_id,"propertyTableId":f.property_table_id,"setIndex":f.set_index,"label":f.label,"positionalLabel":f.positional_label,"offset":null,"repeat":null,"texture":null})
        }
        mc::FeatureId::ImplicitRange(f) => {
            json!({"featureCount":f.feature_count,"nullFeatureId":f.null_feature_id,"propertyTableId":f.property_table_id,"setIndex":null,"label":f.label,"positionalLabel":f.positional_label,"offset":f.offset,"repeat":f.repeat,"texture":null})
        }
        mc::FeatureId::Texture(f) => {
            json!({"featureCount":f.feature_count,"nullFeatureId":f.null_feature_id,"propertyTableId":f.property_table_id,"setIndex":null,"label":f.label,"positionalLabel":f.positional_label,"offset":null,"repeat":null,"texture":f.texture_reader.as_ref().map(|r|{let s=r.texture.as_ref().unwrap().sampler();json!({"index":r.index,"channels":r.channels,"loaded":r.texture.is_some(),"sampler":[s.wrap_s as u32,s.wrap_t as u32,s.minification_filter as u32,s.magnification_filter as u32]})})})
        }
    }
}
fn material<G: Gl>(m: &mc::Material<G>) -> Value {
    json!({"base":m.metallic_roughness.base_color_factor.enc(),"metallic":m.metallic_roughness.metallic_factor,"roughness":m.metallic_roughness.roughness_factor,"emissive":m.emissive_factor.enc(),"alphaMode":m.alpha_mode.as_str(),"alphaCutoff":m.alpha_cutoff,"doubleSided":m.double_sided,"unlit":m.unlit,"pointDiameter":m.point_diameter,"texture":m.metallic_roughness.base_color_texture.as_ref().map(|t|json!({"index":t.index,"texCoord":t.tex_coord,"transform":t.transform.map(|m|m.enc()),"loaded":t.texture.is_some()}))})
}
fn snapshot<G: Gl + 'static>(c: &mc::Components<G>) -> Value {
    json!({"metadata":c.structural_metadata.as_ref().and_then(|r|r.get::<Rc<StructuralMetadata<G>>>()).map(|m|json!({"tables":m.property_table_count(),"textures":m.property_textures().len(),"attributes":m.property_attributes().len()})),"gpm":c.gpm_local.as_ref().map(|g|g.storage_type().as_str()),"articulations":c.articulations.iter().map(|a|json!({"name":a.name,"stages":a.stages.iter().map(|s|json!({"name":s.name,"type":s.stage_type.map(|t|t.as_str()),"min":s.minimum_value,"max":s.maximum_value,"initial":s.initial_value})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"nodes":c.nodes.iter().map(|n|json!({"name":n.name,"index":n.index,"children":n.children,"matrix":n.matrix.map(|m|m.enc()),"translation":n.translation.map(|v|v.enc()),"rotation":n.rotation.map(|v|v.enc()),"scale":n.scale.map(|v|v.enc()),"weights":n.morph_weights,"skin":n.skin,"instances":n.instances.as_ref().map(|i|json!({"attributes":i.attributes.iter().map(attr).collect::<Vec<_>>(),"features":i.feature_ids.iter().map(feature).collect::<Vec<_>>()})),"primitives":n.primitives.iter().map(|p|json!({"attributes":p.attributes.iter().map(attr).collect::<Vec<_>>(),"targets":p.morph_targets.iter().map(|t|t.attributes.iter().map(attr).collect::<Vec<_>>()).collect::<Vec<_>>(),"indices":p.indices.as_ref().map(|i|json!({"count":i.count,"type":i.index_datatype.map(|v|v as u32),"array":i.typed_array.as_ref().map(array),"buffer":buffer(&i.buffer)})),"outline":p.outline_coordinates.as_ref().map(attr),"edge":p.edge_visibility.as_ref().map(|e|json!({"visibility":e.visibility.iter().copied().map(math).collect::<Vec<_>>(),"material":e.material,"silhouetteNormals":e.silhouette_normals.as_ref().map(|v|v.iter().copied().map(math).collect::<Vec<_>>())})),"type":p.primitive_type.map(|v|v as u32),"features":p.feature_ids.iter().map(feature).collect::<Vec<_>>(),"material":p.material.as_ref().map(material)})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"scene":c.scene.as_ref().unwrap().nodes,"skins":c.skins.iter().map(|s|json!({"index":s.index,"joints":s.joints,"matrices":s.inverse_bind_matrices.iter().map(|m|m.enc()).collect::<Vec<_>>()})).collect::<Vec<_>>(),"animations":c.animations.iter().map(|a|json!({"name":a.name,"samplers":a.samplers.iter().map(|s|json!({"input":s.input,"output":s.output.iter().copied().map(math).collect::<Vec<_>>(),"interpolation":s.interpolation.map(|v|v as u8)})).collect::<Vec<_>>(),"channels":a.channels.iter().map(|c|json!({"sampler":c.sampler,"node":c.target.as_ref().and_then(|t|t.node),"path":c.target.as_ref().and_then(|t|t.path).map(|v|v.as_str())})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"transform":c.transform.enc()})
}
#[test]
fn gltf_loader_matches_cesium() {
    run(
        "scene",
        "GltfLoader",
        |_| TIGHT,
        |_, args| {
            let a = &args[0];
            let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport)));
            let resource = Resource::new("https://loader.test/model.gltf").unwrap();
            let context = Context::new_webgl2(Host::default());
            let options = GltfLoaderOptions {
                asynchronous: false,
                incrementally_load_textures: false,
                load_attributes_as_typed_array: a["options"]["loadAttributesAsTypedArray"].as_bool().unwrap(),
                load_for_classification: a["options"]["loadForClassification"].as_bool().unwrap_or(false),
                ..Default::default()
            };
            let loader = GltfLoader::new(
                cache.clone(),
                resource,
                None,
                Some(GltfJsonInput::Document(gltf_pipeline::Gltf::new(a["gltf"].clone()))),
                context,
                Rc::new(Images),
                options,
            );
            let result = (|| {
                ready(loader.load())?;
                for i in 0..30 {
                    let mut frame = frame();
                    frame.frame_number = i;
                    if loader.process(&frame)? {
                        return Ok(snapshot(&loader.components().unwrap()));
                    }
                }
                Err(RuntimeError::new("never ready"))
            })();
            let actual =
                result.unwrap_or_else(|e| json!({"error":e.message.lines().take(2).collect::<Vec<_>>().join("\n")}));
            loader.destroy();
            assert!(cache.is_empty(), "cache leaked: {:?}", cache.entries());
            Some(Out::Ok(actual))
        },
    );
}

fn basic_input(outline: bool) -> Value {
    let suite: Value = serde_json::from_str(include_str!("fixtures/scene/GltfLoader.json")).unwrap();
    suite["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["a"][0]["name"] == if outline { "outline" } else { "basic" })
        .unwrap()["a"][0]["gltf"]
        .clone()
}
fn loader_for(cache: ResourceCache, input: Value, options: GltfLoaderOptions) -> GltfLoader<Host, Images> {
    GltfLoader::new(
        cache,
        Resource::new("https://loader.test/model.gltf").unwrap(),
        None,
        Some(GltfJsonInput::Document(gltf_pipeline::Gltf::new(input))),
        Context::new_webgl2(Host::default()),
        Rc::new(Images),
        options,
    )
}
#[test]
fn load_budget_and_generated_buffer_lifetime() {
    let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport)));
    let loader = loader_for(cache.clone(), basic_input(false), GltfLoaderOptions::default());
    assert!(!loader.process(&frame()).unwrap());
    assert!(ready(loader.load()).unwrap());
    assert!(ready(loader.load()).unwrap());
    let budget = Rc::new(ResourceUploadBudget::new(0, 0));
    let mut frame = frame();
    frame.job_scheduler = Some(FrameResource::new(budget.clone()));
    assert!(!loader.process(&frame).unwrap());
    assert!(!loader.process(&frame).unwrap());
    budget.reset(1, 0);
    assert!(!loader.process(&frame).unwrap());
    budget.reset(1, 0);
    assert!(loader.process(&frame).unwrap());
    assert!(loader.textures_loaded());
    let held = loader.components().unwrap().nodes[0].primitives[0].attributes[0].buffer.clone().unwrap();
    loader.unload();
    assert!(held.is_destroyed());
    assert!(loader.is_unloaded());
    assert!(!loader.process(&frame).unwrap());
    assert!(cache.is_empty());
    let loader =
        loader_for(cache.clone(), basic_input(true), GltfLoaderOptions { asynchronous: false, ..Default::default() });
    assert!(ready(loader.load()).unwrap());
    for _ in 0..10 {
        if loader.process(&frame).unwrap() {
            break;
        }
    }
    let held = loader.components().unwrap().nodes[0].primitives[0]
        .outline_coordinates
        .as_ref()
        .unwrap()
        .buffer
        .clone()
        .unwrap();
    loader.destroy();
    assert!(held.is_destroyed());
    assert!(cache.is_empty());
}
#[test]
fn unload_cancels_pending_json_load() {
    let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport)));
    let loader = loader_for(cache.clone(), basic_input(false), GltfLoaderOptions::default());
    let future = loader.load();
    loader.destroy();
    assert!(!ready(future).unwrap());
    assert!(cache.is_empty());
    assert!(!loader.process(&frame()).unwrap());
}
#[test]
fn failed_geometry_releases_all_leases() {
    let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport)));
    let mut input = basic_input(false);
    input["buffers"][0]["uri"] = json!("missing.bin");
    let loader = loader_for(cache.clone(), input, GltfLoaderOptions::default());
    assert!(ready(loader.load()).unwrap());
    let e = loader.process(&frame()).unwrap_err();
    assert!(e.message.starts_with("Failed to load glTF\n"));
    assert!(loader.components().is_none());
    assert!(cache.is_empty());
}

struct DelayedImages {
    allow: Rc<Cell<bool>>,
    fail: bool,
}
impl ImageHost for DelayedImages {
    type Image = RgbaImage;
    fn dimensions(&self, i: &RgbaImage) -> (u32, u32) {
        (i.width, i.height)
    }
    fn image_from_rgba(&self, i: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(i)
    }
    fn image_pixels(&self, i: &RgbaImage, _: u32, _: u32) -> Result<Vec<u8>, RuntimeError> {
        Ok(i.pixels.clone())
    }
    fn resize_image(&self, i: &RgbaImage, _: u32, _: u32) -> Result<RgbaImage, RuntimeError> {
        Ok(i.clone())
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        Some(Box::pin(std::future::poll_fn(move |_| {
            if !self.allow.get() {
                return Poll::Pending;
            }
            Poll::Ready(if self.fail {
                Err(RuntimeError::new("injected image failure"))
            } else {
                Ok(RgbaImage::new(1, 1, vec![255; 4]).unwrap())
            })
        })))
    }
    fn load_resource_url<'a>(
        &'a self,
        _: &'a str,
        _: bool,
        _: FetchImageOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        self.decode_image(&[], "", ImageDecodeOptions { flip_y: false, skip_color_space_conversion: true })
    }
}
impl ModelImageHost<Host> for DelayedImages {}
#[test]
fn incremental_textures_stream_and_fail_independently() {
    for fail in [false, true] {
        let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport)));
        let allow = Rc::new(Cell::new(false));
        let mut input = basic_input(false);
        input["images"] = json!([{"uri":"x.png"}]);
        input["textures"] = json!([{"source":0}]);
        input["materials"] = json!([{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}]);
        input["meshes"][0]["primitives"][0]["material"] = json!(0);
        let loader = GltfLoader::new(
            cache.clone(),
            Resource::new("https://loader.test/model.gltf").unwrap(),
            None,
            Some(GltfJsonInput::Document(gltf_pipeline::Gltf::new(input))),
            Context::new_webgl2(Host::default()),
            Rc::new(DelayedImages { allow: allow.clone(), fail }),
            GltfLoaderOptions { asynchronous: false, ..Default::default() },
        );
        assert!(ready(loader.load()).unwrap());
        assert!(loader.process(&frame()).unwrap());
        assert!(!loader.textures_loaded());
        allow.set(true);
        if fail {
            let error = loader.process(&frame()).unwrap_err();
            assert!(error.message.starts_with("Failed to load glTF texture\n"));
            assert!(loader.components().is_some());
            assert!(loader.process(&frame()).unwrap());
            assert!(!loader.textures_loaded());
        } else {
            assert!(loader.process(&frame()).unwrap());
            assert!(loader.textures_loaded());
            assert!(loader.components().unwrap().nodes[0].primitives[0]
                .material
                .as_ref()
                .unwrap()
                .metallic_roughness
                .base_color_texture
                .as_ref()
                .unwrap()
                .texture
                .is_some());
        }
        loader.destroy();
        assert!(cache.is_empty());
    }
}

fn tile_attribute<G: Gl>(a: &mc::Attribute<G>) -> Value {
    json!({"name":a.name,"semantic":a.semantic.map(|s|s.as_str()),"setIndex":a.set_index,"type":a.attribute_type.map(|v|v.as_str()),"datatype":a.component_datatype.map(|v|v as u32),"count":a.count,"min":a.min.map(math),"max":a.max.map(math),"constant":a.constant.map(math),"normalized":a.normalized,"array":a.typed_array.as_ref().map(array),"buffer":buffer(&a.buffer),"quantization":a.quantization.as_ref().map(|q|json!({"oct":q.oct_encoded,"zxy":q.oct_encoded_zxy,"range":q.normalization_range.map(math),"offset":q.quantized_volume_offset.map(math),"step":q.quantized_volume_step_size.map(math)}))})
}
fn tile_features<G: Gl>(features: &[mc::FeatureId<G>]) -> Value {
    features.iter().map(|f|json!({"count":f.feature_count(),"table":f.property_table_id(),"set":if let mc::FeatureId::Attribute(a)=f {a.set_index}else{None},"positional":f.positional_label()})).collect()
}
fn tile_snapshot<G: Gl + 'static>(c: &mc::Components<G>) -> Value {
    json!({"transform":c.transform.enc(),"up":c.up_axis.map(|v|v as u32),"forward":c.forward_axis.map(|v|v as u32),"metadata":c.structural_metadata.as_ref().and_then(|r|r.get::<Rc<StructuralMetadata<G>>>()).map(|m|json!({"tables":m.property_table_count(),"attributes":m.property_attributes().len()})),"nodes":c.nodes.iter().map(|n|json!({"instances":n.instances.as_ref().map(|i|json!({"world":i.transform_in_world_space,"attributes":i.attributes.iter().map(tile_attribute).collect::<Vec<_>>(),"features":tile_features(&i.feature_ids)})),"primitives":n.primitives.iter().map(|p|json!({"type":p.primitive_type.map(|v|v as u32),"attributes":p.attributes.iter().map(tile_attribute).collect::<Vec<_>>(),"indices":p.indices.as_ref().map(|i|json!({"count":i.count,"type":i.index_datatype.or_else(||i.buffer.as_ref().and_then(Buffer::index_datatype)).map(|v|v as u32),"bytes":buffer(&i.buffer)})),"features":tile_features(&p.feature_ids),"material":p.material.as_ref().map(|m|json!({"unlit":m.unlit,"alpha":m.alpha_mode.as_str(),"metallic":m.metallic_roughness.metallic_factor,"roughness":m.metallic_roughness.roughness_factor}))})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}

#[test]
fn model_tile_loaders_match_cesium() {
    use cesium_rs::scene::model::*;
    run(
        "scene",
        "ModelTileLoaders",
        |_| Tol { abs: 1e-6, rel: 1e-6, vector: false },
        |_, args| {
            let c = &args[0];
            let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport)));
            let context = Context::new_webgl2(Host::default());
            let bytes = std::sync::Arc::new(
                c["bytes"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_u64().unwrap() as u8).collect())
                    .unwrap_or_default(),
            );
            macro_rules! load {
                ($loader:expr) => {{
                    let loader = $loader;
                    ready(loader.load()).unwrap();
                    let mut done = false;
                    for _ in 0..50 {
                        if loader.process(&frame()).unwrap() {
                            done = true;
                            break;
                        }
                    }
                    assert!(done, "loader did not become ready");
                    let actual = tile_snapshot(&loader.components().unwrap());
                    loader.destroy();
                    assert!(loader.components().is_none());
                    actual
                }};
            }
            let actual = match s(&c["kind"]) {
                "geo" => load!(GeoJsonLoader::new(c["geo"].clone(), context)),
                "pnts" => load!(PntsLoader::new(
                    bytes,
                    None,
                    context,
                    PntsLoaderOptions { load_attributes_for_2d: c["keep"] == true, ..Default::default() }
                )),
                "b3dm" => load!(B3dmLoader::new(
                    cache,
                    Resource::new("https://loader.test/model.b3dm").unwrap(),
                    None,
                    bytes,
                    None,
                    context,
                    Rc::new(Images),
                    B3dmLoaderOptions { asynchronous: false, incrementally_load_textures: false, ..Default::default() }
                )),
                "i3dm" => load!(I3dmLoader::new(
                    cache,
                    Resource::new("https://loader.test/model.i3dm").unwrap(),
                    None,
                    bytes,
                    None,
                    context,
                    Rc::new(Images),
                    I3dmLoaderOptions { asynchronous: false, incrementally_load_textures: false, ..Default::default() }
                )),
                other => panic!("unknown loader {other}"),
            };
            Some(Out::Ok(actual))
        },
    );
}

fn model_tile_bytes(kind: &str, gltf: &Value) -> std::sync::Arc<Vec<u8>> {
    let mut doc = serde_json::to_vec(gltf).unwrap();
    while !doc.len().is_multiple_of(4) {
        doc.push(b' ');
    }
    let mut glb = vec![];
    for word in [0x46546c67, 2, 20 + doc.len() as u32, doc.len() as u32, 0x4e4f534a] {
        glb.extend(word.to_le_bytes());
    }
    glb.extend(doc);
    let header = if kind == "i3dm" { 32 } else { 28 };
    let mut ft = serde_json::to_vec(&if kind == "i3dm" {
        json!({"INSTANCES_LENGTH":1,"POSITION":[1,2,3]})
    } else {
        json!({"BATCH_LENGTH":0})
    })
    .unwrap();
    while !(header + ft.len()).is_multiple_of(8) {
        ft.push(b' ');
    }
    let mut bytes = kind.as_bytes().to_vec();
    for word in [1, (header + ft.len() + glb.len()) as u32, ft.len() as u32, 0, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    if kind == "i3dm" {
        bytes.extend(1u32.to_le_bytes());
    }
    bytes.extend(ft);
    bytes.extend(glb);
    std::sync::Arc::new(bytes)
}

#[test]
fn tile_loaders_keep_streaming_textures_after_geometry_is_ready() {
    use cesium_rs::scene::model::*;
    for kind in ["b3dm", "i3dm"] {
        for fail in [false, true] {
            let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport)));
            let allow = Rc::new(Cell::new(false));
            let mut input = basic_input(false);
            input["images"] = json!([{"uri":"x.png"}]);
            input["textures"] = json!([{"source":0}]);
            input["materials"] = json!([{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}]);
            input["meshes"][0]["primitives"][0]["material"] = json!(0);
            let bytes = model_tile_bytes(kind, &input);
            let host = Rc::new(DelayedImages { allow: allow.clone(), fail });
            let ctx = Context::new_webgl2(Host::default());
            let resource = Resource::new("https://loader.test/tile").unwrap();
            macro_rules! check {
                ($loader:expr) => {{
                    let loader = $loader;
                    assert!(ready(loader.load()).unwrap());
                    assert!(loader.process(&frame()).unwrap());
                    assert!(!loader.textures_loaded());
                    allow.set(true);
                    if fail {
                        assert!(loader.process(&frame()).is_err());
                        assert!(!loader.textures_loaded());
                    } else {
                        assert!(loader.process(&frame()).unwrap());
                        assert!(loader.textures_loaded());
                        assert!(loader.components().unwrap().nodes[0].primitives[0]
                            .material
                            .as_ref()
                            .unwrap()
                            .metallic_roughness
                            .base_color_texture
                            .as_ref()
                            .unwrap()
                            .texture
                            .is_some());
                    }
                    loader.destroy();
                    assert!(cache.is_empty());
                    assert!(!ready(loader.load()).unwrap());
                    assert!(!loader.process(&frame()).unwrap());
                }};
            }
            if kind == "b3dm" {
                check!(B3dmLoader::new(
                    cache.clone(),
                    resource,
                    None,
                    bytes,
                    None,
                    ctx,
                    host,
                    B3dmLoaderOptions { asynchronous: false, ..Default::default() }
                ));
            } else {
                check!(I3dmLoader::new(
                    cache.clone(),
                    resource,
                    None,
                    bytes,
                    None,
                    ctx,
                    host,
                    I3dmLoaderOptions { asynchronous: false, ..Default::default() }
                ));
            }
        }
    }
}

#[test]
fn geojson_failure_and_destroy_do_not_recreate_components() {
    use cesium_rs::scene::model::GeoJsonLoader;
    let loader =
        GeoJsonLoader::new(json!({"type":"FeatureCollection","features":[]}), Context::new_webgl2(Host::default()));
    assert!(loader.process(&frame()).is_err());
    assert_eq!(loader.state(), ResourceLoaderState::Failed);
    assert!(!loader.process(&frame()).unwrap());
    let loader = GeoJsonLoader::new(
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{}}),
        Context::new_webgl2(Host::default()),
    );
    loader.destroy();
    assert!(!loader.process(&frame()).unwrap());
    assert!(loader.components().is_none());
}

#[test]
fn model_statistics_deduplicate_shared_geometry_and_track_batch_texture_growth() {
    use cesium_rs::scene::model::{
        model_statistics::BatchTextureStatistics, node_statistics_pipeline_stage as ns,
        primitive_statistics_pipeline_stage as ps, *,
    };
    let ctx = Context::new_webgl2(Host::default());
    let b = Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(36)).unwrap();
    let p = mc::Primitive {
        primitive_type: Some(PrimitiveType::Triangles),
        attributes: vec![mc::Attribute {
            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
            count: Some(6),
            buffer: Some(b.clone()),
            typed_array: Some(TypedArray::Float32(vec![0.; 9])),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut stats = ModelStatistics::new();
    ps::count_geometry(&mut stats, &p);
    ps::count_geometry(&mut stats, &p);
    assert_eq!((stats.triangles_length, stats.geometry_byte_length), (4, 72));
    let node = mc::Node {
        instances: Some(mc::Instances { attributes: p.attributes.clone(), ..Default::default() }),
        ..Default::default()
    };
    ns::process(
        &mut stats,
        &node,
        &ns::NodeRuntimeBuffers {
            instancing_transforms_buffer: Some(b.clone()),
            instancing_transforms_buffer_2d: None,
            instancing_translation_buffer_2d: None,
        },
    );
    assert_eq!(stats.geometry_byte_length, 72);
    let batch = BatchTextureStatistics::new("batch", 16);
    stats.add_batch_texture(&batch);
    stats.add_batch_texture(&batch);
    batch.byte_length.set(32);
    assert_eq!(stats.textures_byte_length(), 32);
    let texture = ctx.default_texture().unwrap();
    stats.add_texture(&texture);
    stats.add_texture(&texture);
    assert_eq!(stats.get_texture_ids(), &[texture.id().to_string()]);
    assert_eq!(stats.get_texture_byte_length_by_id(texture.id()), Some(texture.size_in_bytes()));
    assert_eq!(stats.textures_byte_length(), 32 + texture.size_in_bytes());
    stats.clear();
    assert_eq!((stats.geometry_byte_length, stats.textures_byte_length()), (0, 0));
    assert!(stats.get_texture_ids().is_empty());
    let clone = b.clone();
    assert_eq!(b.id(), clone.id());
    drop(clone);
    drop(b);
    let other = Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(36)).unwrap();
    assert_ne!(p.attributes[0].buffer.as_ref().unwrap().id(), other.id());
}

#[test]
fn wireframe_outline_and_pick_resources_work_with_real_buffer_storage() {
    use cesium_rs::scene::model::{picking_pipeline_stage::ModelPickObject, *};
    let ctx = Context::new_webgl2(Host::default());
    for webgl2 in [false, true] {
        let context = if webgl2 { ctx.clone() } else { Context::new_webgl1(Host::default()) };
        let indices = TypedArray::Uint16(vec![0, 1, 2]);
        let bytes = indices.to_le_bytes();
        let ib = Buffer::new(
            &context,
            BufferKind::Index(IndexDatatype::UnsignedShort),
            BufferUsage::StaticDraw,
            BufferData::Bytes(&bytes),
        )
        .unwrap();
        let primitive = mc::Primitive {
            primitive_type: Some(PrimitiveType::Triangles),
            attributes: vec![mc::Attribute {
                semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                count: Some(3),
                min: Some(mc::MathValue::Cartesian3(Cartesian3::ZERO)),
                max: Some(mc::MathValue::Cartesian3(Cartesian3::ONE)),
                ..Default::default()
            }],
            indices: Some(mc::Indices {
                buffer: Some(ib),
                index_datatype: Some(IndexDatatype::UnsignedShort),
                count: Some(3),
                typed_array: Some(indices),
            }),
            ..Default::default()
        };
        let m = ModelRenderResources::new(Rc::new(RefCell::new(ModelUniformState::default())));
        let n = NodeRenderResources::new(&m, 0);
        let mut p = PrimitiveRenderResources::new(&n, 0, &primitive, None, None).unwrap();
        let mut stats = ModelStatistics::new();
        wireframe_pipeline_stage::process(&mut p, &primitive, &context, &mut stats).unwrap();
        assert!(p.wireframe_index_buffer.is_some());
        assert_eq!(
            context.host().buffers.borrow()[&context.host().next.get()],
            TypedArray::Uint16(vec![0, 1, 1, 2, 2, 0]).to_le_bytes()
        );
        assert_eq!(p.count, Some(6));
        assert_eq!(p.primitive_type, Some(PrimitiveType::Lines));
        assert_eq!(stats.geometry_byte_length, 12);
        let node = mc::Node::default();
        picking_pipeline_stage::process(&mut p, &node, &primitive, &context, &mut stats, false, &[]).unwrap();
        assert_eq!(p.pick_id.as_deref(), Some("czm_pickColor"));
        let id = p.pick_ids[0].color.to_rgba();
        assert_eq!(context.pick_object::<ModelPickObject>(id).unwrap().instance_id, None);
        drop(p);
        assert!(context.pick_object::<ModelPickObject>(id).is_none());
    }
    let coords = Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(36)).unwrap();
    let primitive = mc::Primitive {
        attributes: vec![mc::Attribute {
            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
            count: Some(3),
            min: Some(mc::MathValue::Cartesian3(Cartesian3::ZERO)),
            max: Some(mc::MathValue::Cartesian3(Cartesian3::ONE)),
            ..Default::default()
        }],
        outline_coordinates: Some(mc::Attribute {
            buffer: Some(coords),
            attribute_type: Some(AttributeType::Vec3),
            ..Default::default()
        }),
        ..Default::default()
    };
    let model = ModelRenderResources::new(Rc::new(RefCell::new(ModelUniformState::default())));
    let mut node_resources = NodeRenderResources::new(&model, 0);
    node_resources.has_instances = true;
    node_resources.instance_count = 2;
    let mut p = PrimitiveRenderResources::new(&node_resources, 0, &primitive, None, None).unwrap();
    primitive_outline_pipeline_stage::process(&mut p, &primitive, &ctx).unwrap();
    assert!(p.texture_uniform_map["model_outlineTexture"]().is_some());
    let node = mc::Node { instances: Some(mc::Instances::default()), ..Default::default() };
    picking_pipeline_stage::process(&mut p, &node, &primitive, &ctx, &mut ModelStatistics::new(), true, &[]).unwrap();
    assert_eq!(p.pick_ids.len(), 2);
    for (i, pick) in p.pick_ids.iter().enumerate() {
        assert_eq!(ctx.pick_object::<ModelPickObject>(pick.color.to_rgba()).unwrap().instance_id, Some(i));
    }
    let attr = p.attributes.last().unwrap();
    assert_eq!(attr.instance_divisor, 1);
    let AttributeSource::Buffer(buffer) = &attr.source else { panic!() };
    assert_eq!(buffer.size_in_bytes(), 32);
}

#[test]
fn clipping_texture_providers_follow_updates_and_use_defaults() {
    use cesium_rs::scene::model::{model_clipping_polygons_pipeline_stage as stage, *};
    let ctx = Context::new_webgl2(Host::default());
    let fallback = ctx.default_texture().unwrap();
    let clipping = Rc::new(RefCell::new(None));
    let mut model = ModelRenderResources::new(Rc::new(RefCell::new(ModelUniformState::default())));
    stage::process(
        &mut model,
        &stage::ModelClippingPolygonsState {
            inverse: false,
            compute_polygon_bounds_length: 1,
            clipping_texture: clipping.clone(),
            extents_texture: Rc::new(RefCell::new(None)),
            default_texture: Some(fallback.clone()),
        },
    );
    let node = NodeRenderResources::new(&model, 0);
    let provider = &node.texture_uniform_map["model_clippingDistance"];
    assert_eq!(provider().unwrap().id(), fallback.id());
    let distance = ctx.default_normal_texture().unwrap();
    *clipping.borrow_mut() = Some(distance.clone());
    assert_eq!(provider().unwrap().id(), distance.id());
    *clipping.borrow_mut() = None;
    assert_eq!(provider().unwrap().id(), fallback.id());
}
