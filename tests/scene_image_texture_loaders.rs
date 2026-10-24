mod common;
use cesium_rs::{
    core::image_helpers::ImageFuture, core::ktx2_transcoder::SupportedTextureFormats, core::*, renderer::*,
    scene::gltf_pipeline::BufferSource, scene::*,
};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context as TaskContext, Poll, Waker},
};
fn ready<T>(f: impl Future<Output = T>) -> T {
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("unexpected pending request"),
    }
}
struct Images {
    log: Rc<RefCell<Vec<Value>>>,
    fail: bool,
    width: u32,
    height: u32,
}
impl Images {
    fn raster(&self) -> RgbaImage {
        RgbaImage::new(self.width, self.height, [100, 150, 200, 255].repeat((self.width * self.height) as usize))
            .unwrap()
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
        Ok(RgbaImage::new(w, h, [100, 150, 200, 255].repeat((w * h) as usize)).unwrap())
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        mime: &'a str,
        o: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        self.log.borrow_mut().push(json!(["decode", mime, o.flip_y, o.skip_color_space_conversion]));
        Some(Box::pin(std::future::ready(if self.fail {
            Err(RuntimeError::new("codec failure"))
        } else {
            Ok(self.raster())
        })))
    }
    fn load_resource_url<'a>(
        &'a self,
        url: &'a str,
        _: bool,
        o: FetchImageOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        self.log.borrow_mut().push(json!(["image", url]));
        if !self.fail {
            self.log.borrow_mut().push(json!(["fetch-options", o.prefer_image_bitmap, o.skip_color_space_conversion]));
        }
        Some(Box::pin(std::future::ready(if self.fail {
            Err(RuntimeError::new("image failure"))
        } else {
            Ok(self.raster())
        })))
    }
}
struct Transport {
    raw: Vec<u8>,
    log: Rc<RefCell<Vec<Value>>>,
    fail: bool,
}
impl ResourceTransport for Transport {
    fn execute(&self, r: HttpRequest) -> RequestFuture<HttpResponse> {
        self.log.borrow_mut().push(json!(["bytes", r.url]));
        Box::pin(std::future::ready(if self.fail {
            Err(RequestErrorEvent::new(None, None, None))
        } else {
            Ok(HttpResponse { status: 200, body: self.raw.clone(), headers: vec![] })
        }))
    }
}
#[derive(Default, Clone)]
struct Host {
    next: Rc<Cell<u32>>,
    mipmaps: Rc<Cell<usize>>,
    fail_texture: Rc<Cell<bool>>,
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name == "createTexture" && self.fail_texture.get() {
            return Err(RuntimeError::new("injected texture allocation failure"));
        }
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        if name == "generateMipmap" {
            self.mipmaps.set(self.mipmaps.get() + 1);
        }
        Ok(match name {
            "getParameter" => GlValue::Number(16384.),
            "getExtension" => GlValue::Object(0),
            _ => GlValue::Null,
        })
    }
}
impl ModelImageHost<Host> for Images {}
fn frame() -> FrameState {
    let mut c = Camera::new(CameraOptions::new(8., 8.));
    let camera = c.uniform_camera();
    let culling = c.frustum.compute_culling_volume(camera.position, camera.direction, camera.up);
    FrameState::new(UniformFrame::new(camera, JulianDate::new(2451545., 0., TimeStandard::Tai)), culling, [8, 8])
}
type Setup = (ResourceCache, GltfResource, Rc<Images>, CacheLease<BufferLoader>);
fn setup(c: &Value) -> Setup {
    use base64::Engine;
    let log = Rc::new(RefCell::new(vec![]));
    let raw = c["raw"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect::<Vec<_>>();
    let host = Rc::new(Images {
        log: log.clone(),
        fail: c["fail"] == true,
        width: c["width"].as_u64().unwrap_or(3) as u32,
        height: c["height"].as_u64().unwrap_or(5) as u32,
    });
    let client = ResourceClient::new(Rc::new(Transport { raw: raw.clone(), log, fail: c["fail"] == true }));
    let cache = ResourceCache::new(client);
    let resource = Resource::new("https://image.test/models/model.gltf").unwrap();
    let name = c["name"].as_str().unwrap();
    let is_ktx = name.contains("ktx2");
    let uri = if c["mode"] == "data" {
        format!(
            "data:image/{};base64,{}",
            if is_ktx { "ktx2" } else { "png" },
            base64::engine::general_purpose::STANDARD.encode(&raw)
        )
    } else {
        format!("image.{}?variant=fixture", if is_ktx { "KtX2" } else { name })
    };
    let image = if c["mode"] == "embedded" {
        json!({"bufferView":0,"mimeType":"deliberately/ignored"})
    } else {
        json!({"uri":uri})
    };
    let json = json!({"buffers":[{"byteLength":raw.len()+3}],"bufferViews":[{"buffer":0,"byteOffset":3,"byteLength":raw.len()}],"images":[image],"textures":[{"source":0,"sampler":0}],"samplers":[{"wrapS":c["wrap"].as_u64().unwrap_or(10497),"wrapT":c["wrap"].as_u64().unwrap_or(10497),"minFilter":c["min"].as_u64().unwrap_or(9729)}]});
    let mut backing = vec![0; raw.len() + 7];
    backing[5..5 + raw.len()].copy_from_slice(&raw);
    let source = BufferSource::view(backing.into(), 2, raw.len() + 3);
    let sources = BTreeMap::from([(0, source.clone())]);
    let request = GltfResource {
        document: Arc::new(LoadedGltfJson { json, embedded_buffers: sources.clone(), buffer_sources: sources }),
        resource: resource.clone(),
        base: resource.clone(),
    };
    let seed = cache.embedded_buffer_loader(&resource, 0, Some(source)).unwrap();
    (cache, request, host, seed)
}
fn formats(c: &Value) -> SupportedTextureFormats {
    SupportedTextureFormats { etc1: c["etc1"] == true, s3tc: c["s3tc"] == true, ..Default::default() }
}
fn info(loader: &GltfImageLoader<Images>, host: &Images) -> Value {
    loader.image().map(|i|{let(width,height,format,bytes)=match &i.image{GltfImage::Raster(i)=>{let(w,h)=host.dimensions(i);(w,h,None,None)},GltfImage::Ktx2(i)=>(i.width,i.height,Some(i.internal_format),Some(i.buffer_view().to_vec()))};json!({"width":width,"height":height,"format":format,"bytes":bytes,"mips":i.mip_levels.as_ref().map(|ls|ls.iter().map(|l|l.to_vec()).collect::<Vec<_>>())})}).unwrap_or(Value::Null)
}
#[test]
fn image_loaders_match_pinned_source() {
    run(
        "scene",
        "GltfImageLoader",
        |_| EXACT,
        |_, a| {
            let c = &a[0];
            let (cache, request, host, seed) = setup(c);
            let lease = cache.image_loader(&request, 0, host.clone(), formats(c)).unwrap();
            let loader = lease.loader();
            let initial = loader.state() as u32;
            let promise = loader.load();
            let loading = loader.state() as u32;
            let mut lease = Some(lease);
            if c["action"] == "destroy" {
                lease.take();
            }
            let (resolved, error) = match ready(promise) {
                Ok(v) => (Some(v), None),
                Err(e) => (None, Some(e.message)),
            };
            let image = info(&loader, &host);
            let state = loader.state() as u32;
            if c["action"] == "unload" {
                loader.unload();
            }
            let after = info(&loader, &host);
            let calls = if c["action"] == "destroy" { vec![] } else { host.log.borrow().clone() };
            let destroyed = loader.is_destroyed();
            drop(lease);
            drop(seed);
            ok(
                json!({"initial":initial,"loading":loading,"state":state,"resolved":resolved,"error":error,"image":image,"after":after,"calls":calls,"destroyed":destroyed,"remaining":cache.len()}),
            )
        },
    );
}
#[test]
fn texture_loaders_match_pinned_source() {
    run(
        "scene",
        "GltfTextureLoader",
        |_| EXACT,
        |_, a| {
            let c = &a[0];
            let (cache, request, host, seed) = setup(c);
            let gpu = Host::default();
            let context = Context::new_webgl1(gpu.clone());
            let loader = cache
                .texture_loader(
                    &request,
                    json!({"index":0}),
                    context,
                    host.clone(),
                    GltfTextureOptions {
                        asynchronous: c["asynchronous"] == true,
                        supported_texture_formats: formats(c),
                        ..Default::default()
                    },
                )
                .unwrap();
            let initial = loader.state() as u32;
            ready(loader.load()).unwrap();
            let loaded = loader.state() as u32;
            let mut f = frame();
            let budget = Rc::new(ResourceUploadBudget::new(0, 0));
            f.job_scheduler = Some(FrameResource::new(budget.clone()));
            let first = loader.process(&f).unwrap();
            let deferred = loader.state() as u32;
            budget.reset(0, 1);
            let is_ready = loader.process(&f).unwrap();
            let texture = loader.texture().unwrap();
            let sampler = texture.sampler();
            let mut result = json!({"initial":initial,"loaded":loaded,"first":first,"deferred":deferred,"ready":is_ready,"state":loader.state()as u32,"dimensions":texture.dimensions(),"format":texture.pixel_format()as u32,"sampler":[sampler.wrap_s as u32,sampler.wrap_t as u32,sampler.minification_filter as u32,sampler.magnification_filter as u32],"flipY":texture.flip_y(),"generated":gpu.mipmaps.get()>0,"bytes":texture.size_in_bytes(),"statistics":cache.statistics().borrow().textures_byte_length,"calls":host.log.borrow().clone()});
            let observed = loader.loader();
            drop(loader);
            drop(seed);
            result.as_object_mut().unwrap().extend(json!({"destroyed":observed.is_destroyed(),"textureDestroyed":texture.is_destroyed(),"remaining":cache.len(),"finalStatistics":cache.statistics().borrow().textures_byte_length}).as_object().unwrap().clone());
            ok(result)
        },
    );
}

fn raster_case() -> Value {
    serde_json::from_str::<Value>(include_str!("fixtures/scene/GltfImageLoader.json")).unwrap()["cases"][0]["a"][0]
        .clone()
}
#[test]
fn textures_share_decode_but_keep_distinct_sampler_resources() {
    let c = raster_case();
    let (cache, mut request, host, seed) = setup(&c);
    let doc = Arc::make_mut(&mut request.document);
    doc.json["samplers"].as_array_mut().unwrap().push(json!({"wrapS":33071,"wrapT":33071}));
    doc.json["textures"].as_array_mut().unwrap().push(json!({"source":0,"sampler":1}));
    let context = Context::new_webgl1(Host::default());
    let a = cache
        .texture_loader(&request, json!({"index":0}), context.clone(), host.clone(), GltfTextureOptions::default())
        .unwrap();
    let b = cache
        .texture_loader(&request, json!({"index":1}), context, host.clone(), GltfTextureOptions::default())
        .unwrap();
    let image_key = ResourceCacheKey::image(request.key_context(), 0).unwrap();
    assert_eq!(cache.reference_count(&image_key), Some(2));
    ready(a.load()).unwrap();
    ready(b.load()).unwrap();
    assert_eq!(host.log.borrow().len(), 1);
    let f = frame();
    a.process(&f).unwrap();
    assert_eq!(cache.reference_count(&image_key), Some(1));
    b.process(&f).unwrap();
    assert_eq!(cache.reference_count(&image_key), None);
    assert_eq!(a.texture().unwrap().dimensions(), [4, 8]);
    assert_eq!(b.texture().unwrap().dimensions(), [3, 5]);
    assert_eq!(cache.statistics().borrow().textures_byte_length, 188);
    drop(a);
    drop(b);
    drop(seed);
    assert!(cache.is_empty());
    assert_eq!(cache.statistics().borrow().textures_byte_length, 0);
}
#[test]
fn upload_quotas_are_separate_and_failed_allocations_can_retry() {
    let c = raster_case();
    let (cache, request, host, seed) = setup(&c);
    let gpu = Host::default();
    let context = Context::new_webgl1(gpu.clone());
    let texture = cache
        .texture_loader(&request, json!({"index":0}), context.clone(), host, GltfTextureOptions::default())
        .unwrap();
    let vertex = cache
        .vertex_buffer_loader(
            &request,
            VertexBufferSource::BufferView(0),
            Some(context),
            GpuBufferOptions { load_buffer: true, ..Default::default() },
        )
        .unwrap();
    ready(texture.load()).unwrap();
    ready(vertex.load()).unwrap();
    let mut f = frame();
    let quota = Rc::new(ResourceUploadBudget::new(1, 0));
    f.job_scheduler = Some(FrameResource::new(quota.clone()));
    assert!(vertex.process(&f).unwrap());
    assert!(!texture.process(&f).unwrap());
    assert!(texture.texture().is_none());
    quota.reset(0, 1);
    gpu.fail_texture.set(true);
    assert!(texture.process(&f).is_err());
    assert_eq!(texture.state(), ResourceLoaderState::Processing);
    assert_eq!(cache.statistics().borrow().textures_byte_length, 0);
    gpu.fail_texture.set(false);
    quota.reset(0, 1);
    assert!(texture.process(&f).unwrap());
    let observed = texture.texture().unwrap();
    assert!(texture.process(&f).unwrap());
    drop(texture);
    assert!(observed.is_destroyed());
    drop(vertex);
    drop(seed);
    assert!(cache.is_empty());
}
struct PendingDecode {
    drops: Rc<Cell<usize>>,
}
impl Future for PendingDecode {
    type Output = Result<RgbaImage, RuntimeError>;
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}
impl Drop for PendingDecode {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
struct SlowImages {
    base: Images,
    drops: Rc<Cell<usize>>,
}
impl ImageHost for SlowImages {
    type Image = RgbaImage;
    fn dimensions(&self, i: &RgbaImage) -> (u32, u32) {
        self.base.dimensions(i)
    }
    fn image_from_rgba(&self, i: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(i)
    }
    fn image_pixels(&self, i: &RgbaImage, w: u32, h: u32) -> Result<Vec<u8>, RuntimeError> {
        self.base.image_pixels(i, w, h)
    }
    fn resize_image(&self, i: &RgbaImage, w: u32, h: u32) -> Result<RgbaImage, RuntimeError> {
        self.base.resize_image(i, w, h)
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        Some(Box::pin(PendingDecode { drops: self.drops.clone() }))
    }
}
#[test]
fn final_image_lease_aborts_pending_decode_and_releases_view() {
    let c = raster_case();
    let (cache, request, _, seed) = setup(&c);
    let drops = Rc::new(Cell::new(0));
    let host = Rc::new(SlowImages {
        base: Images { log: Rc::default(), fail: false, width: 3, height: 5 },
        drops: drops.clone(),
    });
    let loader = cache.image_loader(&request, 0, host, formats(&c)).unwrap();
    let mut request_future = loader.load();
    assert!(std::pin::Pin::new(&mut request_future).poll(&mut TaskContext::from_waker(Waker::noop())).is_pending());
    let observed = loader.loader();
    drop(loader);
    assert!(observed.is_destroyed());
    assert!(!ready(request_future).unwrap());
    assert_eq!(drops.get(), 1);
    assert_eq!(cache.len(), 1);
    drop(seed);
    assert!(cache.is_empty());
}

#[test]
fn image_failure_releases_texture_document_and_preserves_error_chain() {
    let mut c = raster_case();
    c["fail"] = json!(true);
    let (cache, request, host, seed) = setup(&c);
    let document = Arc::downgrade(&request.document);
    let texture = cache
        .texture_loader(
            &request,
            json!({"index":0}),
            Context::new_webgl1(Host::default()),
            host.clone(),
            GltfTextureOptions::default(),
        )
        .unwrap();
    drop(request);
    let error = ready(texture.load()).unwrap_err();
    assert_eq!(error.message, "Failed to load texture\nFailed to load embedded image\ncodec failure");
    assert_eq!(texture.state(), ResourceLoaderState::Failed);
    assert_eq!(cache.len(), 2);
    assert!(document.upgrade().is_none());
    assert_eq!(ready(texture.load()).unwrap_err(), error);
    assert_eq!(host.log.borrow().len(), 1);
    drop(texture);
    drop(seed);
    assert!(cache.is_empty());
}
struct NeverTransport {
    cancelled: Cell<usize>,
}
impl ResourceTransport for NeverTransport {
    fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
        Box::pin(std::future::pending())
    }
    fn cancel(&self, _: &HttpRequest) {
        self.cancelled.set(self.cancelled.get() + 1);
    }
}
#[test]
fn final_uri_image_lease_cancels_the_scheduled_transport() {
    let c = raster_case();
    let (_, mut request, _, _) = setup(&c);
    Arc::make_mut(&mut request.document).json["images"][0] = json!({"uri":"pending.png"});
    let transport = Rc::new(NeverTransport { cancelled: Cell::new(0) });
    let client = ResourceClient::new(transport.clone());
    let cache = ResourceCache::new(client.clone());
    let host = Rc::new(SlowImages {
        base: Images { log: Rc::default(), fail: false, width: 3, height: 5 },
        drops: Rc::new(Cell::new(0)),
    });
    let image = cache.image_loader(&request, 0, host, formats(&c)).unwrap();
    let mut future = image.load();
    assert!(std::pin::Pin::new(&mut future).poll(&mut TaskContext::from_waker(Waker::noop())).is_pending());
    drop(image);
    assert!(!ready(future).unwrap());
    client.scheduler.borrow_mut().poll_update(&mut TaskContext::from_waker(Waker::noop()));
    assert_eq!(transport.cancelled.get(), 1);
    assert!(cache.is_empty());
}
