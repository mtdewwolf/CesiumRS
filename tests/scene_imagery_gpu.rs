//! Actual renderer commands, upload bytes, sampler decisions and queue ownership.
use cesium_rs::{
    core::{Rectangle, RuntimeError},
    renderer::*,
    scene::*,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Calls = Rc<RefCell<Vec<(String, Vec<GlValue<u32>>)>>>;

#[derive(Clone, Default)]
struct Host {
    next: Rc<Cell<u32>>,
    calls: Calls,
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.calls.borrow_mut().push((name.into(), args.to_vec()));
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        Ok(match name {
            "getParameter" => GlValue::Number(16384.),
            "getExtension" => GlValue::Object(0),
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            _ => GlValue::Null,
        })
    }
}
fn settings() -> ImageryTextureSettings {
    ImageryTextureSettings {
        minification_filter: TextureMinificationFilter::Linear,
        magnification_filter: TextureMagnificationFilter::Linear,
        maximum_anisotropy: Some(4.),
    }
}
fn tile(pipeline: &ImageryTexturePipeline<Host>, width: u32, mercator: bool) -> Rc<RefCell<Imagery>> {
    let image = ImageryImage::Image(FrameResource::new(ImageryPixels {
        width,
        height: 4,
        bytes: vec![127; (width * 4 * 4) as usize],
    }));
    let texture = pipeline.upload(&image, true, settings()).unwrap();
    let mut i = Imagery::new(0, 0, 0, Rectangle::new(-1., 0.2, 1., 1.2), None);
    i.add_reference();
    i.state = ImageryState::TextureLoaded;
    if mercator {
        i.texture_web_mercator = Some(texture);
    } else {
        i.texture = Some(texture);
    }
    Rc::new(RefCell::new(i))
}
#[test]
fn upload_and_sampler_rules() {
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let pipeline = ImageryTexturePipeline::new(&context);
    for (width, nearest, mipmaps) in [(4, false, true), (3, false, false), (4, true, false)] {
        let tile = tile(&pipeline, width, false);
        let mut filters = settings();
        if nearest {
            filters.minification_filter = TextureMinificationFilter::Nearest;
        }
        pipeline.reproject(&tile, true, filters).unwrap();
        assert_eq!(tile.borrow().state, ImageryState::Ready);
        let t = tile.borrow().texture.as_ref().unwrap().get::<Rc<Texture<Host>>>().unwrap().clone();
        assert_eq!(
            t.sampler().minification_filter,
            if mipmaps { TextureMinificationFilter::LinearMipmapLinear } else { filters.minification_filter }
        );
        assert_eq!(t.sampler().maximum_anisotropy, if mipmaps { 4. } else { 1. });
    }
    assert!(host.calls.borrow().iter().any(|(name, _)| name == "texImage2D"));
    assert_eq!(host.calls.borrow().iter().filter(|(name, _)| name == "generateMipmap").count(), 1);
}
#[test]
fn reprojection_queue_is_drained_once_and_drop_or_cancel_releases_lease() {
    let context = Context::new_webgl2(Host::default());
    let pipeline = ImageryTexturePipeline::new(&context);
    let tile = tile(&pipeline, 4, true);
    pipeline.reproject(&tile, true, settings()).unwrap();
    assert_eq!(tile.borrow().state, ImageryState::Transitioning);
    assert_eq!(tile.borrow().reference_count, 2);
    let mut commands = Vec::new();
    pipeline.queue_reprojection_commands(&mut commands);
    pipeline.queue_reprojection_commands(&mut commands);
    assert_eq!(commands.len(), 1);
    commands[0].cancel().unwrap();
    commands[0].cancel().unwrap();
    assert_eq!(tile.borrow().reference_count, 1);
    assert_eq!(tile.borrow().state, ImageryState::TextureLoaded);
    drop(commands);
    pipeline.reproject(&tile, true, settings()).unwrap();
    pipeline.cancel_reprojections();
    assert_eq!(pipeline.pending_reprojections(), 0);
    assert_eq!(tile.borrow().reference_count, 1);
    pipeline.reproject(&tile, true, settings()).unwrap();
    let mut commands = Vec::new();
    pipeline.queue_reprojection_commands(&mut commands);
    drop(commands);
    assert_eq!(tile.borrow().reference_count, 1);
    assert_eq!(tile.borrow().state, ImageryState::TextureLoaded);
}
#[test]
fn direct_mercator_and_small_spacing_skip_compute() {
    let context = Context::new_webgl2(Host::default());
    let pipeline = ImageryTexturePipeline::new(&context);
    let tile = tile(&pipeline, 4, true);
    pipeline.reproject(&tile, false, settings()).unwrap();
    assert_eq!(tile.borrow().state, ImageryState::Ready);
    assert!(tile.borrow().texture.is_none());
    tile.borrow_mut().rectangle = Rectangle::new(0., 0.5, 1e-6, 0.50001);
    pipeline.reproject(&tile, true, settings()).unwrap();
    assert!(tile.borrow().texture.is_some());
    assert_eq!(pipeline.pending_reprojections(), 0);
}

struct Provider {
    scheme: cesium_rs::core::WebMercatorTilingScheme,
    errors: ImageryErrorEvent,
}
impl ImageryProvider for Provider {
    fn rectangle(&self) -> &Rectangle {
        use cesium_rs::core::TilingScheme;
        self.scheme.rectangle()
    }
    fn tile_width(&self) -> f64 {
        4.
    }
    fn tile_height(&self) -> f64 {
        4.
    }
    fn maximum_level(&self) -> Option<u32> {
        None
    }
    fn tiling_scheme(&self) -> &dyn cesium_rs::core::TilingScheme {
        &self.scheme
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.errors
    }
    fn request_image(&self, _: i32, _: i32, _: u32) -> Option<ImageryFuture> {
        Some(Box::pin(async {
            Ok(ImageryImage::Image(FrameResource::new(ImageryPixels { width: 4, height: 4, bytes: vec![255; 64] })))
        }))
    }
}
#[test]
#[allow(clippy::arc_with_non_send_sync)] // ImageryProvider is a local trait behind the existing Arc API.
fn provider_request_upload_and_collection_queue_cancellation() {
    use cesium_rs::core::MutableErrorEvent;
    let context = Context::new_webgl2(Host::default());
    let pipeline = Rc::new(ImageryTexturePipeline::new(&context));
    let provider = Provider { scheme: Default::default(), errors: Rc::new(RefCell::new(MutableErrorEvent::new())) };
    let layer = ImageryLayer::handle(Some(std::sync::Arc::new(provider)), Default::default());
    layer.borrow_mut().set_texture_host(pipeline.clone());
    let imagery = layer.borrow_mut().get_imagery_from_cache(0, 0, 0, None);
    layer.borrow_mut().process_imagery_state_machine(&imagery, None, true, false);
    assert!(imagery.borrow().image.is_none());
    assert!(imagery.borrow().texture_web_mercator.is_some());
    assert_eq!(imagery.borrow().state, ImageryState::Transitioning);
    let mut layers = ImageryLayerCollection::new();
    layers.add(layer.clone(), None).unwrap();
    let mut commands: Vec<ComputeCommand<Host>> = Vec::new();
    layers.queue_reprojection_commands(&mut commands).unwrap();
    assert_eq!(commands.len(), 1);
    drop(commands);
    assert_eq!(imagery.borrow().state, ImageryState::TextureLoaded);
    layer.borrow_mut().process_imagery_state_machine(&imagery, None, true, false);
    layers.cancel_reprojections();
    assert_eq!(imagery.borrow().reference_count, 1);
    assert_eq!(imagery.borrow().state, ImageryState::TextureLoaded);
    layer.borrow_mut().process_imagery_state_machine(&imagery, None, true, false);
    layer.borrow_mut().destroy().unwrap();
    assert_eq!(imagery.borrow().reference_count, 1);
}

#[test]
fn compressed_upload_preserves_bytes_and_does_not_generate_mipmaps() {
    use cesium_rs::core::compressed_texture_buffer::CompressedTextureBufferOptions;
    use cesium_rs::core::{CompressedTextureBuffer, PixelFormat};
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let pipeline = ImageryTexturePipeline::new(&context);
    let bytes = vec![7u8; 8];
    let image = ImageryImage::Compressed(CompressedTextureBuffer::new(CompressedTextureBufferOptions {
        internal_format: PixelFormat::RgbDxt1 as u32,
        pixel_datatype: None,
        width: 4,
        height: 4,
        buffer: bytes.clone().into(),
    }));
    let texture = pipeline.upload(&image, false, settings()).unwrap();
    let mut i = Imagery::new(0, 0, 0, Rectangle::MAX_VALUE, None);
    i.texture = Some(texture);
    i.add_reference();
    let i = Rc::new(RefCell::new(i));
    pipeline.reproject(&i, true, settings()).unwrap();
    let calls = host.calls.borrow();
    let (_, args) = calls.iter().find(|(name, _)| name == "compressedTexImage2D").unwrap();
    assert!(args.iter().any(|arg| matches!(arg,GlValue::Bytes(upload) if upload == &bytes)));
    assert!(!calls.iter().any(|(name, _)| name == "generateMipmap"));
}

#[test]
fn decoded_rgba_rows_flip_and_opaque_provider_drops_alpha() {
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let pipeline = ImageryTexturePipeline::new(&context);
    let image = ImageryImage::Image(FrameResource::new(ImageryPixels {
        width: 1,
        height: 2,
        bytes: vec![1, 2, 3, 4, 5, 6, 7, 8],
    }));
    pipeline.upload(&image, false, settings()).unwrap();
    let calls = host.calls.borrow();
    let (_, args) = calls.iter().find(|(name, _)| name == "texImage2D").unwrap();
    assert!(args.iter().any(|arg| matches!(arg,GlValue::Pixels{bytes,..} if bytes == &[5,6,7,1,2,3])));
}

#[test]
fn compressed_mercator_reprojection_uses_rgba8_output() {
    use cesium_rs::core::compressed_texture_buffer::CompressedTextureBufferOptions;
    use cesium_rs::core::{CompressedTextureBuffer, PixelDatatype, PixelFormat};
    let context = Context::new_webgl2(Host::default());
    let pipeline = ImageryTexturePipeline::new(&context);
    let image = ImageryImage::Compressed(CompressedTextureBuffer::new(CompressedTextureBufferOptions {
        internal_format: PixelFormat::RgbaDxt1 as u32,
        pixel_datatype: None,
        width: 4,
        height: 4,
        buffer: vec![0, 248, 0, 0, 0, 0, 0, 0].into(),
    }));
    let source = pipeline.upload(&image, true, settings()).unwrap();
    let mut imagery = Imagery::new(0, 0, 0, Rectangle::new(-1., 0.2, 1., 1.2), None);
    imagery.texture_web_mercator = Some(source);
    imagery.add_reference();
    let imagery = Rc::new(RefCell::new(imagery));
    pipeline.reproject(&imagery, true, settings()).unwrap();
    let mut commands = Vec::new();
    pipeline.queue_reprojection_commands(&mut commands);
    assert_eq!(commands.len(), 1);
    let output = commands[0].output_texture.as_ref().unwrap();
    assert_eq!(output.pixel_format(), PixelFormat::Rgba);
    assert_eq!(output.pixel_datatype(), PixelDatatype::UnsignedByte);
    assert_eq!(imagery.borrow().state, ImageryState::Transitioning);
    drop(commands);
    assert_eq!(imagery.borrow().reference_count, 1);
    assert_eq!(imagery.borrow().state, ImageryState::TextureLoaded);
}
