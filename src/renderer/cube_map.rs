//! Cesium CubeMap and CubeMapFace. Faces share one context-local GPU lease.
use super::{context::*, texture_support as support, *};
use crate::core::{webgl_constants as gl, Cartesian3, PixelFormat};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CubeFace {
    PositiveX = gl::TEXTURE_CUBE_MAP_POSITIVE_X,
    NegativeX = gl::TEXTURE_CUBE_MAP_NEGATIVE_X,
    PositiveY = gl::TEXTURE_CUBE_MAP_POSITIVE_Y,
    NegativeY = gl::TEXTURE_CUBE_MAP_NEGATIVE_Y,
    PositiveZ = gl::TEXTURE_CUBE_MAP_POSITIVE_Z,
    NegativeZ = gl::TEXTURE_CUBE_MAP_NEGATIVE_Z,
}
impl CubeFace {
    pub const ALL: [Self; 6] =
        [Self::PositiveX, Self::NegativeX, Self::PositiveY, Self::NegativeY, Self::PositiveZ, Self::NegativeZ];
    pub fn index(self) -> usize {
        self as usize - gl::TEXTURE_CUBE_MAP_POSITIVE_X as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::PositiveX => "positiveX",
            Self::NegativeX => "negativeX",
            Self::PositiveY => "positiveY",
            Self::NegativeY => "negativeY",
            Self::PositiveZ => "positiveZ",
            Self::NegativeZ => "negativeZ",
        }
    }
    pub fn direction(self) -> Cartesian3 {
        match self {
            Self::PositiveX => Cartesian3::UNIT_X,
            Self::NegativeX => -Cartesian3::UNIT_X,
            Self::PositiveY => Cartesian3::UNIT_Y,
            Self::NegativeY => -Cartesian3::UNIT_Y,
            Self::PositiveZ => Cartesian3::UNIT_Z,
            Self::NegativeZ => -Cartesian3::UNIT_Z,
        }
    }
}
pub struct CubeMapFace<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    options: TextureOptions,
    face: CubeFace,
    initialized: Rc<Cell<bool>>,
}
impl<G: Gl> Clone for CubeMapFace<G> {
    fn clone(&self) -> Self {
        Self {
            resource: self.resource.clone(),
            options: self.options.clone(),
            face: self.face,
            initialized: self.initialized.clone(),
        }
    }
}
impl<G: Gl> CubeMapFace<G> {
    pub fn face(&self) -> CubeFace {
        self.face
    }
    pub fn width(&self) -> u32 {
        self.options.width
    }
    pub fn height(&self) -> u32 {
        self.options.height
    }
    pub fn pixel_format(&self) -> PixelFormat {
        self.options.pixel_format
    }
    pub fn pixel_datatype(&self) -> PixelDatatype {
        self.options.pixel_datatype
    }
    fn bind(&self) -> Result<()> {
        let c = &self.resource.context;
        c.call("activeTexture", &[gl::TEXTURE0.into()])?;
        c.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), self.resource.value()?]).map(|_| ())
    }
    fn upload(
        &self,
        source: Option<&TextureSource<G>>,
        level: u32,
        offset: Option<[u32; 2]>,
        skip: bool,
    ) -> Result<()> {
        let c = &self.resource.context;
        let size = (self.width() >> level.min(31)).max(1);
        let (width, height) = source
            .map(|s| {
                let [w, h] = s.dimensions();
                (w, h)
            })
            .unwrap_or((size, size));
        let image = matches!(source, Some(TextureSource::Image { .. }));
        support::pixel_store(c, &self.options, width, image, skip)?;
        let value = match source {
            None => GlValue::Null,
            Some(TextureSource::Pixels { bytes, .. }) => GlValue::Pixels {
                bytes: support::flip_pixels(
                    bytes,
                    self.pixel_format(),
                    self.pixel_datatype(),
                    width,
                    height,
                    self.options.flip_y,
                ),
                datatype: self.pixel_datatype(),
            },
            Some(TextureSource::Image { image, .. }) => GlValue::Object(image.clone()),
        };
        let mut args = vec![(self.face as u32).into(), level.into()];
        if let Some([x, y]) = offset {
            args.extend([x.into(), y.into()]);
        } else {
            args.push(
                self.pixel_format().to_internal_format(self.pixel_datatype(), self.resource.context.webgl2()).into(),
            );
        }
        if !image {
            args.extend([width.into(), height.into()]);
            if offset.is_none() {
                args.push(0.into());
            }
        }
        args.extend([
            (self.pixel_format() as u32).into(),
            self.pixel_datatype().to_webgl_constant(self.resource.context.webgl2()).into(),
            value,
        ]);
        c.call(if offset.is_some() { "texSubImage2D" } else { "texImage2D" }, &args).map(|_| ())
    }
    fn validate_source(&self, source: &TextureSource<G>, dimensions: [u32; 2]) -> Result<()> {
        ensure(source.dimensions() == dimensions, "Cube face source dimensions do not match")?;
        if let TextureSource::Pixels { bytes, .. } = source {
            ensure(
                bytes.len() == support::pixels_size(self.pixel_format(), self.pixel_datatype(), &dimensions)?,
                "Invalid cube face source length",
            )?;
        }
        Ok(())
    }
    pub fn load(&self, source: Option<&TextureSource<G>>, level: u32, skip_color_space_conversion: bool) -> Result<()> {
        let size = (self.width() >> level.min(31)).max(1);
        if let Some(s) = source {
            self.validate_source(s, [size, size])?;
        }
        self.bind()?;
        let r = self.upload(source, level, None, skip_color_space_conversion);
        self.resource.context.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), GlValue::Null])?;
        r?;
        if level == 0 {
            self.initialized.set(source.is_some());
        }
        Ok(())
    }
    pub fn copy_from(
        &self,
        source: &TextureSource<G>,
        x: u32,
        y: u32,
        skip_color_space_conversion: bool,
    ) -> Result<()> {
        let [width, height] = source.dimensions();
        range(x as usize, width as usize, self.width() as usize)?;
        range(y as usize, height as usize, self.height() as usize)?;
        self.validate_source(source, [width, height])?;
        self.bind()?;
        let result = (|| {
            let full = x == 0 && y == 0 && width == self.width() && height == self.height();
            if !self.initialized.get() && !full {
                let zero = TextureSource::Pixels {
                    width: self.width(),
                    height: self.height(),
                    bytes: vec![
                        0;
                        support::pixels_size(
                            self.pixel_format(),
                            self.pixel_datatype(),
                            &[self.width(), self.height()]
                        )?
                    ],
                };
                self.upload(Some(&zero), 0, None, skip_color_space_conversion)?;
            }
            self.upload(
                Some(source),
                0,
                if !self.initialized.get() && full { None } else { Some([x, y]) },
                skip_color_space_conversion,
            )
        })();
        self.resource.context.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), GlValue::Null])?;
        result?;
        self.initialized.set(true);
        Ok(())
    }
    pub fn copy_from_framebuffer(&self, copy: FramebufferCopy<'_, G>) -> Result<()> {
        ensure(
            !matches!(self.pixel_datatype(), PixelDatatype::Float | PixelDatatype::HalfFloat),
            "Cannot copy framebuffer into floating-point cube faces",
        )?;
        let r = copy.source;
        let [x, y] = copy.destination;
        range(x as usize, r.width as usize, self.width() as usize)?;
        range(y as usize, r.height as usize, self.height() as usize)?;
        let c = &self.resource.context;
        c.bind_framebuffer(copy.framebuffer)?;
        self.bind()?;
        let result = c.call(
            "copyTexSubImage2D",
            &[
                (self.face as u32).into(),
                0.into(),
                x.into(),
                y.into(),
                r.x.into(),
                r.y.into(),
                r.width.into(),
                r.height.into(),
            ],
        );
        c.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), GlValue::Null])?;
        result?;
        self.initialized.set(true);
        Ok(())
    }
    pub fn copy_mipmap_from_framebuffer(
        &self,
        source: TextureRegion,
        level: u32,
        framebuffer: Option<&Framebuffer<G>>,
    ) -> Result<()> {
        ensure(
            !matches!(self.pixel_datatype(), PixelDatatype::Float | PixelDatatype::HalfFloat),
            "Cannot copy framebuffer into floating-point cube faces",
        )?;
        let c = &self.resource.context;
        c.bind_framebuffer(framebuffer)?;
        self.bind()?;
        let result = c.call(
            "copyTexImage2D",
            &[
                (self.face as u32).into(),
                level.into(),
                self.pixel_format().to_internal_format(self.pixel_datatype(), self.resource.context.webgl2()).into(),
                source.x.into(),
                source.y.into(),
                source.width.into(),
                source.height.into(),
                0.into(),
            ],
        );
        c.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), GlValue::Null])?;
        result?;
        self.initialized.set(true);
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
}
pub struct CubeMap<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    options: TextureOptions,
    faces: [CubeMapFace<G>; 6],
    sampler: RefCell<Sampler>,
    has_mipmap: Cell<bool>,
}
impl<G: Gl> CubeMap<G> {
    pub fn create_vertex_array(context: &Context<G>) -> Result<VertexArray<G>> {
        let geometry = crate::core::BoxGeometry::from_dimensions(
            Cartesian3::new(2., 2., 2.),
            crate::core::VertexFormat::POSITION_ONLY,
            None,
        )
        .unwrap()
        .create_geometry()
        .unwrap();
        let locations = crate::core::geometry_pipeline::create_attribute_locations(&geometry).into_iter().collect();
        VertexArray::from_geometry(
            context,
            &geometry,
            GeometryUploadOptions {
                attribute_locations: locations,
                buffer_usage: BufferUsage::StaticDraw,
                ..Default::default()
            },
        )
    }
    pub fn new(context: &Context<G>, options: TextureOptions, source: Option<[TextureSource<G>; 6]>) -> Result<Self> {
        ensure(
            options.width > 0 && options.width == options.height,
            "Cube map dimensions must be positive and square",
        )?;
        ensure(
            support::color_format(options.pixel_format, options.pixel_datatype),
            "Cube maps require uncompressed color pixels",
        )?;
        support::pixels_size(options.pixel_format, options.pixel_datatype, &[options.width, options.height, 6])?;
        options.sampler.validate()?;
        support::validate_format(context, options.pixel_format, options.pixel_datatype)?;
        if let GlValue::Number(max) = context.call("getParameter", &[gl::MAX_CUBE_MAP_TEXTURE_SIZE.into()])? {
            ensure(options.width as f64 <= max, "Cube map exceeds context limits")?;
        }
        if let Some(sources) = &source {
            for s in sources {
                ensure(s.dimensions() == [options.width, options.height], "Cube faces must have equal dimensions")?;
                if let TextureSource::Pixels { bytes, .. } = s {
                    ensure(
                        bytes.len()
                            == support::pixels_size(
                                options.pixel_format,
                                options.pixel_datatype,
                                &[options.width, options.height],
                            )?,
                        "Invalid cube face length",
                    )?;
                }
            }
        }
        let resource = Rc::new(Resource::create(context, "createTexture", &[], "deleteTexture")?);
        let sampler = options.sampler;
        let faces = CubeFace::ALL.map(|face| CubeMapFace {
            resource: resource.clone(),
            options: options.clone(),
            face,
            initialized: Rc::new(Cell::new(source.is_some())),
        });
        let cube = Self { resource, options, faces, sampler: RefCell::new(sampler), has_mipmap: Cell::new(false) };
        for (i, face) in cube.faces.iter().enumerate() {
            face.load(source.as_ref().map(|s| &s[i]), 0, cube.options.skip_color_space_conversion)?;
        }
        cube.set_sampler(sampler)?;
        Ok(cube)
    }
    pub fn face(&self, face: CubeFace) -> &CubeMapFace<G> {
        &self.faces[face.index()]
    }
    pub fn bind(&self, unit: u32) -> Result<()> {
        let c = &self.resource.context;
        if let GlValue::Number(max) = c.call("getParameter", &[gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS.into()])? {
            ensure((unit as f64) < max, "Texture unit out of range")?;
        }
        c.call("activeTexture", &[(gl::TEXTURE0 + unit).into()])?;
        c.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), self.resource.value()?]).map(|_| ())
    }
    pub fn set_sampler(&self, value: Sampler) -> Result<()> {
        value.validate()?;
        self.bind(0)?;
        let c = &self.resource.context;
        let result = support::sampler(c, gl::TEXTURE_CUBE_MAP, self.pixel_format(), self.pixel_datatype(), value);
        c.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), GlValue::Null])?;
        result?;
        *self.sampler.borrow_mut() = value;
        Ok(())
    }
    pub fn sampler(&self) -> Sampler {
        *self.sampler.borrow()
    }
    pub fn width(&self) -> u32 {
        self.options.width
    }
    pub fn height(&self) -> u32 {
        self.options.height
    }
    pub fn pixel_format(&self) -> PixelFormat {
        self.options.pixel_format
    }
    pub fn pixel_datatype(&self) -> PixelDatatype {
        self.options.pixel_datatype
    }
    pub fn pre_multiply_alpha(&self) -> bool {
        self.options.pre_multiply_alpha || matches!(self.pixel_format(), PixelFormat::Rgb | PixelFormat::Luminance)
    }
    pub fn flip_y(&self) -> bool {
        self.options.flip_y
    }
    pub fn size_in_bytes(&self) -> usize {
        let n = support::pixels_size(self.pixel_format(), self.pixel_datatype(), &[self.width(), self.height(), 6])
            .unwrap();
        if self.has_mipmap.get() {
            n * 4 / 3
        } else {
            n
        }
    }
    pub fn load_mipmaps(&self, levels: &[[TextureSource<G>; 6]], skip: bool) -> Result<()> {
        ensure(
            self.width().is_power_of_two() && levels.len() == self.width().ilog2() as usize,
            "All cube mip levels must be supplied",
        )?;
        for (level, faces) in levels.iter().enumerate() {
            for (i, source) in faces.iter().enumerate() {
                self.faces[i].load(Some(source), level as u32 + 1, skip)?;
            }
        }
        self.has_mipmap.set(true);
        Ok(())
    }
    pub fn generate_mipmap(&self, hint: MipmapHint) -> Result<()> {
        ensure(
            self.resource.context.webgl2() || self.width().is_power_of_two(),
            "WebGL1 cube mipmaps require power-of-two dimensions",
        )?;
        ensure(self.width().is_power_of_two(), "Cube mipmap dimensions must be powers of two")?;
        self.bind(0)?;
        let c = &self.resource.context;
        let result = (|| {
            c.call("hint", &[gl::GENERATE_MIPMAP_HINT.into(), (hint as u32).into()])?;
            c.call("generateMipmap", &[gl::TEXTURE_CUBE_MAP.into()])?;
            Ok(())
        })();
        c.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), GlValue::Null])?;
        result?;
        self.has_mipmap.set(true);
        Ok(())
    }
    pub fn copy_face(&self, texture: Rc<Texture<G>>, face: CubeFace, level: u32) -> Result<()> {
        let c = &self.resource.context;
        let f = Framebuffer::new(
            c,
            FramebufferOptions {
                colors: vec![Attachment::Texture(texture.clone())],
                destroy_attachments: false,
                ..Default::default()
            },
        )?;
        self.face(face).copy_mipmap_from_framebuffer(
            TextureRegion { x: 0, y: 0, width: texture.width(), height: texture.height() },
            level,
            Some(&f),
        )?;
        c.bind_framebuffer(None)?;
        Ok(())
    }
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
}
/// The loader owns transport/image decoding; all six requests start together.
/// Browser loaders should request flipped ImageBitmaps as Cesium does.
pub async fn load_cube_map<G: Gl, F, Fut>(
    context: &Context<G>,
    urls: [String; 6],
    skip_color_space_conversion: bool,
    load: F,
) -> Result<CubeMap<G>>
where
    F: Fn(String, bool) -> Fut,
    Fut: Future<Output = Result<TextureSource<G>>>,
{
    let images =
        futures_util::future::try_join_all(urls.into_iter().map(|u| load(u, skip_color_space_conversion))).await?;
    let [width, height] = images[0].dimensions();
    let mut options = TextureOptions::rgba(width, height);
    options.skip_color_space_conversion = skip_color_space_conversion;
    let images: [TextureSource<G>; 6] =
        images.try_into().map_err(|_| crate::core::RuntimeError::new("Cube map requires six images"))?;
    CubeMap::new(context, options, Some(images))
}

pub async fn load_cube_map_resources<G: Gl, H: crate::core::ImageHost<Image = G::Object> + 'static>(
    context: &Context<G>,
    resources: [crate::core::Resource; 6],
    skip: bool,
    client: &crate::core::ResourceClient,
    host: Rc<H>,
) -> Result<CubeMap<G>> {
    let mut futures = vec![];
    for resource in resources {
        let request = client
            .fetch_image(
                &resource,
                host.clone(),
                crate::core::FetchImageOptions {
                    flip_y: true,
                    prefer_image_bitmap: true,
                    skip_color_space_conversion: skip,
                    ..Default::default()
                },
            )
            .ok_or_else(|| crate::core::RuntimeError::new("Cube image request was deferred"))?;
        let host = host.clone();
        futures.push(async move {
            let image = request
                .await
                .map_err(|e| crate::core::RuntimeError::new(e.to_string()))?
                .ok_or_else(|| crate::core::RuntimeError::new("Cube image request returned no image"))?;
            let (width, height) = host.dimensions(&image);
            Ok(TextureSource::Image { width, height, image })
        });
    }
    let images = futures_util::future::try_join_all(futures).await?;
    let [width, height] = images[0].dimensions();
    let mut options = TextureOptions::rgba(width, height);
    options.skip_color_space_conversion = skip;
    CubeMap::new(
        context,
        options,
        Some(images.try_into().map_err(|_| crate::core::RuntimeError::new("Cube map requires six images"))?),
    )
}
