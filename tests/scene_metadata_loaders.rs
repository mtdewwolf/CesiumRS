//! GltfStructuralMetadataLoader and GltfMeshPrimitiveGpmLoader through the
//! shared ResourceCache, against the pinned CesiumJS loaders
//! (`tools/cesium/fixtures/scene-metadata-loaders.mjs`). Images are named by
//! their size; uploads go to a recording GL host.
mod common;
use cesium_rs::core::image_helpers::ImageFuture;
use cesium_rs::core::*;
use cesium_rs::renderer::*;
use cesium_rs::scene::gltf_pipeline::BufferSource;
use cesium_rs::scene::model::extensions::gpm::{GltfMeshPrimitiveGpmLoader, GltfMeshPrimitiveGpmLoaderOptions};
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};

const ROOT: &str = "https://metadata.test/models/model.gltf";
const SCHEMA_URL: &str = "https://metadata.test/models/schema.json";

fn ready<T>(f: impl Future<Output = T>) -> T {
    match std::pin::pin!(f).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("unexpected pending request"),
    }
}

/// Images sized by name: a.png 3×5, b.png 4×7, c.png 2×2.
struct Images;

fn size_of(url: &str) -> (u32, u32) {
    match url.rsplit('/').next().unwrap_or("") {
        "a.png" => (3, 5),
        "b.png" => (4, 7),
        _ => (2, 2),
    }
}

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
        RgbaImage::new(w, h, vec![0; (w * h * 4) as usize]).ok_or_else(|| RuntimeError::new("bad image"))
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        None
    }
    fn load_resource_url<'a>(
        &'a self,
        url: &'a str,
        _: bool,
        _: FetchImageOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        let (w, h) = size_of(url);
        let image = RgbaImage::new(w, h, vec![9; (w * h * 4) as usize]).ok_or_else(|| RuntimeError::new("bad image"));
        Some(Box::pin(std::future::ready(image)))
    }
}

impl ModelImageHost<Host> for Images {}

/// Serves the external schema, or fails it.
struct Transport {
    schema: Value,
    fail: Rc<Cell<bool>>,
}

impl ResourceTransport for Transport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        assert_eq!(request.url, SCHEMA_URL);
        Box::pin(std::future::ready(if self.fail.get() {
            Err(RequestErrorEvent::new(Some(503), None, None))
        } else {
            Ok(HttpResponse { status: 200, body: serde_json::to_vec(&self.schema).unwrap(), headers: vec![] })
        }))
    }
}

#[derive(Clone, Default)]
struct Host {
    next: Rc<Cell<u32>>,
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
        // ALIASED_LINE_WIDTH_RANGE, ALIASED_POINT_SIZE_RANGE and MAX_VIEWPORT_DIMS are pairs.
        let pair = matches!(args.first(), Some(GlValue::Number(p)) if [0x846e, 0x846d, 0x0d3a].contains(&(*p as u32)));
        Ok(match name {
            "getParameter" if pair => GlValue::Numbers(vec![1., 8192.]),
            "getParameter" => GlValue::Number(16384.),
            "getExtension" => GlValue::Object(0),
            _ => GlValue::Null,
        })
    }
}

fn frame() -> FrameState {
    let mut c = Camera::new(CameraOptions::new(8., 8.));
    let camera = c.uniform_camera();
    let culling = c.frustum.compute_culling_volume(camera.position, camera.direction, camera.up);
    FrameState::new(UniformFrame::new(camera, JulianDate::new(2451545., 0., TimeStandard::Tai)), culling, [8, 8])
}

/// The fixture's glTF: images a.png, b.png, c.png behind clamped textures.
fn gltf(c: &Value) -> Value {
    let bytes = c["bytes"].as_array().map_or(0, Vec::len);
    json!({
        "buffers": [{ "byteLength": bytes.max(1) }],
        "bufferViews": c["bufferViews"],
        "images": [{ "uri": "a.png" }, { "uri": "b.png" }, { "uri": "c.png" }],
        "samplers": [{ "wrapS": 33071, "wrapT": 33071, "minFilter": 9729 }],
        "textures": [{ "source": 0, "sampler": 0 }, { "source": 1, "sampler": 0 }, { "source": 2, "sampler": 0 }],
    })
}

struct Setup {
    cache: ResourceCache,
    request: GltfResource,
    seed: Option<CacheLease<BufferLoader>>,
    fail: Rc<Cell<bool>>,
}

fn setup(c: &Value, schema: Value) -> Setup {
    let fail = Rc::new(Cell::new(c["failSchema"] == true));
    let cache = ResourceCache::new(ResourceClient::new(Rc::new(Transport { schema, fail: fail.clone() })));
    let resource = Resource::new(ROOT).unwrap();
    let mut bytes: Vec<u8> =
        c["bytes"].as_array().map_or(Vec::new(), |b| b.iter().map(|x| x.as_u64().unwrap() as u8).collect());
    if bytes.is_empty() {
        bytes.push(0);
    }
    let source = BufferSource::new(bytes);
    let sources = BTreeMap::from([(0, source.clone())]);
    let request = GltfResource {
        document: Arc::new(LoadedGltfJson {
            json: gltf(c),
            embedded_buffers: sources.clone(),
            buffer_sources: sources,
        }),
        resource: resource.clone(),
        base: resource.clone(),
    };
    let seed = cache.embedded_buffer_loader(&resource, 0, Some(source)).unwrap();
    Setup { cache, request, seed: Some(seed), fail }
}

// ---- metadata summaries, as the fixture writes them ----

fn tag(value: &MetadataValue) -> Value {
    let all = |c: &[f64]| c.iter().map(|&x| num(x)).collect::<Vec<_>>();
    match value {
        MetadataValue::Null => json!({ "$null": true }),
        MetadataValue::Boolean(b) => json!(b),
        MetadataValue::Number(x) => num(*x),
        MetadataValue::BigInt(b) => json!({ "$bigint": b.to_string() }),
        MetadataValue::String(s) => json!(s),
        MetadataValue::Array(items) => Value::Array(items.iter().map(tag).collect()),
        MetadataValue::Cartesian2(c) => json!({ "$Cartesian2": all(&[c.x, c.y]) }),
        MetadataValue::Cartesian3(c) => json!({ "$Cartesian3": all(&[c.x, c.y, c.z]) }),
        MetadataValue::Cartesian4(c) => json!({ "$Cartesian4": all(&[c.x, c.y, c.z, c.w]) }),
        MetadataValue::Matrix2(m) => json!({ "$Matrix2": all(&m.0) }),
        MetadataValue::Matrix3(m) => json!({ "$Matrix3": all(&m.0) }),
        MetadataValue::Matrix4(m) => json!({ "$Matrix4": all(&m.0) }),
        MetadataValue::Object(v) => v.clone(),
    }
}

fn val(value: Option<&MetadataValue>) -> Value {
    value.map_or(Value::Null, tag)
}

fn reader_info(p: &PropertyTextureProperty<Host>) -> Value {
    let r = p.texture_reader();
    json!([r.index.map(num), r.texture.as_ref().map(|t| json!([t.width(), t.height()])), num(r.tex_coord), r.channels,])
}

fn metadata_info(m: Option<Rc<StructuralMetadata<Host>>>) -> Value {
    let Some(m) = m else { return Value::Null };
    let mut tables = Vec::new();
    for t in m.property_tables() {
        let mut t = t.clone();
        let mut rows = Vec::new();
        for i in 0..t.count() {
            let mut row = Map::new();
            for id in t.get_property_ids(i).unwrap() {
                let v = t.get_property(i, &id).unwrap();
                row.insert(id, val(v.as_ref()));
            }
            rows.push(Value::Object(row));
        }
        let texture = t.texture().map(|r| {
            let texture = r.get::<Rc<Texture<Host>>>().unwrap();
            json!([texture.width(), texture.height()])
        });
        tables.push(json!([t.id(), t.name(), t.count(), texture, rows]));
    }
    let textures: Vec<Value> = m
        .property_textures()
        .iter()
        .map(|t| {
            let properties: Map<String, Value> = t
                .properties()
                .iter()
                .map(|(k, p)| (k.to_string(), json!([reader_info(p), val(Some(p.offset())), val(Some(p.scale()))])))
                .collect();
            json!([t.id(), t.name(), t.class().id, properties])
        })
        .collect();
    let attributes: Vec<Value> = m
        .property_attributes()
        .iter()
        .map(|a| json!([a.id(), a.name(), a.class().id, a.properties().keys().collect::<Vec<_>>()]))
        .collect();
    json!({
        "schema": m.schema().id,
        "classes": m.schema().classes.keys().collect::<Vec<_>>(),
        "tables": tables,
        "textures": textures,
        "attributes": attributes,
    })
}

fn first_line(e: &RuntimeError) -> String {
    e.message.split('\n').next().unwrap_or("").to_string()
}

/// Load, process twice (deferred, then allowed), describe, unload.
fn lifecycle<L: ResourceLoader>(
    mut s: Setup,
    loader: L,
    c: &Value,
    describe: impl Fn(&L) -> Map<String, Value>,
) -> Value {
    let initial = loader.state() as u32;
    let promise = loader.load();
    let loading = loader.state() as u32;
    if c["action"] == "destroy" {
        loader.destroy();
    }
    let (resolved, error) = match ready(promise) {
        Ok(v) => (json!(v), Value::Null),
        Err(e) => (Value::Null, json!(first_line(&e))),
    };
    let mut result = Map::new();
    result.insert("initial".into(), json!(initial));
    result.insert("loading".into(), json!(loading));
    result.insert("resolved".into(), resolved);
    result.insert("error".into(), error.clone());
    result.insert("loaded".into(), json!(loader.state() as u32));
    if error.is_null() && c["action"] != "destroy" {
        let mut f = frame();
        let budget = Rc::new(ResourceUploadBudget::new(0, 0));
        f.job_scheduler = Some(FrameResource::new(budget.clone()));
        result.insert("first".into(), json!(loader.process(&f).unwrap()));
        result.insert("deferred".into(), json!(loader.state() as u32));
        budget.reset(0, 100);
        result.insert("ready".into(), json!(loader.process(&f).unwrap()));
        result.insert("state".into(), json!(loader.state() as u32));
        result.insert("again".into(), json!(loader.process(&f).unwrap()));
        result.extend(describe(&loader));
        result.insert("processed".into(), json!(s.cache.len()));
        loader.unload();
        result.insert("unloaded".into(), Value::Object(describe(&loader)));
    }
    result.insert("afterUnload".into(), json!(s.cache.len()));
    s.seed.take();
    result.insert("empty".into(), json!(s.cache.len()));
    drop(loader);
    Value::Object(result)
}

fn structural_schema() -> Value {
    load("scene", "parseStructuralMetadata")[0].a[0].clone()
}

#[test]
fn structural_metadata_loader() {
    run(
        "scene",
        "GltfStructuralMetadataLoader",
        |_| EXACT,
        |_, a| {
            let c = &a[0];
            let s = setup(c, structural_schema());
            let loader = GltfStructuralMetadataLoader::new(GltfStructuralMetadataLoaderOptions {
                cache: s.cache.clone(),
                request: s.request.clone(),
                extension: c.get("extension").filter(|v| !v.is_null()).cloned(),
                extension_legacy: c.get("legacy").filter(|v| !v.is_null()).cloned(),
                context: Context::new_webgl1(Host::default()),
                host: Rc::new(Images),
                texture_options: GltfTextureOptions::default(),
                cache_key: None,
            })
            .unwrap();
            assert!(!s.fail.get() || c["failSchema"] == true);
            ok(lifecycle(s, loader, c, |l| {
                let mut m = Map::new();
                m.insert("metadata".into(), metadata_info(l.structural_metadata()));
                m
            }))
        },
    );
}

#[test]
fn mesh_primitive_gpm_loader() {
    run(
        "scene",
        "GltfMeshPrimitiveGpmLoader",
        |_| EXACT,
        |_, a| {
            let c = &a[0];
            let s = setup(c, structural_schema());
            let loader = GltfMeshPrimitiveGpmLoader::new(GltfMeshPrimitiveGpmLoaderOptions {
                cache: s.cache.clone(),
                request: s.request.clone(),
                extension: c["extension"].clone(),
                context: Context::new_webgl1(Host::default()),
                host: Rc::new(Images),
                texture_options: GltfTextureOptions::default(),
                cache_key: None,
            });
            ok(lifecycle(s, loader, c, |l| {
                let mut m = Map::new();
                let local = l.mesh_primitive_gpm_local().map(|local| {
                    local
                        .ppe_textures
                        .iter()
                        .map(|t| {
                            json!([
                                t.traits.source,
                                t.traits.min.map(num),
                                t.traits.max.map(num),
                                t.no_data.map(num),
                                t.offset.map(num),
                                t.scale.map(num),
                                t.index,
                                t.tex_coord
                            ])
                        })
                        .collect::<Vec<_>>()
                });
                m.insert("local".into(), json!(local));
                let metadata = l.structural_metadata();
                let classes = metadata.as_ref().map(|md| {
                    md.schema()
                        .classes
                        .iter()
                        .map(|(id, class)| {
                            let properties: Vec<Value> = class
                                .properties
                                .iter()
                                .map(|(pid, p)| {
                                    json!([
                                        pid,
                                        p.name(),
                                        p.normalized(),
                                        val(Some(p.offset())),
                                        val(Some(p.scale())),
                                        val(p.min()),
                                        val(p.max())
                                    ])
                                })
                                .collect();
                            (id.to_string(), json!([class.name, properties]))
                        })
                        .collect::<Map<String, Value>>()
                });
                m.insert("metadata".into(), metadata_info(metadata));
                m.insert("classes".into(), json!(classes));
                m.insert(
                "cached".into(),
                json!(cesium_rs::scene::model::extensions::gpm::gltf_mesh_primitive_gpm_loader::ppe_textures_metadata_schema_cache_len()),
            );
                m
            }))
        },
    );
}

/// A stalled first image must not hide the second image's decoding failure.
struct FailingImages;

impl ImageHost for FailingImages {
    type Image = RgbaImage;
    fn dimensions(&self, image: &RgbaImage) -> (u32, u32) {
        (image.width, image.height)
    }
    fn image_from_rgba(&self, image: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(image)
    }
    fn image_pixels(&self, image: &RgbaImage, _: u32, _: u32) -> Result<Vec<u8>, RuntimeError> {
        Ok(image.pixels.clone())
    }
    fn resize_image(&self, _: &RgbaImage, _: u32, _: u32) -> Result<RgbaImage, RuntimeError> {
        unreachable!("failed images must never be uploaded")
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        unreachable!("test images use external URIs")
    }
    fn load_resource_url<'a>(
        &'a self,
        url: &'a str,
        _: bool,
        _: FetchImageOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        Some(if url.ends_with("a.png") {
            Box::pin(std::future::pending())
        } else {
            Box::pin(std::future::ready(Err(RuntimeError::new("image decode failed"))))
        })
    }
}

impl ModelImageHost<Host> for FailingImages {}

fn assert_later_image_failure(loader: &impl ResourceLoader, cache: &ResourceCache) {
    let error = ready(loader.load()).expect_err("later failure must reject despite the stalled first image");
    assert!(error.message.contains("Failed to load image: b.png"), "{error}");
    assert_eq!(loader.state(), ResourceLoaderState::Failed);
    assert!(cache.is_empty(), "failed loaders must release their child leases");
}

#[test]
fn structural_metadata_observes_later_image_failure() {
    let s = setup(&json!({}), structural_schema());
    let loader = GltfStructuralMetadataLoader::new(GltfStructuralMetadataLoaderOptions {
        cache: s.cache.clone(),
        request: s.request.clone(),
        extension: Some(json!({
            "schema": {"classes": {}},
            "propertyTextures": [{"properties": {"a": {"index": 0}, "b": {"index": 1}}}]
        })),
        extension_legacy: None,
        context: Context::new_webgl1(Host::default()),
        host: Rc::new(FailingImages),
        texture_options: GltfTextureOptions::default(),
        cache_key: None,
    })
    .unwrap();
    drop(s.seed);
    assert_later_image_failure(&loader, &s.cache);
}

#[test]
fn gpm_observes_later_image_failure() {
    let s = setup(&json!({}), structural_schema());
    let loader = GltfMeshPrimitiveGpmLoader::new(GltfMeshPrimitiveGpmLoaderOptions {
        cache: s.cache.clone(),
        request: s.request.clone(),
        extension: json!({"ppeTextures": [{"index": 0}, {"index": 1}]}),
        context: Context::new_webgl1(Host::default()),
        host: Rc::new(FailingImages),
        texture_options: GltfTextureOptions::default(),
        cache_key: None,
    });
    drop(s.seed);
    assert_later_image_failure(&loader, &s.cache);
}
