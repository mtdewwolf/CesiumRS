//! CesiumJS `Scene/Model/TextureManager`: frame-driven 2D texture uploads for
//! custom-shader sampler uniforms.
//!
//! Upstream starts an asynchronous `Resource.fetchImage()` per URL uniform and
//! pushes the decoded image into a per-frame queue. Rust keeps the same
//! two-phase shape without storing a host-specific image type: a host decodes
//! the image (or the manager receives the RGBA bytes) and the queued entry is
//! uploaded during [`TextureManager::update`], the only method that touches
//! the GPU. WebGL1 power-of-two resizing reuses the host resize boundary, as
//! `GltfTextureLoader` does.

use super::texture_uniform::TextureUniform;
use crate::{
    core::{image_helpers::resize_image_to_next_power_of_two, ImageHost, PixelDatatype, RgbaImage, RuntimeError},
    renderer::{Context, Gl, MipmapHint, Sampler, Texture, TextureMinificationFilter, TextureOptions, TextureWrap},
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

type Result<T> = std::result::Result<T, RuntimeError>;

/// Texture contents queued for the next update.
#[derive(Clone, Debug)]
pub enum LoadedImageSource {
    /// Row-major typed-array contents with the uniform's own format.
    TypedArray(Vec<u8>),
    /// A decoded image. Rows are top-down, matching `ImageHost::image_pixels`.
    Rgba { width: u32, height: u32, pixels: Vec<u8> },
}

/// One queued texture upload.
#[derive(Clone, Debug)]
pub struct LoadedImage<G: Gl> {
    pub id: String,
    pub texture_uniform: TextureUniform,
    pub source: LoadedImageSource,
    marker: std::marker::PhantomData<G>,
}

/// A WebGL1-only warning raised by the upload path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureManagerWarning {
    /// A non-byte typed-array texture cannot be resized for mipmaps/wrapping.
    CannotResize { id: String, need_mipmap: bool },
}

struct ManagerState<G: Gl> {
    textures: BTreeMap<String, Rc<Texture<G>>>,
    /// Set once an upload fails so the uniform getter falls back to the same
    /// default texture upstream assigns in the fetch rejection handler.
    failed: BTreeMap<String, bool>,
}

impl<G: Gl> Default for ManagerState<G> {
    fn default() -> Self {
        Self { textures: BTreeMap::new(), failed: BTreeMap::new() }
    }
}

/// Owns the textures created for custom-shader sampler uniforms.
pub struct TextureManager<G: Gl> {
    state: RefCell<ManagerState<G>>,
    default_texture: RefCell<Option<Rc<Texture<G>>>>,
    loaded_images: RefCell<Vec<LoadedImage<G>>>,
    last_updated_frame: RefCell<Option<u32>>,
}

impl<G: Gl> Default for TextureManager<G> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: Gl> TextureManager<G> {
    pub fn new() -> Self {
        Self {
            state: RefCell::new(ManagerState::default()),
            default_texture: RefCell::new(None),
            loaded_images: RefCell::new(Vec::new()),
            last_updated_frame: RefCell::new(None),
        }
    }

    /// The uploaded texture for `texture_id`, or `None` while it is loading.
    pub fn get_texture(&self, texture_id: &str) -> Option<Rc<Texture<G>>> {
        self.state.borrow().textures.get(texture_id).cloned()
    }

    /// The texture a uniform getter should return: the upload when ready,
    /// otherwise the context's default texture.
    pub fn get_texture_or_default(&self, texture_id: &str) -> Option<Rc<Texture<G>>> {
        self.get_texture(texture_id).or_else(|| self.default_texture.borrow().clone())
    }

    pub fn default_texture(&self) -> Option<Rc<Texture<G>>> {
        self.default_texture.borrow().clone()
    }

    /// Whether `texture_id` failed to load and fell back to the default texture.
    pub fn failed(&self, texture_id: &str) -> bool {
        self.state.borrow().failed.get(texture_id).copied().unwrap_or(false)
    }

    /// Number of uploads waiting for the next frame. Upstream keeps the same
    /// queue in `_loadedImages`.
    pub fn pending_len(&self) -> usize {
        self.loaded_images.borrow().len()
    }

    /// Queue a typed-array texture. URL-backed uniforms are fetched by the
    /// caller and queued through [`Self::load_texture_2d_image`].
    pub fn load_texture_2d(&self, texture_id: &str, texture_uniform: &TextureUniform) {
        if let Some(pixels) = &texture_uniform.typed_array {
            self.loaded_images.borrow_mut().push(LoadedImage {
                id: texture_id.into(),
                texture_uniform: texture_uniform.clone(),
                source: LoadedImageSource::TypedArray(pixels.clone()),
                marker: std::marker::PhantomData,
            });
        }
    }

    /// Queue an image that the host already fetched and decoded.
    pub fn load_texture_2d_image(
        &self,
        texture_id: &str,
        texture_uniform: &TextureUniform,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) {
        self.loaded_images.borrow_mut().push(LoadedImage {
            id: texture_id.into(),
            texture_uniform: texture_uniform.clone(),
            source: LoadedImageSource::Rgba { width, height, pixels },
            marker: std::marker::PhantomData,
        });
    }

    /// Record a failed fetch, replacing any existing texture with the default.
    pub fn load_texture_2d_failed(&self, texture_id: &str) {
        let mut state = self.state.borrow_mut();
        if let Some(texture) = state.textures.remove(texture_id) {
            let default = self.default_texture.borrow();
            if default.as_ref().is_none_or(|d| !Rc::ptr_eq(d, &texture)) {
                let _ = texture.destroy();
            }
        }
        state.failed.insert(texture_id.into(), true);
    }

    /// Upload every queued texture. Upstream skips repeated calls in the same
    /// frame; `frame_number` keeps that behavior observable.
    pub fn update<H: ImageHost>(
        &self,
        context: &Context<G>,
        frame_number: u32,
        default_texture: Option<Rc<Texture<G>>>,
        host: &H,
    ) -> Result<Vec<TextureManagerWarning>> {
        if self.last_updated_frame.borrow().is_some_and(|f| f == frame_number) {
            return Ok(Vec::new());
        }
        *self.last_updated_frame.borrow_mut() = Some(frame_number);
        *self.default_texture.borrow_mut() = default_texture;
        let queued = std::mem::take(&mut *self.loaded_images.borrow_mut());
        let mut warnings = Vec::new();
        for loaded in queued {
            self.create_texture(&loaded, context, host, &mut warnings)?;
        }
        Ok(warnings)
    }

    fn create_texture<H: ImageHost>(
        &self,
        loaded: &LoadedImage<G>,
        context: &Context<G>,
        host: &H,
        warnings: &mut Vec<TextureManagerWarning>,
    ) -> Result<()> {
        let texture = Rc::new(if context.webgl2() {
            self.texture_and_mips(loaded, context, host)?
        } else {
            self.webgl1_texture(loaded, context, host, warnings)?
        });
        let previous = self.state.borrow_mut().textures.insert(loaded.id.clone(), texture);
        if let Some(previous) = previous {
            let default = self.default_texture.borrow();
            if default.as_ref().is_none_or(|d| !Rc::ptr_eq(d, &previous)) {
                previous.destroy()?;
            }
        }
        self.state.borrow_mut().failed.insert(loaded.id.clone(), false);
        Ok(())
    }

    fn texture_and_mips<H: ImageHost>(
        &self,
        loaded: &LoadedImage<G>,
        context: &Context<G>,
        host: &H,
    ) -> Result<Texture<G>> {
        let uniform = &loaded.texture_uniform;
        let texture = match &loaded.source {
            LoadedImageSource::TypedArray(_) => self.texture_from_typed_array(loaded, context)?,
            LoadedImageSource::Rgba { width, height, pixels } => {
                self.texture_from_rgba(*width, *height, pixels, &uniform.sampler, context)?
            }
        };
        if sampler_requires_mipmap(&uniform.sampler) {
            texture.generate_mipmap(MipmapHint::Nicest)?;
        }
        let _ = host;
        Ok(texture)
    }

    fn webgl1_texture<H: ImageHost>(
        &self,
        loaded: &LoadedImage<G>,
        context: &Context<G>,
        host: &H,
        warnings: &mut Vec<TextureManagerWarning>,
    ) -> Result<Texture<G>> {
        let uniform = &loaded.texture_uniform;
        let sampler = &uniform.sampler;
        let need_mipmap = sampler_requires_mipmap(sampler);
        let sampler_repeats = matches!(sampler.wrap_s, TextureWrap::Repeat | TextureWrap::MirroredRepeat)
            || matches!(sampler.wrap_t, TextureWrap::Repeat | TextureWrap::MirroredRepeat);
        let (width, height) = match &loaded.source {
            LoadedImageSource::TypedArray(_) => (
                uniform.width.ok_or_else(|| RuntimeError::new("Texture uniform is missing width"))?,
                uniform.height.ok_or_else(|| RuntimeError::new("Texture uniform is missing height"))?,
            ),
            LoadedImageSource::Rgba { width, height, .. } => (*width, *height),
        };
        let is_power_of_two = width.is_power_of_two() && height.is_power_of_two();
        let requires_resize = (need_mipmap || sampler_repeats) && !is_power_of_two;
        if !requires_resize {
            return self.texture_and_mips(loaded, context, host);
        }
        match &loaded.source {
            LoadedImageSource::Rgba { width, height, pixels } => {
                let image =
                    host.image_from_rgba(RgbaImage::new(*width, *height, pixels.clone()).ok_or_else(|| {
                        RuntimeError::new("Texture uniform image size does not match its byte length")
                    })?)?;
                let resized = resize_image_to_next_power_of_two(host, &image)?;
                let (width, height) = host.dimensions(&resized);
                let pixels = host.image_pixels(&resized, width, height)?;
                let texture = self.texture_from_rgba(width, height, &pixels, sampler, context)?;
                if need_mipmap {
                    texture.generate_mipmap(MipmapHint::Nicest)?;
                }
                Ok(texture)
            }
            LoadedImageSource::TypedArray(pixels) if uniform.pixel_datatype == PixelDatatype::UnsignedByte => {
                let width = uniform.width.expect("checked above");
                let height = uniform.height.expect("checked above");
                let image = host.image_from_rgba(
                    RgbaImage::new(width, height, pixels.clone())
                        .ok_or_else(|| RuntimeError::new("Texture uniform byte length does not match its size"))?,
                )?;
                let resized = resize_image_to_next_power_of_two(host, &image)?;
                let (width, height) = host.dimensions(&resized);
                let pixels = host.image_pixels(&resized, width, height)?;
                let texture = self.texture_from_rgba(width, height, &pixels, sampler, context)?;
                if need_mipmap {
                    texture.generate_mipmap(MipmapHint::Nicest)?;
                }
                Ok(texture)
            }
            LoadedImageSource::TypedArray(_) => {
                warnings.push(TextureManagerWarning::CannotResize { id: loaded.id.clone(), need_mipmap });
                self.texture_from_typed_array(loaded, context)
            }
        }
    }

    fn texture_from_typed_array(&self, loaded: &LoadedImage<G>, context: &Context<G>) -> Result<Texture<G>> {
        let uniform = &loaded.texture_uniform;
        let pixels = match &loaded.source {
            LoadedImageSource::TypedArray(pixels) => pixels,
            LoadedImageSource::Rgba { pixels, .. } => pixels,
        };
        let width = uniform.width.ok_or_else(|| RuntimeError::new("Texture uniform is missing width"))?;
        let height = uniform.height.ok_or_else(|| RuntimeError::new("Texture uniform is missing height"))?;
        let options = TextureOptions {
            width,
            height,
            pixel_format: uniform.pixel_format,
            pixel_datatype: uniform.pixel_datatype,
            flip_y: false,
            sampler: uniform.sampler,
            id: Some(loaded.id.clone()),
            ..TextureOptions::rgba(width, height)
        };
        Texture::new(context, options, Some(pixels))
    }

    fn texture_from_rgba(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
        sampler: &Sampler,
        context: &Context<G>,
    ) -> Result<Texture<G>> {
        let options = TextureOptions {
            sampler: *sampler,
            // `image_pixels` returns top-down rows; `Texture` flips them for
            // OpenGL exactly like UNPACK_FLIP_Y_WEBGL on the upstream upload.
            flip_y: true,
            ..TextureOptions::rgba(width, height)
        };
        Texture::new(context, options, Some(pixels))
    }

    /// Destroy every uploaded texture. Textures shared with the context's
    /// default texture are left alive.
    pub fn destroy(&self) -> Result<()> {
        let default = self.default_texture.borrow().clone();
        let textures = std::mem::take(&mut self.state.borrow_mut().textures);
        for (_, texture) in textures {
            if default.as_ref().is_none_or(|d| !Rc::ptr_eq(d, &texture)) {
                texture.destroy()?;
            }
        }
        self.loaded_images.borrow_mut().clear();
        Ok(())
    }
}

/// Whether a minification filter samples mipmaps.
pub fn sampler_requires_mipmap(sampler: &Sampler) -> bool {
    matches!(
        sampler.minification_filter,
        TextureMinificationFilter::NearestMipmapNearest
            | TextureMinificationFilter::NearestMipmapLinear
            | TextureMinificationFilter::LinearMipmapNearest
            | TextureMinificationFilter::LinearMipmapLinear
    )
}
