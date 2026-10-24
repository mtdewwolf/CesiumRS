//! Cached glTF image decoding through the existing image host and KTX2 codec.
use super::{
    gltf_pipeline::BufferSource,
    resource_loader::{guard_request, ManagedLoader},
    *,
};
use crate::core::{
    ktx2_transcoder::{Ktx2Texture, Ktx2Transcoder, RustBasisTranscoder, SupportedTextureFormats},
    CompressedTextureBuffer, FetchImageOptions, ImageDecodeOptions, ImageHost, Resource, ResourceClient, RuntimeError,
};
use futures_util::FutureExt;
use std::{cell::RefCell, rc::Rc, sync::Arc};
type Result<T> = std::result::Result<T, RuntimeError>;

pub enum GltfImage<I> {
    Raster(I),
    Ktx2(CompressedTextureBuffer),
}
pub struct LoadedGltfImage<I> {
    pub image: GltfImage<I>,
    /// None differs from an explicitly supplied empty mip chain upstream.
    pub mip_levels: Option<Vec<Arc<[u8]>>>,
}
impl<I> LoadedGltfImage<I> {
    fn ktx2(texture: Ktx2Texture) -> Result<Self> {
        let (image, mip_levels) = match texture {
            Ktx2Texture::Image(image) => (image, None),
            Ktx2Texture::Mipmaps(mut levels) => {
                if levels.is_empty() {
                    return Err(RuntimeError::new("KTX2 image has no levels"));
                }
                let image = levels.remove(0);
                (image, Some(levels.into_iter().map(|l| l.shared_buffer().clone()).collect()))
            }
            Ktx2Texture::Cubemap(_) => {
                return Err(RuntimeError::new("A glTF image must be a two-dimensional KTX2 texture"))
            }
        };
        Ok(Self { image: GltfImage::Ktx2(image), mip_levels })
    }
}
/// Pinned embedded-image signature detection; glTF's mimeType is not consulted.
pub fn gltf_image_mime_type(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        Ok("image/jpeg")
    } else if bytes.starts_with(&[0x89, 0x50]) {
        Ok("image/png")
    } else if bytes.starts_with(&[0xab, 0x4b]) {
        Ok("image/ktx2")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Ok("image/webp")
    } else {
        Err(RuntimeError::new("Image format is not recognized"))
    }
}
enum ImageSource {
    Embedded,
    Uri(Resource),
}
pub struct GltfImageLoader<H: ImageHost> {
    data: ManagedLoader<Rc<LoadedGltfImage<H::Image>>>,
    child: Rc<RefCell<Option<CacheLease<GltfBufferViewLoader>>>>,
    source: RefCell<Option<ImageSource>>,
    host: Rc<H>,
    client: ResourceClient,
    formats: SupportedTextureFormats,
    error_message: RefCell<Option<String>>,
}
impl<H: ImageHost + 'static> GltfImageLoader<H>
where
    H::Image: 'static,
{
    fn new(
        request: &GltfResource,
        image_id: usize,
        child: Option<CacheLease<GltfBufferViewLoader>>,
        host: Rc<H>,
        client: ResourceClient,
        formats: SupportedTextureFormats,
        key: String,
    ) -> Result<Self> {
        let image =
            request.document.json["images"].get(image_id).ok_or_else(|| RuntimeError::new("Invalid glTF image ID"))?;
        let (source, error_message) = if child.is_some() {
            (ImageSource::Embedded, "Failed to load embedded image".into())
        } else {
            let uri = image["uri"].as_str().ok_or_else(|| RuntimeError::new("Invalid glTF image URI"))?;
            let resource = request
                .base
                .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                    url: Some(uri.into()),
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("Invalid glTF image URI"))?;
            (ImageSource::Uri(resource), format!("Failed to load image: {uri}"))
        };
        Ok(Self {
            data: ManagedLoader::new(Some(key), None),
            child: Rc::new(RefCell::new(child)),
            source: RefCell::new(Some(source)),
            host,
            client,
            formats,
            error_message: RefCell::new(Some(error_message)),
        })
    }
    pub fn image(&self) -> Option<Rc<LoadedGltfImage<H::Image>>> {
        self.data.value()
    }
}
async fn embedded<H: ImageHost>(
    host: &H,
    bytes: BufferSource,
    formats: SupportedTextureFormats,
) -> Result<LoadedGltfImage<H::Image>> {
    let mime = gltf_image_mime_type(bytes.bytes())?;
    if mime == "image/ktx2" {
        let codec = Ktx2Transcoder { supported_formats: formats, basis: RustBasisTranscoder };
        return LoadedGltfImage::ktx2(codec.transcode(bytes.bytes()).await?);
    }
    let image = host
        .decode_image(bytes.bytes(), mime, ImageDecodeOptions { flip_y: false, skip_color_space_conversion: true })
        .ok_or_else(|| RuntimeError::new("Image decode was deferred."))?
        .await?;
    Ok(LoadedGltfImage { image: GltfImage::Raster(image), mip_levels: None })
}
impl<H: ImageHost + 'static> ResourceLoader for GltfImageLoader<H>
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
        self.source.borrow_mut().take();
        self.error_message.borrow_mut().take();
        self.child.borrow_mut().take();
        self.data.unload();
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
    fn load(&self) -> ResourceLoadFuture {
        let child = self.child.borrow().as_ref().map(CacheLease::loader);
        let uri = match self.source.borrow_mut().take() {
            Some(ImageSource::Uri(r)) => Some(r),
            _ => None,
        };
        let (host, client, formats) = (self.host.clone(), self.client.clone(), self.formats);
        let owner = Rc::downgrade(&self.child);
        self.data.load(self.error_message.borrow_mut().take().unwrap_or_default(), move || {
            // Start the request before the first poll, as the JS async method does.
            let future = if let Some(uri) = uri {
                let path = uri.get_url_component(false, true).to_lowercase();
                if path.starts_with("data:image/ktx2") || path.ends_with(".ktx2") {
                    let request = client.fetch_array_buffer(&uri).map(|f| guard_request(&uri, f));
                    async move {
                        let bytes = request
                            .ok_or_else(|| RuntimeError::new("KTX2 request was throttled"))?
                            .await
                            .map_err(|e| RuntimeError::new(e.to_string()))?;
                        let codec = Ktx2Transcoder { supported_formats: formats, basis: RustBasisTranscoder };
                        LoadedGltfImage::ktx2(codec.transcode(&bytes).await?)
                    }
                    .boxed_local()
                } else {
                    let request = client
                        .fetch_image(
                            &uri,
                            host,
                            FetchImageOptions {
                                prefer_image_bitmap: true,
                                skip_color_space_conversion: true,
                                ..Default::default()
                            },
                        )
                        .map(|f| guard_request(&uri, f));
                    async move {
                        let image = request
                            .ok_or_else(|| RuntimeError::new("Image request was throttled"))?
                            .await
                            .map_err(|e| RuntimeError::new(e.to_string()))?
                            .ok_or_else(|| RuntimeError::new("Image decode was deferred."))?;
                        Ok(LoadedGltfImage { image: GltfImage::Raster(image), mip_levels: None })
                    }
                    .boxed_local()
                }
            } else {
                let request = child.as_ref().map(|l| l.load());
                let weak = child.as_ref().map(Rc::downgrade);
                async move {
                    request.ok_or_else(|| RuntimeError::new("Image buffer view was unloaded"))?.await?;
                    let bytes = weak
                        .and_then(|w| w.upgrade())
                        .and_then(|l| l.typed_array())
                        .ok_or_else(|| RuntimeError::new("Image buffer view was unloaded"))?;
                    embedded(&*host, bytes, formats).await
                }
                .boxed_local()
            };
            async move {
                let result = future.await;
                if let Some(owner) = owner.upgrade() {
                    owner.borrow_mut().take();
                }
                result.map(Rc::new)
            }
            .boxed_local()
        })
    }
}
impl ResourceCache {
    pub fn image_loader<H: ImageHost + 'static>(
        &self,
        request: &GltfResource,
        image_id: usize,
        host: Rc<H>,
        formats: SupportedTextureFormats,
    ) -> Result<CacheLease<GltfImageLoader<H>>>
    where
        H::Image: 'static,
    {
        let key = ResourceCacheKey::image(request.key_context(), image_id)?;
        self.get_or_insert(&key, CacheResourceKind::Image, || {
            let image = request.document.json["images"]
                .get(image_id)
                .ok_or_else(|| RuntimeError::new("Invalid glTF image ID"))?;
            let child = image["bufferView"]
                .as_u64()
                .map(|id| {
                    self.buffer_view_loader(request.document.clone(), id as usize, &request.resource, &request.base)
                })
                .transpose()?;
            GltfImageLoader::new(request, image_id, child, host, self.client(), formats, key.clone())
        })
    }
}
