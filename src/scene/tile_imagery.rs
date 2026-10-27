//! CesiumJS `TileImagery`: the association between a terrain tile and an imagery tile.
//!
//! Manages loading and ready imagery references, texture coordinate transformations,
//! and ancestor imagery fallback during quadtree traversal.

use super::frame_state::FrameState;
use super::imagery::Imagery;
use super::imagery_layer::{ImageryLayer, ImageryLayerHandle};
use crate::core::{Cartesian4, Rectangle};
use std::cell::RefCell;
use std::rc::Rc;

/// The association between a terrain tile and an imagery tile.
#[derive(Debug)]
pub struct TileImagery {
    pub ready_imagery: Option<Rc<RefCell<Imagery>>>,
    pub loading_imagery: Option<Rc<RefCell<Imagery>>>,
    pub texture_coordinate_rectangle: Cartesian4,
    pub texture_translation_and_scale: Option<Cartesian4>,
    pub use_web_mercator_t: bool,
    /// Reference to the owning [`ImageryLayer`] handle, if attached.
    pub imagery_layer: Option<ImageryLayerHandle>,
    /// Stable unique ID of the owning [`ImageryLayer`].
    pub layer_id: Option<u64>,
    /// Index of the owning imagery layer in the collection/layer list.
    pub layer_index: Option<usize>,
}

impl TileImagery {
    /// Takes ownership of one existing imagery reference without an initial owning layer.
    pub fn new(
        imagery: Rc<RefCell<Imagery>>,
        texture_coordinate_rectangle: Cartesian4,
        use_web_mercator_t: bool,
    ) -> Self {
        Self {
            ready_imagery: None,
            loading_imagery: Some(imagery),
            texture_coordinate_rectangle,
            texture_translation_and_scale: None,
            use_web_mercator_t,
            imagery_layer: None,
            layer_id: None,
            layer_index: None,
        }
    }

    /// Constructs a `TileImagery` with an attached owning [`ImageryLayer`] handle.
    pub fn new_with_layer(
        imagery: Rc<RefCell<Imagery>>,
        texture_coordinate_rectangle: Cartesian4,
        use_web_mercator_t: bool,
        layer: ImageryLayerHandle,
    ) -> Self {
        let (id, idx) = if let Ok(l) = layer.try_borrow() {
            let idx = if l.layer_index() >= 0 {
                Some(l.layer_index() as usize)
            } else {
                None
            };
            (Some(l.id()), idx)
        } else {
            (None, None)
        };
        Self {
            ready_imagery: None,
            loading_imagery: Some(imagery),
            texture_coordinate_rectangle,
            texture_translation_and_scale: None,
            use_web_mercator_t,
            imagery_layer: Some(layer),
            layer_id: id,
            layer_index: idx,
        }
    }

    /// Constructs a `TileImagery` with an explicit owning layer index.
    pub fn new_with_layer_index(
        imagery: Rc<RefCell<Imagery>>,
        texture_coordinate_rectangle: Cartesian4,
        use_web_mercator_t: bool,
        layer_index: usize,
    ) -> Self {
        Self {
            ready_imagery: None,
            loading_imagery: Some(imagery),
            texture_coordinate_rectangle,
            texture_translation_and_scale: None,
            use_web_mercator_t,
            imagery_layer: None,
            layer_id: None,
            layer_index: Some(layer_index),
        }
    }

    /// Constructs a `TileImagery` with an explicit stable layer ID.
    pub fn new_with_layer_id(
        imagery: Rc<RefCell<Imagery>>,
        texture_coordinate_rectangle: Cartesian4,
        use_web_mercator_t: bool,
        layer_id: u64,
    ) -> Self {
        Self {
            ready_imagery: None,
            loading_imagery: Some(imagery),
            texture_coordinate_rectangle,
            texture_translation_and_scale: None,
            use_web_mercator_t,
            imagery_layer: None,
            layer_id: Some(layer_id),
            layer_index: None,
        }
    }

    /// Sets the owning [`ImageryLayer`] handle on this instance.
    pub fn set_imagery_layer(&mut self, layer: ImageryLayerHandle) {
        if let Ok(l) = layer.try_borrow() {
            self.layer_id = Some(l.id());
            if l.layer_index() >= 0 {
                self.layer_index = Some(l.layer_index() as usize);
            }
        }
        self.imagery_layer = Some(layer);
    }

    /// Sets the stable layer ID on this instance.
    pub fn set_layer_id(&mut self, id: u64) {
        self.layer_id = Some(id);
    }

    /// Sets the owning layer index on this instance.
    pub fn set_layer_index(&mut self, index: usize) {
        self.layer_index = Some(index);
    }

    /// Returns the resolved stable owning layer identity, either from the attached `imagery_layer` or fallback `layer_id`.
    pub fn layer_id(&self) -> Option<u64> {
        if let Some(layer) = &self.imagery_layer {
            if let Ok(l) = layer.try_borrow() {
                return Some(l.id());
            }
        }
        self.layer_id
    }

    /// Returns the resolved owning layer index, dynamically querying the attached `imagery_layer`
    /// first so reordering updates are immediately reflected, or falling back to `layer_index`.
    pub fn layer_index(&self) -> Option<usize> {
        if let Some(layer) = &self.imagery_layer {
            if let Ok(l) = layer.try_borrow() {
                let idx = l.layer_index();
                if idx >= 0 {
                    return Some(idx as usize);
                }
            }
        }
        self.layer_index
    }

    /// Releases all imagery references held by this instance.
    pub fn free_resources(&mut self) {
        if let Some(ready) = self.ready_imagery.take() {
            ready.borrow_mut().release_reference();
        }
        if let Some(loading) = self.loading_imagery.take() {
            loading.borrow_mut().release_reference();
        }
    }

    /// Processes the load state machine for this tile-imagery association against a specific layer.
    ///
    /// Returns `true` if this tile imagery is done loading (either ready or using
    /// the best available substitute); otherwise `false`.
    pub fn process_state_machine(
        &mut self,
        layer: &mut ImageryLayer,
        terrain_rectangle: &Rectangle,
        frame_state: Option<&FrameState>,
        skip_loading: bool,
    ) -> bool {
        layer.process_tile_imagery(self, terrain_rectangle, frame_state, skip_loading)
    }

    /// Processes the state machine using this instance's own attached owning [`ImageryLayer`].
    ///
    /// Returns `None` if no owning layer handle is attached.
    pub fn process_state_machine_with_own_layer(
        &mut self,
        terrain_rectangle: &Rectangle,
        frame_state: Option<&FrameState>,
        skip_loading: bool,
    ) -> Option<bool> {
        let layer_handle = self.imagery_layer.clone()?;
        let mut layer = layer_handle.borrow_mut();
        Some(self.process_state_machine(&mut layer, terrain_rectangle, frame_state, skip_loading))
    }

    pub fn is_done_loading(&self) -> bool {
        self.loading_imagery.is_none()
    }

    pub fn has_ready_imagery(&self) -> bool {
        self.ready_imagery.is_some()
    }
}

impl Drop for TileImagery {
    fn drop(&mut self) {
        self.free_resources();
    }
}
