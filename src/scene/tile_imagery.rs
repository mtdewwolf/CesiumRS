//! CesiumJS `TileImagery`: the association between a terrain tile and an imagery tile.

use super::imagery::Imagery;
use crate::core::Cartesian4;
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
}

impl TileImagery {
    pub fn new(
        imagery: Rc<RefCell<Imagery>>,
        texture_coordinate_rectangle: Cartesian4,
        use_web_mercator_t: bool,
    ) -> Self {
        imagery.borrow_mut().add_reference();
        Self {
            ready_imagery: None,
            loading_imagery: Some(imagery),
            texture_coordinate_rectangle,
            texture_translation_and_scale: None,
            use_web_mercator_t,
        }
    }

    pub fn free_resources(&mut self) {
        if let Some(ready) = self.ready_imagery.take() {
            ready.borrow_mut().release_reference();
        }
        if let Some(loading) = self.loading_imagery.take() {
            loading.borrow_mut().release_reference();
        }
    }
}

impl Drop for TileImagery {
    fn drop(&mut self) {
        self.free_resources();
    }
}
