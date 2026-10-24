mod common;
use cesium_rs::{core::*, renderer::*, scene::*};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};
fn out<T>(r: Result<T, RuntimeError>, encode: impl FnOnce(T) -> Value) -> Option<Out> {
    Some(match r {
        Ok(v) => Out::Ok(encode(v)),
        Err(e) => Out::Err(e.to_string()),
    })
}
#[test]
fn cache_keys_match_pinned_source() {
    run(
        "scene",
        "ResourceCacheKey",
        |_| EXACT,
        |method, a| {
            let c = &a[0];
            let resource = Resource::new("https://cache.test/models/model.gltf?fixture=sample").unwrap();
            let base = Resource::new("https://cache.test/models/").unwrap();
            let context = GltfKeyContext { gltf: &c["document"], gltf_resource: &resource, base_resource: &base };
            let options = BufferKeyOptions {
                load_buffer: c["loadBuffer"] == true,
                load_typed_array: c["loadTypedArray"] == true,
                context_id: Some(c["context"].as_str().unwrap_or("a")),
                dequantize: c["dequantize"] == true,
            };
            let r = match method {
                "getSchemaCacheKey" => {
                    if c.get("schema").is_some() {
                        ResourceCacheKey::schema(Some(&decode_numbers(&c["schema"])), None)
                    } else {
                        ResourceCacheKey::schema(None, Some(&resource))
                    }
                }
                "getExternalBufferCacheKey" => Ok(ResourceCacheKey::external_buffer(&resource)),
                "getEmbeddedBufferCacheKey" => {
                    Ok(ResourceCacheKey::embedded_buffer(&resource, c["bufferId"].as_u64().unwrap() as usize))
                }
                "getGltfCacheKey" => Ok(ResourceCacheKey::gltf(&resource)),
                "getBufferViewCacheKey" => {
                    ResourceCacheKey::buffer_view(context, c["bufferViewId"].as_u64().unwrap() as usize)
                }
                "getDracoCacheKey" => ResourceCacheKey::draco(context, &c["draco"]),
                "getSpzCacheKey" => ResourceCacheKey::spz(context),
                "getVertexBufferCacheKey" => {
                    let source = if !c["draco"].is_null() {
                        VertexKeySource::Draco { extension: &c["draco"], semantic: s(&c["attributeSemantic"]) }
                    } else if !c["spz"].is_null() {
                        VertexKeySource::Spz { extension: &c["spz"], semantic: s(&c["attributeSemantic"]) }
                    } else {
                        VertexKeySource::BufferView(c["bufferViewId"].as_u64().unwrap() as usize)
                    };
                    ResourceCacheKey::vertex_buffer(context, source, options)
                }
                "getIndexBufferCacheKey" => ResourceCacheKey::index_buffer(
                    context,
                    c["accessorId"].as_u64().unwrap() as usize,
                    c.get("draco"),
                    options,
                ),
                "getImageCacheKey" => ResourceCacheKey::image(context, c["imageId"].as_u64().unwrap() as usize),
                "getTextureCacheKey" => ResourceCacheKey::texture(
                    context,
                    &c["textureInfo"],
                    SupportedImageFormats { webp: c["webp"] == true, basis: c["basis"] == true },
                    options.context_id.unwrap(),
                ),
                _ => return None,
            };
            out(r, Value::String)
        },
    );
}
fn sampler(s: Sampler) -> Value {
    json!([s.wrap_s as u32, s.wrap_t as u32, s.minification_filter as u32, s.magnification_filter as u32])
}
#[test]
fn gltf_loader_utilities_match_pinned_source() {
    run(
        "scene",
        "GltfLoaderUtil",
        |_| TIGHT,
        |method, a| match method {
            "image" => out(
                GltfLoaderUtil::image_id(
                    &a[0],
                    a[1].as_u64().unwrap() as usize,
                    SupportedImageFormats { webp: a[2] == true, basis: a[3] == true },
                ),
                |id| json!(id),
            ),
            "sampler" => out(
                GltfLoaderUtil::create_sampler(
                    &json!({"textures":[{"sampler":0}],"samplers":[{"minFilter":a[0]}]}),
                    &json!({"index":0}),
                    a[1] == true,
                ),
                sampler,
            ),
            "sampler-default" => {
                out(GltfLoaderUtil::create_sampler(&json!({"textures":[{}]}), &json!({"index":0}), false), sampler)
            }
            "reader" => {
                let mut info = json!({"index":2,"texCoord":1,"scale":0.3});
                if a[1] == true {
                    info["extensions"] = json!({"KHR_texture_transform":{"offset":[0.2,-0.4],"scale":[2.0,0.5],"rotation":a[0],"texCoord":3}});
                }
                out(
                    GltfLoaderUtil::create_model_texture_reader(&info, Some("rgb".into()), None),
                    |r| json!({"index":r.index,"texCoord":r.tex_coord,"scale":r.scale,"transform":r.transform.map(|m|m.0),"channels":r.channels}),
                )
            }
            _ => None,
        },
    );
}
#[test]
fn resource_cache_memory_accounting_matches_pinned_source() {
    run(
        "scene",
        "ResourceCacheStatistics",
        |_| EXACT,
        |_, a| {
            let mut statistics = ResourceCacheStatistics::default();
            Some(Out::Ok(Value::Array(
                a[0].as_array()
                    .unwrap()
                    .iter()
                    .map(|op| {
                        let key = op[1].as_str().unwrap_or("");
                        match s(&op[0]) {
                            "geometry" => statistics.add_geometry_loader(key, op[2].as_u64(), op[3].as_u64()),
                            "texture" => statistics.add_texture_loader(key, op[2].as_u64().unwrap()),
                            "remove" => statistics.remove_loader(key),
                            "clear" => statistics.clear(),
                            _ => panic!("unknown op"),
                        };
                        json!([statistics.geometry_byte_length, statistics.textures_byte_length])
                    })
                    .collect(),
            )))
        },
    );
}
struct NoTransport;
impl ResourceTransport for NoTransport {
    fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
        panic!("unexpected request")
    }
}
struct Loader {
    key: String,
    destroyed: Rc<Cell<bool>>,
}
impl ResourceLoader for Loader {
    fn cache_key(&self) -> Option<&str> {
        Some(&self.key)
    }
    fn load(&self) -> ResourceLoadFuture {
        use futures_util::FutureExt;
        futures_util::future::ready(Ok(true)).boxed_local().shared()
    }
    fn unload(&self) {}
    fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }
    fn destroy(&self) {
        self.destroyed.set(true);
    }
    fn state(&self) -> ResourceLoaderState {
        ResourceLoaderState::Ready
    }
}
#[test]
fn resource_cache_references_match_pinned_source() {
    run(
        "scene",
        "ResourceCache",
        |_| EXACT,
        |_, a| {
            let cache = ResourceCache::new(ResourceClient::new(Rc::new(NoTransport)));
            let mut handles = HashMap::<String, Vec<CacheLease<Loader>>>::new();
            let mut destroyed = Vec::new();
            let mut trace = Vec::new();
            for op in a[0].as_array().unwrap() {
                let (op, key) = s(op).split_once(':').unwrap_or((s(op), ""));
                match op {
                    "add" => {
                        let flag = Rc::new(Cell::new(false));
                        destroyed.push(flag.clone());
                        let lease =
                            cache.add(Loader { key: key.into(), destroyed: flag }, CacheResourceKind::Other).unwrap();
                        handles.insert(key.into(), vec![lease]);
                    }
                    "get" => {
                        if let Some(lease) = cache.get::<Loader>(key).unwrap() {
                            handles.entry(key.into()).or_default().push(lease);
                        }
                    }
                    "unload" => {
                        drop(handles.get_mut(key).unwrap().pop());
                    }
                    "clear" => cache.clear_for_specs(),
                    _ => panic!("unknown op"),
                };
                let entries: serde_json::Map<String, Value> = handles
                    .keys()
                    .filter_map(|key| cache.reference_count(key).map(|n| (key.clone(), json!(n))))
                    .collect();
                trace.push(json!({"entries":entries,"destroyed":destroyed.iter().map(|d|d.get()).collect::<Vec<_>>()}));
            }
            Some(Out::Ok(json!(trace)))
        },
    );
}
fn ready<F: Future>(future: F) -> F::Output {
    match std::pin::pin!(future).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("unexpected pending"),
    }
}
#[derive(Default)]
struct Transport {
    calls: RefCell<Vec<String>>,
}
impl ResourceTransport for Transport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        self.calls.borrow_mut().push(request.url);
        Box::pin(std::future::ready(Ok(HttpResponse {
            status: 200,
            body: vec![1, 2, 3, 4, 5, 6, 7, 8],
            headers: vec![],
        })))
    }
}
#[test]
fn shared_views_fetch_once_and_release_last_dependency() {
    let transport = Rc::new(Transport::default());
    let cache = ResourceCache::new(ResourceClient::new(transport.clone()));
    let resource = Resource::new("https://cache.test/model.gltf").unwrap();
    let json = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":8,"uri":"mesh.bin"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":4},{"buffer":0,"byteOffset":4,"byteLength":4}]});
    let gltf = cache
        .gltf_json_loader(
            resource.clone(),
            resource.clone(),
            Some(GltfJsonInput::Document(gltf_pipeline::Gltf::new(json))),
        )
        .unwrap();
    assert!(ready(gltf.load()).unwrap());
    let one = cache.buffer_view_loader(gltf.gltf().unwrap(), 0, &resource, &resource).unwrap();
    let two = cache.buffer_view_loader(gltf.gltf().unwrap(), 1, &resource, &resource).unwrap();
    let duplicate = cache.buffer_view_loader(gltf.gltf().unwrap(), 0, &resource, &resource).unwrap();
    assert!(one.ptr_eq(&duplicate));
    assert!(ready(one.load()).unwrap());
    assert!(ready(two.load()).unwrap());
    assert_eq!(transport.calls.borrow().len(), 1);
    assert_eq!(one.typed_array().unwrap().bytes(), [1, 2, 3, 4]);
    assert_eq!(two.typed_array().unwrap().bytes(), [5, 6, 7, 8]);
    let keep = one.loader();
    drop(one);
    assert!(!keep.is_destroyed());
    drop(duplicate);
    assert!(keep.is_destroyed());
    drop(two);
    drop(gltf);
    assert!(cache.is_empty());
}
#[test]
fn clearing_old_leases_cannot_release_reused_keys() {
    let cache = ResourceCache::new(ResourceClient::new(Rc::new(NoTransport)));
    let old_flag = Rc::new(Cell::new(false));
    let old = cache.add(Loader { key: "same".into(), destroyed: old_flag.clone() }, CacheResourceKind::Other).unwrap();
    cache.clear_for_specs();
    assert!(old_flag.get());
    let new_flag = Rc::new(Cell::new(false));
    let new = cache.add(Loader { key: "same".into(), destroyed: new_flag.clone() }, CacheResourceKind::Other).unwrap();
    drop(old);
    assert_eq!(cache.reference_count("same"), Some(1));
    assert!(!new_flag.get());
    drop(new);
    assert!(new_flag.get());
}

fn decode_numbers(value: &Value) -> Value {
    match value {
        Value::String(s) if s == "-0" => json!(-0.0),
        Value::Array(values) => Value::Array(values.iter().map(decode_numbers).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), decode_numbers(v))).collect()),
        _ => value.clone(),
    }
}
