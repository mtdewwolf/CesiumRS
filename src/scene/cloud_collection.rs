//! CesiumJS `CloudCollection`: cumulus cloud collection, vertex attribute
//! packing, 3D Worley noise compute pass, `VertexArrayFacade` GPU upload
//! (instanced and batched), and translucent draw command emission.

use super::blending_state;
use super::cumulus_cloud::*;
use super::frame_state::FrameState;
use super::scene::{Result as SceneResult, SceneCommand, SceneDrawOptions, ScenePrimitive};
use super::Cesium3DTilePass;
use crate::core::{
    BoundingSphere, Cartesian3, Color, ComponentDatatype, EncodedCartesian3, IndexDatatype, Matrix4, PixelFormat,
    PrimitiveType, RuntimeError,
};
use crate::renderer::render_state::DepthTest;
use crate::renderer::{
    Attachment, Buffer, BufferData, BufferKind, BufferUsage, Context, DrawMetadata, FacadeAttribute, Framebuffer,
    FramebufferOptions, Gl, Pass, PixelDatatype, RenderState, SampledTexture, Sampler, ShaderProgram,
    ShaderProgramOptions, ShaderSource, Texture, TextureMagnificationFilter, TextureMinificationFilter,
    TextureOptions, TextureWrap, UniformValue, VertexArray, VertexArrayFacade, VertexAttribute, Viewport,
};
use crate::shaders::root::{CLOUD_COLLECTION_FS, CLOUD_COLLECTION_VS, CLOUD_NOISE_FS, CLOUD_NOISE_VS};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq)]
struct CloudShaderKey {
    instanced: bool,
    debug_billboards: bool,
    debug_ellipsoids: bool,
}

struct CloudCollectionGpu<G: Gl> {
    context: Context<G>,
    noise_texture: Option<Rc<Texture<G>>>,
    noise_framebuffer: Option<Rc<Framebuffer<G>>>,
    noise_quad_va: Option<Rc<VertexArray<G>>>,
    noise_shader: Option<Rc<ShaderProgram<G>>>,
    noise_dirty: bool,
    last_noise_detail: f64,
    last_noise_offset: Cartesian3,

    facade: Option<VertexArrayFacade<G>>,
    index_buffer: Option<Buffer<G>>,
    index_buffer_capacity: usize,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    shader_key: Option<CloudShaderKey>,
    use_instancing: bool,
    last_cloud_count: usize,
}

impl<G: Gl + 'static> CloudCollectionGpu<G> {
    fn new(context: Context<G>) -> SceneResult<Self> {
        let use_instancing = context.capabilities()?.instanced_arrays;
        Ok(Self {
            context,
            noise_texture: None,
            noise_framebuffer: None,
            noise_quad_va: None,
            noise_shader: None,
            noise_dirty: true,
            last_noise_detail: f64::NAN,
            last_noise_offset: Cartesian3::new(f64::NAN, f64::NAN, f64::NAN),
            facade: None,
            index_buffer: None,
            index_buffer_capacity: 0,
            shader_program: None,
            shader_key: None,
            use_instancing,
            last_cloud_count: usize::MAX,
        })
    }

    fn ensure_noise_resources(&mut self) -> SceneResult<()> {
        if self.noise_texture.is_none() {
            let mut opts = TextureOptions::rgba(NOISE_TEXTURE_WIDTH as u32, NOISE_TEXTURE_HEIGHT as u32);
            opts.pixel_format = PixelFormat::Rgba;
            opts.pixel_datatype = PixelDatatype::UnsignedByte;
            opts.sampler = Sampler {
                wrap_s: TextureWrap::Repeat,
                wrap_t: TextureWrap::Repeat,
                minification_filter: TextureMinificationFilter::Linear,
                magnification_filter: TextureMagnificationFilter::Linear,
                ..Default::default()
            };
            let tex = Rc::new(Texture::new(&self.context, opts, None)?);
            let fb = Rc::new(Framebuffer::new(
                &self.context,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(tex.clone())],
                    ..Default::default()
                },
            )?);
            self.noise_texture = Some(tex);
            self.noise_framebuffer = Some(fb);
        }
        if self.noise_quad_va.is_none() {
            let verts: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0];
            let vbytes: Vec<u8> = verts.iter().flat_map(|f| f.to_le_bytes()).collect();
            let vbuf = Buffer::new(
                &self.context,
                BufferKind::Vertex,
                BufferUsage::StaticDraw,
                BufferData::Bytes(&vbytes),
            )?;
            let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
            let ibytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let ibuf = Buffer::new(
                &self.context,
                BufferKind::Index(IndexDatatype::UnsignedShort),
                BufferUsage::StaticDraw,
                BufferData::Bytes(&ibytes),
            )?;
            let va = Rc::new(VertexArray::new(
                &self.context,
                vec![VertexAttribute::buffer(0, vbuf, 2)],
                Some(ibuf),
            )?);
            self.noise_quad_va = Some(va);
        }
        if self.noise_shader.is_none() {
            let prog = ShaderProgram::from_sources(
                &self.context,
                &ShaderProgramOptions {
                    vertex_shader_source: ShaderSource::new(CLOUD_NOISE_VS.source),
                    fragment_shader_source: ShaderSource::new(CLOUD_NOISE_FS.source),
                    attribute_locations: BTreeMap::from([("position".to_string(), 0)]),
                },
            )?;
            self.noise_shader = Some(Rc::new(prog));
        }
        Ok(())
    }

    fn ensure_index_buffer(&mut self, quad_count: usize) -> SceneResult<Buffer<G>> {
        let needed_quads = if self.use_instancing { 1 } else { quad_count.max(1) };
        if self.index_buffer.is_none() || self.index_buffer_capacity < needed_quads {
            let mut indices = Vec::with_capacity(needed_quads * 6);
            for q in 0..needed_quads {
                let base = (q * 4) as u16;
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let buf = Buffer::new(
                &self.context,
                BufferKind::Index(IndexDatatype::UnsignedShort),
                BufferUsage::StaticDraw,
                BufferData::Bytes(&bytes),
            )?;
            self.index_buffer = Some(buf);
            self.index_buffer_capacity = needed_quads;
        }
        Ok(self.index_buffer.as_ref().expect("initialized above").clone())
    }

    #[allow(clippy::too_many_arguments)]
    fn update_and_emit(
        &mut self,
        clouds: &[Option<CumulusCloudHandle>],
        noise_detail: f64,
        noise_offset: Cartesian3,
        noise_dims: Cartesian3,
        debug_billboards: bool,
        debug_ellipsoids: bool,
        model_matrix: Matrix4,
        bounding_sphere_wc: BoundingSphere,
        frame: &mut FrameState,
    ) -> SceneResult<()> {
        let active: Vec<&CumulusCloudHandle> = clouds.iter().flatten().collect();
        if active.is_empty() {
            return Ok(());
        }

        self.ensure_noise_resources()?;
        if self.noise_dirty || self.last_noise_detail != noise_detail || self.last_noise_offset != noise_offset {
            self.last_noise_detail = noise_detail;
            self.last_noise_offset = noise_offset;
            self.noise_dirty = false;

            let va = self.noise_quad_va.as_ref().expect("noise va").clone();
            let prog = self.noise_shader.as_ref().expect("noise shader").clone();
            let fb = self.noise_framebuffer.as_ref().expect("noise fb").clone();
            let mut draw_opts = SceneDrawOptions::new(va, prog);
            draw_opts.primitive_type = PrimitiveType::Triangles;
            draw_opts.count = Some(6);
            draw_opts.framebuffer = Some(fb);
            draw_opts.state = RenderState {
                viewport: Some(Viewport {
                    x: 0,
                    y: 0,
                    width: NOISE_TEXTURE_WIDTH as u32,
                    height: NOISE_TEXTURE_HEIGHT as u32,
                }),
                depth_test: DepthTest { enabled: false, ..Default::default() },
                depth_mask: false,
                ..Default::default()
            };
            draw_opts.uniforms = vec![
                (
                    "u_noiseTextureDimensions".into(),
                    UniformValue::Float(vec![noise_dims.x as f32, noise_dims.y as f32, noise_dims.z as f32]),
                ),
                ("u_noiseDetail".into(), UniformValue::Float(vec![noise_detail as f32])),
                (
                    "u_noiseOffset".into(),
                    UniformValue::Float(vec![noise_offset.x as f32, noise_offset.y as f32, noise_offset.z as f32]),
                ),
            ];
            let meta = DrawMetadata {
                cull: false,
                owner: Some("CloudCollectionNoise".into()),
                ..Default::default()
            };
            frame.command_list.push(SceneCommand::from_draw(
                "CloudCollectionNoise",
                self.context.clone(),
                Pass::Compute,
                meta,
                draw_opts,
            ));
        }

        let any_dirty = self.facade.is_none()
            || self.last_cloud_count != active.len()
            || active.iter().any(|h| h.borrow().is_dirty());

        if any_dirty {
            let count = active.len();
            let vertex_count = if self.use_instancing { count } else { count * 4 };
            let needs_new_facade = self.facade.is_none();
            if needs_new_facade {
                let mut attrs = Vec::new();
                let base_loc = if self.use_instancing {
                    let dir_data: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
                    let dir_bytes: Vec<u8> = dir_data.iter().flat_map(|f| f.to_le_bytes()).collect();
                    let dir_buf = Buffer::new(
                        &self.context,
                        BufferKind::Vertex,
                        BufferUsage::StaticDraw,
                        BufferData::Bytes(&dir_bytes),
                    )?;
                    let mut dir_attr = FacadeAttribute::new(0, 2);
                    dir_attr.vertex_buffer = Some(dir_buf);
                    attrs.push(dir_attr);
                    1
                } else {
                    0
                };
                attrs.push(FacadeAttribute::new(base_loc, 4));
                attrs.push(FacadeAttribute::new(base_loc + 1, 4));
                attrs.push(FacadeAttribute::new(base_loc + 2, 4));
                attrs.push(FacadeAttribute::new(base_loc + 3, 4));
                let mut color_attr = FacadeAttribute::new(base_loc + 4, 4);
                color_attr.datatype = ComponentDatatype::UnsignedByte;
                color_attr.normalize = true;
                attrs.push(color_attr);
                self.facade = Some(VertexArrayFacade::new(
                    &self.context,
                    attrs,
                    vertex_count,
                    self.use_instancing,
                )?);
            } else if let Some(facade) = self.facade.as_mut() {
                if facade.size() != vertex_count {
                    facade.resize(vertex_count)?;
                }
            }

            let base_loc = if self.use_instancing { 1 } else { 0 };
            let facade = self.facade.as_mut().expect("facade initialized");
            for (idx, handle) in active.iter().enumerate() {
                let mut cloud = handle.borrow_mut();
                if self.use_instancing {
                    let attr = CloudVertexAttributes::instanced_from_cloud(&cloud);
                    facade.write(base_loc, idx, &attr.position_high_and_scale_x)?;
                    facade.write(base_loc + 1, idx, &attr.position_low_and_scale_y)?;
                    facade.write(base_loc + 2, idx, &attr.packed_attribute0)?;
                    facade.write(base_loc + 3, idx, &attr.packed_attribute1)?;
                    facade.write(
                        base_loc + 4,
                        idx,
                        &[
                            f64::from(attr.color[0]),
                            f64::from(attr.color[1]),
                            f64::from(attr.color[2]),
                            f64::from(attr.color[3]),
                        ],
                    )?;
                } else {
                    let quad = CloudVertexAttributes::batched_quad_from_cloud(&cloud);
                    for (corner, attr) in quad.iter().enumerate() {
                        let v = idx * 4 + corner;
                        facade.write(base_loc, v, &attr.position_high_and_scale_x)?;
                        facade.write(base_loc + 1, v, &attr.position_low_and_scale_y)?;
                        facade.write(base_loc + 2, v, &attr.packed_attribute0)?;
                        facade.write(base_loc + 3, v, &attr.packed_attribute1)?;
                        facade.write(
                            base_loc + 4,
                            v,
                            &[
                                f64::from(attr.color[0]),
                                f64::from(attr.color[1]),
                                f64::from(attr.color[2]),
                                f64::from(attr.color[3]),
                            ],
                        )?;
                    }
                }
                cloud.clear_dirty();
            }

            let index_buf = self.ensure_index_buffer(count)?;
            self.facade.as_mut().expect("facade initialized").commit(Some(index_buf))?;
            self.last_cloud_count = count;
        }

        let key = CloudShaderKey {
            instanced: self.use_instancing,
            debug_billboards,
            debug_ellipsoids,
        };
        if self.shader_program.is_none() || self.shader_key != Some(key) {
            if let Some(old) = self.shader_program.take() {
                let _ = old.destroy();
            }
            let mut vs = ShaderSource::new(CLOUD_COLLECTION_VS.source);
            let mut fs = ShaderSource::new(CLOUD_COLLECTION_FS.source);
            let mut locations = Vec::new();
            let base_loc = if self.use_instancing {
                vs.defines.push("INSTANCED".into());
                locations.push(("direction".to_string(), 0));
                1
            } else {
                0
            };
            locations.push(("positionHighAndScaleX".to_string(), base_loc));
            locations.push(("positionLowAndScaleY".to_string(), base_loc + 1));
            locations.push(("packedAttribute0".to_string(), base_loc + 2));
            locations.push(("packedAttribute1".to_string(), base_loc + 3));
            locations.push(("color".to_string(), base_loc + 4));

            if debug_billboards {
                fs.defines.push("DEBUG_BILLBOARDS".into());
            }
            if debug_ellipsoids {
                fs.defines.push("DEBUG_ELLIPSOIDS".into());
            }
            let prog = ShaderProgram::from_sources(
                &self.context,
                &ShaderProgramOptions {
                    vertex_shader_source: vs,
                    fragment_shader_source: fs,
                    attribute_locations: locations.into_iter().collect(),
                },
            )?;
            self.shader_program = Some(Rc::new(prog));
            self.shader_key = Some(key);
        }

        if !frame.passes.render {
            return Ok(());
        }

        let program = self.shader_program.as_ref().expect("compiled above").clone();
        let noise_tex = self.noise_texture.as_ref().expect("noise texture").clone() as Rc<dyn SampledTexture<G>>;
        let arrays = self
            .facade
            .as_ref()
            .and_then(|f| f.vertex_arrays())
            .unwrap_or(&[]);

        for (va_idx, entry) in arrays.iter().enumerate() {
            let mut draw_opts = SceneDrawOptions::new(entry.vertex_array.clone(), program.clone());
            draw_opts.primitive_type = PrimitiveType::Triangles;
            if self.use_instancing {
                draw_opts.count = Some(6);
                draw_opts.instance_count = active.len() as u32;
            } else {
                draw_opts.count = Some(entry.indices_count as usize);
            }
            draw_opts.state = RenderState {
                depth_test: DepthTest { enabled: true, ..Default::default() },
                depth_mask: false,
                blending: blending_state::alpha_blend(),
                ..Default::default()
            };
            draw_opts.uniforms = vec![
                (
                    "u_noiseTextureDimensions".into(),
                    UniformValue::Float(vec![noise_dims.x as f32, noise_dims.y as f32, noise_dims.z as f32]),
                ),
                ("u_noiseDetail".into(), UniformValue::Float(vec![noise_detail as f32])),
            ];
            draw_opts.textures.push(("u_noiseTexture".into(), noise_tex.clone()));
            let meta = DrawMetadata {
                bounding_volume: Some(bounding_sphere_wc),
                model_matrix: Some(model_matrix),
                owner: Some("CloudCollection".into()),
                ..Default::default()
            };
            frame.command_list.push(SceneCommand::from_draw(
                format!("CloudCollection-{va_idx}"),
                self.context.clone(),
                Pass::Translucent,
                meta,
                draw_opts,
            ));
        }

        Ok(())
    }

    fn destroy(&mut self) {
        if let Some(mut f) = self.facade.take() {
            let _ = f.destroy();
        }
        if let Some(p) = self.shader_program.take() {
            let _ = p.destroy();
        }
        if let Some(p) = self.noise_shader.take() {
            let _ = p.destroy();
        }
        if let Some(va) = self.noise_quad_va.take() {
            let _ = va.destroy();
        }
        if let Some(fb) = self.noise_framebuffer.take() {
            let _ = fb.destroy();
        }
        if let Some(tex) = self.noise_texture.take() {
            let _ = tex.destroy();
        }
        self.index_buffer = None;
    }
}

trait CloudCollectionBackend {
    #[allow(clippy::too_many_arguments)]
    fn update_and_emit(
        &mut self,
        clouds: &[Option<CumulusCloudHandle>],
        noise_detail: f64,
        noise_offset: Cartesian3,
        noise_dims: Cartesian3,
        debug_billboards: bool,
        debug_ellipsoids: bool,
        model_matrix: Matrix4,
        bounding_sphere_wc: BoundingSphere,
        frame: &mut FrameState,
    ) -> SceneResult<()>;
    fn destroy(&mut self);
}

impl<G: Gl + 'static> CloudCollectionBackend for CloudCollectionGpu<G> {
    fn update_and_emit(
        &mut self,
        clouds: &[Option<CumulusCloudHandle>],
        noise_detail: f64,
        noise_offset: Cartesian3,
        noise_dims: Cartesian3,
        debug_billboards: bool,
        debug_ellipsoids: bool,
        model_matrix: Matrix4,
        bounding_sphere_wc: BoundingSphere,
        frame: &mut FrameState,
    ) -> SceneResult<()> {
        CloudCollectionGpu::update_and_emit(
            self,
            clouds,
            noise_detail,
            noise_offset,
            noise_dims,
            debug_billboards,
            debug_ellipsoids,
            model_matrix,
            bounding_sphere_wc,
            frame,
        )
    }
    fn destroy(&mut self) {
        CloudCollectionGpu::destroy(self);
    }
}

/// Shared handle modeling CesiumJS cloud mutable reference semantics.
pub type CumulusCloudHandle = Rc<RefCell<CumulusCloud>>;

/// Noise texture dimension constants from CesiumJS CloudCollection.
pub const TEXTURE_SLICE_WIDTH: f64 = 128.0;
pub const NOISE_TEXTURE_ROWS: f64 = 4.0;
pub const NOISE_TEXTURE_WIDTH: usize = 4096; // (128 * 128) / 4
pub const NOISE_TEXTURE_HEIGHT: usize = 512; // 128 * 4

/// Vertex attributes packed for a cloud vertex or instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudVertexAttributes {
    /// High-precision position XYZ with scale X in W.
    pub position_high_and_scale_x: [f64; 4],
    /// Low-precision position XYZ with scale Y in W.
    pub position_low_and_scale_y: [f64; 4],
    /// Packed attribute 0: `[show, brightness, u_direction, v_direction]`.
    pub packed_attribute0: [f64; 4],
    /// Packed attribute 1: `[maximumSize.x, maximumSize.y, maximumSize.z, slice]`.
    pub packed_attribute1: [f64; 4],
    /// Normalized color RGBA bytes.
    pub color: [u8; 4],
}

impl CloudVertexAttributes {
    /// Encodes attributes for a single cloud in instanced rendering mode.
    pub fn instanced_from_cloud(cloud: &CumulusCloud) -> Self {
        let encoded = EncodedCartesian3::from_cartesian(cloud.position());
        let scale = cloud.scale();
        let max_size = cloud.maximum_size();
        let color = cloud.color();

        let show = if cloud.show() { 1.0 } else { 0.0 };
        let red = Color::float_to_byte(color.red) as u8;
        let green = Color::float_to_byte(color.green) as u8;
        let blue = Color::float_to_byte(color.blue) as u8;
        let alpha = Color::float_to_byte(color.alpha) as u8;

        Self {
            position_high_and_scale_x: [encoded.high.x, encoded.high.y, encoded.high.z, scale.x],
            position_low_and_scale_y: [encoded.low.x, encoded.low.y, encoded.low.z, scale.y],
            packed_attribute0: [show, cloud.brightness(), 0.0, 0.0],
            packed_attribute1: [max_size.x, max_size.y, max_size.z, cloud.slice()],
            color: [red, green, blue, alpha],
        }
    }

    /// Encodes the 4 quad vertices for a single cloud in batched rendering mode.
    pub fn batched_quad_from_cloud(cloud: &CumulusCloud) -> [Self; 4] {
        let encoded = EncodedCartesian3::from_cartesian(cloud.position());
        let scale = cloud.scale();
        let max_size = cloud.maximum_size();
        let color = cloud.color();

        let show = if cloud.show() { 1.0 } else { 0.0 };
        let brightness = cloud.brightness();
        let red = Color::float_to_byte(color.red) as u8;
        let green = Color::float_to_byte(color.green) as u8;
        let blue = Color::float_to_byte(color.blue) as u8;
        let alpha = Color::float_to_byte(color.alpha) as u8;

        let high_and_scale_x = [encoded.high.x, encoded.high.y, encoded.high.z, scale.x];
        let low_and_scale_y = [encoded.low.x, encoded.low.y, encoded.low.z, scale.y];
        let packed1 = [max_size.x, max_size.y, max_size.z, cloud.slice()];
        let col = [red, green, blue, alpha];

        // Corner UV directions corresponding to writePackedAttribute0 in CloudCollection.js
        let corner_directions = [
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];

        [
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[0][0], corner_directions[0][1]],
                packed_attribute1: packed1,
                color: col,
            },
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[1][0], corner_directions[1][1]],
                packed_attribute1: packed1,
                color: col,
            },
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[2][0], corner_directions[2][1]],
                packed_attribute1: packed1,
                color: col,
            },
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[3][0], corner_directions[3][1]],
                packed_attribute1: packed1,
                color: col,
            },
        ]
    }
}

/// Options for creating a [`CloudCollection`].
#[derive(Clone, Debug, PartialEq)]
pub struct CloudCollectionOptions {
    /// Whether to display clouds in the collection.
    pub show: bool,
    /// Desired amount of detail in the noise texture (power of two, default: 16.0).
    pub noise_detail: f64,
    /// Desired translation offset of noise texture coordinates (default: ZERO).
    pub noise_offset: Cartesian3,
    /// Debug option rendering billboards with opaque colors.
    pub debug_billboards: bool,
    /// Debug option drawing clouds as opaque ellipsoids.
    pub debug_ellipsoids: bool,
    /// CesiumRS extension: transform from model to world space.
    pub model_matrix: Matrix4,
}

impl Default for CloudCollectionOptions {
    fn default() -> Self {
        Self {
            show: true,
            noise_detail: 16.0,
            noise_offset: Cartesian3::ZERO,
            debug_billboards: false,
            debug_ellipsoids: false,
            model_matrix: Matrix4::IDENTITY,
        }
    }
}

/// A CPU collection of clouds in 3D scene space.
pub struct CloudCollection {
    /// Determines if clouds in this collection will be shown.
    pub show: bool,
    /// Controls the detail in the precomputed noise texture.
    pub noise_detail: f64,
    /// Applies a translation to noise texture coordinates.
    pub noise_offset: Cartesian3,
    /// Debug flag: renders billboards with an opaque color.
    pub debug_billboards: bool,
    /// Debug flag: renders clouds as opaque ellipsoids.
    pub debug_ellipsoids: bool,
    /// CesiumRS extension: model transformation matrix.
    pub model_matrix: Matrix4,

    clouds: Vec<Option<CumulusCloudHandle>>,
    clouds_removed: bool,
    destroyed: bool,

    base_volume: BoundingSphere,
    base_volume_wc: BoundingSphere,
    gpu: Option<Box<dyn CloudCollectionBackend>>,
}

impl Default for CloudCollection {
    fn default() -> Self {
        Self::with_options(CloudCollectionOptions::default())
    }
}

impl CloudCollection {
    /// Creates a new cloud collection with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new cloud collection with custom options.
    pub fn with_options(options: CloudCollectionOptions) -> Self {
        Self {
            show: options.show,
            noise_detail: options.noise_detail,
            noise_offset: options.noise_offset,
            debug_billboards: options.debug_billboards,
            debug_ellipsoids: options.debug_ellipsoids,
            model_matrix: options.model_matrix,

            clouds: Vec::new(),
            clouds_removed: false,
            destroyed: false,

            base_volume: BoundingSphere::default(),
            base_volume_wc: BoundingSphere::default(),
            gpu: None,
        }
    }

    /// Creates a new `CloudCollection` attached to a renderer `Context<G>`.
    pub fn new_with_context<G: Gl + 'static>(
        options: CloudCollectionOptions,
        context: Context<G>,
    ) -> SceneResult<Self> {
        let mut coll = Self::with_options(options);
        coll.attach_context(context)?;
        Ok(coll)
    }

    /// Attaches a renderer `Context<G>` for noise-texture generation,
    /// `VertexArrayFacade` commits, and translucent cloud rendering.
    pub fn attach_context<G: Gl + 'static>(&mut self, context: Context<G>) -> SceneResult<()> {
        if self.destroyed {
            return Err(RuntimeError::new("CloudCollection has been destroyed."));
        }
        self.gpu = Some(Box::new(CloudCollectionGpu::new(context)?));
        Ok(())
    }

    /// Updates GPU resources and emits noise compute + translucent draw commands.
    pub fn update_with_context<G: Gl + 'static>(
        &mut self,
        context: &Context<G>,
        frame: &mut FrameState,
    ) -> SceneResult<()> {
        if self.destroyed {
            return Err(RuntimeError::new("CloudCollection has been destroyed."));
        }
        if self.gpu.is_none() {
            self.attach_context(context.clone())?;
        }
        ScenePrimitive::update(self, frame)
    }

    /// Returns `true` if the collection has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Compacts removed clouds and re-indexes them; fixes upstream's index reassignment typo.
    /// Panics before changing the collection if a handle is borrowed.
    pub fn compact(&mut self) {
        if !self.clouds_removed {
            return;
        }
        for cloud in self.clouds.iter().flatten() {
            assert!(cloud.try_borrow_mut().is_ok(), "cannot compact while a cloud is borrowed");
        }
        self.clouds_removed = false;
        let mut compacted = Vec::with_capacity(self.clouds.len());
        for handle in self.clouds.drain(..).flatten() {
            handle.borrow_mut().set_collection_index(compacted.len());
            compacted.push(Some(handle));
        }
        self.clouds = compacted;
    }

    /// Returns the number of clouds currently in this collection.
    pub fn len(&mut self) -> usize {
        self.compact();
        self.clouds.len()
    }

    /// Returns `true` if there are no clouds in the collection.
    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    /// Creates and adds a cumulus cloud with the specified options to the collection.
    pub fn add(&mut self, options: CumulusCloudOptions) -> Result<CumulusCloudHandle, &'static str> {
        if self.destroyed {
            return Err("CloudCollection has been destroyed.");
        }
        self.compact();
        let index = self.clouds.len();
        let cloud = Rc::new(RefCell::new(CumulusCloud::new(options, index)?));
        self.clouds.push(Some(cloud.clone()));
        Ok(cloud)
    }

    /// Removes a cloud from the collection.
    pub fn remove(&mut self, cloud: &CumulusCloudHandle) -> bool {
        if self.destroyed {
            return false;
        }
        let pos = self
            .clouds
            .iter()
            .position(|opt| opt.as_ref().is_some_and(|c| Rc::ptr_eq(c, cloud)));
        if let Some(index) = pos {
            let Some(removed) = self.clouds[index].as_ref() else { return false };
            let Ok(mut borrowed) = removed.try_borrow_mut() else { return false };
            borrowed.detach();
            drop(borrowed);
            self.clouds[index] = None;
            self.clouds_removed = true;
            return true;
        }
        false
    }

    /// Removes all clouds from the collection.
    pub fn remove_all(&mut self) {
        for handle in self.clouds.iter().flatten() {
            assert!(handle.try_borrow_mut().is_ok(), "cannot remove clouds while a handle is borrowed");
        }
        for handle in self.clouds.drain(..).flatten() {
            handle.borrow_mut().detach();
        }
        self.clouds_removed = false;
    }

    /// Checks whether this collection contains a given cloud.
    pub fn contains(&self, cloud: &CumulusCloudHandle) -> bool {
        self.clouds
            .iter()
            .any(|opt| opt.as_ref().is_some_and(|c| Rc::ptr_eq(c, cloud)))
    }

    /// Returns the cloud at the specified index.
    pub fn get(&mut self, index: usize) -> Option<CumulusCloudHandle> {
        if self.destroyed {
            return None;
        }
        self.compact();
        self.clouds.get(index).and_then(|opt| opt.clone())
    }

    /// Destroys the collection and detaches all contained clouds.
    pub fn destroy(&mut self) {
        if !self.destroyed {
            if let Some(mut gpu) = self.gpu.take() {
                gpu.destroy();
            }
            self.remove_all();
            self.destroyed = true;
        }
    }

    /// Computes a culling-safe bounding sphere in model space incorporating
    /// every cloud's scale and maximum ellipsoid extents.
    pub fn extent_bounding_sphere(&mut self) -> BoundingSphere {
        self.compact();
        let mut union: Option<BoundingSphere> = None;
        for handle in self.clouds.iter().flatten() {
            let c = handle.borrow();
            let sc = c.scale();
            let ms = c.maximum_size();
            let r = 0.5 * sc.x.abs().max(sc.y.abs()).max(ms.x.abs()).max(ms.y.abs()).max(ms.z.abs());
            let bs = BoundingSphere::new(c.position(), r);
            union = Some(match union {
                Some(acc) => acc.union(&bs),
                None => bs,
            });
        }
        union.unwrap_or_default()
    }

    /// Computes a culling-safe bounding sphere in world coordinates.
    pub fn extent_bounding_sphere_wc(&mut self) -> BoundingSphere {
        let model = self.extent_bounding_sphere();
        BoundingSphere::transform(&model, &self.model_matrix)
    }

    /// Recomputes bounding spheres from current cloud positions.
    pub fn update_bounding_volumes(&mut self) {
        self.compact();
        let positions: Vec<Cartesian3> = self
            .clouds
            .iter()
            .filter_map(|opt| opt.as_ref().map(|c| c.borrow().position()))
            .collect();

        if positions.is_empty() {
            self.base_volume = BoundingSphere::default();
            self.base_volume_wc = BoundingSphere::default();
        } else {
            self.base_volume = BoundingSphere::from_points(&positions);
            self.base_volume_wc =
                BoundingSphere::transform(&self.base_volume, &self.model_matrix);
        }
    }

    /// Returns the base bounding sphere in model space.
    pub fn base_volume(&mut self) -> &BoundingSphere {
        self.update_bounding_volumes();
        &self.base_volume
    }

    /// Returns the base bounding sphere transformed into world coordinates.
    pub fn base_volume_wc(&mut self) -> &BoundingSphere {
        self.update_bounding_volumes();
        &self.base_volume_wc
    }

    /// Returns the noise texture dimension metrics vector `(128, 4, 0.25)`
    /// corresponding to `u_noiseTextureDimensions` in CesiumJS.
    pub fn noise_texture_dimensions(&self) -> Cartesian3 {
        Cartesian3::new(
            TEXTURE_SLICE_WIDTH,
            NOISE_TEXTURE_ROWS,
            1.0 / NOISE_TEXTURE_ROWS,
        )
    }

    /// Computes instanced vertex attributes for all active clouds.
    pub fn instanced_attributes(&mut self) -> Vec<CloudVertexAttributes> {
        self.compact();
        let mut result = Vec::with_capacity(self.clouds.len());
        for handle in self.clouds.iter().flatten() {
            let mut cloud = handle.borrow_mut();
            cloud.clear_dirty();
            result.push(CloudVertexAttributes::instanced_from_cloud(&cloud));
        }
        result
    }

    /// Computes batched quad vertex attributes (4 vertices per cloud) for all active clouds.
    pub fn batched_attributes(&mut self) -> Vec<[CloudVertexAttributes; 4]> {
        self.compact();
        let mut result = Vec::with_capacity(self.clouds.len());
        for handle in self.clouds.iter().flatten() {
            let mut cloud = handle.borrow_mut();
            cloud.clear_dirty();
            result.push(CloudVertexAttributes::batched_quad_from_cloud(&cloud));
        }
        result
    }

    /// Returns positions of all active clouds in the collection.
    pub fn positions(&mut self) -> Vec<Cartesian3> {
        self.compact();
        self.clouds
            .iter()
            .filter_map(|opt| opt.as_ref().map(|c| c.borrow().position()))
            .collect()
    }

    /// Counts dirty clouds for each property index, not Cesium's cumulative
    /// `_propertiesChanged`. Generating attributes clears the dirty flags.
    pub fn properties_changed_count(&mut self) -> [usize; NUMBER_OF_PROPERTIES] {
        self.compact();
        let mut counts = [0; NUMBER_OF_PROPERTIES];
        for handle in self.clouds.iter().flatten() {
            let cloud = handle.borrow();
            for (i, count) in counts.iter_mut().enumerate() {
                if cloud.is_property_dirty(i) {
                    *count += 1;
                }
            }
        }
        counts
    }
}

impl ScenePrimitive for CloudCollection {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> SceneResult<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> SceneResult<()> {
        if self.destroyed {
            return Err(RuntimeError::new("CloudCollection has been destroyed."));
        }
        if !self.show {
            return Ok(());
        }
        self.compact();
        if self.clouds.is_empty() {
            return Ok(());
        }
        let extent_wc = self.extent_bounding_sphere_wc();
        let noise_dims = self.noise_texture_dimensions();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.update_and_emit(
                &self.clouds,
                self.noise_detail,
                self.noise_offset,
                noise_dims,
                self.debug_billboards,
                self.debug_ellipsoids,
                self.model_matrix,
                extent_wc,
                frame,
            )?;
        }
        Ok(())
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> SceneResult<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, _frame: &mut FrameState, _pass: Cesium3DTilePass) -> SceneResult<()> {
        Ok(())
    }

    fn destroy(&mut self) -> SceneResult<()> {
        CloudCollection::destroy(self);
        Ok(())
    }
}

impl Drop for CloudCollection {
    fn drop(&mut self) {
        for cloud in self.clouds.iter().flatten() {
            if let Ok(mut c) = cloud.try_borrow_mut() {
                c.detach();
            }
        }
    }
}
