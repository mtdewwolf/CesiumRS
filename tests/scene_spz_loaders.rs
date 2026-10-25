//! SPZ decoding, GltfSpzLoader, ResourceCache.getSpzLoader and SPZ vertex
//! attributes against the pinned CesiumJS loaders and `@spz-loader/core` WASM.
mod common;
use cesium_rs::scene::gltf_pipeline::BufferSource;
use cesium_rs::{core::*, renderer::*, scene::*};
use common::*;
use serde_json::{json, Map, Value};
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
/// External buffers are unreachable.
struct Unreachable;
impl ResourceTransport for Unreachable {
    fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
        Box::pin(std::future::ready(Err(RequestErrorEvent::new(Some(404), None, None))))
    }
}
fn cache() -> ResourceCache {
    ResourceCache::new(ResourceClient::new(Rc::new(Unreachable)))
}
fn ready<T>(future: impl Future<Output = T>) -> T {
    match std::pin::pin!(future).poll(&mut TaskContext::from_waker(Waker::noop())) {
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
fn bytes(v: &Value) -> Vec<u8> {
    v.as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect()
}
/// The glTF of a case; embedded buffers are supplied unless the buffer has a URI.
fn request(c: &Value) -> GltfResource {
    let sources = if c["gltf"]["buffers"][0]["uri"].is_string() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(0, BufferSource::new(bytes(&c["raw"])))])
    };
    let resource = Resource::new("https://spz.test/splats/model.gltf").unwrap();
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
fn seed(cache: &ResourceCache, request: &GltfResource) -> Option<CacheLease<BufferLoader>> {
    let bytes = request.document.embedded_buffers.get(&0).cloned()?;
    Some(cache.embedded_buffer_loader(&request.resource, 0, Some(bytes)).unwrap())
}
fn entries(cache: &ResourceCache) -> Value {
    cache.entries().into_iter().map(|(key, count)| (key, json!(count))).collect::<Map<_, _>>().into()
}
fn floats(values: &[f32]) -> Value {
    values.iter().map(|&v| num(f64::from(v))).collect()
}
fn cloud(c: &GaussianCloud) -> Value {
    json!({"numPoints":c.num_points,"shDegree":c.sh_degree,"antialiased":c.antialiased,"positions":floats(&c.positions),"scales":floats(&c.scales),"rotations":floats(&c.rotations),"alphas":floats(&c.alphas),"colors":floats(&c.colors),"sh":floats(&c.sh)})
}
/// The first line of a loader error and its number of lines; the innermost
/// reason is the decoder's own message rather than the binding's abort.
fn failure(e: &RuntimeError) -> Value {
    json!({"message":e.message.lines().next().unwrap_or(""),"lines":e.message.lines().count()})
}
fn step(result: Result<bool, RuntimeError>) -> Value {
    result.map_or_else(|e| failure(&e), Value::Bool)
}
fn spz_extension() -> (Value, Value) {
    let primitive = json!({"attributes":{"POSITION":0},"extensions":{"KHR_gaussian_splatting":{"extensions":{"KHR_gaussian_splatting_compression_spz_2":{}}}}});
    let extension = primitive["extensions"]["KHR_gaussian_splatting"]["extensions"]
        ["KHR_gaussian_splatting_compression_spz_2"]
        .clone();
    (primitive, extension)
}
fn vertex_loader(
    cache: &ResourceCache,
    request: &GltfResource,
    semantic: &str,
    options: GpuBufferOptions,
    context: Option<Context<Host>>,
) -> CacheLease<GltfVertexBufferLoader<Host>> {
    let (primitive, extension) = spz_extension();
    let source = VertexBufferSource::Spz { primitive, extension, semantic: semantic.into() };
    cache.vertex_buffer_loader(request, source, context, options).unwrap()
}

fn decode(c: &Value) -> Value {
    match load_spz(&bytes(&c["bytes"])) {
        Ok(c) => cloud(&c),
        // The C++ decoder prints its reasons; gzip failures and empty clouds print nothing.
        Err(e) => {
            let logged = if e.message.starts_with("deserializePackedGaussians:") { vec![e.message] } else { vec![] };
            json!({"error":"divide by zero","logged":logged})
        }
    }
}

fn spz_lifecycle(c: &Value) -> Value {
    let cache = cache();
    let request = request(c);
    let seed = seed(&cache, &request);
    let loader = cache.spz_loader(&request).unwrap();
    let again = cache.spz_loader(&request).unwrap();
    let mut frame = frame();
    let state = |l: &GltfSpzLoader| l.state() as i32;
    let mut r = json!({"key":loader.cache_key(),"shared":loader.ptr_eq(&again),"initial":state(&loader),"entriesInitial":entries(&cache)});
    ready(loader.load()).unwrap();
    r["loaded"] = json!(state(&loader));
    r["entriesLoaded"] = entries(&cache);
    // Upstream returns undefined from the call that starts decoding.
    r["first"] = match loader.process(&frame) {
        Ok(false) => Value::Null,
        other => step(other),
    };
    r["processing"] = json!(state(&loader));
    // Upstream's decode settles before the next frame.
    frame.frame_number += 1;
    r["second"] = step(loader.process(&frame));
    r["after"] = json!(state(&loader));
    r["entriesAfter"] = entries(&cache);
    r["cloud"] = loader.decoded_data().map_or(Value::Null, |c| cloud(&c));
    r["third"] = step(loader.process(&frame));
    drop(again);
    r["entriesShared"] = entries(&cache);
    let observed = loader.loader();
    drop(loader);
    r["destroyed"] = json!(observed.is_destroyed());
    drop(seed);
    r["remaining"] = json!(cache.len());
    r
}

fn vertex_lifecycle(c: &Value) -> Value {
    let cache = cache();
    let request = request(c);
    let seed = seed(&cache, &request);
    let mut frame = frame();
    let budget = Rc::new(BufferUploadBudget::new(0));
    frame.job_scheduler = Some(FrameResource::new(budget.clone()));
    let options = GpuBufferOptions {
        asynchronous: true,
        load_buffer: c["upload"] == true,
        load_typed_array: c["retain"] == true,
    };
    let context = options.load_buffer.then(|| Context::new_webgl2(Host::default()));
    let id = context.as_ref().map(|c| c.id().to_string());
    let loader = vertex_loader(&cache, &request, s(&c["semantic"]), options, context);
    let snapshot = |cache: &ResourceCache| json!({"state":loader.state() as i32,"cpu":loader.typed_array().map(|v| json!(v.bytes())),"gpu":loader.buffer().map(|b| b.size_in_bytes()),"geometry":cache.statistics().borrow().geometry_byte_length,"entries":entries(cache)});
    let mut r = json!({"key":loader.cache_key(),"initial":snapshot(&cache)});
    ready(loader.load()).unwrap();
    r["loaded"] = snapshot(&cache);
    r["first"] = step(loader.process(&frame));
    frame.frame_number += 1;
    r["second"] = step(loader.process(&frame));
    r["deferred"] = snapshot(&cache);
    budget.reset(1);
    r["ready"] = step(loader.process(&frame));
    r["after"] = snapshot(&cache);
    let buffer = loader.buffer();
    let observed = loader.loader();
    drop(loader);
    drop(seed);
    r["destroyed"] = json!(observed.is_destroyed());
    r["gpuDestroyed"] = buffer.map_or(Value::Null, |b| json!(b.is_destroyed()));
    r["remaining"] = json!(cache.len());
    r["geometryAfter"] = json!(cache.statistics().borrow().geometry_byte_length);
    // The fixture's stand-in context is named "gpu".
    match id {
        Some(id) => serde_json::from_str(&r.to_string().replace(&id, "gpu")).unwrap(),
        None => r,
    }
}

fn vertex_failure(c: &Value) -> Value {
    let cache = cache();
    let request = request(c);
    let seed = seed(&cache, &request);
    let mut frame = frame();
    let options = GpuBufferOptions { load_typed_array: true, ..Default::default() };
    let loaders = ["POSITION", "COLOR_0"].map(|semantic| vertex_loader(&cache, &request, semantic, options, None));
    let mut r = json!({"entries":entries(&cache)});
    r["load"] = loaders.iter().map(|l| json!(ready(l.load()).unwrap())).collect();
    r["loaded"] = loaders.iter().map(|l| json!(l.state() as i32)).collect();
    let spz = cache.spz_loader(&request).unwrap();
    r["spzState"] = json!(spz.state() as i32);
    drop(spz);
    r["first"] = loaders.iter().map(|l| step(l.process(&frame))).collect();
    frame.frame_number += 1;
    r["second"] = loaders.iter().map(|l| step(l.process(&frame))).collect();
    r["third"] = loaders.iter().map(|l| step(l.process(&frame))).collect();
    r["states"] = loaders.iter().map(|l| json!(l.state() as i32)).collect();
    r["entriesFailed"] = entries(&cache);
    drop(loaders);
    drop(seed);
    r["remaining"] = json!(cache.len());
    r
}

#[test]
fn spz_decoding_and_loader_match_cesium() {
    run(
        "scene/spz",
        "GltfSpzLoader",
        |_| EXACT,
        |f, a| match f {
            "decode" => Some(Out::Ok(decode(&a[0]))),
            "lifecycle" => Some(Out::Ok(spz_lifecycle(&a[0]))),
            _ => None,
        },
    );
}

#[test]
fn spz_vertex_attributes_match_cesium() {
    run(
        "scene/spz",
        "GltfVertexBufferLoader",
        |_| EXACT,
        |f, a| match f {
            "spz" => Some(Out::Ok(vertex_lifecycle(&a[0]))),
            "spz-failure" => Some(Out::Ok(vertex_failure(&a[0]))),
            _ => None,
        },
    );
}

#[test]
fn spz_cache_sharing_matches_cesium() {
    run(
        "scene/spz",
        "ResourceCache",
        |_| EXACT,
        |f, a| {
            (f == "spz-sharing").then(|| {
                let c = &a[0];
                let cache = cache();
                let request = request(c);
                let seed = seed(&cache, &request);
                let mut frame = frame();
                let options = GpuBufferOptions { load_typed_array: true, ..Default::default() };
                let mut trace = vec![entries(&cache)];
                let position = vertex_loader(&cache, &request, "POSITION", options, None);
                let color = vertex_loader(&cache, &request, "COLOR_0", options, None);
                let position_again = vertex_loader(&cache, &request, "POSITION", options, None);
                trace.push(entries(&cache));
                ready(position.load()).unwrap();
                ready(color.load()).unwrap();
                trace.push(entries(&cache));
                position.process(&frame).unwrap();
                frame.frame_number += 1;
                trace.push(json!([position.process(&frame).unwrap(), color.process(&frame).unwrap()]));
                trace.push(entries(&cache));
                let spz = cache.spz_loader(&request).unwrap();
                trace.push(entries(&cache));
                drop(spz);
                drop(position_again);
                drop(position);
                trace.push(entries(&cache));
                drop(color);
                trace.push(entries(&cache));
                drop(seed);
                trace.push(entries(&cache));
                Out::Ok(json!(trace))
            })
        },
    );
}
