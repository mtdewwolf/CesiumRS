//! DracoLoader and GltfDracoLoader against CesiumJS with the real decodeDraco
//! worker (`tools/cesium/fixtures/scene-draco-loaders.mjs`). The worker here
//! runs `workers::decode_draco` in-process, as the fixture's stubbed transport does.
mod common;
use cesium_rs::core::task_processor::{TaskFuture, TaskWorker, WebAssemblyOptions};
use cesium_rs::core::*;
use cesium_rs::scene::gltf_json_loader::LoadedGltfJson;
use cesium_rs::scene::gltf_pipeline::BufferSource;
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};

const URL: &str = "https://draco.test/model.gltf";

/// Records what DracoLoader sends, as the fixture's TaskProcessor stub does.
struct Worker {
    init: String,
    scheduled: RefCell<Vec<Value>>,
}

impl TaskWorker<Value, Value> for Worker {
    fn run(&self, _: u64, parameters: Value) -> TaskFuture<Value> {
        let mut keys: Vec<&String> = parameters.as_object().unwrap().keys().collect();
        keys.sort();
        let bytes =
            parameters.get("array").or_else(|| parameters.get("buffer")).and_then(Value::as_array).map_or(0, Vec::len);
        let field = |k: &str| parameters.get(k).cloned().unwrap_or(Value::Null);
        self.scheduled.borrow_mut().push(json!({
            "task": keys,
            // The parameters, and their buffer, move to the worker: one transfer.
            "transfers": 1,
            "bytes": bytes,
            "bufferView": field("bufferView"),
            "compressedAttributes": field("compressedAttributes"),
            "dequantizeInShader": field("dequantizeInShader"),
            "attributesToSkipTransform": field("attributesToSkipTransform"),
        }));
        let result = cesium_rs::workers::decode_draco(&parameters);
        Box::pin(std::future::ready(result))
    }
    fn init_web_assembly_module(&self, options: WebAssemblyOptions) -> TaskFuture<Value> {
        self.scheduled
            .borrow_mut()
            .push(json!({"init": {"name": DracoLoader::WORKER_NAME, "wasmBinaryFile": options.wasm_binary_file}}));
        let result = match self.init.as_str() {
            "reject" => Err(RuntimeError::new("Worker failed to start")),
            other => Ok(Value::Bool(other == "true")),
        };
        Box::pin(std::future::ready(result))
    }
    fn cancel_task(&self, _: u64) {}
    fn terminate(&self) {}
}

fn worker(init: &str) -> Rc<Worker> {
    Rc::new(Worker { init: init.into(), scheduled: RefCell::new(Vec::new()) })
}

fn poll<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    std::pin::Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn draco_loader() {
    run(
        "scene",
        "DracoLoader",
        |_| EXACT,
        |f, a| {
            (f == "schedule").then_some(())?;
            let w = worker(s(&a[0]));
            let decoder = DracoLoader::new(w.clone(), 1);
            let parameters = a[2].clone();
            let calls: Vec<Value> = (0..3)
                .map(|_| {
                    let task = if s(&a[1]) == "bufferView" {
                        decoder.decode_buffer_view(parameters.clone())
                    } else {
                        decoder.decode_point_cloud(parameters.clone())
                    };
                    match task {
                        Ok(None) => json!("undefined"),
                        Ok(Some(mut task)) => {
                            assert!(poll(&mut task).is_ready());
                            json!("task")
                        }
                        Err(e) => json!({"error": e.message}),
                    }
                })
                .collect();
            let scheduled = w.scheduled.borrow().clone();
            ok(json!({"calls": calls, "scheduled": scheduled}))
        },
    );
}

struct FailingTransport;
impl ResourceTransport for FailingTransport {
    fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
        Box::pin(std::future::ready(Err(RequestErrorEvent::new(Some(503), None, None))))
    }
    fn cancel(&self, _: &HttpRequest) {}
}

fn frame() -> FrameState {
    let mut c = Camera::new(CameraOptions::new(800., 600.));
    let camera = c.uniform_camera();
    let culling = c.frustum.compute_culling_volume(camera.position, camera.direction, camera.up);
    FrameState::new(
        cesium_rs::renderer::UniformFrame::new(camera, JulianDate::new(2451545., 0., TimeStandard::Tai)),
        culling,
        [800, 600],
    )
}

fn gltf_draco(scenario: &str, a: &[Value]) -> Value {
    let document = a[0].clone();
    let raw: Vec<u8> = a[3].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
    let mut bytes = vec![0u8; raw.len() + 8];
    bytes[4..4 + raw.len()].copy_from_slice(&raw);
    if a[5] == true {
        bytes[44..4 + raw.len()].fill(7);
    }
    let gltf = Arc::new(LoadedGltfJson {
        json: document.clone(),
        embedded_buffers: BTreeMap::new(),
        buffer_sources: BTreeMap::new(),
    });
    let w = worker(s(&a[4]));
    let decoder = Rc::new(DracoLoader::new(w.clone(), 1));
    let view_id = a[2]["bufferView"].as_u64().unwrap() as usize;
    let view_key = format!("view-{view_id}");
    let lease: Rc<RefCell<Option<std::rc::Weak<GltfBufferViewLoader>>>> = Rc::default();
    let factory = {
        let (gltf, lease, view_key) = (gltf.clone(), lease.clone(), view_key.clone());
        move || {
            let view = if view_id == 0 {
                let buffer = BufferLoader::new(Some(BufferSource::new(bytes)), None, None, None)?;
                GltfBufferViewLoader::from_buffer_loader(&gltf.json["bufferViews"][0], Rc::new(buffer), Some(view_key))?
            } else {
                let client = ResourceClient::new(Rc::new(FailingTransport));
                GltfBufferViewLoader::new(gltf.clone(), view_id, Resource::new(URL).unwrap(), client, Some(view_key))?
            };
            let view = Rc::new(view);
            *lease.borrow_mut() = Some(Rc::downgrade(&view));
            Ok(view)
        }
    };
    let loader =
        GltfDracoLoader::from_host(gltf, a[1].clone(), a[2].clone(), factory, decoder, Some("draco".into())).unwrap();
    let frame = frame();
    let mut steps = Vec::new();
    let snapshot = |steps: &mut Vec<Value>, step: &str, extra: Value| {
        let mut entry =
            json!({"step": step, "state": loader.state() as u32, "decoded": loader.decoded_draco().is_some()});
        entry.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        steps.push(entry);
    };
    snapshot(&mut steps, "initial", json!({}));
    let mut promise = loader.load();
    snapshot(&mut steps, "loading", json!({}));
    if scenario == "destroyDuringLoad" {
        loader.destroy();
    }
    match poll(&mut promise) {
        Poll::Ready(Ok(resolved)) => snapshot(&mut steps, "loaded", json!({"resolved": resolved})),
        Poll::Ready(Err(e)) => snapshot(&mut steps, "loaded", json!({"error": e.message})),
        Poll::Pending => panic!("load unexpectedly pending"),
    }
    for i in 0..4 {
        if loader.is_destroyed() {
            break;
        }
        let step = format!("process{i}");
        match loader.process(&frame) {
            Ok(ready) => snapshot(&mut steps, &step, json!({"ready": ready})),
            Err(e) => snapshot(&mut steps, &step, json!({"error": e.message.split('\n').next().unwrap()})),
        }
    }
    let released = lease.borrow().as_ref().is_some_and(|weak| weak.upgrade().is_none());
    let mut result = json!({
        "steps": steps,
        "scheduled": *w.scheduled.borrow(),
        // A released lease is upstream's resourceCache.unload of the buffer view loader.
        "unloaded": if released { vec![view_key] } else { vec![] },
    });
    if let Some(decoded) = loader.decoded_draco() {
        result["decoded"] = json!({"indices": decoded.indices, "vertexAttributes": decoded.vertex_attributes});
    }
    if scenario == "unloadAfterReady" {
        loader.unload();
        result["afterUnload"] = json!({"decoded": loader.decoded_draco().is_some(), "state": loader.state() as u32});
    }
    result
}

#[test]
fn gltf_draco_loader() {
    run("scene/foundations", "GltfDracoLoader", |_| EXACT, |f, a| ok(gltf_draco(f, a)));
}

#[test]
fn max_decoding_concurrency_leaves_a_core_free() {
    assert_eq!([0, 1, 2, 3, 8].map(DracoLoader::max_decoding_concurrency), [1, 1, 1, 2, 7]);
}
