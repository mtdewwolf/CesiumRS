//! CesiumJS `BillboardTexture`: tracks a reference to an image and its loading
//! state, as used in a `BillboardCollection` and stored in a texture atlas.

use crate::core::BoundingRectangle;
use crate::scene::BillboardLoadState;

/// Tracks a reference to an image and its loading state, as used in a
/// `BillboardCollection` and stored in a texture atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct BillboardTexture {
    id: Option<String>,
    load_state: BillboardLoadState,
    load_error: Option<String>,
    index: i32,
    width: Option<f64>,
    height: Option<f64>,
    has_subregion: bool,
    subregion: Option<BoundingRectangle>,
    /// Used by billboard collection to track which billboards to update.
    pub dirty: bool,
}

impl Default for BillboardTexture {
    fn default() -> Self {
        Self::new()
    }
}

impl BillboardTexture {
    /// Creates a new `BillboardTexture` in the initial unloaded state.
    pub fn new() -> Self {
        Self {
            id: None,
            load_state: BillboardLoadState::None,
            load_error: None,
            index: -1,
            width: None,
            height: None,
            has_subregion: false,
            subregion: None,
            dirty: false,
        }
    }

    /// If defined, this error was encountered during the loading process.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// The current status of the image load.
    pub fn load_state(&self) -> BillboardLoadState {
        self.load_state
    }

    /// When `true`, this texture is ready to render, i.e., the image has been
    /// loaded and atlas coordinates are available.
    pub fn ready(&self) -> bool {
        self.load_state == BillboardLoadState::Loaded
    }

    /// Returns `true` if there is image data associated with this instance.
    pub fn has_image(&self) -> bool {
        self.load_state != BillboardLoadState::None
    }

    /// A unique identifier for the image, or None if no image data has been associated.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// The width of the associated image. Before the instance is `ready`, this will be None.
    pub fn width(&self) -> Option<f64> {
        self.width
    }

    /// The height of the associated image. Before the instance is `ready`, this will be None.
    pub fn height(&self) -> Option<f64> {
        self.height
    }

    /// The index of the image in the texture atlas.
    pub fn index(&self) -> i32 {
        self.index
    }

    /// Returns `true` if this instance refers to a subregion of an atlas image.
    pub fn has_subregion(&self) -> bool {
        self.has_subregion
    }

    /// Returns the optional subregion rectangle if defined.
    pub fn subregion(&self) -> Option<&BoundingRectangle> {
        self.subregion.as_ref()
    }

    /// Sets the texture into the `Loading` state for the given image ID.
    pub fn set_loading(&mut self, id: impl Into<String>) {
        self.id = Some(id.into());
        self.load_state = BillboardLoadState::Loading;
        self.load_error = None;
    }

    /// Sets the texture into the `Loaded` state with index and dimensions.
    pub fn set_loaded(&mut self, id: impl Into<String>, index: i32, width: f64, height: f64) {
        self.id = Some(id.into());
        self.index = index;
        self.width = Some(width);
        self.height = Some(height);
        self.load_state = BillboardLoadState::Loaded;
        self.load_error = None;
        self.dirty = true;
    }

    /// Sets the texture into the `Error` state with an error message.
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.load_state = BillboardLoadState::Error;
        self.load_error = Some(error.into());
    }

    /// Sets the texture into the `Failed` state.
    pub fn set_failed(&mut self) {
        self.load_state = BillboardLoadState::Failed;
        self.index = -1;
        self.width = None;
        self.height = None;
    }

    /// Assigns a subregion using the index returned by `TextureAtlas::add_image_sub_region`.
    /// The rectangle must be registered with that atlas before calling this method;
    /// passing the parent image index would select the entire parent image.
    pub fn set_image_subregion(&mut self, index: i32, subregion: BoundingRectangle) {
        if index < 0 {
            self.set_failed();
            return;
        }
        self.width = Some(subregion.width);
        self.height = Some(subregion.height);
        self.index = index;
        self.has_subregion = true;
        self.subregion = Some(subregion);
        self.load_state = BillboardLoadState::Loaded;
        self.dirty = true;
    }

    /// Clones state from another `BillboardTexture`.
    pub fn clone_from_source(&mut self, source: &BillboardTexture) {
        self.id = source.id.clone();
        self.load_state = source.load_state;
        self.load_error = source.load_error.clone();
        self.index = source.index;
        self.width = source.width;
        self.height = source.height;
        self.has_subregion = source.has_subregion;
        self.subregion = source.subregion;
        if source.ready() {
            self.dirty = true;
        }
    }

    /// Returns the indexed atlas coordinates, including the atlas-managed subregion offset.
    pub fn compute_texture_coordinates<G: crate::renderer::Gl + 'static>(&self, atlas: &crate::renderer::TextureAtlas<G>) -> std::result::Result<BoundingRectangle, crate::core::RuntimeError> {
        if self.index < 0 {
            return Err(crate::core::RuntimeError::new("Texture has no atlas index"));
        }
        atlas.compute_texture_coordinates(self.index as usize)
    }
}
