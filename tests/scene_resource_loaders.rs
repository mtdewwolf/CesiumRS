mod common;
use cesium_rs::{
    core::*,
    scene::{
        gltf_pipeline::{BufferSource, Gltf},
        *,
    },
};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};
const URL: &str = "https://loader.test/models/model.gltf";
fn schema() -> Value {
    json!({"id":"s","classes":{"test":{"properties":{"height":{"type":"SCALAR","componentType":"FLOAT32"}}}}})
}
#[derive(Default)]
struct Transport {
    bodies: HashMap<String, Vec<u8>>,
    calls: RefCell<Vec<String>>,
    fail: bool,
    pending: bool,
    cancelled: Cell<usize>,
}
impl ResourceTransport for Transport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        self.calls.borrow_mut().push(request.url.clone());
        if self.pending {
            return Box::pin(std::future::pending());
        }
        let body = self.bodies.get(&request.url).cloned().unwrap_or_else(|| vec![1, 2, 3, 4, 5]);
        let result = if self.fail {
            Err(RequestErrorEvent::new(Some(503), None, None))
        } else {
            Ok(HttpResponse { status: 200, body, headers: vec![] })
        };
        Box::pin(std::future::ready(result))
    }
    fn cancel(&self, _: &HttpRequest) {
        self.cancelled.set(self.cancelled.get() + 1);
    }
}
fn ready<F: Future>(future: F) -> F::Output {
    match std::pin::pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test request unexpectedly pending"),
    }
}
fn lifecycle(module: &str, a: &[Value]) -> Value {
    let external = a[0] == true;
    let fail = a[1] == true;
    let action = s(&a[2]);
    let transport = Rc::new(Transport {
        fail,
        bodies: if module == "schema" {
            HashMap::from([(URL.into(), serde_json::to_vec(&schema()).unwrap())])
        } else {
            HashMap::new()
        },
        ..Default::default()
    });
    let client = ResourceClient::new(transport.clone());
    let resource = external.then(|| Resource::new(URL).unwrap());
    let loader: Box<dyn ResourceLoader>;
    let available: Box<dyn Fn() -> bool>;
    let content: Box<dyn Fn() -> (String, Value)>;
    if module == "buffer" {
        let buffer = Rc::new(
            BufferLoader::new(
                (!external).then(|| BufferSource::new(vec![1, 2, 3, 4, 5])),
                resource,
                Some(client),
                Some("k".into()),
            )
            .unwrap(),
        );
        let b = buffer.clone();
        available = Box::new(move || b.typed_array().is_some());
        let b = buffer.clone();
        content = Box::new(move || ("bytes".into(), b.typed_array().map_or(Value::Null, |b| json!(b.bytes()))));
        // ResourceLoader is implemented by the owned loader; shared references
        // let the test inspect it while futures retain request identity.
        loader = Box::new(SharedBuffer(buffer));
    } else {
        let schema = Rc::new(
            MetadataSchemaLoader::new((!external).then(schema), resource, Some(client), Some("k".into())).unwrap(),
        );
        let m = schema.clone();
        available = Box::new(move || m.schema().is_some());
        let m = schema.clone();
        content = Box::new(move || {
            ("schemaId".into(), m.schema().and_then(|s| s.id.clone()).map_or(Value::Null, Value::String))
        });
        loader = Box::new(SharedSchema(schema));
    }
    let initial = json!({"state":loader.state().value(),"available":available(),"destroyed":loader.is_destroyed()});
    let first = loader.load();
    let second = loader.load();
    let loading = json!({"state":loader.state().value(),"available":available(),"destroyed":loader.is_destroyed()});
    if action == "destroy" {
        loader.destroy();
    }
    let result = ready(first);
    let again = ready(second);
    assert_eq!(result, again);
    if action == "unload" {
        loader.unload();
    }
    let mut result = json!({"initial":initial,"loading":loading,"after":{"state":loader.state().value(),"available":available(),"destroyed":loader.is_destroyed()},"calls":transport.calls.borrow().len(),"error":result.as_ref().err().map(|e|e.message.clone()),"completed":result.ok(),"cacheKey":loader.cache_key()});
    let (name, value) = content();
    result[name] = value;
    result
}
macro_rules! shared_loader {
    ($name:ident,$inner:ty) => {
        struct $name(Rc<$inner>);
        impl ResourceLoader for $name {
            fn cache_key(&self) -> Option<&str> {
                self.0.cache_key()
            }
            fn load(&self) -> ResourceLoadFuture {
                self.0.load()
            }
            fn unload(&self) {
                self.0.unload();
            }
            fn destroy(&self) {
                self.0.destroy();
            }
            fn is_destroyed(&self) -> bool {
                self.0.is_destroyed()
            }
            fn state(&self) -> ResourceLoaderState {
                self.0.state()
            }
        }
    };
}
shared_loader!(SharedBuffer, BufferLoader);
shared_loader!(SharedSchema, MetadataSchemaLoader);
#[test]
fn pinned_resource_loader_lifetimes() {
    run(
        "scene",
        "ResourceLoader",
        |_| EXACT,
        |_, a| {
            Some(Out::Ok(json!(
                cesium_rs::scene::resource_loader::loader_error(
                    s(&a[0]),
                    a[1].as_str().map(RuntimeError::new).as_ref()
                )
                .message
            )))
        },
    );
    run("scene", "BufferLoader", |_| EXACT, |_, a| Some(Out::Ok(lifecycle("buffer", a))));
    run("scene", "MetadataSchemaLoader", |_| EXACT, |_, a| Some(Out::Ok(lifecycle("schema", a))));
}
fn glb(document: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(document).unwrap();
    while !bytes.len().is_multiple_of(4) {
        bytes.push(b' ');
    }
    let mut out = Vec::new();
    for value in [0x46546c67, 2, (20 + bytes.len()) as u32, bytes.len() as u32, 0x4e4f534a] {
        out.extend(value.to_le_bytes());
    }
    out.extend(bytes);
    out
}
#[test]
fn pinned_gltf_json_processing() {
    run(
        "scene",
        "GltfJsonLoader",
        |_| EXACT,
        |_, a| {
            let document = a[0].clone();
            let kind = s(&a[1]);
            let bytes = if kind == "glb" { glb(&document) } else { serde_json::to_vec(&document).unwrap() };
            let transport =
                Rc::new(Transport { bodies: HashMap::from([(URL.into(), bytes.clone())]), ..Default::default() });
            let resource = Resource::new(URL).unwrap();
            let input = match kind {
                "json" => Some(GltfJsonInput::Document(Gltf::new(document))),
                "external" => None,
                _ => Some(GltfJsonInput::Bytes(BufferSource::new(bytes))),
            };
            let loader = GltfJsonLoader::new(
                resource.clone(),
                resource,
                ResourceClient::new(transport.clone()),
                input,
                Some("g".into()),
            );
            let result = match ready(loader.load()) {
                Ok(true) => {
                    let g = loader.gltf().unwrap();
                    let embedded: serde_json::Map<String, Value> =
                        g.embedded_buffers.iter().map(|(id, bytes)| (id.to_string(), json!(bytes.bytes()))).collect();
                    json!({"json":g.json,"embedded":embedded,"external":transport.calls.borrow().iter().filter(|url|url.ends_with(".bin")).collect::<Vec<_>>(),"state":loader.state().value()})
                }
                Err(error) => json!({"error":error.message,"state":loader.state().value()}),
                Ok(false) => panic!("loader destroyed unexpectedly"),
            };
            loader.destroy();
            Some(Out::Ok(result))
        },
    );
}
#[test]
fn cancellation_and_buffer_view_ownership() {
    let bytes = Arc::new(vec![9, 1, 2, 3, 8]);
    let loader = BufferLoader::new(Some(BufferSource::view(bytes.clone(), 1, 3)), None, None, None).unwrap();
    assert!(ready(loader.load()).unwrap());
    let view = loader.typed_array().unwrap();
    assert!(Arc::ptr_eq(&view.buffer, &bytes));
    loader.destroy();
    assert_eq!(view.bytes(), [1, 2, 3]);
    assert!(loader.typed_array().is_none());
    assert!(ready(loader.load()).is_err());
    let transport = Rc::new(Transport { pending: true, ..Default::default() });
    let client = ResourceClient::new(transport.clone());
    let loader = BufferLoader::new(None, Some(Resource::new(URL).unwrap()), Some(client.clone()), None).unwrap();
    let mut request = Box::pin(loader.load());
    assert!(request.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());
    loader.destroy();
    assert!(!ready(request).unwrap());
    client.scheduler.borrow_mut().poll_update(&mut Context::from_waker(Waker::noop()));
    assert_eq!(transport.cancelled.get(), 1);
    assert!(loader.typed_array().is_none());
}

#[test]
fn pinned_buffer_view_and_meshopt_decoding() {
    run(
        "scene",
        "GltfBufferViewLoader",
        |_| EXACT,
        |_, a| {
            let scenario = &a[0];
            let pad = scenario["pad"].as_u64().unwrap() as usize;
            let bytes =
                scenario["bytes"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect::<Vec<_>>();
            let mut all = vec![238; pad];
            all.extend(bytes);
            all.extend([238; 3]);
            let source = BufferSource::view(Arc::new(all), pad, scenario["sourceLength"].as_u64().unwrap() as usize);
            let child = Rc::new(BufferLoader::new(Some(source), None, None, None).unwrap());
            let weak = Rc::downgrade(&child);
            let loader =
                GltfBufferViewLoader::from_buffer_loader(&scenario["view"], child, Some("view".into())).unwrap();
            assert!(ready(loader.load()).unwrap());
            let mut result = json!({"bytes":loader.typed_array().unwrap().bytes(),"state":loader.state().value(),"childDestroyed":weak.upgrade().is_none()});
            loader.unload();
            result["unloaded"] = json!(loader.typed_array().is_none());
            loader.destroy();
            result["destroyed"] = json!(loader.is_destroyed());
            Some(Out::Ok(result))
        },
    );
}

#[test]
fn json_loader_feeds_external_and_binary_buffer_views() {
    for binary in [false, true] {
        let mut document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":5}],"bufferViews":[{"buffer":0,"byteOffset":1,"byteLength":3}]});
        let (input, bodies) = if binary {
            let mut bytes = glb(&document);
            let bin = [1, 2, 3, 4, 5, 0, 0, 0];
            bytes.extend((bin.len() as u32).to_le_bytes());
            bytes.extend(0x004e4942u32.to_le_bytes());
            bytes.extend(bin);
            let length = bytes.len() as u32;
            bytes[8..12].copy_from_slice(&length.to_le_bytes());
            (Some(GltfJsonInput::Bytes(BufferSource::new(bytes))), HashMap::new())
        } else {
            document["buffers"][0]["uri"] = json!("external.bin");
            (
                None,
                HashMap::from([
                    (URL.into(), serde_json::to_vec(&document).unwrap()),
                    ("https://loader.test/models/external.bin".into(), vec![1, 2, 3, 4, 5]),
                ]),
            )
        };
        let transport = Rc::new(Transport { bodies, ..Default::default() });
        let client = ResourceClient::new(transport.clone());
        let resource = Resource::new(URL).unwrap();
        let loader = GltfJsonLoader::new(resource.clone(), resource.clone(), client.clone(), input, None);
        assert!(ready(loader.load()).unwrap());
        let view = GltfBufferViewLoader::new(loader.gltf().unwrap(), 0, resource, client, None).unwrap();
        assert!(ready(view.load()).unwrap());
        loader.destroy();
        assert_eq!(view.typed_array().unwrap().bytes(), [2, 3, 4]);
        assert_eq!(transport.calls.borrow().len(), if binary { 0 } else { 2 });
        view.destroy();
    }
}

#[test]
fn buffer_view_failure_and_parent_cancellation_release_child_requests() {
    let child = Rc::new(BufferLoader::new(Some(BufferSource::new(vec![1, 2, 3])), None, None, None).unwrap());
    let weak = Rc::downgrade(&child);
    let loader =
        GltfBufferViewLoader::from_buffer_loader(&json!({"byteOffset":1,"byteLength":4}), child, None).unwrap();
    let error = ready(loader.load()).unwrap_err();
    assert!(error.message.starts_with("Failed to load buffer view\n"));
    assert_eq!(loader.state(), ResourceLoaderState::Failed);
    assert!(weak.upgrade().is_none());
    assert_eq!(ready(loader.load()).unwrap_err(), error);
    let transport = Rc::new(Transport { pending: true, ..Default::default() });
    let client = ResourceClient::new(transport.clone());
    let child =
        Rc::new(BufferLoader::new(None, Some(Resource::new(URL).unwrap()), Some(client.clone()), None).unwrap());
    let view = GltfBufferViewLoader::from_buffer_loader(&json!({"byteOffset":0,"byteLength":3}), child, None).unwrap();
    let mut request = Box::pin(view.load());
    assert!(request.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());
    view.destroy();
    assert!(!ready(request).unwrap());
    client.scheduler.borrow_mut().poll_update(&mut Context::from_waker(Waker::noop()));
    assert_eq!(transport.cancelled.get(), 1);
    assert!(view.typed_array().is_none());
}
