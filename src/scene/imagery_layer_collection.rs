//! CesiumJS `ImageryLayerCollection`: an ordered collection of imagery layers.
//! Includes terrain-ray layer and asynchronous feature picking.
//!
//! Cesium's `add` subscribes once to the layer's `readyEvent` and raises
//! `layerShownOrHidden(layer, layer._layerIndex, layer.show)` when it fires,
//! even if the layer has been removed by then. Core event listeners must be
//! `Send`, so the collection keeps these subscriptions itself and delivers
//! them from [`ImageryLayerCollection::poll_providers`].

use super::imagery_layer::{ImageryLayer, ImageryLayerHandle, ImageryLayerOptions};
use super::imagery_provider::ImageryProvider;
use super::quadtree_tile::QuadtreeTileHandle;
use super::{FrameResource, GlobeSurfaceTile, Imagery, ImageryLayerFeatureInfo, Scene, SceneBackend};
use crate::core::{Cartographic, Event, Ray, Rectangle, RuntimeError};
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, sync::Arc};

/// Ray and rendered-terrain boundary for imagery picking. Scene implements this
/// through its SceneGlobe; other hosts can provide the same loaded surface data.
/// Tile data must be GlobeSurfaceTile, with ready TileImagery attachments.
pub trait ImageryPickSource {
    fn pick_imagery_position(&self, ray: &Ray) -> Option<Cartographic>;
    fn rendered_imagery_tiles(&self) -> &[QuadtreeTileHandle];
}
impl<B: SceneBackend> ImageryPickSource for Scene<B> {
    fn pick_imagery_position(&self, ray: &Ray) -> Option<Cartographic> {
        let point = self.globe.as_ref()?.pick(ray, &self.frame_state)?;
        self.camera.ellipsoid.cartesian_to_cartographic(point)
    }
    fn rendered_imagery_tiles(&self) -> &[QuadtreeTileHandle] {
        self.globe.as_ref().map_or(&[], |globe| globe.rendered_surface_tiles())
    }
}

/// None from pick_imagery_layer_features means no supported requests were
/// started. A returned future can resolve to an empty feature list.
pub type ImageryLayerFeaturesFuture = Pin<Box<dyn Future<Output = Result<Vec<ImageryLayerFeatureInfo>, RuntimeError>>>>;

/// Event payload when a layer is added or removed.
#[derive(Clone, Debug)]
pub struct ImageryLayerEvent {
    pub layer: ImageryLayerHandle,
    pub index: usize,
}

/// Event payload when a layer moves within the collection.
#[derive(Clone, Debug)]
pub struct ImageryLayerMovedEvent {
    pub layer: ImageryLayerHandle,
    pub new_index: usize,
    pub old_index: usize,
}

/// Event payload when a layer's visibility changes.
#[derive(Clone, Debug)]
pub struct ImageryLayerVisibilityEvent {
    pub layer: ImageryLayerHandle,
    pub index: usize,
    pub show: bool,
}

/// An ordered collection of imagery layers for rendering raster imagery on a globe or tileset.
pub struct ImageryLayerCollection {
    layers: Vec<ImageryLayerHandle>,
    destroyed: bool,
    /// One entry per `add` of a layer that was not ready yet.
    ready_subscriptions: Vec<ImageryLayerHandle>,

    pub layer_added: Event<ImageryLayerEvent>,
    pub layer_removed: Event<ImageryLayerEvent>,
    pub layer_moved: Event<ImageryLayerMovedEvent>,
    pub layer_shown_or_hidden: Event<ImageryLayerVisibilityEvent>,
}

impl std::fmt::Debug for ImageryLayerCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageryLayerCollection")
            .field("layers_len", &self.layers.len())
            .field("destroyed", &self.destroyed)
            .finish()
    }
}

impl Default for ImageryLayerCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageryLayerCollection {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            destroyed: false,
            ready_subscriptions: Vec::new(),
            layer_added: Event::new(),
            layer_removed: Event::new(),
            layer_moved: Event::new(),
            layer_shown_or_hidden: Event::new(),
        }
    }

    pub fn length(&self) -> usize {
        self.layers.len()
    }

    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn layers(&self) -> &[ImageryLayerHandle] {
        &self.layers
    }

    pub fn add(&mut self, layer: ImageryLayerHandle, index: Option<usize>) -> Result<(), RuntimeError> {
        let has_index = index.is_some();
        let idx = index.unwrap_or(self.layers.len());
        if idx > self.layers.len() {
            return Err(RuntimeError::new("index must be less than or equal to the number of layers."));
        }

        layer.borrow_mut().set_handle(&layer);

        if has_index {
            self.layers.insert(idx, layer.clone());
        } else {
            self.layers.push(layer.clone());
        }

        self.update();
        self.layer_added.raise_event(&ImageryLayerEvent { layer: layer.clone(), index: idx });
        // A layer that is already ready never raises readyEvent again.
        if !layer.borrow().ready() {
            self.ready_subscriptions.push(layer);
        }
        Ok(())
    }

    /// Drives the provider futures of the layers in this collection and of
    /// layers still waiting to forward readiness, then delivers Cesium's
    /// once-per-`add` `layerShownOrHidden` for every layer that has become
    /// ready. Returns the number of forwarded events.
    pub fn poll_providers(&mut self, cx: &mut std::task::Context<'_>) -> usize {
        let mut polled: Vec<ImageryLayerHandle> = Vec::new();
        for layer in self.layers.iter().chain(&self.ready_subscriptions) {
            if !polled.iter().any(|p| Rc::ptr_eq(p, layer)) {
                polled.push(layer.clone());
                let _ = layer.borrow_mut().poll_provider(cx);
            }
        }
        let mut forwarded = 0;
        let mut pending = Vec::new();
        for layer in std::mem::take(&mut self.ready_subscriptions) {
            let (ready, destroyed, waiting, index, show) = {
                let l = layer.borrow();
                (l.ready(), l.is_destroyed(), l.has_pending_provider(), l.layer_index, l.show)
            };
            if ready && !destroyed {
                self.layer_shown_or_hidden.raise_event(&ImageryLayerVisibilityEvent {
                    layer,
                    index: index.max(0) as usize,
                    show,
                });
                forwarded += 1;
            } else if !destroyed || waiting {
                pending.push(layer);
            }
        }
        self.ready_subscriptions = pending;
        forwarded
    }

    pub fn add_imagery_provider(
        &mut self,
        provider: Arc<dyn ImageryProvider>,
        index: Option<usize>,
    ) -> Result<ImageryLayerHandle, RuntimeError> {
        let layer = ImageryLayer::handle(Some(provider), ImageryLayerOptions::default());
        self.add(layer.clone(), index)?;
        Ok(layer)
    }

    pub fn remove(&mut self, layer: &ImageryLayerHandle, destroy: Option<bool>) -> bool {
        let destroy = destroy.unwrap_or(true);
        if let Some(index) = self.index_of(layer) {
            let removed = self.layers.remove(index);
            self.update();
            self.layer_removed.raise_event(&ImageryLayerEvent { layer: removed.clone(), index });
            if destroy {
                let _ = removed.borrow_mut().destroy();
            }
            true
        } else {
            false
        }
    }

    pub fn remove_all(&mut self, destroy: Option<bool>) {
        let destroy = destroy.unwrap_or(true);
        let old_layers = std::mem::take(&mut self.layers);
        for (i, layer) in old_layers.into_iter().enumerate() {
            self.layer_removed.raise_event(&ImageryLayerEvent { layer: layer.clone(), index: i });
            if destroy {
                let _ = layer.borrow_mut().destroy();
            }
        }
    }

    pub fn contains(&self, layer: &ImageryLayerHandle) -> bool {
        self.index_of(layer).is_some()
    }

    pub fn index_of(&self, layer: &ImageryLayerHandle) -> Option<usize> {
        self.layers.iter().position(|l| Rc::ptr_eq(l, layer))
    }

    pub fn get(&self, index: usize) -> Option<ImageryLayerHandle> {
        self.layers.get(index).cloned()
    }

    fn get_layer_index(&self, layer: &ImageryLayerHandle) -> Result<usize, RuntimeError> {
        self.index_of(layer).ok_or_else(|| RuntimeError::new("layer is not in this collection."))
    }

    pub fn raise(&mut self, layer: &ImageryLayerHandle) -> Result<(), RuntimeError> {
        let index = self.get_layer_index(layer)?;
        self.swap_layers(index, index + 1);
        Ok(())
    }

    pub fn lower(&mut self, layer: &ImageryLayerHandle) -> Result<(), RuntimeError> {
        let index = self.get_layer_index(layer)?;
        self.swap_layers(index, index.saturating_sub(1));
        Ok(())
    }

    pub fn raise_to_top(&mut self, layer: &ImageryLayerHandle) -> Result<(), RuntimeError> {
        let index = self.get_layer_index(layer)?;
        if index == self.layers.len() - 1 {
            return Ok(());
        }
        let item = self.layers.remove(index);
        let new_index = self.layers.len();
        self.layers.push(item.clone());
        self.update();
        self.layer_moved.raise_event(&ImageryLayerMovedEvent { layer: item, new_index, old_index: index });
        Ok(())
    }

    pub fn lower_to_bottom(&mut self, layer: &ImageryLayerHandle) -> Result<(), RuntimeError> {
        let index = self.get_layer_index(layer)?;
        if index == 0 {
            return Ok(());
        }
        let item = self.layers.remove(index);
        self.layers.insert(0, item.clone());
        self.update();
        self.layer_moved.raise_event(&ImageryLayerMovedEvent { layer: item, new_index: 0, old_index: index });
        Ok(())
    }

    fn swap_layers(&mut self, i: usize, j: usize) {
        if self.layers.is_empty() {
            return;
        }
        let max_idx = self.layers.len() - 1;
        let i = i.min(max_idx);
        let j = j.min(max_idx);
        if i == j {
            return;
        }

        self.layers.swap(i, j);
        self.update();

        let moved_layer = self.layers[j].clone();
        self.layer_moved.raise_event(&ImageryLayerMovedEvent { layer: moved_layer, new_index: j, old_index: i });
    }

    pub fn update(&mut self) {
        let mut is_base_layer = true;
        let mut changed_visibility = Vec::new();

        for (i, layer_handle) in self.layers.iter().enumerate() {
            let mut layer = layer_handle.borrow_mut();
            layer.layer_index = i as isize;

            if layer.show {
                layer.is_base_layer = is_base_layer;
                is_base_layer = false;
            } else {
                layer.is_base_layer = false;
            }

            if let Some(show_last) = layer.show_last {
                if show_last != layer.show {
                    changed_visibility.push((layer_handle.clone(), i, layer.show));
                }
            }
            layer.show_last = Some(layer.show);
        }

        for (layer, index, show) in changed_visibility {
            self.layer_shown_or_hidden.raise_event(&ImageryLayerVisibilityEvent { layer, index, show });
        }
    }

    /// Pick attached, ready imagery layers in visual (top-to-bottom) order.
    /// As in Cesium, duplicate layer references are retained when multiple
    /// imagery attachments cover the hit; collection visibility is not rechecked.
    pub fn pick_imagery_layers(&self, ray: &Ray, scene: &impl ImageryPickSource) -> Option<Vec<ImageryLayerHandle>> {
        let location = scene.pick_imagery_position(ray)?;
        let layers: Vec<_> = self
            .pick_imagery(scene, &location)
            .into_iter()
            .filter_map(|(layer, _)| {
                let ready = layer.borrow().ready();
                ready.then_some(layer)
            })
            .collect();
        (!layers.is_empty()).then_some(layers)
    }

    /// Starts feature requests immediately for all applicable, ready imagery.
    /// Requests are polled concurrently and their results retain input order,
    /// independent of completion order. One provider error rejects the result.
    pub fn pick_imagery_layer_features(
        &self,
        ray: &Ray,
        scene: &impl ImageryPickSource,
    ) -> Option<ImageryLayerFeaturesFuture> {
        let location = scene.pick_imagery_position(ray)?;
        let mut requests = Vec::new();
        for (layer, imagery) in self.pick_imagery(scene, &location) {
            // Release the layer and tile borrows before application provider code.
            let provider = layer.borrow().imagery_provider().cloned();
            let Some(provider) = provider else {
                continue;
            };
            let (x, y, level) = {
                let i = imagery.borrow();
                (i.x, i.y, i.level)
            };
            if let Some(future) = provider.pick_features(x, y, level, location.longitude, location.latitude) {
                requests.push(async move {
                    let mut features = future.await?.unwrap_or_default();
                    let owner = FrameResource::new(layer);
                    for feature in &mut features {
                        feature.imagery_layer = Some(owner.clone());
                        if feature.position.is_none() {
                            feature.position = Some(location);
                        }
                    }
                    Ok::<_, RuntimeError>(features)
                });
            }
        }
        if requests.is_empty() {
            return None;
        }
        Some(Box::pin(async move {
            let results = futures_util::future::try_join_all(requests).await?;
            Ok(results.into_iter().flatten().collect())
        }))
    }

    fn pick_imagery(
        &self,
        scene: &impl ImageryPickSource,
        location: &Cartographic,
    ) -> Vec<(ImageryLayerHandle, Rc<RefCell<Imagery>>)> {
        // A shared edge belongs to the first rendered terrain tile, matching
        // Cesium's traversal order rather than choosing a deeper or later tile.
        let Some(tile) = scene.rendered_imagery_tiles().iter().find(|tile| tile.borrow().rectangle.contains(location))
        else {
            return Vec::new();
        };
        let tile = tile.borrow();
        let Some(surface) = tile.data.as_ref().and_then(|data| data.as_any().downcast_ref::<GlobeSurfaceTile>()) else {
            return Vec::new();
        };
        let lerp = |a: f64, b: f64, t: f64| (1. - t) * a + t * b;
        let epsilon = 1. / 1024.;
        let mut result = Vec::new();
        for attachment in surface.imagery.iter().rev() {
            let Some(imagery) = attachment.ready_imagery.as_ref() else {
                continue;
            };
            let layer = attachment.imagery_layer.clone().or_else(|| {
                if let Some(id) = attachment.layer_id {
                    self.layers.iter().find(|layer| layer.borrow().id() == id).cloned()
                } else {
                    attachment.layer_index.and_then(|index| self.get(index))
                }
            });
            let Some(layer) = layer else {
                continue;
            };
            if !imagery.borrow().rectangle.contains(location) {
                continue;
            }
            let t = attachment.texture_coordinate_rectangle;
            let rectangle = &tile.rectangle;
            let applicable = Rectangle::new(
                lerp(rectangle.west, rectangle.east, t.x - epsilon),
                lerp(rectangle.south, rectangle.north, t.y - epsilon),
                lerp(rectangle.west, rectangle.east, t.z + epsilon),
                lerp(rectangle.south, rectangle.north, t.w + epsilon),
            );
            if applicable.contains(location) {
                result.push((layer, imagery.clone()));
            }
        }
        result
    }

    /// Appends each layer's queued work to the renderer compute pass.
    pub fn queue_reprojection_commands<G: crate::renderer::Gl + 'static>(
        &self,
        commands: &mut Vec<crate::renderer::ComputeCommand<G>>,
    ) -> Result<(), RuntimeError> {
        for layer in &self.layers {
            layer.borrow().queue_reprojection_commands(commands)?;
        }
        Ok(())
    }

    /// Cancels reprojections still queued by each layer's renderer pipeline.
    pub fn cancel_reprojections(&self) {
        for layer in &self.layers {
            layer.borrow().cancel_reprojections();
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.remove_all(Some(true));
        self.destroyed = true;
        Ok(())
    }
}
