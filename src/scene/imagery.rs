//! CesiumJS `Imagery`: stores details about a single tile of imagery.
//!
//! Tracks lifecycle state (`ImageryState`), coordinates, imagery bounds,
//! parent tile reference for quadtree ancestor fallbacks, decoded provider
//! images, and in-flight asynchronous requests.
//!
//! Note on GPU gaps: texture creation and Web Mercator to Geographic reprojection
//! require WebGL/GPU compute resources, which are executed during GPU frame passes
//! rather than simulated with mock handles.

use super::{FrameResource, ImageryFuture, ImageryImage, ImageryState};
use crate::core::{Credit, Rectangle};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::collections::HashMap;

pub(crate) type ImageryCache = RefCell<HashMap<(i32, i32, u32), Rc<RefCell<Imagery>>>>;

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
    pub in_flight_request: Option<ImageryFuture>,
    pub(crate) cache: Weak<ImageryCache>,
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
            .field("has_request", &self.in_flight_request.is_some())
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
            in_flight_request: None,
            cache: Weak::new(),
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
            in_flight_request: None,
            cache: Weak::new(),
        }
    }

    pub fn add_reference(&mut self) {
        self.reference_count += 1;
    }

    pub fn release_reference(&mut self) -> usize {
        if self.reference_count == 0 {
            return 0;
        }
        self.reference_count -= 1;
        if self.reference_count == 0 {
            if let Some(cache) = self.cache.upgrade() {
                let mut cache = cache.borrow_mut();
                let key = (self.x, self.y, self.level);
                if cache.get(&key).is_some_and(|entry| std::ptr::eq(entry.as_ptr(), self)) {
                    cache.remove(&key);
                }
            }
            if let Some(parent) = &self.parent {
                parent.borrow_mut().release_reference();
            }
            self.image = None;
            self.texture = None;
            self.texture_web_mercator = None;
            self.in_flight_request = None;
        }
        self.reference_count
    }

    /// Drops the in-flight future and resets state to `Unloaded`.
    /// The transport may require its own cancellation mechanism.
    pub fn cancel_request(&mut self) {
        if self.in_flight_request.take().is_some() && self.state == ImageryState::Transitioning {
            self.state = ImageryState::Unloaded;
        }
    }

    pub fn has_request(&self) -> bool {
        self.in_flight_request.is_some()
    }

    pub fn is_unloaded(&self) -> bool {
        self.state == ImageryState::Unloaded
    }

    pub fn is_transitioning(&self) -> bool {
        self.state == ImageryState::Transitioning
    }

    pub fn is_received(&self) -> bool {
        self.state == ImageryState::Received
    }

    pub fn is_texture_loaded(&self) -> bool {
        self.state == ImageryState::TextureLoaded
    }

    pub fn is_ready(&self) -> bool {
        self.state == ImageryState::Ready
    }

    pub fn is_failed(&self) -> bool {
        self.state == ImageryState::Failed
    }

    pub fn is_invalid(&self) -> bool {
        self.state == ImageryState::Invalid
    }
}
