mod common;
use cesium_rs::scene::gltf_pipeline::BufferSource;
use cesium_rs::{core::*, renderer::*, scene::*};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::Cell,
    collections::BTreeMap,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context as TaskContext, Poll, Waker},
};

#[derive(Clone, Default)]
struct Host(Rc<Cell<u32>>);
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            self.0.set(self.0.get() + 1);
            return Ok(GlValue::Object(self.0.get()));
        }
        Ok(GlValue::Null)
    }
}
struct NoTransport;
impl ResourceTransport for NoTransport {
    fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
        panic!("unexpected fetch")
    }
}
fn cache() -> ResourceCache {
    ResourceCache::new(ResourceClient::new(Rc::new(NoTransport)))
}
fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut f = std::pin::pin!(future);
    match f.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("unexpected pending request"),
    }
}
fn frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(8., 8.));
    let snapshot = camera.uniform_camera();
    let culling = camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
    FrameState::new(UniformFrame::new(snapshot, JulianDate::new(2451545., 0., TimeStandard::Tai)), culling, [8, 8])
}
fn request(c: &Value) -> GltfResource {
    let raw = BufferSource::new(c["raw"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect());
    let sources = BTreeMap::from([(0, raw)]);
    let resource = Resource::new("https://geometry.test/model.gltf").unwrap();
    GltfResource {
        document: Arc::new(LoadedGltfJson {
            json: c["gltf"].clone(),
            embedded_buffers: sources.clone(),
            buffer_sources: sources,
        }),
        base: resource.clone(),
        resource,
    }
}
enum Loader {
    Draco(CacheLease<GltfDracoLoader>),
    Vertex(CacheLease<GltfVertexBufferLoader<Host>>),
    Index(CacheLease<GltfIndexBufferLoader<Host>>),
}
impl Loader {
    fn resource(&self) -> &dyn ResourceLoader {
        match self {
            Self::Draco(l) => &**l,
            Self::Vertex(l) => &**l,
            Self::Index(l) => &**l,
        }
    }
    fn cpu(&self) -> Value {
        match self {
            Self::Draco(_) => Value::Null,
            Self::Vertex(l) => l.typed_array().map(|v| json!(v.bytes())).unwrap_or(Value::Null),
            Self::Index(l) => l.typed_array().map(|v| json!(v.to_f64_vec())).unwrap_or(Value::Null),
        }
    }
    fn buffer(&self) -> Option<Buffer<Host>> {
        match self {
            Self::Draco(_) => None,
            Self::Vertex(l) => l.buffer(),
            Self::Index(l) => l.buffer(),
        }
    }
    fn snapshot(&self, cache: &ResourceCache) -> Value {
        json!({"state":self.resource().state() as u32,"cpu":self.cpu(),"gpu":self.buffer().map(|b|b.size_in_bytes()),"geometry":cache.statistics().borrow().geometry_byte_length})
    }
}
fn value(v: Option<MetadataValue>) -> Value {
    match v {
        None => Value::Null,
        Some(MetadataValue::Number(v)) => json!(v),
        Some(MetadataValue::Cartesian2(v)) => json!([v.x, v.y]),
        Some(MetadataValue::Cartesian3(v)) => json!([v.x, v.y, v.z]),
        Some(MetadataValue::Cartesian4(v)) => json!([v.x, v.y, v.z, v.w]),
        v => panic!("unexpected quantization {v:?}"),
    }
}
fn quantization(q: ModelQuantization) -> Value {
    let mut out = json!({"octEncoded":q.oct_encoded,"octEncodedZXY":q.oct_encoded_zxy,"normalizationRange":value(q.normalization_range),"componentDatatype":q.component_datatype as u32,"type":q.attribute_type.as_str()});
    for (key, v) in [
        ("quantizedVolumeOffset", q.quantized_volume_offset),
        ("quantizedVolumeDimensions", q.quantized_volume_dimensions),
        ("quantizedVolumeStepSize", q.quantized_volume_step_size),
    ] {
        out[key] = value(v);
    }
    out
}
fn lifecycle(c: &Value) -> Value {
    let cache = cache();
    let request = request(c);
    let mut frame = frame();
    let seed =
        cache.embedded_buffer_loader(&request.resource, 0, request.document.embedded_buffers.get(&0).cloned()).unwrap();
    let budget = Rc::new(BufferUploadBudget::new(0));
    frame.job_scheduler = Some(FrameResource::new(budget.clone()));
    let options = GpuBufferOptions {
        asynchronous: true,
        load_buffer: c["upload"] == true,
        load_typed_array: c["retain"] == true,
    };
    let context = Some(Context::new_webgl2(Host::default()));
    let compressed = c["mode"] == "compressed";
    let loader = match c["kind"].as_str().unwrap() {
        "draco" => Loader::Draco(
            cache
                .draco_loader(request.document.clone(), &c["primitive"], &c["draco"], &request.resource, &request.base)
                .unwrap(),
        ),
        "vertex" => Loader::Vertex(
            cache
                .vertex_buffer_loader(
                    &request,
                    if compressed {
                        VertexBufferSource::Draco {
                            primitive: c["primitive"].clone(),
                            extension: c["draco"].clone(),
                            semantic: c["semantic"].as_str().unwrap().into(),
                            accessor_id: c["accessorId"].as_u64().unwrap() as usize,
                        }
                    } else {
                        VertexBufferSource::BufferView(0)
                    },
                    context,
                    options,
                )
                .unwrap(),
        ),
        "index" => Loader::Index(
            cache
                .index_buffer_loader(
                    &request,
                    0,
                    compressed.then_some((&c["primitive"], &c["draco"])),
                    context,
                    options,
                )
                .unwrap(),
        ),
        _ => unreachable!(),
    };
    let initial = loader.snapshot(&cache);
    ready(loader.resource().load()).unwrap();
    let loaded = loader.snapshot(&cache);
    let mut first = loader.resource().process(&frame).unwrap();
    if compressed {
        first = loader.resource().process(&frame).unwrap();
    }
    let deferred = loader.snapshot(&cache);
    budget.reset(1);
    let is_ready = loader.resource().process(&frame).unwrap();
    let after = loader.snapshot(&cache);
    let decoded = match &loader {
        Loader::Draco(l) => l.decoded_data().map(|v| (*v).clone()),
        _ => None,
    };
    let quantization = match &loader {
        Loader::Vertex(l) => l.quantization().map(quantization),
        _ => None,
    };
    let buffer = loader.buffer();
    let observed = match &loader {
        Loader::Draco(l) => Rc::clone(&l.loader()) as Rc<dyn ResourceLoader>,
        Loader::Vertex(l) => l.loader() as Rc<dyn ResourceLoader>,
        Loader::Index(l) => l.loader() as Rc<dyn ResourceLoader>,
    };
    drop(loader);
    drop(seed);
    json!({"initial":initial,"loaded":loaded,"first":first,"deferred":deferred,"ready":is_ready,"after":after,"decoded":decoded,"quantization":quantization,"destroyed":observed.is_destroyed(),"gpuDestroyed":buffer.map(|b|b.is_destroyed()),"remaining":cache.len(),"geometry":cache.statistics().borrow().geometry_byte_length})
}
#[test]
fn draco_loader_matches_cesium() {
    run("scene", "GltfDracoLoader", |_| TIGHT, |_, a| Some(Out::Ok(lifecycle(&a[0]))));
}
#[test]
fn vertex_loader_matches_cesium() {
    run("scene", "GltfVertexBufferLoader", |_| TIGHT, |_, a| Some(Out::Ok(lifecycle(&a[0]))));
}
#[test]
fn index_loader_matches_cesium() {
    run("scene", "GltfIndexBufferLoader", |_| TIGHT, |_, a| Some(Out::Ok(lifecycle(&a[0]))));
}

#[derive(Default)]
struct PendingWorker {
    started: Cell<usize>,
    cancelled: Cell<usize>,
}
impl TaskWorker<Value, Value> for PendingWorker {
    fn run(&self, _: u64, _: Value) -> TaskFuture<Value> {
        self.started.set(self.started.get() + 1);
        Box::pin(std::future::pending())
    }
    fn init_web_assembly_module(&self, _: WebAssemblyOptions) -> TaskFuture<Value> {
        Box::pin(async { Ok(json!(true)) })
    }
    fn cancel_task(&self, _: u64) {
        self.cancelled.set(self.cancelled.get() + 1);
    }
    fn terminate(&self) {}
}
fn draco_case() -> Value {
    serde_json::from_str::<Value>(include_str!("fixtures/scene/GltfDracoLoader.json")).unwrap()["cases"][0]["a"][0]
        .clone()
}
#[test]
fn capacity_defers_decode_and_last_lease_cancels_pending_work() {
    let cache = cache();
    let worker = Rc::new(PendingWorker::default());
    cache.set_draco_decoder(TaskProcessor::new(worker.clone(), Some(1))).unwrap();
    let c = draco_case();
    let a = request(&c);
    let mut b = a.clone();
    b.resource = Resource::new("https://geometry.test/second.gltf").unwrap();
    let first = cache.draco_loader(a.document.clone(), &c["primitive"], &c["draco"], &a.resource, &a.base).unwrap();
    let second = cache.draco_loader(b.document.clone(), &c["primitive"], &c["draco"], &b.resource, &b.base).unwrap();
    ready(first.load()).unwrap();
    ready(second.load()).unwrap();
    let frame = frame();
    assert!(!first.process(&frame).unwrap());
    assert!(!first.process(&frame).unwrap());
    assert!(!second.process(&frame).unwrap());
    assert_eq!(worker.started.get(), 1);
    assert!(cache.set_draco_decoder(TaskProcessor::new(worker.clone(), Some(2))).is_err());
    let observed = first.loader();
    drop(first);
    assert!(observed.is_destroyed());
    assert_eq!(worker.cancelled.get(), 1);
    assert!(!second.process(&frame).unwrap());
    assert_eq!(worker.started.get(), 2);
    drop(second);
    assert_eq!(worker.cancelled.get(), 2);
    assert!(cache.is_empty());
}
#[test]
fn invalid_draco_failure_releases_dependencies_and_propagates_context() {
    let cache = cache();
    let mut c = draco_case();
    c["raw"] = json!([0, 1, 2, 3]);
    c["gltf"]["bufferViews"][0]["byteLength"] = json!(4);
    let request = request(&c);
    let loader = cache
        .index_buffer_loader::<Host>(
            &request,
            0,
            Some((&c["primitive"], &c["draco"])),
            None,
            GpuBufferOptions { load_typed_array: true, ..Default::default() },
        )
        .unwrap();
    ready(loader.load()).unwrap();
    let frame = frame();
    assert!(!loader.process(&frame).unwrap());
    let error = loader.process(&frame).unwrap_err();
    assert!(error.message.starts_with("Failed to load index buffer\nFailed to load Draco\n"));
    assert_eq!(loader.state(), ResourceLoaderState::Failed);
    assert_eq!(cache.len(), 1);
    assert!(loader.typed_array().is_none());
    drop(loader);
    assert!(cache.is_empty());
    assert_eq!(cache.statistics().borrow().geometry_byte_length, 0);
}
#[test]
fn destroying_cache_during_load_resolves_request_without_retaining_pool() {
    let cache = cache();
    let c = draco_case();
    let request = request(&c);
    let loader = cache
        .vertex_buffer_loader::<Host>(
            &request,
            VertexBufferSource::BufferView(0),
            None,
            GpuBufferOptions { load_typed_array: true, ..Default::default() },
        )
        .unwrap();
    let future = loader.load();
    let pool = cache.downgrade();
    drop(cache);
    assert!(pool.upgrade().is_none());
    assert!(loader.is_destroyed());
    assert!(!ready(future).unwrap());
}

#[test]
fn json_cache_registers_embedded_buffers_and_views_outlive_json_lease() {
    let cache = cache();
    let resource = Resource::new("https://geometry.test/embedded.gltf").unwrap();
    let input = GltfJsonInput::Document(cesium_rs::scene::gltf_pipeline::Gltf::new(
        json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"data:application/octet-stream;base64,AQIDBA=="}],"bufferViews":[{"buffer":0,"byteOffset":1,"byteLength":2}]}),
    ));
    let json = cache.gltf_json_loader(resource.clone(), resource.clone(), Some(input)).unwrap();
    ready(json.load()).unwrap();
    let document = json.gltf().unwrap();
    let embedded_key = ResourceCacheKey::embedded_buffer(&resource, 0);
    assert_eq!(cache.reference_count(&embedded_key), Some(1));
    let view = cache.buffer_view_loader(document, 0, &resource, &resource).unwrap();
    assert_eq!(cache.reference_count(&embedded_key), Some(2));
    drop(json);
    assert_eq!(cache.reference_count(&embedded_key), Some(1));
    ready(view.load()).unwrap();
    assert_eq!(view.typed_array().unwrap().bytes(), [2, 3]);
    assert_eq!(cache.len(), 1);
    drop(view);
    assert!(cache.is_empty());
}

#[test]
fn legacy_animation_rewrites_are_published_to_cached_external_buffers() {
    let cache = cache();
    let resource = Resource::new("https://geometry.test/old.gltf").unwrap();
    let external = Resource::new("https://geometry.test/animation.bin").unwrap();
    let bytes =
        [0f32, 0., 1., std::f32::consts::FRAC_PI_2, 0.].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    let key = ResourceCacheKey::external_buffer(&external);
    let seed = cache
        .add(
            BufferLoader::new(Some(BufferSource::new(bytes.clone())), None, None, Some(key.clone())).unwrap(),
            CacheResourceKind::Buffer,
        )
        .unwrap();
    let input = json!({"version":"0.8","asset":{},"buffers":{"b":{"uri":"animation.bin","byteLength":20}},"bufferViews":{"v":{"buffer":"b","byteOffset":0,"byteLength":20}},"accessors":{"r":{"bufferView":"v","byteOffset":0,"componentType":5126,"type":"VEC4","count":1},"t":{"bufferView":"v","byteOffset":16,"componentType":5126,"type":"SCALAR","count":1}},"nodes":{"n":{}},"animations":{"a":{"channels":[{"sampler":"s","target":{"id":"n","path":"rotation"}}],"parameters":{"TIME":"t","rotation":"r"},"samplers":{"s":{"input":"TIME","output":"rotation"}}}}});
    let loader = cache
        .gltf_json_loader(
            resource.clone(),
            resource.clone(),
            Some(GltfJsonInput::Document(cesium_rs::scene::gltf_pipeline::Gltf::new(input))),
        )
        .unwrap();
    ready(loader.load()).unwrap();
    let document = loader.gltf().unwrap();
    let source = document.buffer_sources.get(&0).unwrap();
    assert_ne!(source.bytes(), bytes);
    assert_eq!(seed.typed_array().unwrap().bytes(), source.bytes());
    let view = cache.buffer_view_loader(document.clone(), 0, &resource, &resource).unwrap();
    ready(view.load()).unwrap();
    assert_eq!(view.typed_array().unwrap().bytes(), &source.bytes()[..16]);
    drop(view);
    drop(seed);
    drop(loader);
    assert!(cache.is_empty());
}
