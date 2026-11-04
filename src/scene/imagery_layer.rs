//! CesiumJS `ImageryLayer`: displays tiled image data from a single imagery provider
//! on a globe or 3D tileset.
//!
//! Handles tile skeleton generation, tile request lifecycle, asynchronous image
//! loading, request throttling and retry, discard policy checks, and quadtree
//! ancestor imagery fallback. Polling uses a no-op waker and requires the host
//! to call the state machine again while requests are pending. Skeleton texture
//! rectangles and antimeridian coverage remain incomplete.
//!
//! Attach an ImageryTexturePipeline for renderer texture uploads and deferred
//! Web Mercator reprojection. Drain its typed commands into the compute pass.
//!
//! `fromProviderAsync` and `fromWorldImagery` keep the provider future on the
//! layer. The host drives it with [`ImageryLayer::poll_provider`] (or
//! `ImageryLayerCollection::poll_providers`), which raises `readyEvent` with
//! the provider, as Cesium's promise continuation does.

use super::frame_state::FrameState;
use super::imagery::{Imagery, ImageryCache};
use super::imagery_provider::{
    image_error, report_image_error, ImageryProvider, ImageryProviderError,
};
use super::imagery_state::ImageryState;
use super::model::ImageryLayerSettings;
use super::split_direction::SplitDirection;
use super::tile_imagery::TileImagery;
use crate::core::{
    Cartesian4, Color, Event, Rectangle, RuntimeError, TilingScheme, WebMercatorProjection,
};
use crate::renderer::{TextureMagnificationFilter, TextureMinificationFilter};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// A provider that is still being created, as passed to `fromProviderAsync`.
pub type ImageryProviderFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<Arc<dyn ImageryProvider>, RuntimeError>>>>;

static NEXT_IMAGERY_LAYER_ID: AtomicU64 = AtomicU64::new(1);

/// A shared handle to a mutable [`ImageryLayer`].
pub type ImageryLayerHandle = Rc<RefCell<ImageryLayer>>;

/// Initialization options for an [`ImageryLayer`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageryLayerOptions {
    pub rectangle: Option<Rectangle>,
    pub alpha: Option<f64>,
    pub night_alpha: Option<f64>,
    pub day_alpha: Option<f64>,
    pub brightness: Option<f64>,
    pub contrast: Option<f64>,
    pub hue: Option<f64>,
    pub saturation: Option<f64>,
    pub gamma: Option<f64>,
    pub split_direction: Option<SplitDirection>,
    pub minification_filter: Option<TextureMinificationFilter>,
    pub magnification_filter: Option<TextureMagnificationFilter>,
    pub show: Option<bool>,
    pub maximum_anisotropy: Option<f64>,
    pub minimum_terrain_level: Option<u32>,
    pub maximum_terrain_level: Option<u32>,
    pub cutout_rectangle: Option<Rectangle>,
    pub color_to_alpha: Option<Color>,
    pub color_to_alpha_threshold: Option<f64>,
}

/// An imagery layer that displays tiled image data from a single imagery provider.
pub struct ImageryLayer {
    imagery_provider: Option<Arc<dyn ImageryProvider>>,
    rectangle: Rectangle,
    pub alpha: f64,
    pub night_alpha: f64,
    pub day_alpha: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub hue: f64,
    pub saturation: f64,
    pub gamma: f64,
    pub split_direction: SplitDirection,
    pub minification_filter: TextureMinificationFilter,
    pub magnification_filter: TextureMagnificationFilter,
    pub show: bool,
    pub maximum_anisotropy: Option<f64>,
    pub minimum_terrain_level: Option<u32>,
    pub maximum_terrain_level: Option<u32>,
    pub cutout_rectangle: Option<Rectangle>,
    pub color_to_alpha: Option<Color>,
    pub color_to_alpha_threshold: f64,

    // Internal state
    pub(crate) id: u64,
    pub(crate) weak_self: Option<std::rc::Weak<RefCell<ImageryLayer>>>,
    pub(crate) layer_index: isize,
    pub(crate) is_base_layer: bool,
    pub(crate) show_last: Option<bool>,
    destroyed: bool,

    // Events
    /// Raised with the provider once a `fromProviderAsync` provider resolves.
    pub ready_event: Event<Arc<dyn ImageryProvider>>,
    pub error_event: Event<RuntimeError>,
    pending_provider: Option<ImageryProviderFuture>,

    texture_host: Option<Rc<dyn super::ImageryTextureHost>>,

    // Cache
    imagery_cache: Rc<ImageryCache>,

    // Request error tracking
    request_image_error: Option<ImageryProviderError>,
}

impl std::fmt::Debug for ImageryLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageryLayer")
            .field("id", &self.id)
            .field("rectangle", &self.rectangle)
            .field("alpha", &self.alpha)
            .field("show", &self.show)
            .field("layer_index", &self.layer_index)
            .field("is_base_layer", &self.is_base_layer)
            .field("destroyed", &self.destroyed)
            .finish()
    }
}

impl Default for ImageryLayer {
    fn default() -> Self {
        Self::new(None, ImageryLayerOptions::default())
    }
}

impl ImageryLayer {
    pub const DEFAULT_BRIGHTNESS: f64 = 1.0;
    pub const DEFAULT_CONTRAST: f64 = 1.0;
    pub const DEFAULT_HUE: f64 = 0.0;
    pub const DEFAULT_SATURATION: f64 = 1.0;
    pub const DEFAULT_GAMMA: f64 = 1.0;
    pub const DEFAULT_SPLIT: SplitDirection = SplitDirection::None;
    pub const DEFAULT_MINIFICATION_FILTER: TextureMinificationFilter =
        TextureMinificationFilter::Linear;
    pub const DEFAULT_MAGNIFICATION_FILTER: TextureMagnificationFilter =
        TextureMagnificationFilter::Linear;
    pub const DEFAULT_APPLY_COLOR_TO_ALPHA_THRESHOLD: f64 = 0.004;

    pub fn new(
        imagery_provider: Option<Arc<dyn ImageryProvider>>,
        options: ImageryLayerOptions,
    ) -> Self {
        let rectangle = options.rectangle.unwrap_or(Rectangle::MAX_VALUE);
        let alpha = options.alpha.unwrap_or(1.0);
        let night_alpha = options.night_alpha.unwrap_or(1.0);
        let day_alpha = options.day_alpha.unwrap_or(1.0);
        let brightness = options.brightness.unwrap_or(Self::DEFAULT_BRIGHTNESS);
        let contrast = options.contrast.unwrap_or(Self::DEFAULT_CONTRAST);
        let hue = options.hue.unwrap_or(Self::DEFAULT_HUE);
        let saturation = options.saturation.unwrap_or(Self::DEFAULT_SATURATION);
        let gamma = options.gamma.unwrap_or(Self::DEFAULT_GAMMA);
        let split_direction = options.split_direction.unwrap_or(Self::DEFAULT_SPLIT);
        let minification_filter = options
            .minification_filter
            .unwrap_or(Self::DEFAULT_MINIFICATION_FILTER);
        let magnification_filter = options
            .magnification_filter
            .unwrap_or(Self::DEFAULT_MAGNIFICATION_FILTER);
        let show = options.show.unwrap_or(true);
        let color_to_alpha_threshold = options
            .color_to_alpha_threshold
            .unwrap_or(Self::DEFAULT_APPLY_COLOR_TO_ALPHA_THRESHOLD);

        let id = NEXT_IMAGERY_LAYER_ID.fetch_add(1, Ordering::Relaxed);

        Self {
            imagery_provider,
            rectangle,
            alpha,
            night_alpha,
            day_alpha,
            brightness,
            contrast,
            hue,
            saturation,
            gamma,
            split_direction,
            minification_filter,
            magnification_filter,
            show,
            maximum_anisotropy: options.maximum_anisotropy,
            minimum_terrain_level: options.minimum_terrain_level,
            maximum_terrain_level: options.maximum_terrain_level,
            cutout_rectangle: options.cutout_rectangle,
            color_to_alpha: options.color_to_alpha,
            color_to_alpha_threshold,
            id,
            weak_self: None,
            layer_index: -1,
            is_base_layer: false,
            show_last: Some(true),
            destroyed: false,
            ready_event: Event::new(),
            error_event: Event::new(),
            pending_provider: None,
            imagery_cache: Rc::new(RefCell::new(HashMap::new())),
            request_image_error: None,
            texture_host: None,
        }
    }

    /// `ImageryLayer.fromProviderAsync`: a layer without a provider until
    /// `provider` resolves. Poll it with [`Self::poll_provider`].
    pub fn from_provider_async(
        provider: impl std::future::Future<Output = Result<Arc<dyn ImageryProvider>, RuntimeError>> + 'static,
        options: ImageryLayerOptions,
    ) -> Self {
        let mut layer = Self::new(None, options);
        layer.pending_provider = Some(Box::pin(provider));
        layer
    }

    /// `ImageryLayer.fromWorldImagery`: [`Self::from_provider_async`] with
    /// `createWorldImageryAsync({ style })`.
    pub fn from_world_imagery<H: crate::core::ImageHost + 'static>(
        loader: super::ImageryLoader<H>,
        world: super::create_world_imagery::WorldImageryOptions,
        options: ImageryLayerOptions,
    ) -> Self {
        Self::from_provider_async(
            async move {
                let provider = super::create_world_imagery::create_world_imagery_async(loader, world).await?;
                Ok(Arc::new(provider) as Arc<dyn ImageryProvider>)
            },
            options,
        )
    }

    /// Whether a `fromProviderAsync` provider is still pending.
    pub fn has_pending_provider(&self) -> bool {
        self.pending_provider.is_some()
    }

    /// Cesium's `handlePromise` continuation. On success, a layer that is not
    /// destroyed takes the provider and raises `readyEvent`. On failure,
    /// `errorEvent` is raised when it has listeners, and the error is logged
    /// otherwise; as in Cesium this happens even after `destroy`.
    pub fn poll_provider(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(future) = &mut self.pending_provider else {
            return Poll::Ready(());
        };
        let result = match future.as_mut().poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        self.pending_provider = None;
        match result {
            Ok(provider) => {
                if !self.destroyed {
                    self.imagery_provider = Some(provider.clone());
                    self.ready_event.raise_event(&provider);
                }
            }
            Err(error) => {
                if self.error_event.number_of_listeners() > 0 {
                    self.error_event.raise_event(&error);
                } else {
                    eprintln!("{error}");
                }
            }
        }
        Poll::Ready(())
    }

    /// Wraps a new [`ImageryLayer`] in an [`ImageryLayerHandle`] (`Rc<RefCell<ImageryLayer>>`).
    pub fn handle(
        imagery_provider: Option<Arc<dyn ImageryProvider>>,
        options: ImageryLayerOptions,
    ) -> ImageryLayerHandle {
        let handle = Rc::new(RefCell::new(Self::new(imagery_provider, options)));
        handle.borrow_mut().weak_self = Some(Rc::downgrade(&handle));
        handle
    }

    /// Converts this layer into an [`ImageryLayerHandle`].
    pub fn into_handle(self) -> ImageryLayerHandle {
        let handle = Rc::new(RefCell::new(self));
        handle.borrow_mut().weak_self = Some(Rc::downgrade(&handle));
        handle
    }

    /// Returns the stable, unique identifier for this layer instance.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Associates an [`ImageryLayerHandle`] with this layer via a weak reference.
    pub fn set_handle(&mut self, handle: &ImageryLayerHandle) {
        self.weak_self = Some(Rc::downgrade(handle));
    }

    /// Upgrades the internal weak handle to this layer, if available.
    pub fn handle_ref(&self) -> Option<ImageryLayerHandle> {
        self.weak_self.as_ref().and_then(|w| w.upgrade())
    }

    /// Creates preliminary imagery tile skeletons for a terrain tile using an explicit layer handle.
    pub fn create_tile_imagery_skeletons_for_handle(
        handle: &ImageryLayerHandle,
        tile_rectangle: &Rectangle,
        tile_level: u32,
        target_geometric_error: f64,
    ) -> Vec<TileImagery> {
        let mut layer = handle.borrow_mut();
        layer.weak_self = Some(Rc::downgrade(handle));
        layer.create_tile_imagery_skeletons(tile_rectangle, tile_level, target_geometric_error)
    }

    pub fn imagery_provider(&self) -> Option<&Arc<dyn ImageryProvider>> {
        self.imagery_provider.as_ref()
    }

    pub fn set_imagery_provider(&mut self, provider: Option<Arc<dyn ImageryProvider>>) {
        self.imagery_provider = provider;
        if let Some(provider) = self.imagery_provider.clone() {
            self.ready_event.raise_event(&provider);
        }
    }

    pub fn ready(&self) -> bool {
        self.imagery_provider.is_some()
    }

    pub fn rectangle(&self) -> &Rectangle {
        &self.rectangle
    }

    /// Computes the intersection of this layer's rectangle with the imagery provider's
    /// availability rectangle.
    pub fn get_imagery_rectangle(&self) -> Option<Rectangle> {
        let provider = self.imagery_provider.as_ref()?;
        provider.rectangle().intersection(&self.rectangle)
    }

    pub fn is_base_layer(&self) -> bool {
        self.is_base_layer
    }

    pub fn set_base_layer(&mut self, is_base: bool) {
        self.is_base_layer = is_base;
    }

    pub fn layer_index(&self) -> isize {
        self.layer_index
    }

    pub fn set_layer_index(&mut self, index: isize) {
        self.layer_index = index;
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.cancel_reprojections();
        self.destroyed = true;
        self.imagery_cache.borrow_mut().clear();
        self.request_image_error = None;
        Ok(())
    }

    /// Attaches this layer's renderer pipeline. Keep the typed pipeline handle
    /// to drain commands into the renderer's compute pass each frame.
    pub fn set_texture_host(&mut self, host: Rc<dyn super::ImageryTextureHost>) {
        self.cancel_reprojections();
        self.texture_host = Some(host);
    }

    /// Appends this layer's queued work to the typed renderer compute pass.
    pub fn queue_reprojection_commands<G: crate::renderer::Gl + 'static>(
        &self, commands: &mut Vec<crate::renderer::ComputeCommand<G>>,
    ) -> Result<(), RuntimeError> {
        if let Some(host) = &self.texture_host {
            let pipeline = host.as_any().downcast_ref::<super::ImageryTexturePipeline<G>>()
                .ok_or_else(|| RuntimeError::new("Imagery pipeline belongs to another renderer host"))?;
            pipeline.queue_reprojection_commands(commands);
        }
        Ok(())
    }

    pub fn cancel_reprojections(&self) {
        if let Some(host) = &self.texture_host { host.cancel_reprojections(); }
    }

    fn texture_settings(&self) -> super::ImageryTextureSettings {
        super::ImageryTextureSettings {
            minification_filter: self.minification_filter,
            magnification_filter: self.magnification_filter,
            maximum_anisotropy: self.maximum_anisotropy,
        }
    }

    fn report_texture_error(&mut self, imagery: &Rc<RefCell<Imagery>>, error: RuntimeError) {
        imagery.borrow_mut().state = ImageryState::Failed;
        self.error_event.raise_event(&error);
    }

    /// Computes the intersection of an imagery provider's rectangle and a layer's rectangle.
    pub fn get_imagery_rectangle_for_rectangles(
        provider_rectangle: &Rectangle,
        layer_rectangle: &Rectangle,
    ) -> Option<Rectangle> {
        provider_rectangle.intersection(layer_rectangle)
    }

    /// Computes the level of detail with the specified world coordinate spacing between texels
    /// given a tiling scheme and tile width.
    pub fn compute_level_with_maximum_texel_spacing(
        tiling_scheme: &dyn TilingScheme,
        tile_width: f64,
        texel_spacing: f64,
        latitude_closest_to_equator: f64,
    ) -> i32 {
        let ellipsoid = tiling_scheme.ellipsoid();
        let latitude_factor = if tiling_scheme.projection().is_geographic() {
            1.0
        } else {
            latitude_closest_to_equator.cos()
        };

        let tiling_scheme_rect = tiling_scheme.rectangle();
        let level_zero_max_spacing = (ellipsoid.maximum_radius
            * tiling_scheme_rect.width()
            * latitude_factor)
            / (tile_width * (tiling_scheme.get_number_of_x_tiles_at_level(0) as f64));

        let two_to_the_level = level_zero_max_spacing / texel_spacing;
        let level = two_to_the_level.ln() / std::f64::consts::LN_2;
        level.round() as i32
    }

    /// Computes the level of detail with the specified world coordinate spacing between texels.
    pub fn get_level_with_maximum_texel_spacing(
        &self,
        texel_spacing: f64,
        latitude_closest_to_equator: f64,
    ) -> Option<i32> {
        let provider = self.imagery_provider.as_ref()?;
        Some(Self::compute_level_with_maximum_texel_spacing(
            provider.tiling_scheme(),
            provider.tile_width(),
            texel_spacing,
            latitude_closest_to_equator,
        ))
    }

    /// Calculates the translation and scale for texture mapping between a terrain tile
    /// and an imagery tile.
    pub fn calculate_texture_translation_and_scale(
        terrain_rectangle: &Rectangle,
        imagery_rectangle: &Rectangle,
    ) -> Cartesian4 {
        let terrain_width = terrain_rectangle.width();
        let terrain_height = terrain_rectangle.height();
        let scale_x = terrain_width / imagery_rectangle.width();
        let scale_y = terrain_height / imagery_rectangle.height();
        Cartesian4::new(
            (scale_x * (terrain_rectangle.west - imagery_rectangle.west)) / terrain_width,
            (scale_y * (terrain_rectangle.south - imagery_rectangle.south)) / terrain_height,
            scale_x,
            scale_y,
        )
    }

    /// Calculates texture translation and scale for a specific [`TileImagery`] instance,
    /// taking Web Mercator coordinates into account if `use_web_mercator_t` is true.
    pub fn calculate_texture_translation_and_scale_for_tile(
        &self,
        terrain_rectangle: &Rectangle,
        tile_imagery: &TileImagery,
    ) -> Option<Cartesian4> {
        let ready_imagery = tile_imagery.ready_imagery.as_ref()?;
        let ready = ready_imagery.borrow();
        let mut imagery_rectangle = ready.rectangle;
        let mut terrain_rectangle = *terrain_rectangle;

        if tile_imagery.use_web_mercator_t {
            if let Some(provider) = self.imagery_provider.as_ref() {
                let tiling_scheme = provider.tiling_scheme();
                imagery_rectangle =
                    tiling_scheme.rectangle_to_native_rectangle(&imagery_rectangle);
                terrain_rectangle =
                    tiling_scheme.rectangle_to_native_rectangle(&terrain_rectangle);
            }
        }

        Some(Self::calculate_texture_translation_and_scale(
            &terrain_rectangle,
            &imagery_rectangle,
        ))
    }

    /// Retrieves or allocates an imagery tile in cache.
    pub fn get_imagery_from_cache(
        &mut self,
        x: i32,
        y: i32,
        level: u32,
        rectangle: Option<Rectangle>,
    ) -> Rc<RefCell<Imagery>> {
        let key = (x, y, level);
        let cached = self.imagery_cache.borrow().get(&key).cloned();
        if let Some(imagery) = cached {
            if imagery.borrow().reference_count > 0 {
                let img = imagery.clone();
                img.borrow_mut().add_reference();
                return img;
            }
            self.imagery_cache.borrow_mut().remove(&key);
        }

        let rect = rectangle.unwrap_or_else(|| {
            if let Some(provider) = &self.imagery_provider {
                provider
                    .tiling_scheme()
                    .tile_xy_to_rectangle(x as f64, y as f64, level)
            } else {
                Rectangle::MAX_VALUE
            }
        });

        let parent = if level > 0 {
            let parent_x = x / 2;
            let parent_y = y / 2;
            let parent_level = level - 1;
            Some(self.get_imagery_from_cache(parent_x, parent_y, parent_level, None))
        } else {
            None
        };

        let mut imagery = Imagery::new(x, y, level, rect, parent);
        imagery.cache = Rc::downgrade(&self.imagery_cache);
        imagery.add_reference();
        let rc = Rc::new(RefCell::new(imagery));
        self.imagery_cache.borrow_mut().insert(key, rc.clone());
        rc
    }

    pub fn remove_imagery_from_cache(&mut self, x: i32, y: i32, level: u32) {
        self.imagery_cache.borrow_mut().remove(&(x, y, level));
    }

    pub fn remove_imagery(&mut self, imagery: &Imagery) {
        self.remove_imagery_from_cache(imagery.x, imagery.y, imagery.level);
    }

    pub fn imagery_cache_len(&self) -> usize {
        self.imagery_cache.borrow().len()
    }

    pub fn request_image_error(&self) -> Option<&ImageryProviderError> {
        self.request_image_error.as_ref()
    }

    pub fn clear_request_image_error(&mut self) {
        self.request_image_error = None;
    }

    /// Requests a tile image from the imagery provider.
    ///
    /// If throttled by the provider/scheduler, the tile state remains `Unloaded`.
    /// When successfully dispatched, state transitions to `Transitioning`.
    pub fn request_imagery(&mut self, imagery_cell: &Rc<RefCell<Imagery>>) {
        let (x, y, level) = {
            let img = imagery_cell.borrow();
            (img.x, img.y, img.level)
        };

        let provider = match self.imagery_provider.as_ref() {
            Some(p) => p.clone(),
            None => {
                let mut img = imagery_cell.borrow_mut();
                img.state = ImageryState::Failed;
                return;
            }
        };

        let credits = provider.get_tile_credits(x, y, level);
        let future_opt = provider.request_image(x, y, level);

        let mut img = imagery_cell.borrow_mut();
        match future_opt {
            None => {
                // Postpone loading tile: throttled by request scheduler / provider.
                img.state = ImageryState::Unloaded;
                img.in_flight_request = None;
            }
            Some(future) => {
                img.state = ImageryState::Transitioning;
                img.in_flight_request = Some(future);
                img.credits = credits;
            }
        }
    }

    /// Polls any in-flight asynchronous image request on `imagery_cell`.
    ///
    /// If the request future is ready, transitions state to `Received` on success,
    /// or `Failed` on error (handling error events and retries). A no-op waker
    /// is used; callers must poll again on later frames while the request is pending.
    pub fn poll_imagery(&mut self, imagery_cell: &Rc<RefCell<Imagery>>) -> bool {
        let mut future = {
            let mut img = imagery_cell.borrow_mut();
            if img.state != ImageryState::Transitioning {
                return false;
            }
            match img.in_flight_request.take() {
                Some(f) => f,
                None => return false,
            }
        };

        let (x, y, level) = {
            let img = imagery_cell.borrow();
            (img.x, img.y, img.level)
        };

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        match future.as_mut().poll(&mut cx) {
            Poll::Pending => {
                let mut img = imagery_cell.borrow_mut();
                img.in_flight_request = Some(future);
                false
            }
            Poll::Ready(Ok(image)) => {
                let mut img = imagery_cell.borrow_mut();
                img.image = Some(image);
                img.state = ImageryState::Received;
                img.in_flight_request = None;
                if let Some(err) = &mut self.request_image_error {
                    err.report_success();
                }
                true
            }
            Poll::Ready(Err(_err)) => {
                {
                    let mut img = imagery_cell.borrow_mut();
                    img.in_flight_request = None;
                    img.state = ImageryState::Failed;
                }
                let provider = self.imagery_provider.clone();
                if let Some(provider) = provider {
                    let message = format!(
                        "Failed to obtain image tile X: {} Y: {} Level: {}.",
                        x, y, level
                    );
                    let mut error = image_error(
                        self.request_image_error.as_ref(),
                        message,
                        Some(x),
                        Some(y),
                        Some(level),
                    );
                    let retry = report_image_error(provider.error_event(), &mut error);
                    self.request_image_error = Some(error);
                    if retry {
                        self.request_imagery(imagery_cell);
                    }
                }
                true
            }
        }
    }

    /// Advances the load state machine for an imagery tile.
    ///
    /// Mirrors CesiumJS `Imagery.prototype.processStateMachine`:
    /// 1. UNLOADED -> TRANSITIONING and initiates request (unless skip_loading).
    /// 2. Polls TRANSITIONING requests.
    /// 3. RECEIVED -> Checks discard policy (if policy not ready, stays RECEIVED;
    ///    if discarded, marks INVALID).
    /// 4. TEXTURE_LOADED or needsReprojection -> Evaluates projection and transitions
    ///    to READY when geographic projection is satisfied or Web Mercator is used directly.
    pub fn process_imagery_state_machine(
        &mut self,
        imagery_cell: &Rc<RefCell<Imagery>>,
        _frame_state: Option<&FrameState>,
        need_geographic_projection: bool,
        skip_loading: bool,
    ) {
        let state = imagery_cell.borrow().state;

        // 1. UNLOADED: initiate request
        if state == ImageryState::Unloaded && !skip_loading {
            self.request_imagery(imagery_cell);
            self.poll_imagery(imagery_cell);
        } else if state == ImageryState::Transitioning {
            // 2. TRANSITIONING: poll in-flight request
            self.poll_imagery(imagery_cell);
        }

        let state = imagery_cell.borrow().state;

        // 3. RECEIVED: check discard policy
        if state == ImageryState::Received {
            let provider = match self.imagery_provider.as_ref() {
                Some(p) => p.clone(),
                None => return,
            };

            if let Some(discard_policy) = provider.tile_discard_policy() {
                if !discard_policy.is_ready() {
                    return;
                }
                let should_discard = {
                    let img = imagery_cell.borrow();
                    match &img.image {
                        Some(image) => discard_policy.should_discard_image(image),
                        None => true,
                    }
                };
                if should_discard {
                    let mut img = imagery_cell.borrow_mut();
                    img.state = ImageryState::Invalid;
                    img.image = None;
                    return;
                }
            }

            let has_texture = {
                let img = imagery_cell.borrow();
                img.texture.is_some() || img.texture_web_mercator.is_some()
            };

            if has_texture {
                let mut img = imagery_cell.borrow_mut();
                img.state = ImageryState::TextureLoaded;
            } else if let Some(host) = &self.texture_host {
                let image = imagery_cell.borrow().image.clone();
                if let Some(image) = image {
                    match host.upload(&image, provider.has_alpha_channel(), self.texture_settings()) {
                        Ok(texture) => {
                            let mut img = imagery_cell.borrow_mut();
                            if provider.tiling_scheme().projection().is_geographic() {
                                img.texture = Some(texture);
                            } else {
                                img.texture_web_mercator = Some(texture);
                            }
                            img.image = None;
                            img.state = ImageryState::TextureLoaded;
                        }
                        Err(error) => self.report_texture_error(imagery_cell, error),
                    }
                }
            }
        }

        let state = imagery_cell.borrow().state;

        // 4. TEXTURE_LOADED or needsReprojection:
        let is_ready = state == ImageryState::Ready;
        let has_texture = imagery_cell.borrow().texture.is_some();
        let needs_reprojection = is_ready && need_geographic_projection && !has_texture;

        if state == ImageryState::TextureLoaded || needs_reprojection {
            let provider = match self.imagery_provider.as_ref() {
                Some(p) => p.clone(),
                None => return,
            };

            let is_geographic = provider.tiling_scheme().projection().is_geographic();

            if let Some(host) = &self.texture_host {
                if let Err(error) = host.reproject(imagery_cell, need_geographic_projection, self.texture_settings()) {
                    self.report_texture_error(imagery_cell, error);
                }
            } else if need_geographic_projection && !is_geographic {
                // Wait until a renderer pipeline is attached.
            } else {
                let mut img = imagery_cell.borrow_mut();
                if need_geographic_projection && img.texture.is_none() {
                    img.texture = img.texture_web_mercator.clone();
                }
                img.state = ImageryState::Ready;
            }
        }
    }

    /// Processes the load state machine for a [`TileImagery`] instance.
    ///
    /// Returns `true` if this instance is done loading (ready or best available substitute);
    /// otherwise `false`.
    pub fn process_tile_imagery(
        &mut self,
        tile_imagery: &mut TileImagery,
        terrain_rectangle: &Rectangle,
        frame_state: Option<&FrameState>,
        skip_loading: bool,
    ) -> bool {
        let loading_imagery = match &tile_imagery.loading_imagery {
            Some(img) => img.clone(),
            None => return true,
        };

        let need_geographic_projection = !tile_imagery.use_web_mercator_t;
        self.process_imagery_state_machine(
            &loading_imagery,
            frame_state,
            need_geographic_projection,
            skip_loading,
        );

        let loading_state = loading_imagery.borrow().state;
        if loading_state == ImageryState::Ready {
            if let Some(ready) = tile_imagery.ready_imagery.take() {
                ready.borrow_mut().release_reference();
            }
            tile_imagery.ready_imagery = tile_imagery.loading_imagery.take();
            tile_imagery.texture_translation_and_scale = self
                .calculate_texture_translation_and_scale_for_tile(
                    terrain_rectangle,
                    tile_imagery,
                );
            return true;
        }

        // Find some ancestor imagery we can use while this imagery is still loading.
        let mut ancestor = loading_imagery.borrow().parent.clone();
        let mut closest_ancestor_that_needs_loading = None;

        while let Some(current_ancestor) = ancestor {
            let is_ancestor_ready = {
                let a = current_ancestor.borrow();
                let has_required_texture =
                    tile_imagery.use_web_mercator_t || a.texture.is_some();
                a.state == ImageryState::Ready && has_required_texture
            };

            if is_ancestor_ready {
                ancestor = Some(current_ancestor);
                break;
            }

            let state = current_ancestor.borrow().state;
            if state != ImageryState::Failed
                && state != ImageryState::Invalid
                && closest_ancestor_that_needs_loading.is_none()
            {
                closest_ancestor_that_needs_loading = Some(current_ancestor.clone());
            }

            ancestor = current_ancestor.borrow().parent.clone();
        }

        let same_ancestor = match (&tile_imagery.ready_imagery, &ancestor) {
            (Some(r), Some(a)) => Rc::ptr_eq(r, a),
            (None, None) => true,
            _ => false,
        };

        if !same_ancestor {
            if let Some(ready) = tile_imagery.ready_imagery.take() {
                ready.borrow_mut().release_reference();
            }
            tile_imagery.ready_imagery = ancestor;
            if let Some(ready) = &tile_imagery.ready_imagery {
                ready.borrow_mut().add_reference();
                tile_imagery.texture_translation_and_scale = self
                    .calculate_texture_translation_and_scale_for_tile(
                        terrain_rectangle,
                        tile_imagery,
                    );
            }
        }

        if loading_state == ImageryState::Failed || loading_state == ImageryState::Invalid {
            if let Some(ancestor_to_load) = closest_ancestor_that_needs_loading {
                self.process_imagery_state_machine(
                    &ancestor_to_load,
                    frame_state,
                    need_geographic_projection,
                    skip_loading,
                );
                return false;
            }
            return true;
        }

        false
    }

    /// Creates preliminary imagery tile skeletons for a terrain tile.
    ///
    /// Skeletons are automatically associated with this layer's stable identity
    /// and attached handle (if wrapped in an [`ImageryLayerHandle`]).
    pub fn create_tile_imagery_skeletons(
        &mut self,
        tile_rectangle: &Rectangle,
        tile_level: u32,
        target_geometric_error: f64,
    ) -> Vec<TileImagery> {
        let provider = match self.imagery_provider.as_ref() {
            Some(p) => p,
            None => return Vec::new(),
        };

        if let Some(min_level) = self.minimum_terrain_level {
            if tile_level < min_level {
                return Vec::new();
            }
        }
        if let Some(max_level) = self.maximum_terrain_level {
            if tile_level > max_level {
                return Vec::new();
            }
        }

        let imagery_bounds = match provider.rectangle().intersection(&self.rectangle) {
            Some(b) => b,
            None => return Vec::new(),
        };

        let overlap = match tile_rectangle.intersection(&imagery_bounds) {
            Some(r) => r,
            None => {
                if !self.is_base_layer {
                    return Vec::new();
                }
                imagery_bounds
            }
        };

        let latitude_closest_to_equator = if overlap.south > 0.0 {
            overlap.south
        } else if overlap.north < 0.0 {
            overlap.north
        } else {
            0.0
        };

        let imagery_level = self
            .get_level_with_maximum_texel_spacing(
                target_geometric_error,
                latitude_closest_to_equator,
            )
            .unwrap_or(0)
            .max(0) as u32;

        let imagery_level = provider
            .maximum_level()
            .map_or(imagery_level, |max| imagery_level.min(max));
        let imagery_level = imagery_level.max(provider.minimum_level());

        let tiling_scheme = provider.tiling_scheme();
        let nw = match tiling_scheme.position_to_tile_xy(&overlap.northwest(), imagery_level) {
            Some(c) => c,
            None => return Vec::new(),
        };
        let se = match tiling_scheme.position_to_tile_xy(&overlap.southeast(), imagery_level) {
            Some(c) => c,
            None => return Vec::new(),
        };

        let max_lat = WebMercatorProjection::maximum_latitude();
        let use_web_mercator_t = !provider.tiling_scheme().projection().is_geographic()
            && tile_rectangle.north < max_lat
            && tile_rectangle.south > -max_lat;

        let handle_opt = self.weak_self.as_ref().and_then(|w| w.upgrade());
        let layer_id = self.id();
        let layer_idx = if self.layer_index >= 0 {
            Some(self.layer_index as usize)
        } else {
            None
        };

        let mut results = Vec::new();
        for x in (nw.x as i32)..=(se.x as i32) {
            for y in (nw.y as i32)..=(se.y as i32) {
                let img = self.get_imagery_from_cache(x, y, imagery_level, None);
                let tex_coords = Cartesian4::new(0.0, 0.0, 1.0, 1.0);
                let mut tile_img = TileImagery::new(img, tex_coords, use_web_mercator_t);
                tile_img.imagery_layer = handle_opt.clone();
                tile_img.layer_id = Some(layer_id);
                tile_img.layer_index = layer_idx;
                results.push(tile_img);
            }
        }
        results
    }
}

impl ImageryLayerSettings for ImageryLayer {
    fn show(&self) -> bool {
        self.show
    }
    fn alpha(&self) -> f64 {
        self.alpha
    }
    fn brightness(&self) -> f64 {
        self.brightness
    }
    fn contrast(&self) -> f64 {
        self.contrast
    }
    fn hue(&self) -> f64 {
        self.hue
    }
    fn saturation(&self) -> f64 {
        self.saturation
    }
    fn gamma(&self) -> f64 {
        self.gamma
    }
    fn color_to_alpha(&self) -> Option<Color> {
        self.color_to_alpha
    }
}

impl ImageryLayerSettings for ImageryLayerHandle {
    fn show(&self) -> bool {
        self.borrow().show()
    }
    fn alpha(&self) -> f64 {
        self.borrow().alpha()
    }
    fn brightness(&self) -> f64 {
        self.borrow().brightness()
    }
    fn contrast(&self) -> f64 {
        self.borrow().contrast()
    }
    fn hue(&self) -> f64 {
        self.borrow().hue()
    }
    fn saturation(&self) -> f64 {
        self.borrow().saturation()
    }
    fn gamma(&self) -> f64 {
        self.borrow().gamma()
    }
    fn color_to_alpha(&self) -> Option<Color> {
        self.borrow().color_to_alpha()
    }
}
