//! CesiumJS `BillboardTexture`: a billboard's reference to an image in its
//! collection's texture atlas and the image's load state.
//!
//! Cesium's `loadImage`, `loadImageSubRegion` and `clone` are `async`
//! functions that resume when the atlas resolves an image. Here each resumption
//! is a pending continuation polled by the collection at the start of every
//! update and right after its `afterRender` atlas update, the points at which
//! Cesium's promises have settled in an application. A request the atlas has
//! already resolved continues immediately, as Cesium's `addImage` then returns
//! an index rather than a promise.
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use futures_util::FutureExt;

use crate::{
    core::{BoundingRectangle, RuntimeError},
    renderer::{AtlasImageRequest, Gl, TextureAtlas, TextureSource},
    scene::BillboardLoadState,
};

/// The part of a billboard collection a texture needs: the atlas, the
/// `billboardTextureCache` and the queue of unresolved loads.
pub(crate) struct TextureHost<G: Gl + 'static> {
    pub(crate) atlas: RefCell<Rc<RefCell<TextureAtlas<G>>>>,
    pub(crate) cache: RefCell<std::collections::HashMap<String, Rc<RefCell<BillboardTexture<G>>>>>,
    pub(crate) pending: RefCell<Vec<Weak<RefCell<BillboardTexture<G>>>>>,
}

impl<G: Gl + 'static> TextureHost<G> {
    pub(crate) fn atlas(&self) -> Rc<RefCell<TextureAtlas<G>>> {
        self.atlas.borrow().clone()
    }
}

enum Continuation<G: Gl + 'static> {
    /// The rest of `loadImage(id, …)`; it updates the cache entry and, while
    /// the texture still shows `id`, the texture.
    Image { id: String, request: AtlasImageRequest, entry: Rc<RefCell<BillboardTexture<G>>> },
    /// The rest of `loadImageSubRegion`.
    SubRegion { id: String, sub_region: BoundingRectangle, request: AtlasImageRequest },
    /// `completeLoad` of `BillboardTexture.clone`.
    Clone { id: String, source: Rc<RefCell<BillboardTexture<G>>>, request: Option<AtlasImageRequest> },
}

/// An image reference and its load state.
pub struct BillboardTexture<G: Gl + 'static> {
    host: Weak<TextureHost<G>>,
    id: Option<String>,
    load_state: BillboardLoadState,
    load_error: Option<String>,
    index: i64,
    width: Option<f64>,
    height: Option<f64>,
    has_subregion: bool,
    /// Set when the image changes; the collection rewrites the billboard.
    pub dirty: bool,
    continuations: Vec<Continuation<G>>,
}

impl<G: Gl + 'static> std::fmt::Debug for BillboardTexture<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BillboardTexture")
            .field("id", &self.id)
            .field("load_state", &self.load_state)
            .field("index", &self.index)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl<G: Gl + 'static> BillboardTexture<G> {
    pub(crate) fn new(host: Weak<TextureHost<G>>) -> Self {
        Self {
            host,
            id: None,
            load_state: BillboardLoadState::None,
            load_error: None,
            index: -1,
            width: None,
            height: None,
            has_subregion: false,
            dirty: false,
            continuations: Vec::new(),
        }
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }
    pub fn load_state(&self) -> BillboardLoadState {
        self.load_state
    }
    pub fn ready(&self) -> bool {
        self.load_state == BillboardLoadState::Loaded
    }
    pub fn has_image(&self) -> bool {
        self.load_state != BillboardLoadState::None
    }
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub fn width(&self) -> Option<f64> {
        self.width
    }
    pub fn height(&self) -> Option<f64> {
        self.height
    }
    /// The atlas index, or -1.
    pub fn index(&self) -> i64 {
        self.index
    }
    pub fn has_subregion(&self) -> bool {
        self.has_subregion
    }

    /// `unload`.
    pub(crate) fn unload(&mut self) {
        if self.load_state == BillboardLoadState::None {
            return;
        }
        self.id = None;
        self.load_error = None;
        self.load_state = BillboardLoadState::None;
        self.index = -1;
        self.width = None;
        self.height = None;
        self.dirty = true;
    }

    fn register(this: &Rc<RefCell<Self>>) {
        if let Some(host) = this.borrow().host.upgrade() {
            host.pending.borrow_mut().push(Rc::downgrade(this));
        }
    }

    /// `loadImage(id, image, width, height)`.
    pub(crate) fn load_image(
        this: &Rc<RefCell<Self>>,
        id: String,
        image: super::billboard::BillboardImage<G>,
        size: (Option<f64>, Option<f64>),
    ) {
        if this.borrow().id.as_deref() == Some(id.as_str()) {
            return;
        }
        let Some(host) = this.borrow().host.upgrade() else { return };
        let entry = host
            .cache
            .borrow_mut()
            .entry(id.clone())
            .or_insert_with(|| Rc::new(RefCell::new(BillboardTexture::new(Rc::downgrade(&host)))))
            .clone();
        for t in [&entry, this] {
            let mut t = t.borrow_mut();
            t.id = Some(id.clone());
            t.load_state = BillboardLoadState::Loading;
            t.load_error = None;
        }
        let atlas = host.atlas();
        let request = image.add_to(&mut atlas.borrow_mut(), id.clone(), size);
        match request {
            Err(error) => Self::finish_image(this, &entry, &id, Err(error), &atlas),
            Ok(request) => match request.clone().now_or_never() {
                Some(result) => Self::finish_image(this, &entry, &id, result, &atlas),
                None => {
                    this.borrow_mut().continuations.push(Continuation::Image { id, request, entry });
                    Self::register(this);
                }
            },
        }
    }

    fn finish_image(
        this: &Rc<RefCell<Self>>,
        entry: &Rc<RefCell<Self>>,
        id: &str,
        result: Result<i64, RuntimeError>,
        atlas: &Rc<RefCell<TextureAtlas<G>>>,
    ) {
        let same = |t: &Self| t.id.as_deref() == Some(id);
        match result {
            Err(error) => {
                {
                    let mut e = entry.borrow_mut();
                    e.load_state = BillboardLoadState::Error;
                    e.load_error = Some(error.message.clone());
                }
                let mut t = this.borrow_mut();
                if same(&t) {
                    t.load_state = BillboardLoadState::Error;
                    t.load_error = Some(error.message);
                }
            }
            Ok(-1) => {
                {
                    let mut e = entry.borrow_mut();
                    e.load_state = BillboardLoadState::Failed;
                    e.index = -1;
                }
                let mut t = this.borrow_mut();
                if same(&t) {
                    t.load_state = BillboardLoadState::Failed;
                    t.index = -1;
                }
            }
            Ok(index) => {
                let rectangle = atlas.borrow().rectangles().get(index as usize).copied().flatten().unwrap_or_default();
                if !Rc::ptr_eq(entry, this) {
                    let mut e = entry.borrow_mut();
                    e.index = index;
                    e.load_state = BillboardLoadState::Loaded;
                    e.width = Some(rectangle.width);
                    e.height = Some(rectangle.height);
                }
                let mut t = this.borrow_mut();
                if same(&t) {
                    t.index = index;
                    t.load_state = BillboardLoadState::Loaded;
                    t.width = Some(rectangle.width);
                    t.height = Some(rectangle.height);
                    t.dirty = true;
                }
            }
        }
    }

    /// `addImageSubRegion(id, subRegion)`.
    pub(crate) fn add_image_sub_region(
        this: &Rc<RefCell<Self>>,
        id: String,
        sub_region: BoundingRectangle,
    ) -> Result<(), RuntimeError> {
        {
            let mut t = this.borrow_mut();
            t.id = Some(id.clone());
            t.load_error = None;
            t.has_subregion = true;
        }
        let Some(host) = this.borrow().host.upgrade() else { return Ok(()) };
        let atlas = host.atlas();
        let before = atlas.borrow().number_of_images();
        let request = atlas.borrow_mut().add_image_sub_region(&id, sub_region)?;
        let cached = atlas.borrow().number_of_images() == before;
        // A cached sub-region of a resolved image is an index; anything else
        // is a promise.
        if cached {
            if let Some(Ok(index)) = request.clone().now_or_never() {
                this.borrow_mut().set_image_sub_region(index, sub_region);
                return Ok(());
            }
        }
        let mut t = this.borrow_mut();
        t.load_state = BillboardLoadState::Loading;
        t.continuations.push(Continuation::SubRegion { id, sub_region, request });
        drop(t);
        Self::register(this);
        Ok(())
    }

    /// `setImageSubRegion(index, subRegion)`.
    fn set_image_sub_region(&mut self, index: i64, sub_region: BoundingRectangle) {
        if self.index == index {
            return;
        }
        if index == -1 {
            self.load_state = BillboardLoadState::Failed;
            self.index = -1;
            self.width = None;
            self.height = None;
            return;
        }
        self.width = Some(sub_region.width);
        self.height = Some(sub_region.height);
        self.index = index;
        self.dirty = true;
    }

    /// `computeTextureCoordinates`.
    pub fn compute_texture_coordinates(&self) -> Result<BoundingRectangle, RuntimeError> {
        let Some(host) = self.host.upgrade() else { return Ok(BoundingRectangle::default()) };
        if self.index < 0 {
            return Ok(BoundingRectangle::default());
        }
        let atlas = host.atlas();
        let atlas = atlas.borrow();
        atlas.compute_texture_coordinates(self.index as usize)
    }

    /// `BillboardTexture.clone(source, target)`.
    pub(crate) fn clone_into(source: &Rc<RefCell<Self>>, target: &Rc<RefCell<Self>>) {
        if Rc::ptr_eq(source, target) {
            return;
        }
        let ready = {
            let s = source.borrow();
            let mut t = target.borrow_mut();
            t.id = s.id.clone();
            t.load_state = s.load_state;
            t.load_error = None;
            t.index = s.index;
            t.width = s.width;
            t.height = s.height;
            t.has_subregion = s.has_subregion;
            s.ready()
        };
        if ready {
            target.borrow_mut().dirty = true;
            return;
        }
        let Some(id) = source.borrow().id.clone() else { return };
        let request = source.borrow().host.upgrade().and_then(|h| h.atlas().borrow().image_request(&id));
        target.borrow_mut().continuations.push(Continuation::Clone { id, source: source.clone(), request });
        Self::register(target);
    }

    /// Runs the continuations whose requests have settled. Returns whether
    /// any remain.
    pub(crate) fn poll(this: &Rc<RefCell<Self>>) -> bool {
        let continuations = std::mem::take(&mut this.borrow_mut().continuations);
        let mut remaining = Vec::new();
        for continuation in continuations {
            match continuation {
                Continuation::Image { id, request, entry } => match request.clone().now_or_never() {
                    Some(result) => {
                        let Some(host) = this.borrow().host.upgrade() else { continue };
                        Self::finish_image(this, &entry, &id, result, &host.atlas());
                    }
                    None => remaining.push(Continuation::Image { id, request, entry }),
                },
                Continuation::SubRegion { id, sub_region, request } => match request.clone().now_or_never() {
                    Some(Err(error)) => {
                        let mut t = this.borrow_mut();
                        t.load_state = BillboardLoadState::Error;
                        t.load_error = Some(error.message);
                    }
                    Some(Ok(index)) => {
                        let mut t = this.borrow_mut();
                        if t.id.as_deref() == Some(id.as_str()) {
                            t.load_state = BillboardLoadState::Loaded;
                            t.set_image_sub_region(index, sub_region);
                        }
                    }
                    None => remaining.push(Continuation::SubRegion { id, sub_region, request }),
                },
                Continuation::Clone { id, source, request } => {
                    let settled = request.as_ref().is_none_or(|r| r.clone().now_or_never().is_some());
                    if !settled {
                        remaining.push(Continuation::Clone { id, source, request });
                        continue;
                    }
                    if request.is_some_and(|r| r.now_or_never().is_some_and(|r| r.is_err())) {
                        continue;
                    }
                    let s = source.borrow();
                    let mut t = this.borrow_mut();
                    if t.id.as_deref() != Some(id.as_str()) {
                        continue;
                    }
                    t.load_state = s.load_state;
                    t.load_error = s.load_error.clone();
                    t.index = s.index;
                    t.width = s.width;
                    t.height = s.height;
                    t.dirty = true;
                }
            }
        }
        let mut t = this.borrow_mut();
        remaining.append(&mut t.continuations);
        t.continuations = remaining;
        !t.continuations.is_empty()
    }
}

/// Polls every texture with pending loads.
pub(crate) fn poll_pending<G: Gl + 'static>(host: &TextureHost<G>) {
    let pending = std::mem::take(&mut *host.pending.borrow_mut());
    let mut keep = Vec::new();
    for texture in pending {
        if let Some(t) = texture.upgrade() {
            if BillboardTexture::poll(&t) {
                keep.push(texture);
            }
        }
    }
    host.pending.borrow_mut().extend(keep);
}

/// What an image becomes in the atlas.
pub(crate) fn source_with_size<G: Gl>(
    mut source: TextureSource<G>,
    size: (Option<f64>, Option<f64>),
) -> TextureSource<G> {
    if let TextureSource::Image { width, height, .. } = &mut source {
        if let Some(w) = size.0 {
            *width = w as u32;
        }
        if let Some(h) = size.1 {
            *height = h as u32;
        }
    }
    source
}
