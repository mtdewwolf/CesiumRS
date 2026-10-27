//! CesiumJS `Imagery`: stores details about a single tile of imagery.

use super::{FrameResource, ImageryImage, ImageryState};
use crate::core::{Credit, Rectangle};
use std::cell::RefCell;
use std::rc::Rc;

/// Stores details about a single tile of imagery.
pub struct Imagery {
    pub x: i32,
    pub y: i32,
    pub level: u32,
    pub state: ImageryState,
    pub rectangle: Rectangle,
    pub image_url: Option<String>,
    pub image: Option<ImageryImage>,
    pub texture: Option<FrameResource>,
    pub texture_web_mercator: Option<FrameResource>,
    pub credits: Option<Vec<Credit>>,
    pub reference_count: usize,
    pub parent: Option<Rc<RefCell<Imagery>>>,
}

impl std::fmt::Debug for Imagery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Imagery")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("level", &self.level)
            .field("state", &self.state)
            .field("rectangle", &self.rectangle)
            .field("reference_count", &self.reference_count)
            .finish()
    }
}

impl Imagery {
    pub fn new(
        x: i32,
        y: i32,
        level: u32,
        rectangle: Rectangle,
        parent: Option<Rc<RefCell<Imagery>>>,
    ) -> Self {
        Self {
            x,
            y,
            level,
            state: ImageryState::Unloaded,
            rectangle,
            image_url: None,
            image: None,
            texture: None,
            texture_web_mercator: None,
            credits: None,
            reference_count: 0,
            parent,
        }
    }

    pub fn create_placeholder() -> Self {
        Self {
            x: 0,
            y: 0,
            level: 0,
            state: ImageryState::Placeholder,
            rectangle: Rectangle::MAX_VALUE,
            image_url: None,
            image: None,
            texture: None,
            texture_web_mercator: None,
            credits: None,
            reference_count: 1,
            parent: None,
        }
    }

    pub fn add_reference(&mut self) {
        self.reference_count += 1;
    }

    pub fn release_reference(&mut self) -> usize {
        self.reference_count = self.reference_count.saturating_sub(1);
        if self.reference_count == 0 {
            if let Some(parent) = &self.parent {
                parent.borrow_mut().release_reference();
            }
            self.image = None;
            self.texture = None;
            self.texture_web_mercator = None;
        }
        self.reference_count
    }
}
