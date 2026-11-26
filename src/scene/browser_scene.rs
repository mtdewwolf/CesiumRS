//! Browser WebAssembly scene facade over `Scene<RendererSceneBackend<BrowserGl>>`.
//!
//! Exposes a coarse-grained, typed-array-friendly `BrowserScene` over
//! `wasm-bindgen` so browser hosts (such as `gods-eye-view`'s renderer
//! adapter) can drive `CesiumRS`'s real WebGL2 `Scene`, `Globe`, imagery
//! layers, primitive collections, camera, and GPU picking without mirroring
//! CesiumJS's JavaScript object graph across the WASM boundary.

use super::{
    billboard::{Billboard, BillboardImage, BillboardOptions},
    billboard_collection::{BillboardCollection, BillboardCollectionOptions},
    blending_state,
    brdf_lut_generator::BrdfLutGenerator,
    camera::{CameraFrustum, CameraOptions},
    camera_event_type::CameraEventType,
    cesium_3d_tileset::{Cesium3DTileset, TilesetOptions},
    clipping_plane::ClippingPlane,
    clipping_plane_collection::ClippingPlaneCollection,
    ellipsoid_surface_appearance::{EllipsoidSurfaceAppearance, EllipsoidSurfaceAppearanceOptions},
    globe::{Globe, GlobeOptions},
    globe_surface_tile::GlobeSurfaceTile,
    globe_surface_tile_provider::GlobeTerrainProviderAdapter,
    imagery_layer::{ImageryLayer, ImageryLayerHandle, ImageryLayerOptions},
    imagery_provider::{ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider},
    imagery_state::ImageryState,
    material::{
        Fabric, Material, MaterialImage, MaterialOptions, MaterialValue, COLOR_TYPE, POLYLINE_DASH_TYPE,
        POLYLINE_GLOW_TYPE, POLYLINE_OUTLINE_TYPE,
    },
    model::{
        model_clipping_planes_pipeline_stage::ModelClippingPlanesState,
        picking_pipeline_stage::ModelPickObject,
        CustomShader, CustomShaderMode, CustomShaderOptions, CustomShaderUniformValue, LightingModel, Model,
        ModelOptions, ModelTileFactory, TextureUniform, UniformType, VaryingType,
    },
    per_instance_color_appearance::{PerInstanceColorAppearance, PerInstanceColorAppearanceOptions},
    pick_framebuffer::PickedObject,
    pick_object::ScenePickObject,
    point_primitive::{PointPrimitive, PointPrimitiveOptions},
    point_primitive_collection::{PointPrimitiveCollection, PointPrimitiveCollectionOptions},
    polyline::{Polyline, PolylineOptions, PolylinePick},
    polyline_collection::{PolylineCollection, PolylineCollectionOptions},
    post_process_stage::{PostProcessStage, PostProcessStageOptions, PostProcessUniformValue},
    post_process_stage_collection::{PostProcessSceneEffect, PostProcessStageCollection},
    post_process_stage_composite::PostProcessStageItem,
    primitive::{Primitive, PrimitiveOptions, PrimitivePickObject},
    resource_cache::ResourceCache,
    scene::{RendererSceneBackend, Scene, SceneGlobe, SceneOptions, ScenePrimitive},
    screen_space_camera_controller::{CameraInputBinding, ControllerEnvironment, ScreenSpaceCameraController},
    single_tile_imagery_provider::{SingleTileImageryProvider, SingleTileImageryProviderOptions},
    sky_atmosphere::SkyAtmosphere,
    split_direction::SplitDirection,
    tileset_content::{RenderableTileFactory, TilesetResourceLoader},
    url_template_imagery_provider::{UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions},
    BlendOption, Cesium3DTilePass, ColorBlendMode, FrameResource, FrameState, GltfJsonInput, HorizontalOrigin,
    MetadataValue, VerticalOrigin,
};
use crate::{
    core::{
        browser_host::{BrowserImageHost, BrowserTransport},
        cesium_terrain_provider::{CesiumTerrainOptions, CesiumTerrainProvider},
        color_geometry_instance_attribute::ColorGeometryInstanceAttribute,
        ellipse_geometry::{EllipseGeometry, EllipseGeometryOptions},
        ellipsoid_terrain_provider::EllipsoidTerrainProvider,
        geometry_instance::GeometryInstance,
        intersection_tests::ray_ellipsoid,
        ion::Ion,
        ion_resource::IonAssetOptions,
        occluder::Occluder,
        polygon_geometry::{PolygonGeometry, PolygonGeometryOptions},
        polygon_hierarchy::PolygonHierarchy,
        rectangle_geometry::{RectangleGeometry, RectangleGeometryOptions},
        sample_terrain::{sample_terrain_most_detailed, TerrainSamplingHost},
        terrain_provider::TerrainProvider,
        wall_geometry::{WallGeometry, WallGeometryOptions},
        webgl_constants as gl, BoundingSphere, Cartesian2, Cartesian3, Cartesian4, Cartographic, Color, Credit,
        DistanceDisplayCondition, Ellipsoid, GeographicTilingScheme, ImageHost, JulianDate, KeyboardEventModifier,
        Matrix4, MutableErrorEvent, NearFarScalar, PixelDatatype, PixelFormat, Ray, Rectangle, Resource,
        ResourceClient, RuntimeError, ScreenSpaceEventType, ScreenSpaceInputEvent, TilingScheme,
        WebMercatorTilingScheme,
    },
    renderer::{
        browser::BrowserGl,
        render_state::{Blending, DepthTest, RenderState},
        Context, Gl, Pass, Sampler, Texture, TextureBinding, TextureMagnificationFilter, TextureMinificationFilter,
        TextureOptions, TextureWrap,
    },
};
use super::gltf_pipeline::BufferSource;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{future_to_promise, JsFuture};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(s: &str);
}

fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            console_error(&format!("[CesiumRS panic] {info}"));
        }));
    });
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn epoch_ms_to_julian(ms: f64) -> JulianDate {
    JulianDate::from_date(if ms.is_finite() { ms } else { 0.0 })
}

fn color_from_rgba(r: f64, g: f64, b: f64, a: f64) -> Color {
    Color::new(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0))
}

fn encode_pick_id(set_id: &str, item_id: &str) -> String {
    let mut s = String::with_capacity(set_id.len() + 1 + item_id.len());
    s.push_str(set_id);
    s.push('\0');
    s.push_str(item_id);
    s
}

fn encode_unpickable_id(set_id: &str, item_id: &str) -> String {
    let mut s = String::with_capacity(set_id.len() + 2 + item_id.len());
    s.push('\x01');
    s.push_str(set_id);
    s.push('\0');
    s.push_str(item_id);
    s
}

fn encode_item_pick_id(set_id: &str, item_id: &str, pickable: bool) -> String {
    if pickable {
        encode_pick_id(set_id, item_id)
    } else {
        encode_unpickable_id(set_id, item_id)
    }
}

fn extract_pick_id(obj: &PickedObject) -> Option<String> {
    if let Some(pick) = obj.downcast_ref::<ScenePickObject>() {
        if let Some(MetadataValue::String(id)) = &pick.id {
            return Some(id.clone());
        }
    }
    if let Some(pick) = obj.downcast_ref::<PolylinePick>() {
        if let Some(id) = &pick.id {
            return Some(id.clone());
        }
    }
    if let Some(pick) = obj.downcast_ref::<PrimitivePickObject>() {
        if let Some(id) = &pick.id {
            return Some(id.clone());
        }
    }
    if let Some(pick) = obj.downcast_ref::<ModelPickObject>() {
        if let Some(id) = &pick.model_id {
            return Some(id.clone());
        }
    }
    None
}

/// Shared wrapper so `BrowserScene` keeps typed mutable access to `Globe`
/// while `Scene` drives it via `Box<dyn SceneGlobe>`.
struct SharedGlobe(Rc<RefCell<Globe<BrowserGl>>>);

impl SceneGlobe for SharedGlobe {
    fn show(&self) -> bool {
        SceneGlobe::show(&*self.0.borrow())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        SceneGlobe::update(&mut *self.0.borrow_mut(), frame)
    }
    fn render(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        SceneGlobe::render(&mut *self.0.borrow_mut(), frame)
    }
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Option<Cartesian3> {
        SceneGlobe::pick(&*self.0.borrow(), ray, frame)
    }
    fn get_height(&self, cartographic: &Cartographic) -> Option<f64> {
        SceneGlobe::get_height(&*self.0.borrow(), cartographic)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        SceneGlobe::destroy(&mut *self.0.borrow_mut())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SetBlendMode {
    Default,
    Opaque,
    Translucent,
    Additive,
}

fn apply_blend_override(frame: &mut FrameState, start: usize, blend: SetBlendMode) {
    if blend == SetBlendMode::Default || frame.passes.pick {
        return;
    }
    let Some(context) = frame.context.as_ref().and_then(|c| c.get::<Context<BrowserGl>>()).cloned() else {
        return;
    };
    for cmd in &mut frame.command_list[start..] {
        if let Some(mut derived) = cmd.derive_draw(context.clone(), |opts, _| match blend {
            SetBlendMode::Opaque => {
                opts.state.blending = Blending::default();
                opts.state.depth_mask = true;
                opts.state.depth_test.enabled = true;
            }
            SetBlendMode::Translucent => {
                opts.state.blending = blending_state::alpha_blend();
                opts.state.depth_mask = false;
                opts.state.depth_test.enabled = true;
            }
            SetBlendMode::Additive => {
                opts.state.blending = blending_state::additive_blend();
                opts.state.depth_mask = false;
                opts.state.depth_test.enabled = true;
            }
            SetBlendMode::Default => {}
        }) {
            derived.pass = match blend {
                SetBlendMode::Opaque => Pass::Opaque,
                SetBlendMode::Translucent | SetBlendMode::Additive => Pass::Translucent,
                SetBlendMode::Default => derived.pass,
            };
            *cmd = derived;
        }
    }
}

struct SharedPointCollection {
    collection: Rc<RefCell<PointPrimitiveCollection<BrowserGl>>>,
    pickable: Rc<Cell<bool>>,
    blend: Rc<Cell<SetBlendMode>>,
}

impl ScenePrimitive for SharedPointCollection {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().pre_passes_update(frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if frame.passes.pick && !self.pickable.get() {
            return Ok(());
        }
        let start = frame.command_list.len();
        self.collection.borrow_mut().update(frame)?;
        apply_blend_override(frame, start, self.blend.get());
        Ok(())
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().post_passes_update(frame)
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().update_for_pass(frame, pass)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().destroy()
    }
}

struct SharedBillboardCollection {
    collection: Rc<RefCell<BillboardCollection<BrowserGl>>>,
    pickable: Rc<Cell<bool>>,
    blend: Rc<Cell<SetBlendMode>>,
}

impl ScenePrimitive for SharedBillboardCollection {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().pre_passes_update(frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if frame.passes.pick && !self.pickable.get() {
            return Ok(());
        }
        let start = frame.command_list.len();
        self.collection.borrow_mut().update(frame)?;
        apply_blend_override(frame, start, self.blend.get());
        Ok(())
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().post_passes_update(frame)
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().update_for_pass(frame, pass)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().destroy()
    }
}

struct SharedPolylineCollection {
    collection: Rc<RefCell<PolylineCollection<BrowserGl>>>,
    pickable: Rc<Cell<bool>>,
    blend: Rc<Cell<SetBlendMode>>,
}

impl ScenePrimitive for SharedPolylineCollection {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().pre_passes_update(frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if frame.passes.pick && !self.pickable.get() {
            return Ok(());
        }
        let start = frame.command_list.len();
        self.collection.borrow_mut().update(frame)?;
        apply_blend_override(frame, start, self.blend.get());
        Ok(())
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().post_passes_update(frame)
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().update_for_pass(frame, pass)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.collection.borrow_mut().destroy()
    }
}

struct ShapeSetState {
    show: bool,
    primitives: Vec<Primitive<BrowserGl>>,
}

struct SharedShapeSet {
    state: Rc<RefCell<ShapeSetState>>,
    pickable: Rc<Cell<bool>>,
    blend: Rc<Cell<SetBlendMode>>,
}

impl ScenePrimitive for SharedShapeSet {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        if !s.show {
            return Ok(());
        }
        for p in &mut s.primitives {
            p.pre_passes_update(frame)?;
        }
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if frame.passes.pick && !self.pickable.get() {
            return Ok(());
        }
        let start = frame.command_list.len();
        {
            let mut s = self.state.borrow_mut();
            if !s.show {
                return Ok(());
            }
            for p in &mut s.primitives {
                p.update(frame)?;
            }
        }
        apply_blend_override(frame, start, self.blend.get());
        Ok(())
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        if !s.show {
            return Ok(());
        }
        for p in &mut s.primitives {
            p.post_passes_update(frame)?;
        }
        Ok(())
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        if !s.show {
            return Ok(());
        }
        for p in &mut s.primitives {
            p.update_for_pass(frame, pass)?;
        }
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        for p in &mut s.primitives {
            let _ = p.destroy();
        }
        s.primitives.clear();
        Ok(())
    }
}

struct PointSetEntry {
    collection: Rc<RefCell<PointPrimitiveCollection<BrowserGl>>>,
    items: HashMap<String, PointPrimitive>,
}

struct IconSetEntry {
    collection: Rc<RefCell<BillboardCollection<BrowserGl>>>,
    items: HashMap<String, Billboard<BrowserGl>>,
    uploaded_images: HashSet<String>,
}

struct LineSetEntry {
    collection: Rc<RefCell<PolylineCollection<BrowserGl>>>,
    items: HashMap<String, Vec<Polyline>>,
}

struct ModelItemEntry {
    model: Rc<RefCell<Model<BrowserGl>>>,
    parts_shader: Option<Rc<CustomShader<BrowserGl>>>,
    parts_rows: u32,
    parts_key: String,
    base_shader: Option<Rc<CustomShader<BrowserGl>>>,
    base_shader_key: String,
}

struct ModelSetState {
    show: bool,
    items: HashMap<String, ModelItemEntry>,
}

struct SharedModelSet {
    state: Rc<RefCell<ModelSetState>>,
    pickable: Rc<Cell<bool>>,
    blend: Rc<Cell<SetBlendMode>>,
}

impl ScenePrimitive for SharedModelSet {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let s = self.state.borrow();
        if !s.show {
            return Ok(());
        }
        for entry in s.items.values() {
            entry.model.borrow_mut().pre_passes_update(frame)?;
        }
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if frame.passes.pick && !self.pickable.get() {
            return Ok(());
        }
        let start = frame.command_list.len();
        {
            let s = self.state.borrow();
            if !s.show {
                return Ok(());
            }
            for entry in s.items.values() {
                entry.model.borrow_mut().update(frame)?;
            }
        }
        apply_blend_override(frame, start, self.blend.get());
        Ok(())
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let s = self.state.borrow();
        if !s.show {
            return Ok(());
        }
        for entry in s.items.values() {
            entry.model.borrow_mut().post_passes_update(frame)?;
        }
        Ok(())
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        let s = self.state.borrow();
        if !s.show {
            return Ok(());
        }
        for entry in s.items.values() {
            entry.model.borrow_mut().update_for_pass(frame, pass)?;
        }
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        for entry in s.items.values() {
            let _ = entry.model.borrow_mut().destroy();
        }
        s.items.clear();
        Ok(())
    }
}

struct RasterShellState {
    show: bool,
    height_m: f64,
    primitive: Primitive<BrowserGl>,
}

struct SharedRasterShell {
    state: Rc<RefCell<RasterShellState>>,
}

impl ScenePrimitive for SharedRasterShell {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        if !s.show {
            return Ok(());
        }
        s.primitive.pre_passes_update(frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if frame.passes.pick {
            return Ok(());
        }
        let mut s = self.state.borrow_mut();
        if !s.show {
            return Ok(());
        }
        s.primitive.update(frame)
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        if !s.show {
            return Ok(());
        }
        s.primitive.post_passes_update(frame)
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        if !s.show {
            return Ok(());
        }
        s.primitive.update_for_pass(frame, pass)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        let mut s = self.state.borrow_mut();
        let _ = s.primitive.destroy();
        Ok(())
    }
}

enum DrawSetHandle {
    Points(PointSetEntry),
    Icons(IconSetEntry),
    Lines(LineSetEntry),
    Shapes(Rc<RefCell<ShapeSetState>>),
    Models(Rc<RefCell<ModelSetState>>),
}

struct DrawSetEntry {
    draw_order: f64,
    seq: u64,
    pickable: Rc<Cell<bool>>,
    include_in_height_queries: Rc<Cell<bool>>,
    blend: Rc<Cell<SetBlendMode>>,
    handle: DrawSetHandle,
}

struct SharedSkyAtmosphere {
    sky: Rc<RefCell<SkyAtmosphere<BrowserGl>>>,
    globe: Rc<RefCell<Globe<BrowserGl>>>,
}

impl ScenePrimitive for SharedSkyAtmosphere {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let globe_show = self.globe.borrow().show;
        if let Some(cmd) = self.sky.borrow_mut().update(frame, Some(globe_show))? {
            frame.command_list.push(cmd);
        }
        Ok(())
    }
    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, _frame: &mut FrameState, _pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.sky.borrow_mut().destroy();
        Ok(())
    }
}

struct SharedTileset(Rc<RefCell<Cesium3DTileset>>);

impl ScenePrimitive for SharedTileset {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        ScenePrimitive::pre_passes_update(&mut *self.0.borrow_mut(), frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        ScenePrimitive::update(&mut *self.0.borrow_mut(), frame)
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        ScenePrimitive::post_passes_update(&mut *self.0.borrow_mut(), frame)
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        ScenePrimitive::update_for_pass(&mut *self.0.borrow_mut(), frame, pass)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        ScenePrimitive::destroy(&mut *self.0.borrow_mut())
    }
}

struct ImageryEntry {
    handle: ImageryLayerHandle,
    on_tileset: bool,
}

/// JS-callback-backed `ImageryProvider` powering `imageHooks` (`canvas`,
/// `tileTransform`, `admit`, `fetchTile`), `vectorBase`, `imageryFrames`
/// (`frames`, `video`), and `imageryDecoded`.
struct JsCallbackImageryProvider {
    callback: js_sys::Function,
    tiling_scheme: Arc<dyn TilingScheme>,
    rectangle: Rectangle,
    tile_width: f64,
    tile_height: f64,
    min_level: u32,
    max_level: Option<u32>,
    credit: Option<Credit>,
    error_event: ImageryErrorEvent,
}

fn js_val_message(val: JsValue) -> String {
    if let Some(s) = val.as_string() {
        return s;
    }
    if let Ok(msg) = js_sys::Reflect::get(&val, &JsValue::from_str("message")) {
        if let Some(s) = msg.as_string() {
            return s;
        }
    }
    format!("{val:?}")
}

impl ImageryProvider for JsCallbackImageryProvider {
    fn rectangle(&self) -> &Rectangle {
        &self.rectangle
    }
    fn tile_width(&self) -> f64 {
        self.tile_width
    }
    fn tile_height(&self) -> f64 {
        self.tile_height
    }
    fn maximum_level(&self) -> Option<u32> {
        self.max_level
    }
    fn minimum_level(&self) -> u32 {
        self.min_level
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.tiling_scheme.as_ref()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }
    fn has_alpha_channel(&self) -> bool {
        true
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let res = self.callback.call3(&JsValue::NULL, &JsValue::from(x), &JsValue::from(y), &JsValue::from(level));
        match res {
            Err(err) => Some(Box::pin(async move { Err(RuntimeError::new(js_val_message(err))) })),
            Ok(val) => {
                if val.is_undefined() || val.is_null() {
                    return None;
                }
                if val.as_string().as_deref() == Some("missing") || val.as_bool() == Some(false) {
                    return Some(Box::pin(async { Err(RuntimeError::new("MissingTile")) }));
                }
                if let Ok(promise) = val.clone().dyn_into::<js_sys::Promise>() {
                    return Some(Box::pin(async move {
                        match JsFuture::from(promise).await {
                            Ok(img) => {
                                if img.is_undefined() || img.is_null() {
                                    Err(RuntimeError::new("MissingTile"))
                                } else {
                                    Ok(ImageryImage::Image(FrameResource::new(img)))
                                }
                            }
                            Err(err) => Err(RuntimeError::new(js_val_message(err))),
                        }
                    }));
                }
                Some(Box::pin(async move { Ok(ImageryImage::Image(FrameResource::new(val))) }))
            }
        }
    }
}

struct BrowserSamplingHost;

impl TerrainSamplingHost for BrowserSamplingHost {
    type Delay<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;
    fn delay(&self, milliseconds: u32) -> Self::Delay<'_> {
        Box::pin(async move {
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                if let Ok(set_timeout) = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("setTimeout")) {
                    if let Ok(f) = set_timeout.dyn_into::<js_sys::Function>() {
                        let _ = f.call2(&JsValue::NULL, &resolve, &JsValue::from(milliseconds));
                        return;
                    }
                }
                let _ = resolve.call0(&JsValue::NULL);
            });
            let _ = JsFuture::from(promise).await;
        })
    }
}

fn split_direction_from_i32(dir: i32) -> SplitDirection {
    if dir < 0 {
        SplitDirection::Left
    } else if dir > 0 {
        SplitDirection::Right
    } else {
        SplitDirection::None
    }
}

fn hillshade_material(has_terrain_normals: bool, strength: f64) -> Result<Material, RuntimeError> {
    let normal_expr = if has_terrain_normals {
        "vec3 normal = normalize(materialInput.normalEC);"
    } else {
        "vec3 worldPosition = (czm_inverseView * vec4(-materialInput.positionToEyeEC, 1.0)).xyz;\n\
         vec3 geodeticNormal = normalize(worldPosition * czm_ellipsoidInverseRadii * czm_ellipsoidInverseRadii);\n\
         vec3 normal = normalize(czm_viewRotation * geodeticNormal);"
    };
    let source = format!(
        "czm_material czm_getMaterial(czm_materialInput materialInput)\n\
         {{\n\
             czm_material material = czm_getDefaultMaterial(materialInput);\n\
             {normal_expr}\n\
             vec3 displayLight = normalize(vec3(-0.45, 0.55, 0.8));\n\
             float shade = clamp((1.0 - dot(normal, displayLight)) * strength * (0.32 / 0.28), 0.0, strength);\n\
             material.diffuse = vec3(0.0);\n\
             material.alpha = shade;\n\
             return material;\n\
         }}"
    );
    let type_name = if has_terrain_normals { "GEVHillshadeTerrain" } else { "GEVHillshadeCurvature" };
    let fabric = Fabric::new(type_name)
        .uniform("strength", MaterialValue::Float(strength.clamp(0.0, 1.0)))
        .source(source);
    Material::try_new(MaterialOptions { fabric, translucent: Some(true), ..Default::default() })
}

/// Async-loaded terrain handle applied synchronously via `BrowserScene::apply_loaded_terrain`.
#[wasm_bindgen]
pub struct LoadedTerrain {
    provider: Rc<CesiumTerrainProvider>,
    has_vertex_normals: bool,
}

#[wasm_bindgen]
impl LoadedTerrain {
    pub fn has_vertex_normals(&self) -> bool {
        self.has_vertex_normals
    }
}

/// Async-loaded 3D Tileset handle applied synchronously via `BrowserScene::set_photoreal_tileset`.
#[wasm_bindgen]
pub struct LoadedTileset {
    tileset: Rc<RefCell<Cesium3DTileset>>,
}

/// Async-loaded glTF/GLB model handle applied synchronously via `BrowserScene::attach_model`.
#[wasm_bindgen]
pub struct LoadedModel {
    model: Rc<RefCell<Model<BrowserGl>>>,
}

#[derive(serde::Deserialize)]
struct JsModelOptions {
    model_matrix: [f64; 16],
    #[serde(default = "default_one")]
    scale: f64,
    #[serde(default)]
    min_pixel_size: f64,
    #[serde(default)]
    max_scale: Option<f64>,
    #[serde(default = "default_true")]
    show: bool,
    #[serde(default = "default_true")]
    pickable: bool,
    #[serde(default)]
    color: Option<[f64; 4]>,
    #[serde(default)]
    color_blend_mode: Option<String>,
    #[serde(default)]
    color_blend_amount: Option<f64>,
    #[serde(default)]
    shading: Option<String>,
    #[serde(default)]
    mask_color: Option<[f64; 3]>,
    #[serde(default)]
    clip_below_origin: bool,
    #[serde(default)]
    display_range: Option<[f64; 2]>,
    #[serde(default = "default_true")]
    environment_map: bool,
}

#[derive(serde::Deserialize)]
struct JsIconPatch {
    id: String,
    #[serde(default)]
    pos: Option<[f64; 3]>,
    #[serde(default)]
    rotation: Option<f64>,
    #[serde(default)]
    scale: Option<f64>,
    #[serde(default)]
    color: Option<[f64; 4]>,
    #[serde(default)]
    show: Option<bool>,
}

#[derive(serde::Deserialize)]
struct JsPointPatch {
    id: String,
    #[serde(default)]
    pos: Option<[f64; 3]>,
    #[serde(default)]
    pixel_size: Option<f64>,
    #[serde(default)]
    color: Option<[f64; 4]>,
    #[serde(default)]
    outline_color: Option<[f64; 4]>,
    #[serde(default)]
    show: Option<bool>,
}

const RASTER_SHELL_MATERIAL_SOURCE: &str = r#"
czm_material czm_getMaterial(czm_materialInput materialInput) {
  czm_material material = czm_getDefaultMaterial(materialInput);
  vec2 st = materialInput.st;
  bool inDetail =
    window.z > window.x && window.w > window.y &&
    st.x >= window.x && st.x <= window.z &&
    st.y >= window.y && st.y <= window.w;
  vec4 sampleColor;
  if (inDetail) {
    vec2 dst = vec2(
      (st.x - window.x) / max(window.z - window.x, 1e-6),
      (st.y - window.y) / max(window.w - window.y, 1e-6)
    );
    sampleColor = texture(detail, dst);
  } else {
    sampleColor = texture(image, st);
  }
  material.diffuse = sampleColor.rgb;
  material.alpha = sampleColor.a * alpha;
  return material;
}
"#;

const MODEL_PARTS_VERTEX: &str = r#"
void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) {
  float row = (floor(vsInput.attributes.part + 0.5) + 0.5) / u_partRows;
  vec4 r0 = texture(u_partMatrices, vec2(0.0625, row));
  vec4 r1 = texture(u_partMatrices, vec2(0.1875, row));
  vec4 r2 = texture(u_partMatrices, vec2(0.3125, row));
  vec4 n0 = texture(u_partMatrices, vec2(0.4375, row));
  v_partHighlight = texture(u_partMatrices, vec2(0.8125, row));
  vec4 p = vec4(vsOutput.positionMC, 1.0);
  vsOutput.positionMC = vec3(dot(r0, p), dot(r1, p), dot(r2, p));
  v_partMoved = n0.a;
  if (n0.a > 0.5) {
    vec4 n1 = texture(u_partMatrices, vec2(0.5625, row));
    vec4 n2 = texture(u_partMatrices, vec2(0.6875, row));
    mat3 turn = transpose(mat3(n0.xyz, n1.xyz, n2.xyz));
    v_partNormalEC = czm_normal * turn * inverse(czm_normal);
  } else {
    v_partNormalEC = mat3(1.0);
  }
}
"#;

fn build_base_model_shader(shading: &str, mask_color: Option<[f64; 3]>) -> Result<Option<Rc<CustomShader<BrowserGl>>>, RuntimeError> {
    if shading == "thermal" {
        let shader = CustomShader::new(CustomShaderOptions {
            mode: CustomShaderMode::ModifyMaterial,
            lighting_model: Some(LightingModel::Unlit),
            fragment_shader_text: Some(
                "void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = vec3(1.0);\n}".to_string(),
            ),
            ..Default::default()
        })?;
        return Ok(Some(Rc::new(shader)));
    }
    if let Some(mask) = mask_color {
        let mut uniforms = std::collections::BTreeMap::new();
        uniforms.insert(
            "u_maskColor".to_string(),
            (UniformType::Vec3, CustomShaderUniformValue::Vec3(Cartesian3::new(mask[0], mask[1], mask[2]))),
        );
        let shader = CustomShader::new(CustomShaderOptions {
            mode: CustomShaderMode::ModifyMaterial,
            lighting_model: if shading == "unlit" { Some(LightingModel::Unlit) } else { None },
            uniforms,
            fragment_shader_text: Some(
                "void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  vec3 mask = czm_srgbToLinear(u_maskColor);\n  material.diffuse *= mix(vec3(1.0), mask, fsInput.attributes.tint);\n}".to_string(),
            ),
            ..Default::default()
        })?;
        return Ok(Some(Rc::new(shader)));
    }
    if shading == "unlit" {
        let shader = CustomShader::new(CustomShaderOptions {
            mode: CustomShaderMode::ModifyMaterial,
            lighting_model: Some(LightingModel::Unlit),
            ..Default::default()
        })?;
        return Ok(Some(Rc::new(shader)));
    }
    Ok(None)
}

fn build_parts_texture_uniform(rows: u32, floats: &[f32]) -> TextureUniform {
    let mut bytes = Vec::with_capacity(floats.len() * 4);
    for &f in floats {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    let sampler = Sampler {
        wrap_r: TextureWrap::ClampToEdge,
        wrap_s: TextureWrap::ClampToEdge,
        wrap_t: TextureWrap::ClampToEdge,
        minification_filter: TextureMinificationFilter::Nearest,
        magnification_filter: TextureMagnificationFilter::Nearest,
        maximum_anisotropy: 1.0,
    };
    TextureUniform::from_typed_array(bytes, 8, rows, PixelFormat::Rgba, PixelDatatype::Float, sampler)
}

fn build_parts_model_shader(
    shading: &str,
    mask_color: Option<[f64; 3]>,
    rows: u32,
    floats: &[f32],
) -> Result<Rc<CustomShader<BrowserGl>>, RuntimeError> {
    let lit = shading != "unlit" && shading != "thermal";
    let masked = mask_color.is_some() && shading != "thermal";
    let mut uniforms = std::collections::BTreeMap::new();
    uniforms.insert(
        "u_partMatrices".to_string(),
        (UniformType::Sampler2D, CustomShaderUniformValue::Sampler2D(build_parts_texture_uniform(rows, floats))),
    );
    uniforms.insert("u_partRows".to_string(), (UniformType::Float, CustomShaderUniformValue::Number(f64::from(rows))));
    if let Some(mask) = mask_color {
        if masked {
            uniforms.insert(
                "u_maskColor".to_string(),
                (UniformType::Vec3, CustomShaderUniformValue::Vec3(Cartesian3::new(mask[0], mask[1], mask[2]))),
            );
        }
    }
    let mut varyings = std::collections::BTreeMap::new();
    varyings.insert("v_partNormalEC".to_string(), VaryingType::Mat3);
    varyings.insert("v_partMoved".to_string(), VaryingType::Float);
    varyings.insert("v_partHighlight".to_string(), VaryingType::Vec4);

    let mut frag = String::from("void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n");
    if lit {
        frag.push_str("  if (v_partMoved > 0.5) { material.normalEC = normalize(v_partNormalEC * material.normalEC); }\n");
    }
    if shading == "thermal" {
        frag.push_str("  material.diffuse = vec3(1.0);\n");
    }
    if masked {
        frag.push_str("  vec3 mask = czm_srgbToLinear(u_maskColor);\n  material.diffuse *= mix(vec3(1.0), mask, fsInput.attributes.tint);\n");
    }
    frag.push_str(
        "  if (v_partHighlight.a > 0.0) {\n    vec3 highlight = czm_srgbToLinear(v_partHighlight.rgb);\n    material.diffuse = mix(material.diffuse, highlight, 0.60);\n    material.emissive = max(material.emissive, highlight * 0.5);\n  }\n}",
    );

    let shader = CustomShader::new(CustomShaderOptions {
        mode: CustomShaderMode::ModifyMaterial,
        lighting_model: if lit { None } else { Some(LightingModel::Unlit) },
        uniforms,
        varyings,
        vertex_shader_text: Some(MODEL_PARTS_VERTEX.to_string()),
        fragment_shader_text: Some(frag),
        ..Default::default()
    })?;
    Ok(Rc::new(shader))
}

fn parse_pp_uniforms(json: &str) -> Result<HashMap<String, PostProcessUniformValue<BrowserGl>>, RuntimeError> {
    let mut out = HashMap::new();
    if json.trim().is_empty() || json.trim() == "{}" {
        return Ok(out);
    }
    let map: HashMap<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| RuntimeError::new(e.to_string()))?;
    for (k, v) in map {
        if let Some(b) = v.as_bool() {
            out.insert(k, PostProcessUniformValue::Bool(b));
        } else if let Some(n) = v.as_f64() {
            out.insert(k, PostProcessUniformValue::Float(n as f32));
        } else if let Some(arr) = v.as_array() {
            let nums: Vec<f32> = arr.iter().filter_map(|x| x.as_f64().map(|n| n as f32)).collect();
            match nums.len() {
                2 => {
                    out.insert(k, PostProcessUniformValue::Vec2([nums[0], nums[1]]));
                }
                3 => {
                    out.insert(k, PostProcessUniformValue::Vec3([nums[0], nums[1], nums[2]]));
                }
                4 => {
                    out.insert(k, PostProcessUniformValue::Vec4([nums[0], nums[1], nums[2], nums[3]]));
                }
                9 => {
                    let mut m = [0.0f32; 9];
                    m.copy_from_slice(&nums);
                    out.insert(k, PostProcessUniformValue::Mat3(m));
                }
                16 => {
                    let mut m = [0.0f32; 16];
                    m.copy_from_slice(&nums);
                    out.insert(k, PostProcessUniformValue::Mat4(m));
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

#[derive(serde::Deserialize)]
struct JsPointItem {
    id: String,
    x: f64,
    y: f64,
    z: f64,
    #[serde(default = "default_point_size")]
    pixel_size: f64,
    #[serde(default = "default_rgba")]
    color: [f64; 4],
    #[serde(default)]
    outline_color: Option<[f64; 4]>,
    #[serde(default)]
    outline_width: f64,
    #[serde(default = "default_true")]
    show: bool,
    #[serde(default = "default_true")]
    pickable: bool,
    #[serde(default)]
    disable_depth_test: Option<f64>,
    #[serde(default)]
    scale_by_distance: Option<[f64; 4]>,
    #[serde(default)]
    fade_by_distance: Option<[f64; 4]>,
    #[serde(default)]
    display_range: Option<[f64; 2]>,
}

#[derive(serde::Deserialize)]
struct JsLineItem {
    id: String,
    /// Packed ECEF `[x0, y0, z0, x1, y1, z1, ...]`.
    positions: Vec<f64>,
    #[serde(default = "default_line_width")]
    width_px: f64,
    #[serde(default = "default_line_kind")]
    style_kind: String,
    #[serde(default = "default_rgba")]
    color: [f64; 4],
    #[serde(default)]
    outline_color: Option<[f64; 4]>,
    #[serde(default)]
    outline_width: f64,
    #[serde(default)]
    glow_power: Option<f64>,
    #[serde(default)]
    dash_length: Option<f64>,
    #[serde(default)]
    dash_pattern: Option<u32>,
    #[serde(default = "default_true")]
    show: bool,
    #[serde(default = "default_true")]
    pickable: bool,
}

#[derive(serde::Deserialize)]
struct JsShapeItem {
    id: String,
    kind: String,
    /// Polygon outer ring `[x, y, z, ...]` or Wall positions `[x, y, z, ...]`.
    #[serde(default)]
    positions: Vec<f64>,
    /// Optional polygon holes (each packed `[x, y, z, ...]`).
    #[serde(default)]
    holes: Vec<Vec<f64>>,
    /// Rectangle `[west_deg, south_deg, east_deg, north_deg]`.
    #[serde(default)]
    rect: Option<[f64; 4]>,
    /// Ellipse center ECEF `[x, y, z]`.
    #[serde(default)]
    center: Option<[f64; 3]>,
    #[serde(default)]
    semi_major: Option<f64>,
    #[serde(default)]
    semi_minor: Option<f64>,
    #[serde(default)]
    rotation: Option<f64>,
    #[serde(default)]
    height: Option<f64>,
    #[serde(default)]
    extrude_height: Option<f64>,
    #[serde(default)]
    per_position_height: bool,
    #[serde(default)]
    minimum_heights: Option<Vec<f64>>,
    #[serde(default)]
    maximum_heights: Option<Vec<f64>>,
    #[serde(default)]
    fill_color: Option<[f64; 4]>,
    #[serde(default)]
    lit: Option<bool>,
    #[serde(default = "default_true")]
    show: bool,
    #[serde(default = "default_true")]
    pickable: bool,
}

#[derive(serde::Deserialize)]
struct JsIconOptions {
    id: String,
    #[serde(default)]
    image_id: Option<String>,
    x: f64,
    y: f64,
    z: f64,
    width: f64,
    height: f64,
    #[serde(default = "default_one")]
    scale: f64,
    #[serde(default = "default_rgba")]
    color: [f64; 4],
    #[serde(default)]
    rotation: f64,
    #[serde(default)]
    pixel_offset: Option<[f64; 2]>,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default = "default_true")]
    show: bool,
    #[serde(default = "default_true")]
    pickable: bool,
    #[serde(default)]
    disable_depth_test: Option<f64>,
    #[serde(default)]
    scale_by_distance: Option<[f64; 4]>,
    #[serde(default)]
    pixel_offset_scale_by_distance: Option<[f64; 4]>,
    #[serde(default)]
    fade_by_distance: Option<[f64; 4]>,
    #[serde(default)]
    display_range: Option<[f64; 2]>,
    #[serde(default)]
    aligned_axis: Option<[f64; 3]>,
}

fn default_true() -> bool {
    true
}
fn default_one() -> f64 {
    1.0
}
fn default_point_size() -> f64 {
    6.0
}
fn default_line_width() -> f64 {
    2.0
}
fn default_line_kind() -> String {
    "solid".to_string()
}
fn default_rgba() -> [f64; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn disable_depth_from(val: Option<f64>) -> f64 {
    match val {
        Some(v) if v < 0.0 || v.is_infinite() => f64::INFINITY,
        Some(v) if v.is_finite() => v.max(0.0),
        _ => 0.0,
    }
}

fn near_far_from(arr: Option<[f64; 4]>) -> Option<NearFarScalar> {
    arr.and_then(|[near, near_val, far, far_val]| {
        if far > near {
            Some(NearFarScalar::new(near, near_val, far, far_val))
        } else {
            None
        }
    })
}

fn ddc_from(arr: Option<[f64; 2]>) -> Option<DistanceDisplayCondition> {
    arr.and_then(|[near, far]| {
        if far > near {
            Some(DistanceDisplayCondition::new(near.max(0.0), far))
        } else {
            None
        }
    })
}

fn build_line_material(item: &JsLineItem) -> Material {
    let color = color_from_rgba(item.color[0], item.color[1], item.color[2], item.color[3]);
    match item.style_kind.as_str() {
        "solid" if item.outline_color.is_some() && item.outline_width > 0.0 => {
            let oc = item.outline_color.unwrap();
            let outline = color_from_rgba(oc[0], oc[1], oc[2], oc[3]);
            Material::from_type_with_uniforms(
                POLYLINE_OUTLINE_TYPE,
                vec![
                    ("color".into(), MaterialValue::from_color(color)),
                    ("outlineColor".into(), MaterialValue::from_color(outline)),
                    ("outlineWidth".into(), MaterialValue::Float(item.outline_width)),
                ],
            )
            .unwrap_or_else(|_| {
                Material::from_type_with_uniforms(COLOR_TYPE, vec![("color".into(), MaterialValue::from_color(color))])
                    .unwrap()
            })
        }
        "glow" => Material::from_type_with_uniforms(
            POLYLINE_GLOW_TYPE,
            vec![
                ("color".into(), MaterialValue::from_color(color)),
                ("glowPower".into(), MaterialValue::Float(item.glow_power.unwrap_or(0.25))),
            ],
        )
        .unwrap_or_else(|_| {
            Material::from_type_with_uniforms(COLOR_TYPE, vec![("color".into(), MaterialValue::from_color(color))])
                .unwrap()
        }),
        "dash" => Material::from_type_with_uniforms(
            POLYLINE_DASH_TYPE,
            vec![
                ("color".into(), MaterialValue::from_color(color)),
                ("dashLength".into(), MaterialValue::Float(item.dash_length.unwrap_or(16.0))),
                ("dashPattern".into(), MaterialValue::Float(f64::from(item.dash_pattern.unwrap_or(255)))),
            ],
        )
        .unwrap_or_else(|_| {
            Material::from_type_with_uniforms(COLOR_TYPE, vec![("color".into(), MaterialValue::from_color(color))])
                .unwrap()
        }),
        _ => Material::from_type_with_uniforms(COLOR_TYPE, vec![("color".into(), MaterialValue::from_color(color))])
            .expect("Color material exists"),
    }
}

fn unpack_positions(flat: &[f64]) -> Vec<Cartesian3> {
    flat.chunks_exact(3).map(|c| Cartesian3::new(c[0], c[1], c[2])).collect()
}

fn build_shape_primitive(set_id: &str, items: &[JsShapeItem]) -> Option<Vec<Primitive<BrowserGl>>> {
    let mut flat_opaque = Vec::new();
    let mut flat_translucent = Vec::new();
    let mut lit_opaque = Vec::new();
    let mut lit_translucent = Vec::new();

    for item in items {
        if !item.show {
            continue;
        }
        let Some(rgba) = item.fill_color else {
            continue;
        };
        if rgba[3] <= 0.0 {
            continue;
        }
        let lit = item.lit.unwrap_or(item.extrude_height.is_some());
        let translucent = rgba[3] < 0.999;
        let vertex_format =
            if lit { PerInstanceColorAppearance::VERTEX_FORMAT } else { PerInstanceColorAppearance::FLAT_VERTEX_FORMAT };
        let geometry = match item.kind.as_str() {
            "polygon" => {
                let outer = unpack_positions(&item.positions);
                if outer.len() < 3 {
                    continue;
                }
                let holes = item
                    .holes
                    .iter()
                    .map(|h| PolygonHierarchy::new(unpack_positions(h), Vec::new()))
                    .collect();
                let mut opts = PolygonGeometryOptions::new(PolygonHierarchy::new(outer, holes));
                opts.height = item.height.unwrap_or(0.0);
                opts.extruded_height = item.extrude_height;
                opts.per_position_height = item.per_position_height;
                opts.vertex_format = vertex_format;
                PolygonGeometry::new(opts).create_geometry()
            }
            "rectangle" => {
                let Some([w, s, e, n]) = item.rect else {
                    continue;
                };
                let rect = Rectangle::from_degrees(w, s, e, n);
                let mut opts = RectangleGeometryOptions::new(rect);
                opts.height = item.height.unwrap_or(0.0);
                opts.extruded_height = item.extrude_height;
                opts.rotation = item.rotation.unwrap_or(0.0);
                opts.vertex_format = vertex_format;
                RectangleGeometry::new(opts).create_geometry()
            }
            "ellipse" => {
                let Some([cx, cy, cz]) = item.center else {
                    continue;
                };
                let (Some(semi_major), Some(semi_minor)) = (item.semi_major, item.semi_minor) else {
                    continue;
                };
                let mut opts = EllipseGeometryOptions::new(Cartesian3::new(cx, cy, cz), semi_major, semi_minor);
                opts.height = item.height.unwrap_or(0.0);
                opts.extruded_height = item.extrude_height;
                opts.rotation = item.rotation.unwrap_or(0.0);
                opts.vertex_format = vertex_format;
                EllipseGeometry::new(opts).and_then(|g| g.create_geometry())
            }
            "wall" => {
                let positions = unpack_positions(&item.positions);
                if positions.len() < 2 {
                    continue;
                }
                let mut opts = WallGeometryOptions::new(positions);
                opts.minimum_heights = item.minimum_heights.clone();
                opts.maximum_heights = item.maximum_heights.clone();
                opts.vertex_format = vertex_format;
                WallGeometry::new(opts).create_geometry()
            }
            _ => None,
        };
        let Some(geometry) = geometry else {
            continue;
        };
        let color = color_from_rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let mut instance = GeometryInstance::new(geometry, None);
        instance.attributes.push(("color".into(), ColorGeometryInstanceAttribute::from_color(&color).into()));
        instance.id = Some(encode_item_pick_id(set_id, &item.id, item.pickable));
        match (lit, translucent) {
            (false, false) => flat_opaque.push(instance),
            (false, true) => flat_translucent.push(instance),
            (true, false) => lit_opaque.push(instance),
            (true, true) => lit_translucent.push(instance),
        }
    }

    let mut out = Vec::new();
    for (instances, flat, translucent) in [
        (flat_opaque, true, false),
        (flat_translucent, true, true),
        (lit_opaque, false, false),
        (lit_translucent, false, true),
    ] {
        if !instances.is_empty() {
            let appearance = PerInstanceColorAppearance::new(PerInstanceColorAppearanceOptions {
                flat: Some(flat),
                translucent: Some(translucent),
                closed: Some(false),
                ..Default::default()
            });
            out.push(Primitive::new(PrimitiveOptions {
                geometry_instances: instances,
                appearance: Some(Box::new(appearance)),
                asynchronous: Some(false),
                allow_picking: Some(true),
                ..Default::default()
            }));
        }
    }
    Some(out)
}

#[wasm_bindgen]
pub struct BrowserScene {
    scene: Scene<RendererSceneBackend<BrowserGl>>,
    context: Context<BrowserGl>,
    globe: Rc<RefCell<Globe<BrowserGl>>>,
    sky_atmosphere: Rc<RefCell<SkyAtmosphere<BrowserGl>>>,
    client: Rc<ResourceClient>,
    image_host: Rc<BrowserImageHost>,
    resource_cache: ResourceCache,
    terrain_provider: Option<Rc<CesiumTerrainProvider>>,
    photoreal_tileset: Option<Rc<RefCell<Cesium3DTileset>>>,
    shading_strength: Option<f64>,
    shading_had_normals: bool,
    imagery_layers: HashMap<u32, ImageryEntry>,
    next_imagery_id: u32,
    draw_sets: HashMap<String, DrawSetEntry>,
    raster_shells: HashMap<String, Rc<RefCell<RasterShellState>>>,
    post_process_collection: Rc<RefCell<PostProcessStageCollection<BrowserGl>>>,
    camera_controller: ScreenSpaceCameraController,
    brdf_lut_ready: bool,
    next_draw_seq: u64,
    width: u32,
    height: u32,
    pixel_ratio: f64,
    destroyed: bool,
}

impl BrowserScene {
    fn ensure_brdf_lut(&mut self) {
        if self.brdf_lut_ready || self.destroyed {
            return;
        }
        let mut brdf_lut_gen = BrdfLutGenerator::new();
        if brdf_lut_gen.update(&self.context).is_ok() {
            if let Some(brdf_tex) = brdf_lut_gen.color_texture().cloned() {
                self.scene
                    .backend
                    .textures
                    .textures
                    .insert("czm_brdfLut".into(), TextureBinding::Texture(brdf_tex));
            }
        }
        self.brdf_lut_ready = true;
    }

    fn rebuild_scene_primitives(&mut self) {
        let mut ordered: Vec<&DrawSetEntry> = self.draw_sets.values().collect();
        ordered.sort_by(|a, b| b.draw_order.total_cmp(&a.draw_order).then_with(|| a.seq.cmp(&b.seq)));
        self.scene.primitives.clear();
        self.scene.primitives.push(Box::new(SharedSkyAtmosphere {
            sky: Rc::clone(&self.sky_atmosphere),
            globe: Rc::clone(&self.globe),
        }));
        if let Some(ref ts) = self.photoreal_tileset {
            self.scene.primitives.push(Box::new(SharedTileset(Rc::clone(ts))));
        }
        // Raster shells draw in ascending heightM, below draw sets and never picked.
        let mut shells: Vec<Rc<RefCell<RasterShellState>>> = self.raster_shells.values().cloned().collect();
        shells.sort_by(|a, b| a.borrow().height_m.total_cmp(&b.borrow().height_m));
        for shell in shells {
            self.scene.primitives.push(Box::new(SharedRasterShell { state: shell }));
        }
        for entry in ordered {
            let pickable = Rc::clone(&entry.pickable);
            let blend = Rc::clone(&entry.blend);
            let prim: Box<dyn ScenePrimitive> = match &entry.handle {
                DrawSetHandle::Points(e) => Box::new(SharedPointCollection { collection: Rc::clone(&e.collection), pickable, blend }),
                DrawSetHandle::Icons(e) => Box::new(SharedBillboardCollection { collection: Rc::clone(&e.collection), pickable, blend }),
                DrawSetHandle::Lines(e) => Box::new(SharedPolylineCollection { collection: Rc::clone(&e.collection), pickable, blend }),
                DrawSetHandle::Shapes(s) => Box::new(SharedShapeSet { state: Rc::clone(s), pickable, blend }),
                DrawSetHandle::Models(s) => Box::new(SharedModelSet { state: Rc::clone(s), pickable, blend }),
            };
            self.scene.primitives.push(prim);
        }
    }

    fn apply_model_options(
        context: &Context<BrowserGl>,
        set_id: &str,
        item_id: &str,
        entry: &mut ModelItemEntry,
        opts: &JsModelOptions,
    ) -> Result<(), RuntimeError> {
        let mut model = entry.model.borrow_mut();
        model.options.id = Some(encode_item_pick_id(set_id, item_id, opts.pickable));
        model.options.model_matrix = Matrix4(opts.model_matrix);
        model.options.scale = if opts.scale.is_finite() && opts.scale >= 0.0 { opts.scale } else { 1.0 };
        model.options.minimum_pixel_size = if opts.min_pixel_size.is_finite() && opts.min_pixel_size >= 0.0 {
            opts.min_pixel_size
        } else {
            0.0
        };
        model.options.maximum_scale = opts.max_scale.filter(|v| v.is_finite() && *v > 0.0);
        model.options.show = opts.show;
        model.options.color = opts.color.map(|c| color_from_rgba(c[0], c[1], c[2], c[3]));
        model.options.color_blend_mode = match opts.color_blend_mode.as_deref() {
            Some("replace") => ColorBlendMode::Replace,
            Some("mix") => ColorBlendMode::Mix,
            _ => ColorBlendMode::Highlight,
        };
        model.options.color_blend_amount = opts.color_blend_amount.unwrap_or(0.5).clamp(0.0, 1.0);
        model.options.distance_display_condition = ddc_from(opts.display_range);
        // Enable procedural spherical harmonics so lit models remain readable at night.
        model.image_based_lighting.enabled = true;
        model.image_based_lighting.use_default_spherical_harmonics = true;
        model.image_based_lighting.spherical_harmonic_coefficients = Some([
            Cartesian3::new(0.45, 0.45, 0.48),
            Cartesian3::ZERO,
            Cartesian3::ZERO,
            Cartesian3::ZERO,
            Cartesian3::ZERO,
            Cartesian3::ZERO,
            Cartesian3::ZERO,
            Cartesian3::ZERO,
            Cartesian3::ZERO,
        ]);
        model.image_based_lighting.environment_map_intensity = if opts.environment_map { 0.7 } else { 0.35 };

        // Clipping plane below origin (local +Z up keeps above origin).
        if opts.clip_below_origin && model.clipping_planes.is_none() {
            let plane = Rc::new(RefCell::new(ClippingPlane::new(Cartesian3::UNIT_Z, 0.0)));
            let mut planes = ClippingPlaneCollection::new(vec![plane]);
            planes.edge_width = 0.0;
            let use_float = context.capabilities().map(|c| c.floating_point_texture).unwrap_or(false);
            let max_tex = context.limits().map(|l| l.maximum_texture_size).unwrap_or(4096);
            model.clipping_planes = Some(ModelClippingPlanesState {
                clipping_planes: Rc::new(RefCell::new(planes)),
                clipping_planes_matrix: Rc::new(RefCell::new(Matrix4::IDENTITY)),
                use_float_texture: use_float,
                max_texture_size: max_tex,
                default_texture: context.default_texture().ok(),
            });
        } else if !opts.clip_below_origin && model.clipping_planes.is_some() {
            model.clipping_planes = None;
        }

        let shading = opts.shading.as_deref().unwrap_or("pbr");
        let mask_key = opts
            .mask_color
            .map(|m| format!("{:.4},{:.4},{:.4}", m[0], m[1], m[2]))
            .unwrap_or_default();
        let base_key = format!("{shading}|{mask_key}");
        if entry.base_shader_key != base_key {
            entry.base_shader = build_base_model_shader(shading, opts.mask_color)?;
            entry.base_shader_key = base_key;
        }
        if entry.parts_shader.is_none() {
            model.custom_shader = entry.base_shader.clone();
        }
        Ok(())
    }

    fn apply_shading_material(&mut self) {
        let mut globe = self.globe.borrow_mut();
        match self.shading_strength {
            None => {
                globe.surface.tile_provider.material = None;
                self.shading_had_normals = false;
            }
            Some(strength) => {
                let normals = globe.terrain_provider().has_vertex_normals();
                if let Ok(mat) = hillshade_material(normals, strength) {
                    globe.surface.tile_provider.material = Some(mat);
                    self.shading_had_normals = normals;
                }
            }
        }
    }

    fn set_draw_set_visible_raw(handle: &DrawSetHandle, show: bool) -> bool {
        match handle {
            DrawSetHandle::Points(e) => {
                let mut c = e.collection.borrow_mut();
                let prev = c.show;
                c.show = show;
                prev
            }
            DrawSetHandle::Icons(e) => {
                let mut c = e.collection.borrow_mut();
                let prev = c.show;
                c.show = show;
                prev
            }
            DrawSetHandle::Lines(e) => {
                let mut c = e.collection.borrow_mut();
                let prev = c.show;
                c.show = show;
                prev
            }
            DrawSetHandle::Shapes(s) => {
                let mut st = s.borrow_mut();
                let prev = st.show;
                st.show = show;
                prev
            }
            DrawSetHandle::Models(s) => {
                let mut st = s.borrow_mut();
                let prev = st.show;
                st.show = show;
                prev
            }
        }
    }

    fn has_active_ground_sets(&self, excluded: &HashSet<String>) -> bool {
        for (id, entry) in &self.draw_sets {
            if !entry.include_in_height_queries.get() || excluded.contains(id) {
                continue;
            }
            let shown = match &entry.handle {
                DrawSetHandle::Points(e) => e.collection.borrow().show && !e.items.is_empty(),
                DrawSetHandle::Icons(e) => e.collection.borrow().show && !e.items.is_empty(),
                DrawSetHandle::Lines(e) => e.collection.borrow().show && !e.items.is_empty(),
                DrawSetHandle::Shapes(s) => {
                    let st = s.borrow();
                    st.show && !st.primitives.is_empty()
                }
                DrawSetHandle::Models(s) => {
                    let st = s.borrow();
                    st.show && !st.items.is_empty()
                }
            };
            if shown {
                return true;
            }
        }
        false
    }

    fn build_billboard_options(set_id: &str, item: &JsIconOptions) -> BillboardOptions {
        let vertical_origin = match item.anchor.as_deref() {
            Some("bottom") => VerticalOrigin::Bottom,
            Some("top") => VerticalOrigin::Top,
            _ => VerticalOrigin::Center,
        };
        let pixel_offset = item
            .pixel_offset
            .map(|[x, y]| Cartesian2::new(x, y))
            .unwrap_or(Cartesian2::ZERO);
        let aligned_axis = item
            .aligned_axis
            .map(|[x, y, z]| {
                let v = Cartesian3::new(x, y, z);
                let mag = v.magnitude();
                if mag > 1e-12 { v / mag } else { Cartesian3::ZERO }
            })
            .unwrap_or(Cartesian3::ZERO);
        BillboardOptions {
            show: item.show,
            position: Cartesian3::new(item.x, item.y, item.z),
            pixel_offset,
            horizontal_origin: HorizontalOrigin::Center,
            vertical_origin,
            scale: item.scale,
            color: color_from_rgba(item.color[0], item.color[1], item.color[2], item.color[3]),
            rotation: item.rotation,
            aligned_axis,
            width: Some(item.width.max(1.0)),
            height: Some(item.height.max(1.0)),
            scale_by_distance: near_far_from(item.scale_by_distance),
            translucency_by_distance: near_far_from(item.fade_by_distance),
            pixel_offset_scale_by_distance: near_far_from(item.pixel_offset_scale_by_distance),
            distance_display_condition: ddc_from(item.display_range),
            disable_depth_test_distance: Some(disable_depth_from(item.disable_depth_test)),
            id: Some(encode_item_pick_id(set_id, &item.id, item.pickable)),
            image_id: item.image_id.clone(),
            ..Default::default()
        }
    }
}

#[wasm_bindgen]
impl BrowserScene {
    /// Creates a new WebGL2 `Scene` + `Globe` over an existing WebGL2 context.
    #[wasm_bindgen(constructor)]
    pub fn new(gl: JsValue, width: u32, height: u32, pixel_ratio: f64, epoch_ms: f64) -> Result<BrowserScene, JsValue> {
        install_panic_hook();
        let browser_gl = BrowserGl::new(gl).map_err(js_err)?;
        let context = Context::new_webgl2(browser_gl);
        let mut backend = RendererSceneBackend::new(context.clone());
        let post_process_collection = Rc::new(RefCell::new(PostProcessStageCollection::new(&context).map_err(js_err)?));
        let post_process_inputs = Rc::new(RefCell::new(None));
        backend.set_post_process_binding(Rc::clone(&post_process_collection), Rc::clone(&post_process_inputs));

        let w = width.max(1);
        let h = height.max(1);
        let pr = if pixel_ratio.is_finite() && pixel_ratio > 0.0 { pixel_ratio } else { 1.0 };

        let css_w = (f64::from(w) / pr).max(1.0);
        let css_h = (f64::from(h) / pr).max(1.0);
        let mut camera_options = CameraOptions::new(css_w, css_h);
        camera_options.ellipsoid = Ellipsoid::WGS84;

        let scene_options = SceneOptions {
            order_independent_translucency: false,
            request_render_mode: true,
            maximum_render_time_change: None,
            scene_3d_only: true,
            logarithmic_depth_buffer: true,
            high_dynamic_range: false,
            ..Default::default()
        };

        let mut scene = Scene::new(backend, camera_options, scene_options, epoch_ms_to_julian(epoch_ms))
            .map_err(js_err)?;
        scene.camera.client_width = css_w;
        scene.camera.client_height = css_h;
        scene.camera.drawing_buffer_width = f64::from(w);
        scene.camera.drawing_buffer_height = f64::from(h);
        let camera_controller = ScreenSpaceCameraController::new(&scene.camera);
        scene.rethrow_render_errors = true;
        scene.use_depth_picking = true;
        scene.background_color = Color::new(0.015, 0.025, 0.05, 1.0);
        scene.fog.enabled = false;
        scene.frame_state.split_position = 0.0;
        scene.effects.push(Box::new(PostProcessSceneEffect {
            context: context.clone(),
            collection: Rc::clone(&post_process_collection),
            inputs: Rc::clone(&post_process_inputs),
        }));

        let mut globe = Globe::new(&context, GlobeOptions::default());
        globe.surface_options_mut().base_color = Color::new(0.04, 0.09, 0.17, 1.0);
        globe.surface_options_mut().enable_lighting = false;
        globe.surface_options_mut().show_ground_atmosphere = false;
        globe.surface.tile_provider.set_image_upload(Rc::new(|ctx, res, mut opts| {
            let img = res
                .get::<JsValue>()
                .ok_or_else(|| RuntimeError::new("Expected browser image JsValue"))?;
            let (iw, ih) = BrowserImageHost.dimensions(img);
            opts.width = iw.max(1);
            opts.height = ih.max(1);
            Texture::from_image(ctx, opts, img.clone())
        }));

        let globe_rc = Rc::new(RefCell::new(globe));
        scene.globe = Some(Box::new(SharedGlobe(Rc::clone(&globe_rc))));

        let mut sky_atm = SkyAtmosphere::new(context.clone(), Some(Ellipsoid::WGS84));
        sky_atm.show = false;
        let sky_atmosphere = Rc::new(RefCell::new(sky_atm));

        let client = Rc::new(ResourceClient::new(Rc::new(BrowserTransport::default())));
        let image_host = Rc::new(BrowserImageHost);
        let resource_cache = ResourceCache::new(client.as_ref().clone());

        let mut bs = BrowserScene {
            scene,
            context,
            globe: globe_rc,
            sky_atmosphere,
            client,
            image_host,
            resource_cache,
            terrain_provider: None,
            photoreal_tileset: None,
            shading_strength: None,
            shading_had_normals: false,
            imagery_layers: HashMap::new(),
            next_imagery_id: 1,
            draw_sets: HashMap::new(),
            raster_shells: HashMap::new(),
            post_process_collection,
            camera_controller,
            brdf_lut_ready: false,
            next_draw_seq: 1,
            width: w,
            height: h,
            pixel_ratio: pr,
            destroyed: false,
        };
        bs.rebuild_scene_primitives();
        Ok(bs)
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        self.draw_sets.clear();
        self.raster_shells.clear();
        self.imagery_layers.clear();
        self.terrain_provider = None;
        self.photoreal_tileset = None;
        let _ = self.scene.destroy();
    }

    pub fn resize(&mut self, width: u32, height: u32, pixel_ratio: f64) {
        if self.destroyed {
            return;
        }
        let w = width.max(1);
        let h = height.max(1);
        self.width = w;
        self.height = h;
        if pixel_ratio.is_finite() && pixel_ratio > 0.0 {
            self.pixel_ratio = pixel_ratio;
        }
        let css_w = (f64::from(w) / self.pixel_ratio).max(1.0);
        let css_h = (f64::from(h) / self.pixel_ratio).max(1.0);
        self.scene.camera.client_width = css_w;
        self.scene.camera.client_height = css_h;
        self.scene.camera.drawing_buffer_width = f64::from(w);
        self.scene.camera.drawing_buffer_height = f64::from(h);
        self.camera_controller.aggregator.set_client_width(css_w);
        if let CameraFrustum::Perspective(ref mut frustum) = self.scene.camera.frustum {
            frustum.aspect_ratio = f64::from(w) / f64::from(h);
        }
        self.scene.frame_state.drawing_buffer_width = w;
        self.scene.frame_state.drawing_buffer_height = h;
        self.scene.request_render();
    }

    pub fn request_render(&self) {
        if !self.destroyed {
            self.scene.request_render();
        }
    }

    pub fn render_requested(&self) -> bool {
        !self.destroyed && self.scene.render_requested()
    }

    pub fn initialize_frame(&mut self, monotonic_seconds: f64) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        self.client.scheduler.borrow_mut().poll_update(&mut cx);
        self.scene.initialize_frame(monotonic_seconds).map_err(js_err)
    }

    pub fn render(&mut self, epoch_ms: f64) -> Result<bool, JsValue> {
        if self.destroyed {
            return Ok(false);
        }
        self.scene.render(epoch_ms_to_julian(epoch_ms)).map_err(js_err)
    }

    pub fn force_render(&mut self, epoch_ms: f64) -> Result<bool, JsValue> {
        if self.destroyed {
            return Ok(false);
        }
        self.scene.force_render(epoch_ms_to_julian(epoch_ms)).map_err(js_err)
    }

    /// Synchronizes the Rust `Scene`'s camera from an exact ECEF eye pose and vertical FOV.
    #[allow(clippy::too_many_arguments)]
    pub fn set_camera_pose(
        &mut self,
        px: f64,
        py: f64,
        pz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        ux: f64,
        uy: f64,
        uz: f64,
        rx: f64,
        ry: f64,
        rz: f64,
        fovy: f64,
        near: f64,
        far: f64,
    ) {
        if self.destroyed {
            return;
        }
        let cam = &mut self.scene.camera;
        cam.position = Cartesian3::new(px, py, pz);
        cam.direction = Cartesian3::new(dx, dy, dz);
        cam.up = Cartesian3::new(ux, uy, uz);
        cam.right = Cartesian3::new(rx, ry, rz);
        if let CameraFrustum::Perspective(ref mut frustum) = cam.frustum {
            let aspect = f64::from(self.width.max(1)) / f64::from(self.height.max(1));
            frustum.aspect_ratio = aspect;
            if fovy.is_finite() && fovy > 0.0 && fovy < std::f64::consts::PI {
                // Cesium PerspectiveFrustum::fov is the largest-axis FOV (horizontal when aspect > 1).
                frustum.fov = if aspect <= 1.0 {
                    fovy
                } else {
                    ((fovy * 0.5).tan() * aspect).atan() * 2.0
                };
            }
            if near.is_finite() && near > 0.0 {
                frustum.near = near;
            }
            if far.is_finite() && far > frustum.near {
                frustum.far = far;
            }
        }
        self.scene.request_render();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn camera_input_event(
        &mut self,
        kind: &str,
        modifier: &str,
        x: f64,
        y: f64,
        end_x: f64,
        end_y: f64,
        wheel_delta: f64,
        time_ms: f64,
    ) {
        if self.destroyed {
            return;
        }
        let ev_mod = match modifier {
            "shift" => Some(KeyboardEventModifier::Shift),
            "ctrl" => Some(KeyboardEventModifier::Ctrl),
            "alt" => Some(KeyboardEventModifier::Alt),
            _ => None,
        };
        let mapped = match kind {
            "left_down" => Some((ScreenSpaceEventType::LeftDown, ScreenSpaceInputEvent::Position(Cartesian2::new(x, y)))),
            "left_up" => Some((ScreenSpaceEventType::LeftUp, ScreenSpaceInputEvent::Position(Cartesian2::new(x, y)))),
            "right_down" => Some((ScreenSpaceEventType::RightDown, ScreenSpaceInputEvent::Position(Cartesian2::new(x, y)))),
            "right_up" => Some((ScreenSpaceEventType::RightUp, ScreenSpaceInputEvent::Position(Cartesian2::new(x, y)))),
            "middle_down" => Some((ScreenSpaceEventType::MiddleDown, ScreenSpaceInputEvent::Position(Cartesian2::new(x, y)))),
            "middle_up" => Some((ScreenSpaceEventType::MiddleUp, ScreenSpaceInputEvent::Position(Cartesian2::new(x, y)))),
            "move" => Some((
                ScreenSpaceEventType::MouseMove,
                ScreenSpaceInputEvent::Movement {
                    start_position: Cartesian2::new(x, y),
                    end_position: Cartesian2::new(end_x, end_y),
                },
            )),
            "wheel" => Some((ScreenSpaceEventType::Wheel, ScreenSpaceInputEvent::Wheel(wheel_delta))),
            "pinch_start" => Some((
                ScreenSpaceEventType::PinchStart,
                ScreenSpaceInputEvent::TwoPositions {
                    position1: Cartesian2::new(x, y),
                    position2: Cartesian2::new(end_x, end_y),
                },
            )),
            "pinch_end" => Some((ScreenSpaceEventType::PinchEnd, ScreenSpaceInputEvent::End)),
            _ => None,
        };
        if let Some((ev_kind, ev)) = mapped {
            let _ = self
                .camera_controller
                .aggregator
                .process_event(ev_kind, ev_mod, &ev, time_ms / 1000.0);
            self.scene.request_render();
        }
    }

    pub fn set_camera_controller_options(&mut self, json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        #[derive(serde::Deserialize)]
        struct CtrlOpts {
            enable_inputs: Option<bool>,
            enable_rotate: Option<bool>,
            enable_tilt: Option<bool>,
            enable_zoom: Option<bool>,
            enable_look: Option<bool>,
            minimum_zoom_distance: Option<f64>,
            maximum_zoom_distance: Option<f64>,
            zoom_factor: Option<f64>,
            inertia_spin: Option<f64>,
            inertia_translate: Option<f64>,
            inertia_zoom: Option<f64>,
            enable_collision_detection: Option<bool>,
            maximum_tilt_angle: Option<Option<f64>>,
            zoom_inputs: Option<Vec<String>>,
        }
        let opts: CtrlOpts = serde_json::from_str(json).map_err(js_err)?;
        let c = &mut self.camera_controller;
        if let Some(v) = opts.enable_inputs { c.enable_inputs = v; }
        if let Some(v) = opts.enable_rotate { c.enable_rotate = v; }
        if let Some(v) = opts.enable_tilt { c.enable_tilt = v; }
        if let Some(v) = opts.enable_zoom { c.enable_zoom = v; }
        if let Some(v) = opts.enable_look { c.enable_look = v; }
        if let Some(v) = opts.minimum_zoom_distance { c.minimum_zoom_distance = v; }
        if let Some(v) = opts.maximum_zoom_distance { c.maximum_zoom_distance = v; }
        if let Some(v) = opts.zoom_factor { c.zoom_factor = v; }
        if let Some(v) = opts.inertia_spin { c.inertia_spin = v; }
        if let Some(v) = opts.inertia_translate { c.inertia_translate = v; }
        if let Some(v) = opts.inertia_zoom { c.inertia_zoom = v; }
        if let Some(v) = opts.enable_collision_detection { c.enable_collision_detection = v; }
        if let Some(v) = opts.maximum_tilt_angle { c.maximum_tilt_angle = v; }
        if let Some(list) = opts.zoom_inputs {
            let mut bindings = Vec::with_capacity(list.len());
            for name in list {
                match name.as_str() {
                    "wheel" => bindings.push(CameraEventType::Wheel.into()),
                    "pinch" => bindings.push(CameraEventType::Pinch.into()),
                    "rightDrag" => bindings.push(CameraEventType::RightDrag.into()),
                    "ctrlWheel" => bindings.push(CameraInputBinding::modified(
                        CameraEventType::Wheel,
                        KeyboardEventModifier::Ctrl,
                    )),
                    _ => {}
                }
            }
            c.zoom_event_types = bindings;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step_camera_controller(
        &mut self,
        time_ms: f64,
        px: f64,
        py: f64,
        pz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        ux: f64,
        uy: f64,
        uz: f64,
        rx: f64,
        ry: f64,
        rz: f64,
        fovy: f64,
        near: f64,
        far: f64,
        frame_matrix: &[f64],
    ) -> Vec<f64> {
        if self.destroyed {
            return vec![0.0];
        }
        let cam = &mut self.scene.camera;
        let _ = cam.set_transform(Matrix4::IDENTITY);
        cam.position = Cartesian3::new(px, py, pz);
        cam.direction = Cartesian3::new(dx, dy, dz);
        cam.up = Cartesian3::new(ux, uy, uz);
        cam.right = Cartesian3::new(rx, ry, rz);
        if let CameraFrustum::Perspective(ref mut frustum) = cam.frustum {
            let aspect = f64::from(self.width.max(1)) / f64::from(self.height.max(1));
            frustum.aspect_ratio = aspect;
            if fovy.is_finite() && fovy > 0.0 && fovy < std::f64::consts::PI {
                frustum.fov = if aspect <= 1.0 {
                    fovy
                } else {
                    ((fovy * 0.5).tan() * aspect).atan() * 2.0
                };
            }
            if near.is_finite() && near > 0.0 {
                frustum.near = near;
            }
            if far.is_finite() && far > frustum.near {
                frustum.far = far;
            }
        }
        if frame_matrix.len() == 16 {
            let mut arr = [0.0f64; 16];
            arr.copy_from_slice(frame_matrix);
            let m = Matrix4(arr);
            if m != Matrix4::IDENTITY {
                let _ = cam.set_transform(m);
            }
        }
        let before_p = cam.position_wc();
        let before_d = cam.direction_wc();
        let before_u = cam.up_wc();
        let env = ControllerEnvironment {
            has_globe: self.terrain_provider.is_some(),
            ..Default::default()
        };
        let _ = self
            .camera_controller
            .update_with_environment(cam, time_ms / 1000.0, env);
        let after_p = cam.position_wc();
        let after_d = cam.direction_wc();
        let after_u = cam.up_wc();
        let after_r = cam.right_wc();
        let _ = cam.set_transform(Matrix4::IDENTITY);
        let moved = before_p != after_p || before_d != after_d || before_u != after_u;
        if moved {
            self.scene.request_render();
            vec![
                1.0, after_p.x, after_p.y, after_p.z, after_d.x, after_d.y, after_d.z, after_u.x, after_u.y,
                after_u.z, after_r.x, after_r.y, after_r.z,
            ]
        } else {
            vec![0.0]
        }
    }

    pub fn set_globe_visible(&mut self, visible: bool) {
        if self.destroyed {
            return;
        }
        self.globe.borrow_mut().show = visible;
        self.scene.request_render();
    }

    pub fn globe_visible(&self) -> bool {
        !self.destroyed && self.globe.borrow().show
    }

    pub fn set_sky(&mut self, atmosphere: bool, fog: bool, lighting: bool) {
        if self.destroyed {
            return;
        }
        self.scene.fog.enabled = fog;
        self.sky_atmosphere.borrow_mut().show = atmosphere;
        {
            let mut globe = self.globe.borrow_mut();
            let opts = globe.surface_options_mut();
            opts.show_ground_atmosphere = atmosphere;
            opts.enable_lighting = lighting;
        }
        self.scene.request_render();
    }

    pub fn set_surface_shading(&mut self, enabled: bool, strength: f64) {
        if self.destroyed {
            return;
        }
        self.shading_strength = if enabled { Some(strength.clamp(0.0, 1.0)) } else { None };
        self.apply_shading_material();
        self.scene.request_render();
    }

    pub fn set_split_position(&mut self, pos: f64) {
        if self.destroyed {
            return;
        }
        self.scene.frame_state.split_position = if pos.is_finite() { pos.clamp(0.0, 1.0) } else { 0.0 };
        self.scene.request_render();
    }

    pub fn get_split_position(&self) -> f64 {
        if self.destroyed {
            return 0.0;
        }
        self.scene.frame_state.split_position
    }

    pub fn set_vertical_exaggeration(&mut self, exaggeration: f64) {
        if self.destroyed {
            return;
        }
        let ex = if exaggeration.is_finite() && exaggeration >= 0.0 { exaggeration } else { 1.0 };
        self.scene.frame_state.vertical_exaggeration = ex;
        self.scene.request_render();
    }

    pub fn set_terrain_ellipsoid(&mut self, exaggeration: f64) {
        if self.destroyed {
            return;
        }
        let terrain = EllipsoidTerrainProvider::new(None, Some(Ellipsoid::WGS84), None);
        self.globe.borrow_mut().set_terrain_provider(Rc::new(GlobeTerrainProviderAdapter::new(terrain, None)));
        self.terrain_provider = None;
        self.set_vertical_exaggeration(exaggeration);
        if self.shading_strength.is_some() && self.shading_had_normals {
            self.apply_shading_material();
        }
        self.scene.request_render();
    }

    /// Asynchronously loads a quantized-mesh/heightmap terrain provider from a URL.
    pub fn load_terrain_url(&self, url: String, request_vertex_normals: bool) -> js_sys::Promise {
        let client = self.client.as_ref().clone();
        future_to_promise(async move {
            let resource = Resource::new(url).ok_or_else(|| js_err("Invalid terrain URL"))?;
            let opts = CesiumTerrainOptions {
                request_vertex_normals,
                request_water_mask: false,
                request_metadata: true,
                ellipsoid: Ellipsoid::WGS84,
                credit: None,
            };
            let provider = CesiumTerrainProvider::from_url(resource, opts, client).await.map_err(js_err)?;
            let has_vertex_normals = provider.has_vertex_normals();
            Ok(JsValue::from(LoadedTerrain { provider: Rc::new(provider), has_vertex_normals }))
        })
    }

    /// Asynchronously loads a Cesium ion terrain asset (e.g. World Terrain asset 1).
    pub fn load_ion_terrain(&self, asset_id: u64, access_token: String, request_vertex_normals: bool) -> js_sys::Promise {
        let client = self.client.as_ref().clone();
        future_to_promise(async move {
            let ion = Ion::default();
            let asset = IonAssetOptions { access_token: Some(access_token), ..Default::default() };
            let opts = CesiumTerrainOptions {
                request_vertex_normals,
                request_water_mask: false,
                request_metadata: true,
                ellipsoid: Ellipsoid::WGS84,
                credit: None,
            };
            let provider = CesiumTerrainProvider::from_ion_asset_id(asset_id, &ion, asset, opts, client)
                .await
                .map_err(js_err)?;
            let has_vertex_normals = provider.has_vertex_normals();
            Ok(JsValue::from(LoadedTerrain { provider: Rc::new(provider), has_vertex_normals }))
        })
    }

    /// Applies an async-loaded terrain provider synchronously to the globe.
    pub fn apply_loaded_terrain(&mut self, terrain: &LoadedTerrain, exaggeration: f64) {
        if self.destroyed {
            return;
        }
        let provider = (*terrain.provider).clone();
        self.globe.borrow_mut().set_terrain_provider(Rc::new(GlobeTerrainProviderAdapter::new(provider, None)));
        self.terrain_provider = Some(Rc::clone(&terrain.provider));
        self.set_vertical_exaggeration(exaggeration);
        if self.shading_strength.is_some() {
            self.apply_shading_material();
        }
        self.scene.request_render();
    }

    /// Samples the finest available terrain heights for packed `[lon_rad, lat_rad, ...]` coordinates.
    /// Resolves to a `Float64Array`-compatible JS array of heights (NaN for misses).
    pub fn sample_terrain_finest(&self, lon_lat_rad: Vec<f64>) -> js_sys::Promise {
        let provider = self.terrain_provider.clone();
        future_to_promise(async move {
            let count = lon_lat_rad.len() / 2;
            let Some(provider) = provider else {
                let zeros = js_sys::Float64Array::new_with_length(count as u32);
                for i in 0..count {
                    zeros.set_index(i as u32, 0.0);
                }
                return Ok(zeros.into());
            };
            let mut positions = Vec::with_capacity(count);
            for i in 0..count {
                positions.push(Cartographic::new(lon_lat_rad[i * 2], lon_lat_rad[i * 2 + 1], f64::NAN));
            }
            let host = BrowserSamplingHost;
            let _ = sample_terrain_most_detailed(provider.as_ref(), &mut positions, false, &host).await;
            let out = js_sys::Float64Array::new_with_length(count as u32);
            for (i, pos) in positions.iter().enumerate() {
                out.set_index(i as u32, pos.height);
            }
            Ok(out.into())
        })
    }

    /// Asynchronously loads a 3D Tileset from a URL (with optional Authorization header).
    pub fn load_tileset_url(&self, url: String, auth_header: Option<String>, credit_text: String) -> js_sys::Promise {
        let client = self.client.as_ref().clone();
        let context = self.context.clone();
        let cache = self.resource_cache.clone();
        let image_host = Rc::clone(&self.image_host);
        future_to_promise(async move {
            let resource = Resource::new(url).ok_or_else(|| js_err("Invalid tileset URL"))?;
            if let Some(auth) = auth_header.filter(|s| !s.is_empty()) {
                resource.options_mut().headers = vec![("Authorization".to_string(), auth)];
            }
            if !credit_text.is_empty() {
                resource.options_mut().credits = Some(vec![Credit::new(credit_text, true)]);
            }
            let factory: Rc<dyn RenderableTileFactory> = Rc::new(ModelTileFactory {
                context,
                cache,
                host: image_host,
                options: ModelOptions::default(),
            });
            let loader: Rc<dyn super::cesium_3d_tileset::TilesetContentLoader> =
                Rc::new(TilesetResourceLoader::new(client.clone(), Some(factory)));
            let opts = TilesetOptions {
                cache_bytes: 512 * 1024 * 1024,
                maximum_cache_overflow_bytes: 256 * 1024 * 1024,
                enable_collision: true,
                ..Default::default()
            };
            let tileset = Cesium3DTileset::from_url(resource, &client, opts, Some(loader)).await.map_err(js_err)?;
            Ok(JsValue::from(LoadedTileset { tileset: Rc::new(RefCell::new(tileset)) }))
        })
    }

    /// Asynchronously loads a 3D Tileset from Cesium ion (e.g. Google Photorealistic 3D Tiles asset 2275207).
    pub fn load_ion_tileset(&self, asset_id: u64, access_token: String) -> js_sys::Promise {
        let client = self.client.as_ref().clone();
        let context = self.context.clone();
        let cache = self.resource_cache.clone();
        let image_host = Rc::clone(&self.image_host);
        future_to_promise(async move {
            let ion = Ion::default();
            let ion_opts = IonAssetOptions { access_token: Some(access_token), ..Default::default() };
            let factory: Rc<dyn RenderableTileFactory> = Rc::new(ModelTileFactory {
                context,
                cache,
                host: image_host,
                options: ModelOptions::default(),
            });
            let loader: Rc<dyn super::cesium_3d_tileset::TilesetContentLoader> =
                Rc::new(TilesetResourceLoader::new(client.clone(), Some(factory)));
            let opts = TilesetOptions {
                cache_bytes: 512 * 1024 * 1024,
                maximum_cache_overflow_bytes: 256 * 1024 * 1024,
                enable_collision: true,
                ..Default::default()
            };
            let tileset = Cesium3DTileset::from_ion_asset_id(asset_id, &ion, ion_opts, &client, opts, Some(loader))
                .await
                .map_err(js_err)?;
            Ok(JsValue::from(LoadedTileset { tileset: Rc::new(RefCell::new(tileset)) }))
        })
    }

    pub fn set_photoreal_tileset(&mut self, tileset: Option<LoadedTileset>) {
        if self.destroyed {
            return;
        }
        // Detach any imagery layers currently hosted on the old tileset before replacing it.
        if let Some(old_ts) = self.photoreal_tileset.take() {
            let mut ts = old_ts.borrow_mut();
            for entry in self.imagery_layers.values_mut() {
                if entry.on_tileset {
                    ts.imagery_layers_mut().remove(&entry.handle, Some(false));
                    entry.on_tileset = false;
                }
            }
            let _ = ts.destroy();
        }
        if tileset.is_some() {
            self.ensure_brdf_lut();
        }
        self.photoreal_tileset = tileset.map(|t| t.tileset);
        self.rebuild_scene_primitives();
        self.scene.request_render();
    }

    pub fn set_photoreal_show(&mut self, show: bool) {
        if self.destroyed {
            return;
        }
        if let Some(ref ts) = self.photoreal_tileset {
            ts.borrow_mut().options.show = show;
            self.scene.request_render();
        }
    }

    pub fn has_shown_tileset(&self) -> bool {
        if self.destroyed {
            return false;
        }
        self.photoreal_tileset.as_ref().is_some_and(|ts| ts.borrow().options.show)
    }

    pub fn photoreal_tiles_loaded(&self) -> bool {
        if self.destroyed {
            return true;
        }
        match &self.photoreal_tileset {
            Some(ts) => {
                let t = ts.borrow();
                !t.options.show || t.tiles_loaded()
            }
            None => true,
        }
    }

    pub fn globe_pending_tiles(&self) -> u32 {
        if self.destroyed {
            return 0;
        }
        let g = self.globe.borrow();
        (g.surface.tile_load_queue_high.len()
            + g.surface.tile_load_queue_medium.len()
            + g.surface.tile_load_queue_low.len()) as u32
    }

    pub fn is_ellipsoid_terrain(&self) -> bool {
        self.terrain_provider.is_none()
    }

    pub fn tiles_loaded(&self) -> bool {
        if self.destroyed {
            return true;
        }
        self.globe.borrow().tiles_loaded()
    }

    pub fn icons_ready(&self) -> bool {
        if self.destroyed {
            return true;
        }
        for entry in self.draw_sets.values() {
            if let DrawSetHandle::Icons(icon_set) = &entry.handle {
                let col = icon_set.collection.borrow();
                if col.show && !icon_set.items.is_empty() && !col.ready() {
                    return false;
                }
            }
        }
        true
    }

    pub fn rendered_globe_tiles(&self) -> u32 {
        if self.destroyed {
            return 0;
        }
        self.globe.borrow().rendered_surface_tiles().len() as u32
    }

    /// Adds a Web Mercator or Geographic XYZ URL template imagery layer.
    #[allow(clippy::too_many_arguments)]
    pub fn add_xyz_imagery(
        &mut self,
        url: &str,
        subdomains_json: &str,
        geographic: bool,
        west_deg: f64,
        south_deg: f64,
        east_deg: f64,
        north_deg: f64,
        min_level: u32,
        max_level: i32,
        tile_size: u32,
        alpha: f64,
        show: bool,
        credit_text: &str,
        index: i32,
    ) -> Result<u32, JsValue> {
        if self.destroyed {
            return Err(js_err("renderer destroyed"));
        }
        let resource = Resource::new(url).ok_or_else(|| js_err("Invalid imagery URL"))?;
        let mut opts = UrlTemplateImageryProviderOptions::new(resource);
        let subs: Vec<String> = serde_json::from_str(subdomains_json).unwrap_or_default();
        if !subs.is_empty() {
            opts.subdomains = Some(subs);
        }
        opts.minimum_level = Some(min_level);
        if max_level >= 0 {
            opts.maximum_level = Some(max_level as u32);
        }
        if tile_size > 0 {
            opts.tile_width = Some(f64::from(tile_size));
            opts.tile_height = Some(f64::from(tile_size));
        }
        let tiling: Arc<dyn TilingScheme> = if geographic {
            Arc::new(GeographicTilingScheme::default())
        } else {
            Arc::new(WebMercatorTilingScheme::default())
        };
        opts.tiling_scheme = Some(tiling);
        if west_deg.is_finite() && south_deg.is_finite() && east_deg.is_finite() && north_deg.is_finite() {
            opts.rectangle = Some(Rectangle::from_degrees(west_deg, south_deg, east_deg, north_deg));
        }
        if !credit_text.is_empty() {
            opts.credit = Some(Credit::new(credit_text, false));
        }
        let loader = ImageryLoader::new(Rc::clone(&self.client), Rc::clone(&self.image_host));
        let provider = UrlTemplateImageryProvider::new(loader, opts).map_err(js_err)?;
        let layer_opts = ImageryLayerOptions { alpha: Some(alpha), show: Some(show), ..Default::default() };
        let layer = ImageryLayer::handle(Some(Arc::new(provider)), layer_opts);
        let idx = if index >= 0 { Some(index as usize) } else { None };
        self.globe.borrow_mut().imagery_layers_mut().add(Rc::clone(&layer), idx).map_err(js_err)?;
        let id = self.next_imagery_id;
        self.next_imagery_id += 1;
        self.imagery_layers.insert(id, ImageryEntry { handle: layer, on_tileset: false });
        self.scene.request_render();
        Ok(id)
    }

    /// Adds a single-image imagery layer over a geodetic rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn add_single_image_imagery(
        &mut self,
        url: &str,
        west_deg: f64,
        south_deg: f64,
        east_deg: f64,
        north_deg: f64,
        tile_width: u32,
        tile_height: u32,
        alpha: f64,
        show: bool,
        credit_text: &str,
        index: i32,
    ) -> Result<u32, JsValue> {
        if self.destroyed {
            return Err(js_err("renderer destroyed"));
        }
        let resource = Resource::new(url).ok_or_else(|| js_err("Invalid imagery URL"))?;
        let rectangle = if west_deg.is_finite() && south_deg.is_finite() && east_deg.is_finite() && north_deg.is_finite()
        {
            Some(Rectangle::from_degrees(west_deg, south_deg, east_deg, north_deg))
        } else {
            None
        };
        let credit = if credit_text.is_empty() { None } else { Some(Credit::new(credit_text, false)) };
        let loader = ImageryLoader::new(Rc::clone(&self.client), Rc::clone(&self.image_host));
        let provider = SingleTileImageryProvider::new(
            loader,
            SingleTileImageryProviderOptions {
                url: resource,
                tile_width: f64::from(tile_width.max(1)),
                tile_height: f64::from(tile_height.max(1)),
                rectangle,
                credit,
                ellipsoid: Some(Ellipsoid::WGS84),
            },
        );
        let layer_opts = ImageryLayerOptions { alpha: Some(alpha), show: Some(show), ..Default::default() };
        let layer = ImageryLayer::handle(Some(Arc::new(provider)), layer_opts);
        let idx = if index >= 0 { Some(index as usize) } else { None };
        self.globe.borrow_mut().imagery_layers_mut().add(Rc::clone(&layer), idx).map_err(js_err)?;
        let id = self.next_imagery_id;
        self.next_imagery_id += 1;
        self.imagery_layers.insert(id, ImageryEntry { handle: layer, on_tileset: false });
        self.scene.request_render();
        Ok(id)
    }

    /// Adds a JS-callback-backed imagery layer (powers `imageHooks`, `vectorBase`,
    /// `imageryFrames`, `imageryDecoded`, and custom tile hooks).
    #[allow(clippy::too_many_arguments)]
    pub fn add_js_imagery(
        &mut self,
        callback: js_sys::Function,
        geographic: bool,
        single_tile: bool,
        west_deg: f64,
        south_deg: f64,
        east_deg: f64,
        north_deg: f64,
        min_level: u32,
        max_level: i32,
        tile_size: u32,
        alpha: f64,
        show: bool,
        split_dir: i32,
        min_terrain_level: i32,
        credit_text: &str,
        index: i32,
        on_tileset: bool,
    ) -> Result<u32, JsValue> {
        if self.destroyed {
            return Err(js_err("renderer destroyed"));
        }
        let custom_rect = if west_deg.is_finite() && south_deg.is_finite() && east_deg.is_finite() && north_deg.is_finite()
        {
            Some(Rectangle::from_degrees(west_deg, south_deg, east_deg, north_deg))
        } else {
            None
        };
        let tiling_scheme: Arc<dyn TilingScheme> = if single_tile {
            Arc::new(GeographicTilingScheme::new(
                Ellipsoid::WGS84,
                custom_rect.unwrap_or(Rectangle::MAX_VALUE),
                1,
                1,
            ))
        } else if geographic {
            Arc::new(GeographicTilingScheme::default())
        } else {
            Arc::new(WebMercatorTilingScheme::default())
        };
        let rectangle = custom_rect.unwrap_or(*tiling_scheme.rectangle());
        let size = f64::from(if tile_size > 0 { tile_size } else { 256 });
        let provider = JsCallbackImageryProvider {
            callback,
            tiling_scheme,
            rectangle,
            tile_width: size,
            tile_height: size,
            min_level: if single_tile { 0 } else { min_level },
            max_level: if single_tile { Some(0) } else if max_level >= 0 { Some(max_level as u32) } else { None },
            credit: if credit_text.is_empty() { None } else { Some(Credit::new(credit_text, false)) },
            error_event: Rc::new(RefCell::new(MutableErrorEvent::new())),
        };
        let layer_opts = ImageryLayerOptions {
            alpha: Some(alpha.clamp(0.0, 1.0)),
            show: Some(show),
            split_direction: Some(split_direction_from_i32(split_dir)),
            minimum_terrain_level: if min_terrain_level >= 0 { Some(min_terrain_level as u32) } else { None },
            ..Default::default()
        };
        let layer = ImageryLayer::handle(Some(Arc::new(provider)), layer_opts);
        let idx = if index >= 0 { Some(index as usize) } else { None };
        let placed_on_tileset = if on_tileset {
            if let Some(ref ts) = self.photoreal_tileset {
                ts.borrow_mut().imagery_layers_mut().add(Rc::clone(&layer), idx).map_err(js_err)?;
                true
            } else {
                false
            }
        } else {
            self.globe.borrow_mut().imagery_layers_mut().add(Rc::clone(&layer), idx).map_err(js_err)?;
            false
        };
        let id = self.next_imagery_id;
        self.next_imagery_id += 1;
        self.imagery_layers.insert(id, ImageryEntry { handle: layer, on_tileset: placed_on_tileset });
        self.scene.request_render();
        Ok(id)
    }

    pub fn set_imagery_alpha(&mut self, id: u32, alpha: f64) {
        if let Some(entry) = self.imagery_layers.get(&id) {
            entry.handle.borrow_mut().alpha = alpha.clamp(0.0, 1.0);
            self.scene.request_render();
        }
    }

    pub fn set_imagery_show(&mut self, id: u32, show: bool) {
        if let Some(entry) = self.imagery_layers.get(&id) {
            entry.handle.borrow_mut().show = show;
            if entry.on_tileset {
                if let Some(ref ts) = self.photoreal_tileset {
                    ts.borrow_mut().imagery_layers_mut().update();
                }
            } else {
                self.globe.borrow_mut().imagery_layers_mut().update();
            }
            self.scene.request_render();
        }
    }

    pub fn set_imagery_split_direction(&mut self, id: u32, dir: i32) {
        if let Some(entry) = self.imagery_layers.get(&id) {
            entry.handle.borrow_mut().split_direction = split_direction_from_i32(dir);
            self.scene.request_render();
        }
    }

    /// Moves an imagery layer between the globe and the photoreal 3D tileset (or reorders it at `index`).
    pub fn move_imagery_layer(&mut self, id: u32, to_tileset: bool, index: i32) {
        let Some(entry) = self.imagery_layers.get_mut(&id) else {
            return;
        };
        let handle = Rc::clone(&entry.handle);
        let was_tileset = entry.on_tileset;
        // Remove from current host without destroying the layer.
        if was_tileset {
            if let Some(ref ts) = self.photoreal_tileset {
                ts.borrow_mut().imagery_layers_mut().remove(&handle, Some(false));
            }
        } else {
            self.globe.borrow_mut().imagery_layers_mut().remove(&handle, Some(false));
        }
        let idx = if index >= 0 { Some(index as usize) } else { None };
        if to_tileset {
            if let Some(ref ts) = self.photoreal_tileset {
                let mut t = ts.borrow_mut();
                let clamped = idx.map(|i| i.min(t.imagery_layers().length()));
                let _ = t.imagery_layers_mut().add(handle, clamped);
                entry.on_tileset = true;
            } else {
                entry.on_tileset = false;
            }
        } else {
            let mut g = self.globe.borrow_mut();
            let clamped = idx.map(|i| i.min(g.imagery_layers().length()));
            let _ = g.imagery_layers_mut().add(handle, clamped);
            entry.on_tileset = false;
        }
        self.scene.request_render();
    }

    /// Returns true if every currently rendered globe tile intersecting this layer's
    /// rectangle has drawn this layer from its own imagery (or settled on failure).
    pub fn is_imagery_layer_drawn(&self, id: u32) -> bool {
        if self.destroyed {
            return false;
        }
        let Some(entry) = self.imagery_layers.get(&id) else {
            return false;
        };
        if entry.on_tileset {
            return self.photoreal_tiles_loaded();
        }
        let (layer_id, layer_rect, is_ready) = {
            let l = entry.handle.borrow();
            (l.id(), *l.rectangle(), l.ready())
        };
        if !is_ready {
            return false;
        }
        let globe = self.globe.borrow();
        let tiles = globe.rendered_surface_tiles();
        if tiles.is_empty() {
            return false;
        }
        for tile_handle in tiles {
            let tile = tile_handle.borrow();
            let Some(data) = tile.data.as_ref().and_then(|d| d.as_any().downcast_ref::<GlobeSurfaceTile>())
            else {
                return false;
            };
            let mut found = false;
            for ti in &data.imagery {
                let matches = ti.layer_id == Some(layer_id)
                    || ti
                        .imagery_layer
                        .as_ref()
                        .is_some_and(|h| h.try_borrow().is_ok_and(|l| l.id() == layer_id));
                if !matches {
                    continue;
                }
                found = true;
                if let Some(ref loading) = ti.loading_imagery {
                    let st = loading.borrow().state;
                    if !matches!(st, ImageryState::Ready | ImageryState::Failed | ImageryState::Invalid) {
                        return false;
                    }
                    if st == ImageryState::Ready {
                        let same = ti.ready_imagery.as_ref().is_some_and(|r| Rc::ptr_eq(r, loading));
                        if !same {
                            return false;
                        }
                    }
                }
            }
            if !found && Rectangle::intersection(&tile.rectangle, &layer_rect).is_some() {
                return false;
            }
        }
        true
    }

    pub fn raise_imagery(&mut self, id: u32) {
        if let Some(entry) = self.imagery_layers.get(&id) {
            let layer = Rc::clone(&entry.handle);
            if entry.on_tileset {
                if let Some(ref ts) = self.photoreal_tileset {
                    let _ = ts.borrow_mut().imagery_layers_mut().raise(&layer);
                }
            } else {
                let _ = self.globe.borrow_mut().imagery_layers_mut().raise(&layer);
            }
            self.scene.request_render();
        }
    }

    pub fn lower_imagery(&mut self, id: u32) {
        if let Some(entry) = self.imagery_layers.get(&id) {
            let layer = Rc::clone(&entry.handle);
            if entry.on_tileset {
                if let Some(ref ts) = self.photoreal_tileset {
                    let _ = ts.borrow_mut().imagery_layers_mut().lower(&layer);
                }
            } else {
                let _ = self.globe.borrow_mut().imagery_layers_mut().lower(&layer);
            }
            self.scene.request_render();
        }
    }

    pub fn remove_imagery(&mut self, id: u32) {
        if let Some(entry) = self.imagery_layers.remove(&id) {
            if entry.on_tileset {
                if let Some(ref ts) = self.photoreal_tileset {
                    ts.borrow_mut().imagery_layers_mut().remove(&entry.handle, Some(true));
                }
            } else {
                self.globe.borrow_mut().imagery_layers_mut().remove(&entry.handle, Some(true));
            }
            self.scene.request_render();
        }
    }

    pub fn clear_imagery(&mut self) {
        self.imagery_layers.clear();
        self.globe.borrow_mut().imagery_layers_mut().remove_all(Some(true));
        if let Some(ref ts) = self.photoreal_tileset {
            ts.borrow_mut().imagery_layers_mut().remove_all(Some(true));
        }
        self.scene.request_render();
    }

    // ── Draw sets ──────────────────────────────────────────────────────────

    pub fn create_point_set(&mut self, id: &str, show: bool, pickable: bool, draw_order: f64) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let col = Rc::new(RefCell::new(PointPrimitiveCollection::new(PointPrimitiveCollectionOptions {
            show,
            blend_option: BlendOption::OpaqueAndTranslucent,
            ..Default::default()
        })));
        let seq = self.next_draw_seq;
        self.next_draw_seq += 1;
        self.draw_sets.insert(
            id.to_string(),
            DrawSetEntry {
                draw_order: if draw_order.is_finite() { draw_order } else { 0.0 },
                seq,
                pickable: Rc::new(Cell::new(pickable)),
                include_in_height_queries: Rc::new(Cell::new(false)),
                blend: Rc::new(Cell::new(SetBlendMode::Default)),
                handle: DrawSetHandle::Points(PointSetEntry { collection: col, items: HashMap::new() }),
            },
        );
        self.rebuild_scene_primitives();
        self.scene.request_render();
        Ok(())
    }

    pub fn replace_points(&mut self, set_id: &str, items_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Points(entry), .. }) = self.draw_sets.get_mut(set_id) else {
            return Ok(());
        };
        let items: Vec<JsPointItem> = serde_json::from_str(items_json).map_err(js_err)?;
        let mut col = entry.collection.borrow_mut();
        col.remove_all().map_err(js_err)?;
        entry.items.clear();
        for item in items {
            let color = color_from_rgba(item.color[0], item.color[1], item.color[2], item.color[3]);
            let outline_color = item
                .outline_color
                .map(|c| color_from_rgba(c[0], c[1], c[2], c[3]))
                .unwrap_or(Color::TRANSPARENT);
            let opts = PointPrimitiveOptions {
                show: item.show,
                position: Cartesian3::new(item.x, item.y, item.z),
                color,
                outline_color,
                outline_width: item.outline_width.max(0.0),
                pixel_size: item.pixel_size.max(1.0),
                scale_by_distance: near_far_from(item.scale_by_distance),
                translucency_by_distance: near_far_from(item.fade_by_distance),
                distance_display_condition: ddc_from(item.display_range),
                disable_depth_test_distance: disable_depth_from(item.disable_depth_test),
                id: Some(encode_item_pick_id(set_id, &item.id, item.pickable)),
                ..Default::default()
            };
            let pt = col.add(opts).map_err(js_err)?;
            entry.items.insert(item.id, pt);
        }
        self.scene.request_render();
        Ok(())
    }

    pub fn create_icon_set(&mut self, id: &str, show: bool, pickable: bool, draw_order: f64) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let col = Rc::new(RefCell::new(BillboardCollection::new(BillboardCollectionOptions {
            show,
            blend_option: BlendOption::OpaqueAndTranslucent,
            ..Default::default()
        })));
        let seq = self.next_draw_seq;
        self.next_draw_seq += 1;
        self.draw_sets.insert(
            id.to_string(),
            DrawSetEntry {
                draw_order: if draw_order.is_finite() { draw_order } else { 0.0 },
                seq,
                pickable: Rc::new(Cell::new(pickable)),
                include_in_height_queries: Rc::new(Cell::new(false)),
                blend: Rc::new(Cell::new(SetBlendMode::Default)),
                handle: DrawSetHandle::Icons(IconSetEntry {
                    collection: col,
                    items: HashMap::new(),
                    uploaded_images: HashSet::new(),
                }),
            },
        );
        self.rebuild_scene_primitives();
        self.scene.request_render();
        Ok(())
    }

    /// Upserts one billboard icon using RGBA8 pixels decoded on the JS side.
    /// When `image_id` is set and already uploaded to this icon set, `rgba` may be empty.
    pub fn upsert_icon_rgba(
        &mut self,
        set_id: &str,
        options_json: &str,
        img_width: u32,
        img_height: u32,
        rgba: &[u8],
    ) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Icons(entry), .. }) = self.draw_sets.get_mut(set_id) else {
            return Ok(());
        };
        let item: JsIconOptions = serde_json::from_str(options_json).map_err(js_err)?;
        let mut col = entry.collection.borrow_mut();
        if let Some(old) = entry.items.remove(&item.id) {
            let _ = col.remove(&old);
        }
        let opts = Self::build_billboard_options(set_id, &item);
        let already_uploaded = item.image_id.as_ref().is_some_and(|k| entry.uploaded_images.contains(k));
        let image = if !already_uploaded && img_width > 0 && img_height > 0 && rgba.len() >= (img_width * img_height * 4) as usize {
            if let Some(ref k) = item.image_id {
                entry.uploaded_images.insert(k.clone());
            }
            Some(BillboardImage::pixels(img_width, img_height, rgba.to_vec()))
        } else {
            Some(BillboardImage::pixels(1, 1, vec![255, 255, 255, 255]))
        };
        let bb = col.add(opts, image).map_err(js_err)?;
        entry.items.insert(item.id, bb);
        self.scene.request_render();
        Ok(())
    }

    /// Batch-upserts icons whose `image_id` has already been uploaded into this icon set's atlas.
    pub fn upsert_icons_cached(&mut self, set_id: &str, items_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Icons(entry), .. }) = self.draw_sets.get_mut(set_id) else {
            return Ok(());
        };
        let items: Vec<JsIconOptions> = serde_json::from_str(items_json).map_err(js_err)?;
        let mut col = entry.collection.borrow_mut();
        for item in items {
            if let Some(old) = entry.items.remove(&item.id) {
                let _ = col.remove(&old);
            }
            let opts = Self::build_billboard_options(set_id, &item);
            let image = Some(BillboardImage::pixels(1, 1, vec![255, 255, 255, 255]));
            let bb = col.add(opts, image).map_err(js_err)?;
            entry.items.insert(item.id, bb);
        }
        self.scene.request_render();
        Ok(())
    }

    pub fn remove_icon(&mut self, set_id: &str, item_id: &str) {
        if let Some(DrawSetEntry { handle: DrawSetHandle::Icons(entry), .. }) = self.draw_sets.get_mut(set_id) {
            if let Some(old) = entry.items.remove(item_id) {
                let _ = entry.collection.borrow_mut().remove(&old);
                self.scene.request_render();
            }
        }
    }

    pub fn clear_icon_set(&mut self, set_id: &str) {
        if let Some(DrawSetEntry { handle: DrawSetHandle::Icons(entry), .. }) = self.draw_sets.get_mut(set_id) {
            entry.items.clear();
            let _ = entry.collection.borrow_mut().remove_all();
            self.scene.request_render();
        }
    }

    pub fn create_line_set(&mut self, id: &str, show: bool, pickable: bool, draw_order: f64) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let col = Rc::new(RefCell::new(PolylineCollection::new(PolylineCollectionOptions {
            show,
            ..Default::default()
        })));
        let seq = self.next_draw_seq;
        self.next_draw_seq += 1;
        self.draw_sets.insert(
            id.to_string(),
            DrawSetEntry {
                draw_order: if draw_order.is_finite() { draw_order } else { 0.0 },
                seq,
                pickable: Rc::new(Cell::new(pickable)),
                include_in_height_queries: Rc::new(Cell::new(false)),
                blend: Rc::new(Cell::new(SetBlendMode::Default)),
                handle: DrawSetHandle::Lines(LineSetEntry { collection: col, items: HashMap::new() }),
            },
        );
        self.rebuild_scene_primitives();
        self.scene.request_render();
        Ok(())
    }

    pub fn replace_lines(&mut self, set_id: &str, items_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Lines(entry), .. }) = self.draw_sets.get_mut(set_id) else {
            return Ok(());
        };
        let items: Vec<JsLineItem> = serde_json::from_str(items_json).map_err(js_err)?;
        let mut col = entry.collection.borrow_mut();
        col.remove_all().map_err(js_err)?;
        entry.items.clear();
        for item in items {
            let positions = unpack_positions(&item.positions);
            if positions.len() < 2 {
                continue;
            }
            let material = Rc::new(RefCell::new(build_line_material(&item)));
            let opts = PolylineOptions {
                show: Some(item.show),
                width: Some(item.width_px.max(1.0)),
                r#loop: Some(false),
                material: Some(material),
                positions,
                id: Some(encode_item_pick_id(set_id, &item.id, item.pickable)),
                distance_display_condition: None,
            };
            let line = col.add(opts).map_err(js_err)?;
            entry.items.entry(item.id).or_default().push(line);
        }
        self.scene.request_render();
        Ok(())
    }

    pub fn create_shape_set(&mut self, id: &str, show: bool, pickable: bool, draw_order: f64) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let state = Rc::new(RefCell::new(ShapeSetState { show, primitives: Vec::new() }));
        let seq = self.next_draw_seq;
        self.next_draw_seq += 1;
        self.draw_sets.insert(
            id.to_string(),
            DrawSetEntry {
                draw_order: if draw_order.is_finite() { draw_order } else { 0.0 },
                seq,
                pickable: Rc::new(Cell::new(pickable)),
                include_in_height_queries: Rc::new(Cell::new(false)),
                blend: Rc::new(Cell::new(SetBlendMode::Default)),
                handle: DrawSetHandle::Shapes(state),
            },
        );
        self.rebuild_scene_primitives();
        self.scene.request_render();
        Ok(())
    }

    pub fn replace_shapes(&mut self, set_id: &str, items_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Shapes(state), .. }) = self.draw_sets.get(set_id) else {
            return Ok(());
        };
        let items: Vec<JsShapeItem> = serde_json::from_str(items_json).map_err(js_err)?;
        let mut state = state.borrow_mut();
        for p in &mut state.primitives {
            let _ = p.destroy();
        }
        state.primitives = build_shape_primitive(set_id, &items).unwrap_or_default();
        self.scene.request_render();
        Ok(())
    }

    pub fn set_draw_set_show(&mut self, set_id: &str, show: bool) {
        if self.destroyed {
            return;
        }
        match self.draw_sets.get(set_id).map(|e| &e.handle) {
            Some(DrawSetHandle::Points(entry)) => entry.collection.borrow_mut().show = show,
            Some(DrawSetHandle::Icons(entry)) => entry.collection.borrow_mut().show = show,
            Some(DrawSetHandle::Lines(entry)) => entry.collection.borrow_mut().show = show,
            Some(DrawSetHandle::Shapes(state)) => state.borrow_mut().show = show,
            Some(DrawSetHandle::Models(state)) => state.borrow_mut().show = show,
            None => return,
        }
        self.scene.request_render();
    }

    pub fn set_draw_set_order(&mut self, set_id: &str, draw_order: f64) {
        if self.destroyed {
            return;
        }
        let order = if draw_order.is_finite() { draw_order } else { 0.0 };
        let Some(entry) = self.draw_sets.get_mut(set_id) else {
            return;
        };
        if entry.draw_order.total_cmp(&order).is_ne() {
            entry.draw_order = order;
            self.rebuild_scene_primitives();
            self.scene.request_render();
        }
    }

    pub fn set_draw_set_pickable(&mut self, set_id: &str, pickable: bool) {
        if self.destroyed {
            return;
        }
        if let Some(entry) = self.draw_sets.get(set_id) {
            entry.pickable.set(pickable);
        }
    }

    pub fn set_draw_set_height_query(&mut self, set_id: &str, include: bool) {
        if self.destroyed {
            return;
        }
        if let Some(entry) = self.draw_sets.get(set_id) {
            entry.include_in_height_queries.set(include);
        }
    }

    pub fn set_draw_set_blend(&mut self, set_id: &str, blend: &str) {
        if self.destroyed {
            return;
        }
        let (mode, option) = match blend {
            "opaque" => (SetBlendMode::Opaque, BlendOption::Opaque),
            "translucent" => (SetBlendMode::Translucent, BlendOption::Translucent),
            "additive" => (SetBlendMode::Additive, BlendOption::Translucent),
            _ => (SetBlendMode::Default, BlendOption::OpaqueAndTranslucent),
        };
        let Some(entry) = self.draw_sets.get(set_id) else {
            return;
        };
        entry.blend.set(mode);
        match &entry.handle {
            DrawSetHandle::Points(e) => e.collection.borrow_mut().blend_option = option,
            DrawSetHandle::Icons(e) => e.collection.borrow_mut().blend_option = option,
            _ => {}
        }
        self.scene.request_render();
    }

    pub fn dispose_draw_set(&mut self, set_id: &str) {
        if let Some(entry) = self.draw_sets.remove(set_id) {
            match entry.handle {
                DrawSetHandle::Points(e) => {
                    let mut c = e.collection.borrow_mut();
                    c.show = false;
                    let _ = c.remove_all();
                }
                DrawSetHandle::Icons(e) => {
                    let mut c = e.collection.borrow_mut();
                    c.show = false;
                    let _ = c.remove_all();
                }
                DrawSetHandle::Lines(e) => {
                    let mut c = e.collection.borrow_mut();
                    c.show = false;
                    let _ = c.remove_all();
                }
                DrawSetHandle::Shapes(state) => {
                    let mut s = state.borrow_mut();
                    s.show = false;
                    for p in &mut s.primitives {
                        let _ = p.destroy();
                    }
                    s.primitives.clear();
                }
                DrawSetHandle::Models(state) => {
                    let mut s = state.borrow_mut();
                    s.show = false;
                    for item in s.items.values() {
                        let _ = item.model.borrow_mut().destroy();
                    }
                    s.items.clear();
                }
            }
            self.rebuild_scene_primitives();
            self.scene.request_render();
        }
    }

    // ── Queries & Picking ──────────────────────────────────────────────────

    /// GPU color pick at CSS pixel `(x, y)` with optional window `(w, h)`.
    /// Returns `"{set_id}\0{item_id}"` or `None`, skipping unpickable (`\x01`-prefixed) items.
    pub fn pick(&mut self, x: f64, y: f64, width: f64, height: f64) -> Result<Option<String>, JsValue> {
        let mut hits = self.drill_pick(x, y, width, height, 1)?;
        Ok(hits.pop())
    }

    /// GPU multi-hit drill pick at CSS pixel `(x, y)` returning `"{set_id}\0{item_id}"` entries.
    pub fn drill_pick(&mut self, x: f64, y: f64, width: f64, height: f64, limit: u32) -> Result<Vec<String>, JsValue> {
        if self.destroyed {
            return Ok(Vec::new());
        }
        let pos = Cartesian2::new(x * self.pixel_ratio, y * self.pixel_ratio);
        let w = (width.max(1.0) * self.pixel_ratio).max(1.0);
        let h = (height.max(1.0) * self.pixel_ratio).max(1.0);
        let max_hits = if limit == 0 { 64usize } else { limit as usize };

        let mut results = Vec::new();
        let mut seen = HashSet::new();
        let mut hidden_points: Vec<PointPrimitive> = Vec::new();
        let mut hidden_icons: Vec<Billboard<BrowserGl>> = Vec::new();
        let mut hidden_lines: Vec<Polyline> = Vec::new();
        let mut hidden_shapes: Vec<Rc<RefCell<ShapeSetState>>> = Vec::new();
        let mut hidden_models: Vec<Rc<RefCell<Model<BrowserGl>>>> = Vec::new();

        let mut pick_err: Option<JsValue> = None;
        let mut passes = 0usize;
        while results.len() < max_hits && passes < 64 {
            passes += 1;
            let picked = match self.scene.pick(pos, Some(w), Some(h)) {
                Ok(p) => p,
                Err(e) => {
                    pick_err = Some(js_err(e));
                    break;
                }
            };
            let Some(encoded) = picked.and_then(|obj| extract_pick_id(&obj)) else {
                break;
            };
            if !seen.insert(encoded.clone()) {
                break;
            }
            let (is_unpickable, stripped) = match encoded.strip_prefix('\x01') {
                Some(rest) => (true, rest),
                None => (false, encoded.as_str()),
            };
            if !is_unpickable {
                results.push(encoded.clone());
                if results.len() >= max_hits {
                    break;
                }
            }
            let Some((set_id, item_id)) = stripped.split_once('\0') else {
                break;
            };
            let mut hid_any = false;
            if let Some(entry) = self.draw_sets.get(set_id) {
                match &entry.handle {
                    DrawSetHandle::Points(e) => {
                        if let Some(pt) = e.items.get(item_id) {
                            if pt.show() {
                                pt.set_show(false);
                                hidden_points.push(pt.clone());
                                hid_any = true;
                            }
                        }
                    }
                    DrawSetHandle::Icons(e) => {
                        if let Some(bb) = e.items.get(item_id) {
                            if bb.show() {
                                bb.set_show(false);
                                hidden_icons.push(bb.clone());
                                hid_any = true;
                            }
                        }
                    }
                    DrawSetHandle::Lines(e) => {
                        if let Some(lines) = e.items.get(item_id) {
                            for ln in lines {
                                if ln.show() {
                                    ln.set_show(false);
                                    hidden_lines.push(ln.clone());
                                    hid_any = true;
                                }
                            }
                        }
                    }
                    DrawSetHandle::Shapes(state) => {
                        let mut s = state.borrow_mut();
                        if s.show {
                            s.show = false;
                            hidden_shapes.push(Rc::clone(state));
                            hid_any = true;
                        }
                    }
                    DrawSetHandle::Models(state) => {
                        let s = state.borrow();
                        if let Some(m_entry) = s.items.get(item_id) {
                            let mut m = m_entry.model.borrow_mut();
                            if m.options.show {
                                m.options.show = false;
                                hidden_models.push(Rc::clone(&m_entry.model));
                                hid_any = true;
                            }
                        }
                    }
                }
            }
            if !hid_any {
                break;
            }
        }

        for pt in hidden_points {
            pt.set_show(true);
        }
        for bb in hidden_icons {
            bb.set_show(true);
        }
        for ln in hidden_lines {
            ln.set_show(true);
        }
        for st in hidden_shapes {
            st.borrow_mut().show = true;
        }
        for m in hidden_models {
            m.borrow_mut().options.show = true;
        }

        if let Some(err) = pick_err {
            return Err(err);
        }
        Ok(results)
    }

    /// Depth-buffer pick at CSS pixel `(x, y)`. Returns `[x, y, z]` ECEF or empty.
    pub fn pick_position(&mut self, x: f64, y: f64) -> Vec<f64> {
        if self.destroyed || !self.scene.pick_position_supported() {
            return Vec::new();
        }
        let pos = Cartesian2::new(x * self.pixel_ratio, y * self.pixel_ratio);
        match self.scene.pick_position(pos) {
            Ok(Some(p)) if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() => vec![p.x, p.y, p.z],
            _ => Vec::new(),
        }
    }

    pub fn pick_position_supported(&self) -> bool {
        !self.destroyed && self.scene.pick_position_supported()
    }

    pub fn sample_height_supported(&self) -> bool {
        !self.destroyed && self.scene.sample_height_supported()
    }

    /// Ray-ellipsoid intersection for CSS pixel `(x, y)`. Returns `[x, y, z]` or empty.
    pub fn pick_ellipsoid(&mut self, x: f64, y: f64) -> Vec<f64> {
        if self.destroyed {
            return Vec::new();
        }
        let pos = Cartesian2::new(x * self.pixel_ratio, y * self.pixel_ratio);
        match self.scene.camera.pick_ellipsoid(pos, &Ellipsoid::WGS84) {
            Some(p) => vec![p.x, p.y, p.z],
            None => Vec::new(),
        }
    }

    /// Ray intersection against the globe surface (terrain tiles when loaded, else ellipsoid).
    pub fn pick_globe_ray(&self, ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64) -> Vec<f64> {
        if self.destroyed {
            return Vec::new();
        }
        let ray = Ray::new(Cartesian3::new(ox, oy, oz), Cartesian3::new(dx, dy, dz));
        if let Some(hit) = self.globe.borrow().pick(&ray, &self.scene.frame_state) {
            return vec![hit.x, hit.y, hit.z];
        }
        if self.globe.borrow().show {
            if let Some(interval) = ray_ellipsoid(&ray, &Ellipsoid::WGS84) {
                let t = if interval.start > 0.0 { interval.start } else { interval.stop };
                if t >= 0.0 {
                    let hit = ray.origin + ray.direction * t;
                    return vec![hit.x, hit.y, hit.z];
                }
            }
        }
        Vec::new()
    }

    /// Synchronous height query at `(lon_rad, lat_rad)` against `'surface' | 'terrain' | 'tiles'`.
    /// Returns `[height_m, source_code]` where `source_code`: 0=ellipsoid, 1=terrain, 2=tiles, 3=set;
    /// or empty Vec on miss. Draw sets without `include_in_height_queries` are ignored.
    pub fn sample_height_now(
        &mut self,
        lon_rad: f64,
        lat_rad: f64,
        against: &str,
        exclude_json: &str,
    ) -> Vec<f64> {
        if self.destroyed || !lon_rad.is_finite() || !lat_rad.is_finite() {
            return Vec::new();
        }
        let carto = Cartographic::new(lon_rad, lat_rad, 0.0);
        let is_ellipsoid = self.terrain_provider.is_none();
        let terrain_code = if is_ellipsoid { 0.0 } else { 1.0 };

        if against == "terrain" {
            if !self.globe.borrow().show {
                return Vec::new();
            }
            if let Some(h) = self.globe.borrow().get_height(&carto) {
                if h.is_finite() {
                    return vec![h, terrain_code];
                }
            }
            return if is_ellipsoid { vec![0.0, 0.0] } else { Vec::new() };
        }

        let excluded: HashSet<String> = serde_json::from_str::<Vec<String>>(exclude_json)
            .unwrap_or_default()
            .into_iter()
            .collect();

        // 1. Check active photoreal 3D tileset.
        let mut tiles_hit: Option<f64> = None;
        if let Some(ref ts) = self.photoreal_tileset {
            let t = ts.borrow();
            if t.options.show {
                if let Ok(Some(h)) = t.get_height(carto, &self.scene.frame_state, Some(&Ellipsoid::WGS84)) {
                    if h.is_finite() {
                        tiles_hit = Some(h);
                    }
                }
            }
        }

        // 2. Check draw sets explicitly opted into height queries (never ordinary sets!).
        let mut set_hit: Option<f64> = None;
        if self.has_active_ground_sets(&excluded) && self.scene.sample_height_supported() {
            let prev_globe = self.globe.borrow().show;
            let prev_ts = self.photoreal_tileset.as_ref().map(|ts| ts.borrow().options.show);
            let mut saved: Vec<(String, bool)> = Vec::new();
            for (id, entry) in &self.draw_sets {
                if !entry.include_in_height_queries.get() || excluded.contains(id) {
                    let prev = Self::set_draw_set_visible_raw(&entry.handle, false);
                    saved.push((id.clone(), prev));
                }
            }
            self.globe.borrow_mut().show = false;
            if let Some(ref ts) = self.photoreal_tileset {
                ts.borrow_mut().options.show = false;
            }
            if let Ok(Some(h)) = self.scene.sample_height(&carto, &[], None) {
                if h.is_finite() {
                    set_hit = Some(h);
                }
            }
            self.globe.borrow_mut().show = prev_globe;
            if let (Some(ts), Some(prev)) = (&self.photoreal_tileset, prev_ts) {
                ts.borrow_mut().options.show = prev;
            }
            for (id, prev) in saved {
                if let Some(entry) = self.draw_sets.get(&id) {
                    Self::set_draw_set_visible_raw(&entry.handle, prev);
                }
            }
        }

        if against == "tiles" {
            if let (Some(sh), Some(th)) = (set_hit, tiles_hit) {
                return if sh >= th { vec![sh, 3.0] } else { vec![th, 2.0] };
            }
            if let Some(sh) = set_hit {
                return vec![sh, 3.0];
            }
            if let Some(th) = tiles_hit {
                return vec![th, 2.0];
            }
            return Vec::new();
        }

        // against == "surface": prefer ground set / tileset if higher or present, else globe terrain/ellipsoid.
        let globe_hit = if self.globe.borrow().show {
            self.globe.borrow().get_height(&carto).filter(|h| h.is_finite()).or(if is_ellipsoid { Some(0.0) } else { None })
        } else {
            None
        };

        let mut best: Option<(f64, f64)> = None;
        for candidate in [
            globe_hit.map(|h| (h, terrain_code)),
            tiles_hit.map(|h| (h, 2.0)),
            set_hit.map(|h| (h, 3.0)),
        ]
        .into_iter()
        .flatten()
        {
            best = match best {
                None => Some(candidate),
                Some(prev) if candidate.0 > prev.0 => Some(candidate),
                Some(prev) => Some(prev),
            };
        }
        match best {
            Some((h, code)) => vec![h, code],
            None => Vec::new(),
        }
    }

    /// Casts a ray against `'surface' | 'terrain' | 'tiles'` (excluding non-ground draw sets).
    /// Returns `[x, y, z]` ECEF or empty Vec.
    #[allow(clippy::too_many_arguments)]
    pub fn ray_cast(
        &mut self,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        against: &str,
        exclude_json: &str,
        max_distance: f64,
    ) -> Vec<f64> {
        if self.destroyed {
            return Vec::new();
        }
        let dir = Cartesian3::new(dx, dy, dz);
        let Some(dir) = dir.try_normalize() else {
            return Vec::new();
        };
        let ray = Ray::new(Cartesian3::new(ox, oy, oz), dir);
        let max_d = if max_distance.is_finite() && max_distance > 0.0 { max_distance } else { f64::INFINITY };

        let mut best: Option<(f64, Cartesian3)> = None;
        let mut consider = |p: Cartesian3| {
            let d = (p - ray.origin).magnitude();
            if d.is_finite() && d <= max_d && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, p));
            }
        };

        if against == "terrain" || against == "surface" {
            if let Some(hit) = self.globe.borrow().pick(&ray, &self.scene.frame_state) {
                consider(hit);
            } else if self.globe.borrow().show && self.terrain_provider.is_none() {
                if let Some(interval) = ray_ellipsoid(&ray, &Ellipsoid::WGS84) {
                    let t = if interval.start > 0.0 { interval.start } else { interval.stop };
                    if t >= 0.0 {
                        consider(ray.origin + ray.direction * t);
                    }
                }
            }
        }

        if against == "tiles" || against == "surface" {
            if let Some(ref ts) = self.photoreal_tileset {
                let t = ts.borrow();
                if t.options.show {
                    if let Ok(Some(hit)) = t.pick(&ray, &self.scene.frame_state, true) {
                        consider(hit);
                    }
                }
            }
            let excluded: HashSet<String> = serde_json::from_str::<Vec<String>>(exclude_json)
                .unwrap_or_default()
                .into_iter()
                .collect();
            if self.has_active_ground_sets(&excluded) && self.scene.sample_height_supported() {
                let prev_globe = self.globe.borrow().show;
                let prev_ts = self.photoreal_tileset.as_ref().map(|ts| ts.borrow().options.show);
                let mut saved: Vec<(String, bool)> = Vec::new();
                for (id, entry) in &self.draw_sets {
                    if !entry.include_in_height_queries.get() || excluded.contains(id) {
                        let prev = Self::set_draw_set_visible_raw(&entry.handle, false);
                        saved.push((id.clone(), prev));
                    }
                }
                self.globe.borrow_mut().show = false;
                if let Some(ref ts) = self.photoreal_tileset {
                    ts.borrow_mut().options.show = false;
                }
                if let Ok(Some(res)) = self.scene.pick_from_ray(ray, &[], None) {
                    if let Some(p) = res.position {
                        consider(p);
                    }
                }
                self.globe.borrow_mut().show = prev_globe;
                if let (Some(ts), Some(prev)) = (&self.photoreal_tileset, prev_ts) {
                    ts.borrow_mut().options.show = prev;
                }
                for (id, prev) in saved {
                    if let Some(entry) = self.draw_sets.get(&id) {
                        Self::set_draw_set_visible_raw(&entry.handle, prev);
                    }
                }
            }
        }

        match best {
            Some((_, p)) => vec![p.x, p.y, p.z],
            None => Vec::new(),
        }
    }

    /// Whether an ECEF point is occluded behind the WGS84 ellipsoid horizon from the current eye.
    pub fn is_occluded(&self, x: f64, y: f64, z: f64) -> bool {
        let occluder = Occluder::new(
            &BoundingSphere::new(Cartesian3::ZERO, Ellipsoid::WGS84.minimum_radius),
            self.scene.camera.position,
        );
        !occluder.is_point_visible(Cartesian3::new(x, y, z))
    }

    /// Reads RGBA8 pixels from the default framebuffer (top-left origin flipped to standard ImageData order).
    pub fn read_pixels(&self, x: i32, y: i32, width: u32, height: u32) -> Result<Vec<u8>, JsValue> {
        if self.destroyed {
            return Err(js_err("renderer destroyed"));
        }
        let gl = self.context.host();
        let w = width.max(1);
        let h = height.max(1);
        let buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let result = gl
            .invoke(
                "readPixels",
                &[
                    x.into(),
                    y.into(),
                    w.into(),
                    h.into(),
                    gl::RGBA.into(),
                    gl::UNSIGNED_BYTE.into(),
                    crate::renderer::GlValue::Bytes(buf),
                ],
            )
            .map_err(js_err)?;
        let mut bytes = match result {
            crate::renderer::GlValue::Bytes(b) => b,
            _ => return Err(js_err("readPixels returned unexpected value")),
        };
        // Flip vertically from GL bottom-left origin to ImageData top-left origin.
        let row_bytes = (w as usize) * 4;
        for row in 0..(h as usize / 2) {
            let top = row * row_bytes;
            let bot = (h as usize - 1 - row) * row_bytes;
            for col in 0..row_bytes {
                bytes.swap(top + col, bot + col);
            }
        }
        Ok(bytes)
    }

    // ── Phase 3: Fast in-place patches (motionSegments, columns, screenModulation) ──

    pub fn patch_icons(&mut self, set_id: &str, patches_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Icons(entry), .. }) = self.draw_sets.get_mut(set_id) else {
            return Ok(());
        };
        let patches: Vec<JsIconPatch> = serde_json::from_str(patches_json).map_err(js_err)?;
        let mut changed = false;
        for p in patches {
            let Some(bb) = entry.items.get(&p.id) else {
                continue;
            };
            if let Some([x, y, z]) = p.pos {
                bb.set_position(Cartesian3::new(x, y, z));
                changed = true;
            }
            if let Some(rot) = p.rotation {
                bb.set_rotation(rot);
                changed = true;
            }
            if let Some(sc) = p.scale {
                bb.set_scale(sc);
                changed = true;
            }
            if let Some(c) = p.color {
                bb.set_color(color_from_rgba(c[0], c[1], c[2], c[3]));
                changed = true;
            }
            if let Some(sh) = p.show {
                bb.set_show(sh);
                changed = true;
            }
        }
        if changed {
            self.scene.request_render();
        }
        Ok(())
    }

    pub fn patch_points(&mut self, set_id: &str, patches_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Points(entry), .. }) = self.draw_sets.get_mut(set_id) else {
            return Ok(());
        };
        let patches: Vec<JsPointPatch> = serde_json::from_str(patches_json).map_err(js_err)?;
        let mut changed = false;
        for p in patches {
            let Some(pt) = entry.items.get(&p.id) else {
                continue;
            };
            if let Some([x, y, z]) = p.pos {
                pt.set_position(Cartesian3::new(x, y, z));
                changed = true;
            }
            if let Some(sz) = p.pixel_size {
                pt.set_pixel_size(sz.max(1.0));
                changed = true;
            }
            if let Some(c) = p.color {
                pt.set_color(color_from_rgba(c[0], c[1], c[2], c[3]));
                changed = true;
            }
            if let Some(oc) = p.outline_color {
                pt.set_outline_color(color_from_rgba(oc[0], oc[1], oc[2], oc[3]));
                changed = true;
            }
            if let Some(sh) = p.show {
                pt.set_show(sh);
                changed = true;
            }
        }
        if changed {
            self.scene.request_render();
        }
        Ok(())
    }

    // ── Phase 3: Post-processing stages & bloom ─────────────────────────────

    pub fn add_post_process_stage(
        &mut self,
        name: &str,
        fragment_shader: &str,
        uniforms_json: &str,
        enabled: bool,
    ) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let uniforms = parse_pp_uniforms(uniforms_json).map_err(js_err)?;
        let mut opts = PostProcessStageOptions::new(fragment_shader.to_string());
        opts.name = Some(name.to_string());
        opts.uniforms = uniforms;
        let mut stage = PostProcessStage::new(opts).map_err(js_err)?;
        stage.set_enabled(enabled);
        let mut col = self.post_process_collection.borrow_mut();
        let _ = col.remove_by_name(name);
        col.add(stage).map_err(js_err)?;
        self.scene.request_render();
        Ok(())
    }

    pub fn set_post_process_stage_enabled(&mut self, name: &str, enabled: bool) {
        if self.destroyed {
            return;
        }
        let mut col = self.post_process_collection.borrow_mut();
        if let Some(stage) = col.get_stage_by_name_mut(name) {
            stage.set_enabled(enabled);
            self.scene.request_render();
        }
    }

    pub fn set_post_process_stage_uniforms(&mut self, name: &str, uniforms_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let uniforms = parse_pp_uniforms(uniforms_json).map_err(js_err)?;
        let mut col = self.post_process_collection.borrow_mut();
        if let Some(PostProcessStageItem::Stage(stage)) = col.get_stage_by_name_mut(name) {
            for (k, v) in uniforms {
                stage.set_uniform(k, v);
            }
            self.scene.request_render();
        }
        Ok(())
    }

    pub fn remove_post_process_stage(&mut self, name: &str) {
        if self.destroyed {
            return;
        }
        let mut col = self.post_process_collection.borrow_mut();
        if col.remove_by_name(name).unwrap_or(false) {
            self.scene.request_render();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_bloom(
        &mut self,
        enabled: bool,
        contrast: f64,
        brightness: f64,
        delta: f64,
        sigma: f64,
        step_size: f64,
    ) {
        if self.destroyed {
            return;
        }
        let mut col = self.post_process_collection.borrow_mut();
        let bloom = col.bloom_mut();
        bloom.set_enabled(enabled);
        if enabled {
            bloom.set_uniform("glowOnly", false);
            bloom.set_uniform("contrast", contrast as f32);
            bloom.set_uniform("brightness", brightness as f32);
            bloom.set_uniform("delta", delta as f32);
            bloom.set_uniform("sigma", sigma as f32);
            bloom.set_uniform("stepSize", step_size as f32);
        }
        self.scene.request_render();
    }

    // ── Phase 3: 3D Models (models, modelNodes, modelParts) ─────────────────

    pub fn load_model_url(&self, url: String) -> js_sys::Promise {
        let cache = self.resource_cache.clone();
        let context = self.context.clone();
        let host = Rc::clone(&self.image_host);
        future_to_promise(async move {
            let resource = Resource::new(url).ok_or_else(|| js_err("Invalid model URL"))?;
            let model = Model::from_gltf(cache, resource, None, None, context, host, ModelOptions::default())
                .await
                .map_err(js_err)?;
            Ok(JsValue::from(LoadedModel { model: Rc::new(RefCell::new(model)) }))
        })
    }

    pub fn load_model_bytes(&self, url: String, bytes: Vec<u8>) -> js_sys::Promise {
        let cache = self.resource_cache.clone();
        let context = self.context.clone();
        let host = Rc::clone(&self.image_host);
        future_to_promise(async move {
            let resource = Resource::new(url)
                .or_else(|| Resource::new("memory://model.glb"))
                .ok_or_else(|| js_err("Invalid model resource"))?;
            let input = Some(GltfJsonInput::Bytes(BufferSource::new(bytes)));
            let model = Model::from_gltf(cache, resource, None, input, context, host, ModelOptions::default())
                .await
                .map_err(js_err)?;
            Ok(JsValue::from(LoadedModel { model: Rc::new(RefCell::new(model)) }))
        })
    }

    pub fn create_model_set(&mut self, id: &str, show: bool, pickable: bool, draw_order: f64) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        self.ensure_brdf_lut();
        let state = Rc::new(RefCell::new(ModelSetState { show, items: HashMap::new() }));
        let seq = self.next_draw_seq;
        self.next_draw_seq += 1;
        self.draw_sets.insert(
            id.to_string(),
            DrawSetEntry {
                draw_order: if draw_order.is_finite() { draw_order } else { 0.0 },
                seq,
                pickable: Rc::new(Cell::new(pickable)),
                include_in_height_queries: Rc::new(Cell::new(false)),
                blend: Rc::new(Cell::new(SetBlendMode::Default)),
                handle: DrawSetHandle::Models(state),
            },
        );
        self.rebuild_scene_primitives();
        self.scene.request_render();
        Ok(())
    }

    pub fn attach_model(
        &mut self,
        set_id: &str,
        item_id: &str,
        loaded: LoadedModel,
        options_json: &str,
    ) -> Result<(), JsValue> {
        if self.destroyed {
            let _ = loaded.model.borrow_mut().destroy();
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Models(state), .. }) = self.draw_sets.get(set_id) else {
            let _ = loaded.model.borrow_mut().destroy();
            return Ok(());
        };
        let opts: JsModelOptions = serde_json::from_str(options_json).map_err(js_err)?;
        let mut entry = ModelItemEntry {
            model: loaded.model,
            parts_shader: None,
            parts_rows: 0,
            parts_key: String::new(),
            base_shader: None,
            base_shader_key: String::new(),
        };
        Self::apply_model_options(&self.context, set_id, item_id, &mut entry, &opts).map_err(js_err)?;
        let mut st = state.borrow_mut();
        if let Some(old) = st.items.insert(item_id.to_string(), entry) {
            let _ = old.model.borrow_mut().destroy();
        }
        self.scene.request_render();
        Ok(())
    }

    pub fn update_model(&mut self, set_id: &str, item_id: &str, options_json: &str) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Models(state), .. }) = self.draw_sets.get(set_id) else {
            return Ok(());
        };
        let opts: JsModelOptions = serde_json::from_str(options_json).map_err(js_err)?;
        let mut st = state.borrow_mut();
        if let Some(entry) = st.items.get_mut(item_id) {
            Self::apply_model_options(&self.context, set_id, item_id, entry, &opts).map_err(js_err)?;
            self.scene.request_render();
        }
        Ok(())
    }

    pub fn set_model_node_matrix(&mut self, set_id: &str, item_id: &str, node_name: &str, matrix: &[f64]) {
        if self.destroyed {
            return;
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Models(state), .. }) = self.draw_sets.get(set_id) else {
            return;
        };
        let st = state.borrow();
        let Some(entry) = st.items.get(item_id) else {
            return;
        };
        let mut model = entry.model.borrow_mut();
        if !model.ready() {
            return;
        }
        let node = model.get_node(node_name).ok().flatten().cloned();
        if let (Some(node), Some(graph)) = (node, model.scene_graph.as_mut()) {
            if matrix.len() >= 16 {
                let mut arr = [0.0f64; 16];
                arr.copy_from_slice(&matrix[..16]);
                node.set_matrix(graph, Some(&Matrix4(arr)));
            } else {
                node.set_matrix(graph, None);
                graph.user_animation_dirty = true;
            }
            self.scene.request_render();
        }
    }

    pub fn set_model_parts(
        &mut self,
        set_id: &str,
        item_id: &str,
        shading: &str,
        mask_rgb: &[f64],
        rows: u32,
        floats: &[f32],
    ) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let Some(DrawSetEntry { handle: DrawSetHandle::Models(state), .. }) = self.draw_sets.get(set_id) else {
            return Ok(());
        };
        let mut st = state.borrow_mut();
        let Some(entry) = st.items.get_mut(item_id) else {
            return Ok(());
        };
        if rows == 0 || floats.len() < (rows as usize) * 32 {
            if entry.parts_shader.is_some() {
                entry.parts_shader = None;
                entry.parts_rows = 0;
                entry.parts_key.clear();
                entry.model.borrow_mut().custom_shader = entry.base_shader.clone();
                self.scene.request_render();
            }
            return Ok(());
        }
        let mask_color = if mask_rgb.len() >= 3 { Some([mask_rgb[0], mask_rgb[1], mask_rgb[2]]) } else { None };
        let mask_key = mask_color.map(|m| format!("{:.4},{:.4},{:.4}", m[0], m[1], m[2])).unwrap_or_default();
        let key = format!("{shading}|{mask_key}|{rows}");
        if entry.parts_key == key {
            if let Some(ref shader) = entry.parts_shader {
                let tex = build_parts_texture_uniform(rows, floats);
                shader.set_uniform("u_partMatrices", CustomShaderUniformValue::Sampler2D(tex)).map_err(js_err)?;
                self.scene.request_render();
                return Ok(());
            }
        }
        let shader = build_parts_model_shader(shading, mask_color, rows, floats).map_err(js_err)?;
        entry.parts_shader = Some(Rc::clone(&shader));
        entry.parts_rows = rows;
        entry.parts_key = key;
        entry.model.borrow_mut().custom_shader = Some(shader);
        self.scene.request_render();
        Ok(())
    }

    pub fn remove_model(&mut self, set_id: &str, item_id: &str) {
        if let Some(DrawSetEntry { handle: DrawSetHandle::Models(state), .. }) = self.draw_sets.get(set_id) {
            if let Some(old) = state.borrow_mut().items.remove(item_id) {
                let _ = old.model.borrow_mut().destroy();
                self.scene.request_render();
            }
        }
    }

    pub fn clear_models(&mut self, set_id: &str) {
        if let Some(DrawSetEntry { handle: DrawSetHandle::Models(state), .. }) = self.draw_sets.get(set_id) {
            let mut st = state.borrow_mut();
            for old in st.items.values() {
                let _ = old.model.borrow_mut().destroy();
            }
            st.items.clear();
            self.scene.request_render();
        }
    }

    /// Returns `[ready (0/1), bounding_radius, computed_scale]` for a model item, or empty if absent.
    pub fn model_info(&self, set_id: &str, item_id: &str) -> Vec<f64> {
        let Some(DrawSetEntry { handle: DrawSetHandle::Models(state), .. }) = self.draw_sets.get(set_id) else {
            return Vec::new();
        };
        let st = state.borrow();
        let Some(entry) = st.items.get(item_id) else {
            return Vec::new();
        };
        let m = entry.model.borrow();
        let ready = if m.ready() { 1.0 } else { 0.0 };
        let radius = m.bounding_sphere().map(|s| s.radius).unwrap_or(0.0);
        let scale = m.computed_scale();
        vec![ready, radius, scale]
    }

    pub fn models_ready(&self) -> bool {
        if self.destroyed {
            return true;
        }
        for entry in self.draw_sets.values() {
            if let DrawSetHandle::Models(state) = &entry.handle {
                let st = state.borrow();
                if !st.show {
                    continue;
                }
                for item in st.items.values() {
                    if !item.model.borrow().ready() {
                        return false;
                    }
                }
            }
        }
        true
    }

    // ── Phase 3: Raster shell fields & dynamic textures ─────────────────────

    pub fn upload_texture_rgba(&mut self, tex_id: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let w = width.max(1);
        let h = height.max(1);
        let mut opts = TextureOptions::rgba(w, h);
        opts.flip_y = true;
        opts.sampler = Sampler {
            wrap_r: TextureWrap::ClampToEdge,
            wrap_s: TextureWrap::ClampToEdge,
            wrap_t: TextureWrap::ClampToEdge,
            minification_filter: TextureMinificationFilter::Linear,
            magnification_filter: TextureMagnificationFilter::Linear,
            maximum_anisotropy: 1.0,
        };
        let tex = Rc::new(Texture::new(&self.context, opts, Some(rgba)).map_err(js_err)?);
        self.context.with_texture_cache(|cache| cache.replace_texture(tex_id, tex)).map_err(js_err)?;
        self.scene.request_render();
        Ok(())
    }

    pub fn remove_texture(&mut self, tex_id: &str) {
        if self.destroyed {
            return;
        }
        let _ = self.context.with_texture_cache(|cache| cache.remove_texture(tex_id));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_raster_shell(
        &mut self,
        id: &str,
        west_deg: f64,
        south_deg: f64,
        east_deg: f64,
        north_deg: f64,
        height_m: f64,
        image_tex_id: &str,
        detail_tex_id: &str,
        window: &[f64],
        alpha: f64,
        lift_m: f64,
        show: bool,
    ) -> Result<(), JsValue> {
        if self.destroyed {
            return Ok(());
        }
        let rect = Rectangle::from_degrees(west_deg, south_deg, east_deg, north_deg);
        let mut geom_opts = RectangleGeometryOptions::new(rect);
        geom_opts.height = height_m;
        geom_opts.granularity = 0.5_f64.to_radians();
        geom_opts.vertex_format = EllipsoidSurfaceAppearance::VERTEX_FORMAT;
        let geometry = RectangleGeometry::new(geom_opts)
            .create_geometry()
            .ok_or_else(|| js_err("Failed to create rasterShell geometry"))?;

        let win = if window.len() >= 4 {
            Cartesian4::new(window[0], window[1], window[2], window[3])
        } else {
            Cartesian4::new(1.0, 1.0, 0.0, 0.0)
        };
        let detail_img = if detail_tex_id.is_empty() {
            MaterialImage::Default
        } else {
            MaterialImage::Texture(detail_tex_id.to_string())
        };
        let fabric = Fabric::new("GEVRasterShell")
            .uniform("image", MaterialValue::Sampler2D(MaterialImage::Texture(image_tex_id.to_string())))
            .uniform("detail", MaterialValue::Sampler2D(detail_img))
            .uniform("window", MaterialValue::Vec4(win))
            .uniform("alpha", MaterialValue::Float(alpha.clamp(0.0, 1.0)))
            .source(RASTER_SHELL_MATERIAL_SOURCE);
        let material = Material::try_new(MaterialOptions { fabric, translucent: Some(true), ..Default::default() })
            .map_err(js_err)?;

        let render_state = RenderState {
            depth_test: DepthTest { enabled: true, ..Default::default() },
            depth_mask: false,
            blending: blending_state::alpha_blend(),
            ..Default::default()
        };
        let appearance = EllipsoidSurfaceAppearance::new(EllipsoidSurfaceAppearanceOptions {
            flat: Some(true),
            translucent: Some(true),
            above_ground: Some(true),
            material: Some(material),
            render_state: Some(render_state),
            ..Default::default()
        });
        let lift_scale = 1.0 + lift_m / Ellipsoid::WGS84.maximum_radius;
        let mut primitive = Primitive::new(PrimitiveOptions {
            geometry_instances: vec![GeometryInstance::new(geometry, None)],
            appearance: Some(Box::new(appearance)),
            asynchronous: Some(false),
            allow_picking: Some(false),
            model_matrix: Some(Matrix4::from_uniform_scale(lift_scale)),
            ..Default::default()
        });
        primitive.show = show;
        let state = Rc::new(RefCell::new(RasterShellState { show, height_m, primitive }));
        if let Some(old) = self.raster_shells.insert(id.to_string(), state) {
            let _ = old.borrow_mut().primitive.destroy();
        }
        self.rebuild_scene_primitives();
        self.scene.request_render();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_raster_shell(
        &mut self,
        id: &str,
        image_tex_id: &str,
        detail_tex_id: &str,
        window: &[f64],
        alpha: f64,
        lift_m: f64,
        show: bool,
    ) {
        if self.destroyed {
            return;
        }
        let Some(state) = self.raster_shells.get(id) else {
            return;
        };
        let mut st = state.borrow_mut();
        st.show = show;
        st.primitive.show = show;
        let lift_scale = 1.0 + lift_m / Ellipsoid::WGS84.maximum_radius;
        st.primitive.model_matrix = Matrix4::from_uniform_scale(lift_scale);
        if let Some(app) = st.primitive.appearance.as_deref_mut() {
            if let Some(mat) = app.material_mut() {
                if !image_tex_id.is_empty() {
                    let _ = mat.set_uniform("image", MaterialValue::Sampler2D(MaterialImage::Texture(image_tex_id.to_string())));
                }
                let detail_img = if detail_tex_id.is_empty() {
                    MaterialImage::Default
                } else {
                    MaterialImage::Texture(detail_tex_id.to_string())
                };
                let _ = mat.set_uniform("detail", MaterialValue::Sampler2D(detail_img));
                if window.len() >= 4 {
                    let _ = mat.set_uniform(
                        "window",
                        MaterialValue::Vec4(Cartesian4::new(window[0], window[1], window[2], window[3])),
                    );
                }
                let _ = mat.set_uniform("alpha", MaterialValue::Float(alpha.clamp(0.0, 1.0)));
            }
        }
        self.scene.request_render();
    }

    pub fn remove_raster_shell(&mut self, id: &str) {
        if let Some(old) = self.raster_shells.remove(id) {
            let _ = old.borrow_mut().primitive.destroy();
            self.rebuild_scene_primitives();
            self.scene.request_render();
        }
    }
}
