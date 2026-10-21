//! Cesium TextureAtlas packing, asynchronous image requests and GPU repacking.
use super::{context::*, *};
use crate::core::{create_guid::create_guid, BoundingRectangle, PixelFormat, RuntimeError, TexturePacker};
use futures_channel::oneshot;
use futures_util::{
    future::{select, Either, LocalBoxFuture, Shared},
    stream::FuturesUnordered,
    FutureExt, StreamExt,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    rc::Rc,
};
pub type AtlasImageRequest = Shared<LocalBoxFuture<'static, Result<i64>>>;
#[derive(Clone, Copy, Debug)]
pub struct TextureAtlasOptions {
    pub border_width_in_pixels: f64,
    pub initial_size: [u32; 2],
    pub pixel_format: PixelFormat,
    pub sampler: Option<Sampler>,
}
impl Default for TextureAtlasOptions {
    fn default() -> Self {
        Self { border_width_in_pixels: 1., initial_size: [16, 16], pixel_format: PixelFormat::Rgba, sampler: None }
    }
}
struct AtlasLife {
    destroyed: Cell<bool>,
    rectangles: RefCell<Vec<Option<BoundingRectangle>>>,
    cancel: RefCell<BTreeMap<usize, oneshot::Sender<()>>>,
}
struct PendingImage<G: Gl> {
    index: usize,
    image: TextureSource<G>,
    complete: oneshot::Sender<Result<i64>>,
}
pub struct TextureAtlas<G: Gl + 'static> {
    options: TextureAtlasOptions,
    texture: Option<Rc<Texture<G>>>,
    packer: Option<TexturePacker>,
    guid: String,
    life: Rc<AtlasLife>,
    inbox: Rc<RefCell<Vec<PendingImage<G>>>>,
    index_by_id: BTreeMap<String, usize>,
    requests: BTreeMap<String, AtlasImageRequest>,
    pending_requests: RefCell<FuturesUnordered<AtlasImageRequest>>,
    subregions: BTreeMap<usize, usize>,
    subrequests: BTreeMap<usize, AtlasImageRequest>,
    next_index: usize,
}
impl<G: Gl + 'static> TextureAtlas<G> {
    pub fn add_image_resource<H: crate::core::ImageHost<Image = G::Object> + 'static>(
        &mut self,
        id: impl Into<String>,
        resource: crate::core::Resource,
        client: &crate::core::ResourceClient,
        host: Rc<H>,
    ) -> Result<AtlasImageRequest> {
        let id = id.into();
        if let Some(request) = self.requests.get(&id) {
            return Ok(request.clone());
        }
        let future = client
            .fetch_image(&resource, host.clone(), Default::default())
            .ok_or_else(|| RuntimeError::new("Atlas image request was deferred"))?;
        self.add_image_future(id, async move {
            let image = future
                .await
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .ok_or_else(|| RuntimeError::new("Atlas image request returned no image"))?;
            let (width, height) = host.dimensions(&image);
            Ok(TextureSource::Image { width, height, image })
        })
    }
    pub fn new(options: TextureAtlasOptions) -> Result<Self> {
        ensure(
            options.border_width_in_pixels.is_finite()
                && options.border_width_in_pixels >= 0.
                && options.initial_size.iter().all(|&n| n > 0),
            "Invalid atlas border or initial dimensions",
        )?;
        Ok(Self {
            options,
            texture: None,
            packer: None,
            guid: create_guid(),
            life: Rc::new(AtlasLife {
                destroyed: Cell::new(false),
                rectangles: RefCell::new(vec![]),
                cancel: RefCell::new(BTreeMap::new()),
            }),
            inbox: Rc::new(RefCell::new(vec![])),
            index_by_id: BTreeMap::new(),
            requests: BTreeMap::new(),
            pending_requests: RefCell::new(FuturesUnordered::new()),
            subregions: BTreeMap::new(),
            subrequests: BTreeMap::new(),
            next_index: 0,
        })
    }
    pub fn border_width_in_pixels(&self) -> f64 {
        self.options.border_width_in_pixels
    }
    pub fn pixel_format(&self) -> PixelFormat {
        self.options.pixel_format
    }
    pub fn sampler(&self) -> Option<Sampler> {
        self.options.sampler
    }
    pub fn texture(&self) -> Option<&Rc<Texture<G>>> {
        self.texture.as_ref()
    }
    pub fn guid(&self) -> &str {
        &self.guid
    }
    pub fn number_of_images(&self) -> usize {
        self.next_index
    }
    pub fn size_in_bytes(&self) -> usize {
        self.texture.as_ref().map_or(0, |t| t.size_in_bytes())
    }
    pub fn rectangles(&self) -> std::cell::Ref<'_, Vec<Option<BoundingRectangle>>> {
        self.life.rectangles.borrow()
    }
    pub fn get_image_index(&self, id: &str) -> Option<usize> {
        self.index_by_id.get(id).copied()
    }
    pub fn add_image(&mut self, id: impl Into<String>, image: TextureSource<G>) -> Result<AtlasImageRequest> {
        self.add_image_future(id, std::future::ready(Ok(image)))
    }
    pub fn add_image_future<F>(&mut self, id: impl Into<String>, image: F) -> Result<AtlasImageRequest>
    where
        F: Future<Output = Result<TextureSource<G>>> + 'static,
    {
        ensure(!self.is_destroyed(), "Texture atlas is destroyed")?;
        let id = id.into();
        if let Some(request) = self.requests.get(&id) {
            return Ok(request.clone());
        }
        let index = self.next_index;
        self.next_index += 1;
        self.life.rectangles.borrow_mut().resize(self.next_index, None);
        self.index_by_id.insert(id.clone(), index);
        let (cancel, canceled) = oneshot::channel();
        self.life.cancel.borrow_mut().insert(index, cancel);
        let life = Rc::downgrade(&self.life);
        let inbox = Rc::downgrade(&self.inbox);
        let request = async move {
            let result = async {
                let (image, canceled) = match select(image.boxed_local(), canceled).await {
                    Either::Left((image, canceled)) => (image?, canceled),
                    Either::Right(_) => return Ok(-1),
                };
                if life.upgrade().is_none_or(|l| l.destroyed.get()) {
                    return Ok(-1);
                }
                let Some(inbox) = inbox.upgrade() else { return Ok(-1) };
                let (complete, result) = oneshot::channel();
                inbox.borrow_mut().push(PendingImage { index, image, complete });
                match select(result, canceled).await {
                    Either::Left((result, _)) => result.unwrap_or(Ok(-1)),
                    Either::Right(_) => Ok(-1),
                }
            }
            .await;
            if let Some(life) = life.upgrade() {
                life.cancel.borrow_mut().remove(&index);
            }
            result
        }
        .boxed_local()
        .shared();
        self.requests.insert(id, request.clone());
        self.pending_requests.borrow_mut().push(request.clone());
        self.drive_requests();
        Ok(request)
    }
    pub fn add_image_with<F, Fut>(&mut self, id: impl Into<String>, provider: F) -> Result<AtlasImageRequest>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<TextureSource<G>>> + 'static,
    {
        let id = id.into();
        if let Some(request) = self.requests.get(&id) {
            return Ok(request.clone());
        }
        let future = provider(id.clone());
        self.add_image_future(id, future)
    }
    pub fn add_image_sub_region(&mut self, id: &str, rectangle: BoundingRectangle) -> Result<AtlasImageRequest> {
        ensure(!self.is_destroyed(), "Texture atlas is destroyed")?;
        let parent = *self
            .index_by_id
            .get(id)
            .ok_or_else(|| RuntimeError::new(format!("image with id \"{id}\" not found in the atlas.")))?;
        for (&index, &p) in &self.subregions {
            if p == parent && self.life.rectangles.borrow()[index] == Some(rectangle) {
                return Ok(self.subrequests[&index].clone());
            }
        }
        let index = self.next_index;
        self.next_index += 1;
        self.life.rectangles.borrow_mut().resize(self.next_index, None);
        self.life.rectangles.borrow_mut()[index] = Some(rectangle);
        self.subregions.insert(index, parent);
        let life = Rc::downgrade(&self.life);
        let parent_request = self.requests[id].clone();
        let request = async move {
            if parent_request.await? == -1 {
                return Ok(-1);
            }
            let Some(life) = life.upgrade() else { return Ok(-1) };
            if life.destroyed.get() {
                return Ok(-1);
            }
            let parent = life.rectangles.borrow()[parent]
                .ok_or_else(|| RuntimeError::new("Parent image has not been packed"))?;
            ensure(
                rectangle.x <= parent.width
                    && rectangle.x + rectangle.width <= parent.width
                    && rectangle.y <= parent.height
                    && rectangle.y + rectangle.height <= parent.height,
                "Subregion exceeds parent image",
            )?;
            Ok(index as i64)
        }
        .boxed_local()
        .shared();
        self.subrequests.insert(index, request.clone());
        self.pending_requests.borrow_mut().push(request.clone());
        self.drive_requests();
        Ok(request)
    }
    pub fn compute_texture_coordinates(&self, index: usize) -> Result<BoundingRectangle> {
        let rectangles = self.life.rectangles.borrow();
        let Some(Some(rectangle)) = rectangles.get(index) else { return Ok(BoundingRectangle::default()) };
        let texture = self.texture.as_ref().ok_or_else(|| RuntimeError::new("Atlas has not been updated"))?;
        let mut r = *rectangle;
        if let Some(&parent) = self.subregions.get(&index) {
            let p = rectangles[parent].ok_or_else(|| RuntimeError::new("Parent image has not been packed"))?;
            r.x += p.x;
            r.y += p.y;
        }
        r.x /= texture.width() as f64;
        r.y /= texture.height() as f64;
        r.width /= texture.width() as f64;
        r.height /= texture.height() as f64;
        Ok(r)
    }
    fn create_texture(&self, context: &Context<G>, width: u32, height: u32) -> Result<Rc<Texture<G>>> {
        let mut options = TextureOptions::rgba(width, height);
        options.pixel_format = self.options.pixel_format;
        options.sampler = self.options.sampler.unwrap_or_default();
        Ok(Rc::new(Texture::new(context, options, None)?))
    }
    fn drive_requests(&self) {
        // Poll newly registered tasks immediately, then process wakeups in
        // completion order. Image IDs must not determine equal-area packing.
        let mut pending = self.pending_requests.borrow_mut();
        while let Some(Some(_)) = pending.next().now_or_never() {}
    }
    pub fn update(&mut self, context: &Context<G>) -> Result<bool> {
        ensure(!self.is_destroyed(), "Texture atlas is destroyed")?;
        if let Some(texture) = &self.texture {
            context.same(&texture.resource.context)?;
        } else {
            let [w, h] = self.options.initial_size;
            self.texture = Some(self.create_texture(context, w, h)?);
            self.packer = Some(TexturePacker::new(w as f64, h as f64, self.options.border_width_in_pixels));
        }
        self.drive_requests();
        let mut queue = std::mem::take(&mut *self.inbox.borrow_mut());
        if queue.is_empty() {
            return Ok(false);
        }
        queue.sort_by_key(|r| {
            let [w, h] = r.image.dimensions();
            std::cmp::Reverse(w as u64 * h as u64)
        });
        let result = self.pack_queue(context, &queue);
        if let Err(error) = result {
            for request in queue {
                let _ = request.complete.send(Err(error.clone()));
            }
            self.drive_requests();
            return Err(error);
        }
        let texture = self.texture.as_ref().unwrap();
        for request in queue {
            let r = self.life.rectangles.borrow()[request.index].unwrap();
            let region = TextureRegion { x: r.x as u32, y: r.y as u32, width: r.width as u32, height: r.height as u32 };
            let result = match request.image {
                TextureSource::Pixels { bytes, .. } => texture.copy_from(&bytes, region),
                TextureSource::Image { image, .. } => texture.copy_from_image(image, region),
            };
            let _ = request.complete.send(result.map(|_| request.index as i64));
        }
        self.drive_requests();
        Ok(true)
    }
    fn pack_queue(&mut self, context: &Context<G>, queue: &[PendingImage<G>]) -> Result<()> {
        let mut packer = self.packer.as_ref().unwrap().clone();
        let mut rectangles = self.life.rectangles.borrow().clone();
        let mut failed = None;
        for (i, request) in queue.iter().enumerate() {
            let [w, h] = request.image.dimensions();
            if let Some(node) = packer.pack(request.index, w as f64, h as f64) {
                rectangles[request.index] = Some(node.rectangle);
            } else {
                failed = Some(i);
                break;
            }
        }
        let Some(offset) = failed else {
            self.packer = Some(packer);
            *self.life.rectangles.borrow_mut() = rectangles;
            return Ok(());
        };
        let old = self.texture.as_ref().unwrap().clone();
        let mut width = old.width();
        let mut height = old.height();
        let mut area = 0u64;
        let mut to_pack: Vec<_> = rectangles
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.filter(|_| !self.subregions.contains_key(&i)).map(|r| (i, r.width as u32, r.height as u32))
            })
            .collect();
        for request in &queue[offset..] {
            let [w, h] = request.image.dimensions();
            width = width.max(w);
            height = height.max(h);
            area = area.checked_add(w as u64 * h as u64).ok_or_else(|| RuntimeError::new("Atlas area overflow"))?;
            to_pack.push((request.index, w, h));
        }
        width = width.checked_next_power_of_two().ok_or_else(|| RuntimeError::new("Atlas width overflow"))?;
        height = height.checked_next_power_of_two().ok_or_else(|| RuntimeError::new("Atlas height overflow"))?;
        let grow = |w: &mut u32, h: &mut u32| -> Result<()> {
            let d = if *w > *h { h } else { w };
            *d = d.checked_mul(2).ok_or_else(|| RuntimeError::new("Atlas size overflow"))?;
            Ok(())
        };
        while area >= width as u64 * height as u64 {
            grow(&mut width, &mut height)?;
        }
        to_pack.sort_by_key(|&(_, w, h)| std::cmp::Reverse(w as u64 * h as u64));
        let max = match context.call("getParameter", &[crate::core::webgl_constants::MAX_TEXTURE_SIZE.into()])? {
            GlValue::Number(n) => n as u32,
            _ => u32::MAX,
        };
        let (new_packer, new_rectangles) = loop {
            ensure(width <= max && height <= max, "Atlas exceeds maximum texture size")?;
            let mut p = TexturePacker::new(width as f64, height as f64, self.options.border_width_in_pixels);
            let mut next = vec![None; self.next_index];
            for &i in self.subregions.keys() {
                next[i] = rectangles[i];
            }
            let mut fits = true;
            for &(index, w, h) in &to_pack {
                if let Some(node) = p.pack(index, w as f64, h as f64) {
                    next[index] = Some(node.rectangle);
                } else {
                    fits = false;
                    break;
                }
            }
            if fits {
                break (p, next);
            }
            grow(&mut width, &mut height)?;
        };
        let texture = self.create_texture(context, width, height)?;
        let framebuffer = Framebuffer::new(
            context,
            FramebufferOptions {
                colors: vec![Attachment::Texture(old.clone())],
                destroy_attachments: false,
                ..Default::default()
            },
        )?;
        for (i, source) in rectangles.iter().enumerate() {
            if self.subregions.contains_key(&i) {
                continue;
            }
            if let (Some(source), Some(destination)) = (source, new_rectangles[i]) {
                texture.copy_from_framebuffer(FramebufferCopy {
                    framebuffer: Some(&framebuffer),
                    source: TextureRegion {
                        x: source.x as u32,
                        y: source.y as u32,
                        width: destination.width as u32,
                        height: destination.height as u32,
                    },
                    destination: [destination.x as u32, destination.y as u32],
                })?;
            }
        }
        context.bind_framebuffer(None)?;
        old.destroy()?;
        self.texture = Some(texture);
        self.packer = Some(new_packer);
        *self.life.rectangles.borrow_mut() = new_rectangles;
        self.guid = create_guid();
        Ok(())
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.is_destroyed() {
            return Ok(());
        }
        if let Some(texture) = &self.texture {
            texture.destroy()?;
        }
        self.texture = None;
        self.life.destroyed.set(true);
        let cancel = std::mem::take(&mut *self.life.cancel.borrow_mut());
        for (_, sender) in cancel {
            let _ = sender.send(());
        }
        for item in self.inbox.borrow_mut().drain(..) {
            let _ = item.complete.send(Ok(-1));
        }
        self.drive_requests();
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.life.destroyed.get()
    }
}
impl<G: Gl + 'static> Drop for TextureAtlas<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
