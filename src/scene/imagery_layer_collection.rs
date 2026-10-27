//! CesiumJS `ImageryLayerCollection`: an ordered collection of imagery layers.

use super::frame_state::FrameState;
use super::imagery_layer::{ImageryLayer, ImageryLayerHandle, ImageryLayerOptions};
use super::imagery_provider::ImageryProvider;
use crate::core::{Event, RuntimeError};
use std::rc::Rc;
use std::sync::Arc;

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

    pub fn add(&mut self, layer: ImageryLayerHandle, index: Option<usize>) -> Result<(), RuntimeError> {
        let has_index = index.is_some();
        let idx = index.unwrap_or(self.layers.len());
        if idx > self.layers.len() {
            return Err(RuntimeError::new("index must be less than or equal to the number of layers."));
        }

        if has_index {
            self.layers.insert(idx, layer.clone());
        } else {
            self.layers.push(layer.clone());
        }

        self.update();
        self.layer_added.raise_event(&ImageryLayerEvent {
            layer,
            index: idx,
        });
        Ok(())
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
            self.layer_removed.raise_event(&ImageryLayerEvent {
                layer: removed.clone(),
                index,
            });
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
            self.layer_removed.raise_event(&ImageryLayerEvent {
                layer: layer.clone(),
                index: i,
            });
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
        self.index_of(layer)
            .ok_or_else(|| RuntimeError::new("layer is not in this collection."))
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
        self.layer_moved.raise_event(&ImageryLayerMovedEvent {
            layer: item,
            new_index,
            old_index: index,
        });
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
        self.layer_moved.raise_event(&ImageryLayerMovedEvent {
            layer: item,
            new_index: 0,
            old_index: index,
        });
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
        self.layer_moved.raise_event(&ImageryLayerMovedEvent {
            layer: moved_layer,
            new_index: j,
            old_index: i,
        });
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
            self.layer_shown_or_hidden.raise_event(&ImageryLayerVisibilityEvent {
                layer,
                index,
                show,
            });
        }
    }

    pub fn queue_reprojection_commands(&self, frame_state: &mut FrameState) {
        for layer in &self.layers {
            layer.borrow().queue_reprojection_commands(frame_state);
        }
    }

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

    pub fn iter(&self) -> impl Iterator<Item = &ImageryLayerHandle> {
        self.layers.iter()
    }
}
