//! Frame-driven glTF texture uploads with shared image leases and host resizing.
use super::{resource_loader::ManagedLoader, *};
use crate::{
    core::{
        image_helpers::resize_image_to_next_power_of_two, ktx2_transcoder::SupportedTextureFormats, ImageHost,
        PixelFormat, RuntimeError,
    },
    renderer::{Context, Gl, MipmapHint, Texture, TextureMinificationFilter as Min, TextureOptions, TextureWrap},
};
use futures_util::FutureExt;
use serde_json::Value;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    sync::Arc,
};
type Result<T> = std::result::Result<T, RuntimeError>;

/// Image upload boundary. Browser hosts override this to upload the original
/// DOM image/bitmap without a canvas readback or additional color conversion.
pub trait ModelImageHost<G: Gl>: ImageHost {
    fn upload_texture(&self, context: &Context<G>, options: TextureOptions, image: &Self::Image) -> Result<Texture<G>> {
        let pixels = self.image_pixels(image, options.width, options.height)?;
        Texture::new(context, options, Some(&pixels))
    }
}
#[cfg(all(feature = "browser", target_arch = "wasm32"))]
impl ModelImageHost<crate::renderer::browser::BrowserGl> for crate::core::browser_host::BrowserImageHost {
    fn upload_texture(
        &self,
        context: &Context<crate::renderer::browser::BrowserGl>,
        options: TextureOptions,
        image: &Self::Image,
    ) -> Result<Texture<crate::renderer::browser::BrowserGl>> {
        Texture::from_image(context, options, image.clone())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct GltfTextureOptions {
    pub asynchronous: bool,
    pub supported_image_formats: SupportedImageFormats,
    pub supported_texture_formats: SupportedTextureFormats,
}
impl Default for GltfTextureOptions {
    fn default() -> Self {
        Self {
            asynchronous: true,
            supported_image_formats: SupportedImageFormats::default(),
            supported_texture_formats: SupportedTextureFormats::default(),
        }
    }
}
/// Optional per-frame quotas for both geometry and texture creation. Reset
/// each frame and place Rc<ResourceUploadBudget> in FrameState.job_scheduler.
pub struct ResourceUploadBudget {
    pub buffers: BufferUploadBudget,
    pub textures: BufferUploadBudget,
}
impl ResourceUploadBudget {
    pub fn new(buffers: usize, textures: usize) -> Self {
        Self { buffers: BufferUploadBudget::new(buffers), textures: BufferUploadBudget::new(textures) }
    }
    pub fn reset(&self, buffers: usize, textures: usize) {
        self.buffers.reset(buffers);
        self.textures.reset(textures);
    }
}
pub struct GltfTextureLoader<G: Gl, H: ImageHost + 'static>
where
    H::Image: 'static,
{
    data: ManagedLoader<Rc<LoadedGltfImage<H::Image>>>,
    child: Rc<RefCell<Option<CacheLease<GltfImageLoader<H>>>>>,
    document: Rc<RefCell<Option<Arc<LoadedGltfJson>>>>,
    texture_info: Value,
    texture: RefCell<Option<Rc<Texture<G>>>>,
    context: Context<G>,
    host: Rc<H>,
    asynchronous: bool,
    statistics: Weak<RefCell<ResourceCacheStatistics>>,
}
impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> GltfTextureLoader<G, H>
where
    H::Image: 'static,
{
    pub fn texture(&self) -> Option<Rc<Texture<G>>> {
        self.texture.borrow().clone()
    }
    fn create_texture(&self, image: &LoadedGltfImage<H::Image>) -> Result<Texture<G>> {
        let document = self.document.borrow();
        let gltf = &document.as_ref().ok_or_else(|| RuntimeError::new("Texture document was unloaded"))?.json;
        let (width, height, internal) = match &image.image {
            GltfImage::Raster(i) => {
                let (w, h) = self.host.dimensions(i);
                (w, h, None)
            }
            GltfImage::Ktx2(i) => (
                i.width,
                i.height,
                Some(
                    PixelFormat::from_value(i.internal_format)
                        .ok_or_else(|| RuntimeError::new("Invalid KTX2 pixel format"))?,
                ),
            ),
        };
        let no_mipmap = internal.is_some_and(|f| f.is_compressed_format()) && image.mip_levels.is_none();
        let sampler = GltfLoaderUtil::create_sampler(gltf, &self.texture_info, no_mipmap)?;
        let mip = matches!(
            sampler.minification_filter,
            Min::NearestMipmapNearest | Min::NearestMipmapLinear | Min::LinearMipmapNearest | Min::LinearMipmapLinear
        );
        let generate_mipmap = internal.is_none() && mip;
        let requires_pot = generate_mipmap
            || matches!(sampler.wrap_s, TextureWrap::Repeat | TextureWrap::MirroredRepeat)
            || matches!(sampler.wrap_t, TextureWrap::Repeat | TextureWrap::MirroredRepeat);
        let mut options = TextureOptions {
            flip_y: false,
            sampler,
            skip_color_space_conversion: true,
            id: self.data.key.clone(),
            ..TextureOptions::rgba(width, height)
        };
        let texture = match &image.image {
            GltfImage::Raster(i) => {
                if requires_pot && (!width.is_power_of_two() || !height.is_power_of_two()) {
                    let resized = resize_image_to_next_power_of_two(&*self.host, i)?;
                    (options.width, options.height) = self.host.dimensions(&resized);
                    self.host.upload_texture(&self.context, options, &resized)?
                } else {
                    self.host.upload_texture(&self.context, options, i)?
                }
            }
            GltfImage::Ktx2(i) => {
                // The pinned typed-buffer path keeps Texture's default orientation.
                options.flip_y = true;
                options.skip_color_space_conversion = false;
                options.pixel_format = internal.expect("KTX2 format");
                options.pre_multiply_alpha = matches!(options.pixel_format, PixelFormat::Rgb | PixelFormat::Luminance);
                let levels = image
                    .mip_levels
                    .as_ref()
                    .map(|ls| ls.iter().map(|l| l.to_vec()).collect::<Vec<_>>())
                    .unwrap_or_default();
                if options.pixel_format.is_compressed_format() {
                    Texture::compressed(&self.context, options, i.buffer_view(), &levels)?
                } else {
                    Texture::with_mipmaps(&self.context, options, i.buffer_view(), &levels)?
                }
            }
        };
        if generate_mipmap {
            texture.generate_mipmap(MipmapHint::DontCare)?;
        }
        Ok(texture)
    }
}
impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> ResourceLoader for GltfTextureLoader<G, H>
where
    H::Image: 'static,
{
    fn cache_key(&self) -> Option<&str> {
        self.data.key.as_deref()
    }
    fn state(&self) -> ResourceLoaderState {
        self.data.state()
    }
    fn is_destroyed(&self) -> bool {
        self.data.is_destroyed()
    }
    fn unload(&self) {
        if let Some(t) = self.texture.borrow_mut().take() {
            let _ = t.destroy();
        }
        self.child.borrow_mut().take();
        self.document.borrow_mut().take();
        self.data.unload();
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
    fn load(&self) -> ResourceLoadFuture {
        let child = self.child.borrow().as_ref().map(CacheLease::loader);
        let owner = Rc::downgrade(&self.child);
        let document = Rc::downgrade(&self.document);
        self.data.load_with_state("Failed to load texture".into(), ResourceLoaderState::Loaded, move || {
            let request = child.as_ref().map(|l| l.load());
            let weak = child.as_ref().map(Rc::downgrade);
            async move {
                let result = async move {
                    request.ok_or_else(|| RuntimeError::new("Image loader was unloaded"))?.await?;
                    weak.and_then(|w| w.upgrade())
                        .and_then(|l| l.image())
                        .ok_or_else(|| RuntimeError::new("Image loader was unloaded"))
                }
                .await;
                if result.is_err() {
                    if let Some(document) = document.upgrade() {
                        document.borrow_mut().take();
                    }
                    if let Some(owner) = owner.upgrade() {
                        owner.borrow_mut().take();
                    }
                }
                result
            }
            .boxed_local()
        })
    }
    fn process(&self, frame: &FrameState) -> Result<bool> {
        if self.is_destroyed() {
            return Err(RuntimeError::new("Texture loader was destroyed"));
        }
        if self.state() == ResourceLoaderState::Ready {
            return Ok(true);
        }
        if !matches!(self.state(), ResourceLoaderState::Loaded | ResourceLoaderState::Processing) {
            return Ok(false);
        }
        let Some(image) = self.data.value() else { return Ok(false) };
        self.data.set_state(ResourceLoaderState::Processing);
        if self.asynchronous {
            if let Some(scheduler) = &frame.job_scheduler {
                let allowed = if let Some(b) = scheduler.get::<Rc<ResourceUploadBudget>>() {
                    b.textures.claim()
                } else if let Some(b) = scheduler.get::<Rc<BufferUploadBudget>>() {
                    b.claim()
                } else {
                    return Err(RuntimeError::new("Unsupported texture upload scheduler"));
                };
                if !allowed {
                    return Ok(false);
                }
            }
        }
        let texture = Rc::new(self.create_texture(&image)?);
        self.unload();
        if let Some(s) = self.statistics.upgrade() {
            s.borrow_mut().add_texture_loader(self.cache_key().expect("key"), texture.size_in_bytes() as u64);
        }
        *self.texture.borrow_mut() = Some(texture);
        self.data.set_state(ResourceLoaderState::Ready);
        Ok(true)
    }
}
impl ResourceCache {
    pub fn texture_loader<G: Gl + 'static, H: ModelImageHost<G> + 'static>(
        &self,
        request: &GltfResource,
        texture_info: Value,
        context: Context<G>,
        host: Rc<H>,
        options: GltfTextureOptions,
    ) -> Result<CacheLease<GltfTextureLoader<G, H>>>
    where
        H::Image: 'static,
    {
        let key = ResourceCacheKey::texture(
            request.key_context(),
            &texture_info,
            options.supported_image_formats,
            context.id(),
        )?;
        self.get_or_insert(&key, CacheResourceKind::Texture, || {
            let id = GltfLoaderUtil::image_id(
                &request.document.json,
                texture_info["index"].as_u64().ok_or_else(|| RuntimeError::new("Invalid glTF texture ID"))? as usize,
                options.supported_image_formats,
            )?;
            let image = self.image_loader(request, id, host.clone(), options.supported_texture_formats)?;
            Ok(GltfTextureLoader {
                data: ManagedLoader::new(Some(key.clone()), None),
                child: Rc::new(RefCell::new(Some(image))),
                document: Rc::new(RefCell::new(Some(request.document.clone()))),
                texture_info,
                texture: RefCell::new(None),
                context,
                host,
                asynchronous: options.asynchronous,
                statistics: Rc::downgrade(&self.statistics()),
            })
        })
    }
}
