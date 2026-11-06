//! Public Scene-to-globe imagery picking, replayed against real Cesium methods.
#![allow(clippy::arc_with_non_send_sync)] // Existing provider API uses Arc with local hosts.
mod common;
use cesium_rs::scene::quadtree_tile::{QuadtreeTile, QuadtreeTileHandle, QuadtreeTileOptions};
use cesium_rs::{core::*, renderer::ContextCapabilities, scene::*};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};

type Calls = Rc<RefCell<Vec<Value>>>;
struct Provider {
    config: Value,
    scheme: GeographicTilingScheme,
    errors: ImageryErrorEvent,
    calls: Calls,
}
impl ImageryProvider for Provider {
    fn rectangle(&self) -> &Rectangle {
        self.scheme.rectangle()
    }
    fn tile_width(&self) -> f64 {
        256.
    }
    fn tile_height(&self) -> f64 {
        256.
    }
    fn maximum_level(&self) -> Option<u32> {
        None
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        &self.scheme
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.errors
    }
    fn request_image(&self, _: i32, _: i32, _: u32) -> Option<ImageryFuture> {
        None
    }
    fn pick_features(&self, x: i32, y: i32, level: u32, lon: f64, lat: f64) -> Option<ImageryFeatureFuture> {
        if self.config["support"] == false {
            return None;
        }
        self.calls.borrow_mut().push(json!([self.config["name"], x, y, level, lon, lat]));
        if self.config["response"] == "undefined" {
            return None;
        }
        let config = self.config.clone();
        let mut delay = config["delay"].as_u64().unwrap_or(0);
        Some(Box::pin(async move {
            std::future::poll_fn(move |cx| {
                if delay == 0 {
                    Poll::Ready(())
                } else {
                    delay -= 1;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            if config["response"] == "error" {
                return Err(RuntimeError::new(format!("{} rejected", config["name"].as_str().unwrap())));
            }
            if config["response"] == "none" {
                return Ok(None);
            }
            Ok(Some(config["features"].as_array().into_iter().flatten().map(feature).collect()))
        }))
    }
}
fn cartographic(v: &Value) -> Cartographic {
    Cartographic::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap())
}
fn rectangle(v: &Value) -> Rectangle {
    Rectangle::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap(), v[3].as_f64().unwrap())
}
fn feature(v: &Value) -> ImageryLayerFeatureInfo {
    let optional = |key: &str| v.get(key).filter(|v| !v.is_null()).cloned();
    ImageryLayerFeatureInfo {
        name: optional("name"),
        description: v["description"].as_str().map(str::to_owned),
        properties: optional("properties"),
        data: optional("data"),
        position: optional("position").map(|v| cartographic(&v)),
        imagery_layer: Some(FrameResource::new("old provider owner")),
    }
}
#[derive(Default)]
struct Backend;
impl SceneBackend for Backend {
    fn capabilities(&self) -> Result<ContextCapabilities, RuntimeError> {
        Ok(ContextCapabilities::default())
    }
    fn dimensions(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([64, 64])
    }
    fn begin_frame(&mut self, _: &FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn clear(&mut self, _: Color, _: Option<f64>, _: Option<i32>) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn set_frustum(&mut self, _: &FrameState, _: f64, _: f64) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn execute(&mut self, _: &SceneCommand, _: SceneSubmission) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn end_frame(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
struct Globe {
    point: Option<Cartesian3>,
    tiles: Vec<QuadtreeTileHandle>,
    hits: Rc<Cell<usize>>,
}
impl SceneGlobe for Globe {
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn render(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn get_height(&self, _: &Cartographic) -> Option<f64> {
        None
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn pick(&self, ray: &Ray, _: &FrameState) -> Option<Cartesian3> {
        assert_eq!(*ray, test_ray());
        self.hits.set(self.hits.get() + 1);
        self.point
    }
    fn rendered_surface_tiles(&self) -> &[QuadtreeTileHandle] {
        &self.tiles
    }
}
fn test_ray() -> Ray {
    Ray::new(Cartesian3::new(7_000_000., 0., 0.), Cartesian3::new(-1., 0., 0.))
}
fn new_scene() -> Scene<Backend> {
    Scene::new(Backend, CameraOptions::new(64., 64.), SceneOptions::default(), JulianDate::default()).unwrap()
}
fn tile(bounds: Rectangle, imagery: Vec<TileImagery>) -> QuadtreeTileHandle {
    let mut tile = QuadtreeTile::new(QuadtreeTileOptions {
        tiling_scheme: Arc::new(GeographicTilingScheme::default()),
        x: 0,
        y: 0,
        level: 0,
        parent: None,
    });
    tile.rectangle = bounds;
    let mut surface = GlobeSurfaceTile::new();
    surface.imagery = imagery;
    tile.data = Some(Box::new(surface));
    Rc::new(RefCell::new(tile))
}
fn attachment(
    layer: ImageryLayerHandle,
    bounds: Rectangle,
    coords: [i32; 3],
    texture: Cartesian4,
    ready: bool,
) -> TileImagery {
    let mut image = Imagery::new(coords[0], coords[1], coords[2] as u32, bounds, None);
    image.add_reference();
    image.state = ImageryState::Ready;
    let mut attachment = TileImagery::new_with_layer(Rc::new(RefCell::new(image)), texture, false, layer);
    if ready {
        attachment.ready_imagery = attachment.loading_imagery.take();
    }
    attachment
}
fn owner_name(owner: &FrameResource, layers: &[ImageryLayerHandle], configs: &Value) -> Value {
    let handle = owner.get::<ImageryLayerHandle>().expect("typed layer owner");
    let index = layers.iter().position(|l| Rc::ptr_eq(l, handle)).expect("original layer identity");
    configs[index]["name"].clone()
}
fn drive<T>(mut future: std::pin::Pin<Box<dyn std::future::Future<Output = T>>>) -> T {
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..64 {
        if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
            return result;
        }
    }
    panic!("pick did not settle");
}
#[test]
fn collection_picking_matches_pinned_cesium() {
    let fixture: Value = serde_json::from_str(include_str!("data/scene-imagery-picking.json")).unwrap();
    assert_eq!(fixture["scenarios"].as_array().unwrap().len(), 33);
    for scenario in fixture["scenarios"].as_array().unwrap() {
        let calls: Calls = Rc::default();
        let mut collection = ImageryLayerCollection::new();
        let mut layers = Vec::new();
        for config in scenario["layers"].as_array().unwrap() {
            let provider = Arc::new(Provider {
                config: config.clone(),
                scheme: Default::default(),
                errors: Rc::new(RefCell::new(MutableErrorEvent::new())),
                calls: calls.clone(),
            });
            let options = ImageryLayerOptions {
                show: config["show"].as_bool(),
                alpha: config["alpha"].as_f64(),
                ..Default::default()
            };
            let layer = ImageryLayer::handle(
                (config["ready"] != false).then_some(provider as Arc<dyn ImageryProvider>),
                options,
            );
            if config["inCollection"] != false {
                collection.add(layer.clone(), None).unwrap();
            }
            layers.push(layer);
        }
        let tiles = scenario["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let imagery = t["imagery"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| {
                        let coords = [
                            a["coordinates"][0].as_i64().unwrap() as i32,
                            a["coordinates"][1].as_i64().unwrap() as i32,
                            a["coordinates"][2].as_i64().unwrap() as i32,
                        ];
                        let v = &a["texture"];
                        let texture = Cartesian4::new(
                            v[0].as_f64().unwrap(),
                            v[1].as_f64().unwrap(),
                            v[2].as_f64().unwrap(),
                            v[3].as_f64().unwrap(),
                        );
                        attachment(
                            layers[a["layer"].as_u64().unwrap() as usize].clone(),
                            rectangle(&a["rectangle"]),
                            coords,
                            texture,
                            a["ready"] != false,
                        )
                    })
                    .collect();
                tile(rectangle(&t["rectangle"]), imagery)
            })
            .collect();
        let ellipsoid = if let Some(r) = scenario["ellipsoid"].as_array() {
            Ellipsoid::from_cartesian3(Cartesian3::new(
                r[0].as_f64().unwrap(),
                r[1].as_f64().unwrap(),
                r[2].as_f64().unwrap(),
            ))
        } else {
            Ellipsoid::WGS84
        };
        let hits = Rc::new(Cell::new(0));
        let mut scene = new_scene();
        scene.camera.ellipsoid = ellipsoid; // Projection is deliberately WGS84, independently of the scene ellipsoid.
        scene.globe = Some(Box::new(Globe {
            point: (scenario["hit"] != false)
                .then(|| ellipsoid.cartographic_to_cartesian(&cartographic(&scenario["location"]))),
            tiles,
            hits: hits.clone(),
        }));
        let picked = collection.pick_imagery_layers(&test_ray(), &scene);
        let picked = picked.map(|p| {
            p.iter()
                .map(|layer| owner_name(&FrameResource::new(layer.clone()), &layers, &scenario["layers"]))
                .collect::<Vec<_>>()
        });
        let future = collection.pick_imagery_layer_features(&test_ray(), &scene);
        let has_future = future.is_some();
        let mut features = None;
        let mut error = None;
        if let Some(future) = future {
            match drive(future) {
                Ok(result) => {
                    features=Some(result.iter().map(|f|json!({"name":f.name,"description":f.description,"properties":f.properties,"data":f.data,"position":f.position.map(|p|[p.longitude,p.latitude,p.height]),"layer":owner_name(f.imagery_layer.as_ref().unwrap(),&layers,&scenario["layers"])})).collect::<Vec<_>>());
                }
                Err(e) => {
                    error = Some(e.message);
                }
            }
        }
        let actual = json!({"layers":picked,"hasFuture":has_future,"features":features,"error":error,"calls":*calls.borrow(),"hits":hits.get()});
        assert!(
            common::close(&actual, &scenario["expected"], common::Tol { rel: 1e-12, abs: 1e-8, vector: false }),
            "{}\nactual={actual}\nexpected={}",
            scenario["name"],
            scenario["expected"]
        );
    }
}

struct DeferredProvider {
    base: Provider,
    future: RefCell<Option<ImageryFeatureFuture>>,
    on_pick: Option<Rc<dyn Fn()>>,
}
impl ImageryProvider for DeferredProvider {
    fn rectangle(&self) -> &Rectangle {
        self.base.rectangle()
    }
    fn tile_width(&self) -> f64 {
        256.
    }
    fn tile_height(&self) -> f64 {
        256.
    }
    fn maximum_level(&self) -> Option<u32> {
        None
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.base.tiling_scheme()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        self.base.error_event()
    }
    fn request_image(&self, _: i32, _: i32, _: u32) -> Option<ImageryFuture> {
        None
    }
    fn pick_features(&self, x: i32, y: i32, level: u32, lon: f64, lat: f64) -> Option<ImageryFeatureFuture> {
        self.base.calls.borrow_mut().push(json!([self.base.config["name"], x, y, level, lon, lat]));
        if let Some(callback) = &self.on_pick {
            callback();
        }
        self.future.borrow_mut().take()
    }
}
type FeatureResult = Result<Option<Vec<ImageryLayerFeatureInfo>>, RuntimeError>;
fn deferred_layer(
    name: &str,
    calls: Calls,
    on_pick: Option<Rc<dyn Fn()>>,
) -> (ImageryLayerHandle, futures_channel::oneshot::Sender<FeatureResult>) {
    let (sender, receiver) = futures_channel::oneshot::channel();
    let provider = DeferredProvider {
        base: Provider {
            config: json!({"name":name}),
            scheme: Default::default(),
            errors: Rc::new(RefCell::new(MutableErrorEvent::new())),
            calls,
        },
        future: RefCell::new(Some(Box::pin(async move { receiver.await.expect("test sender must settle") }))),
        on_pick,
    };
    (ImageryLayer::handle(Some(Arc::new(provider)), Default::default()), sender)
}
fn setup(layers: &[ImageryLayerHandle]) -> (ImageryLayerCollection, Scene<Backend>) {
    let mut collection = ImageryLayerCollection::new();
    for layer in layers {
        collection.add(layer.clone(), None).unwrap();
    }
    let tiles = vec![tile(
        Rectangle::new(-1., -1., 1., 1.),
        layers
            .iter()
            .map(|layer| {
                attachment(layer.clone(), Rectangle::MAX_VALUE, [0, 0, 0], Cartesian4::new(0., 0., 1., 1.), true)
            })
            .collect(),
    )];
    let mut scene = new_scene();
    scene.globe = Some(Box::new(Globe {
        point: Some(Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::new(0.2, 0.3, 12.))),
        tiles,
        hits: Rc::new(Cell::new(0)),
    }));
    (collection, scene)
}
fn features(name: &str) -> FeatureResult {
    Ok(Some(vec![ImageryLayerFeatureInfo { name: Some(json!(name)), ..Default::default() }]))
}

#[test]
fn starts_all_providers_eagerly_and_preserves_order_when_responses_arrive_out_of_order() {
    let calls: Calls = Rc::default();
    let (bottom, bottom_response) = deferred_layer("bottom", calls.clone(), None);
    let (top, top_response) = deferred_layer("top", calls.clone(), None);
    let layers = vec![bottom, top];
    let (collection, scene) = setup(&layers);
    let mut future = collection.pick_imagery_layer_features(&test_ray(), &scene).unwrap();
    assert_eq!(
        calls.borrow().iter().map(|call| call[0].clone()).collect::<Vec<_>>(),
        vec![json!("top"), json!("bottom")]
    );
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    bottom_response.send(features("bottom")).unwrap();
    assert!(future.as_mut().poll(&mut cx).is_pending());
    top_response.send(features("top")).unwrap();
    let features = drive(future).unwrap();
    assert_eq!(
        features.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        vec![Some(json!("top")), Some(json!("bottom"))]
    );
    assert!(Rc::ptr_eq(features[0].imagery_layer.as_ref().unwrap().get::<ImageryLayerHandle>().unwrap(), &layers[1]));
}
#[test]
fn provider_failure_rejects_without_waiting_for_an_earlier_pending_result() {
    let calls: Calls = Rc::default();
    let (bottom, response) = deferred_layer("bottom", calls.clone(), None);
    let (top, still_pending) = deferred_layer("top", calls, None);
    let (collection, scene) = setup(&[bottom, top]);
    let mut future = collection.pick_imagery_layer_features(&test_ray(), &scene).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    response.send(Err(RuntimeError::new("bottom failed"))).unwrap();
    assert_eq!(drive(future).unwrap_err().message, "bottom failed");
    assert!(still_pending.is_canceled()); // Native RAII releases outstanding requests on failure.
}
#[test]
fn dropping_pending_pick_releases_removed_layers_and_provider_requests() {
    let (layer, response) = deferred_layer("layer", Rc::default(), None);
    let weak = Rc::downgrade(&layer);
    let (mut collection, mut scene) = setup(std::slice::from_ref(&layer));
    let future = collection.pick_imagery_layer_features(&test_ray(), &scene).unwrap();
    collection.remove_all(Some(false));
    scene.globe = None;
    drop(layer);
    assert!(weak.upgrade().is_some());
    drop(future);
    assert!(weak.upgrade().is_none());
    assert!(response.is_canceled());
}
#[test]
fn completed_features_retain_original_layer_identity_after_removal() {
    let (layer, response) = deferred_layer("layer", Rc::default(), None);
    let weak = Rc::downgrade(&layer);
    let (mut collection, mut scene) = setup(std::slice::from_ref(&layer));
    let future = collection.pick_imagery_layer_features(&test_ray(), &scene).unwrap();
    response.send(features("feature")).unwrap();
    let result = drive(future).unwrap();
    collection.remove_all(Some(false));
    scene.globe = None;
    drop(layer);
    let owner = result[0].imagery_layer.as_ref().unwrap().get::<ImageryLayerHandle>().unwrap();
    assert!(Rc::ptr_eq(owner, &weak.upgrade().unwrap()));
    drop(result);
    assert!(weak.upgrade().is_none());
}
#[test]
fn provider_callback_can_mutate_surface_without_an_outstanding_tile_borrow() {
    let surface: Rc<RefCell<Option<QuadtreeTileHandle>>> = Rc::default();
    let callback_surface = surface.clone();
    let callback = Rc::new(move || {
        callback_surface.borrow().as_ref().unwrap().borrow_mut().distance = 42.;
    });
    let (layer, response) = deferred_layer("layer", Rc::default(), Some(callback));
    let (collection, scene) = setup(&[layer]);
    let tile = scene.globe.as_ref().unwrap().rendered_surface_tiles()[0].clone();
    *surface.borrow_mut() = Some(tile.clone());
    let future = collection.pick_imagery_layer_features(&test_ray(), &scene).unwrap();
    assert_eq!(tile.borrow().distance, 42.);
    response.send(features("feature")).unwrap();
    assert_eq!(drive(future).unwrap().len(), 1);
}
#[test]
fn missing_globe_does_not_substitute_an_ellipsoid_hit() {
    let (layer, response) = deferred_layer("layer", Rc::default(), None);
    let (collection, mut scene) = setup(&[layer]);
    scene.globe = None;
    assert!(collection.pick_imagery_layers(&test_ray(), &scene).is_none());
    assert!(collection.pick_imagery_layer_features(&test_ray(), &scene).is_none());
    assert!(!response.is_canceled());
}
#[test]
fn stable_layer_ids_resolve_attachments_after_collection_reordering() {
    let (bottom, _) = deferred_layer("bottom", Rc::default(), None);
    let (top, _) = deferred_layer("top", Rc::default(), None);
    let (mut collection, scene) = setup(&[bottom.clone(), top.clone()]);
    let tile = scene.globe.as_ref().unwrap().rendered_surface_tiles()[0].clone();
    {
        let mut tile = tile.borrow_mut();
        let surface = tile.data.as_mut().unwrap().as_any_mut().downcast_mut::<GlobeSurfaceTile>().unwrap();
        for imagery in &mut surface.imagery {
            imagery.imagery_layer = None;
        }
    }
    collection.raise_to_top(&bottom).unwrap();
    let selected = collection.pick_imagery_layers(&test_ray(), &scene).unwrap();
    assert!(Rc::ptr_eq(&selected[0], &top));
    assert!(Rc::ptr_eq(&selected[1], &bottom));
}
