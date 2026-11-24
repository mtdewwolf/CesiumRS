//! CesiumJS `PointCloud`: the contents of one 3D Tiles point-cloud (pnts) tile,
//! including decoding, styling, rendering resources and picking.
use super::{
    blending_state,
    cesium_3d_tile_style::Cesium3DTileStyle,
    get_clip_and_style_code::get_clip_and_style_code,
    get_clipping_function::get_clipping_function,
    pnts_parser::{self, ParsedPnts},
    scene_mode::SceneMode,
    shadow_mode::ShadowMode,
    stencil_constants,
    ClippingPlaneCollection, FrameState, SceneCommand, SceneDrawOptions, SplitDirection,
};
use crate::{
    core::{
        math::MersenneTwister, BoundingSphere, Cartesian3, Cartesian4, Color, ComponentDatatype, Matrix4,
        PrimitiveType, RuntimeError, TypedArray,
    },
    renderer::{
        render_state::DepthTest,
        Buffer, BufferData, BufferKind, BufferUsage, Context, DrawMetadata, Gl, Pass, RenderState,
        SampledTexture, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformValue, VertexArray,
        VertexAttribute,
    },
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::Rc,
    sync::Arc,
};
type Result<T> = std::result::Result<T, RuntimeError>;

/// Creates a modified vertex shader.
pub type PointCloudVertexShaderLoaded = Rc<dyn Fn(&str) -> String>;
/// Creates a modified fragment shader.
pub type PointCloudFragmentShaderLoaded = Rc<dyn Fn(&str) -> String>;
/// Adds uniforms to the generated map.
pub type PointCloudUniformMapLoaded =
    Rc<dyn Fn(Vec<(String, UniformValue)>) -> Vec<(String, UniformValue)>>;
/// Receives the parsed batch table.
pub type PointCloudBatchTableLoaded = Rc<dyn Fn(usize, Option<Value>, Option<Vec<u8>>)>;
/// Returns the GLSL pick uniform name.
pub type PointCloudPickIdLoaded = Rc<dyn Fn() -> String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodingState {
    NeedsDecode = 0,
    Decoding = 1,
    Ready = 2,
    Failed = 3,
}

/// Callbacks the containing tileset or time-dynamic cloud installs.
#[derive(Clone)]
pub struct PointCloudCallbacks<G: Gl> {
    pub vertex_shader_loaded: Option<PointCloudVertexShaderLoaded>,
    pub fragment_shader_loaded: Option<PointCloudFragmentShaderLoaded>,
    pub uniform_map_loaded: Option<PointCloudUniformMapLoaded>,
    pub batch_table_loaded: Option<PointCloudBatchTableLoaded>,
    pub pick_id_loaded: Option<PointCloudPickIdLoaded>,
    pub _marker: std::marker::PhantomData<G>,
}
impl<G: Gl> Default for PointCloudCallbacks<G> {
    fn default() -> Self {
        Self {
            vertex_shader_loaded: None,
            fragment_shader_loaded: None,
            uniform_map_loaded: None,
            batch_table_loaded: None,
            pick_id_loaded: None,
            _marker: std::marker::PhantomData,
        }
    }
}

/// `new PointCloud(options)`.
pub struct PointCloudOptions<G: Gl> {
    pub array_buffer: Arc<Vec<u8>>,
    pub byte_offset: usize,
    pub cull: bool,
    pub opaque_pass: Pass,
    pub split_direction: SplitDirection,
    pub callbacks: PointCloudCallbacks<G>,
}
impl<G: Gl> Default for PointCloudOptions<G> {
    fn default() -> Self {
        Self {
            array_buffer: Arc::new(Vec::new()),
            byte_offset: 0,
            cull: true,
            opaque_pass: Pass::Opaque,
            split_direction: SplitDirection::None,
            callbacks: PointCloudCallbacks::default(),
        }
    }
}

struct PointCloudUniforms {
    values: Vec<(String, UniformValue)>,
}

struct VertexAttributeState {
    enabled: bool,
}

/// Geometry and shading state for a point cloud tile.
pub struct PointCloud<G: Gl + 'static> {
    parsed_content: Option<ParsedPnts>,
    draw_command: Option<SceneCommand>,
    is_translucent: bool,
    style_translucent: bool,
    constant_color: Color,
    highlight_color: Color,
    point_size: f64,
    rtc_center: Option<Cartesian3>,
    quantized_volume_scale: Option<Cartesian3>,
    quantized_volume_offset: Option<Cartesian3>,
    styleable_shader_attributes: BTreeMap<String, (u32, u32)>,
    is_quantized: bool,
    is_oct_encoded16p: bool,
    is_rgb565: bool,
    has_colors: bool,
    has_normals: bool,
    has_batch_ids: bool,
    decoding_state: DecodingState,
    is_quantized_draco: bool,
    is_oct_encoded_draco: bool,
    quantized_range: f64,
    oct_encoded_range: f64,
    pub back_face_culling: bool,
    back_face_culling_old: bool,
    pub normal_shading: bool,
    normal_shading_old: bool,
    opaque_render_state: RenderState,
    translucent_render_state: RenderState,
    mode: Option<SceneMode>,
    ready: bool,
    points_length: usize,
    geometry_byte_length: usize,
    callbacks: PointCloudCallbacks<G>,
    opaque_pass: Pass,
    cull: bool,
    pub style: Option<Rc<RefCell<Cesium3DTileStyle>>>,
    style_old: Option<Rc<RefCell<Cesium3DTileStyle>>>,
    pub style_dirty: bool,
    pub model_matrix: Matrix4,
    model_matrix_old: Matrix4,
    computed_model_matrix: Matrix4,
    pub time: f64,
    pub shadows: ShadowMode,
    local_bounding_sphere: Option<BoundingSphere>,
    bounding_sphere: Option<BoundingSphere>,
    pub clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
    pub is_clipped: bool,
    pub clipping_planes_dirty: bool,
    pub clipping_planes_origin_matrix: Option<Matrix4>,
    pub attenuation: bool,
    attenuation_old: bool,
    pub geometric_error: f64,
    pub geometric_error_scale: f64,
    pub maximum_attenuation: f64,
    pub split_direction: SplitDirection,
    splitting_enabled: bool,
    error: Option<RuntimeError>,
    uniforms: Rc<RefCell<PointCloudUniforms>>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    draw_command_translucent: bool,
    style_attributes: Vec<VertexAttributeState>,
    textures: Vec<(String, Rc<dyn SampledTexture<G>>)>,
    shader_dirty: bool,
    shader_source: Option<(String, String)>,
    uniform_names: Vec<String>,
    destroyed: bool,
}

const POSITION_LOCATION: u32 = 0;
const COLOR_LOCATION: u32 = 1;
const NORMAL_LOCATION: u32 = 2;
const BATCH_ID_LOCATION: u32 = 3;
const NUMBER_OF_ATTRIBUTES: u32 = 4;

fn random_values_20() -> &'static [f64; 20] {
    use std::sync::OnceLock;
    static VALUES: OnceLock<[f64; 20]> = OnceLock::new();
    VALUES.get_or_init(|| {
        let mut rng = MersenneTwister::new(0);
        std::array::from_fn(|_| rng.random())
    })
}

/// Cesium's sampled bounding sphere; exposed for validation.
pub fn approximate_bounding_sphere_from_positions(positions: &TypedArray) -> BoundingSphere {
    let points_length = positions.len() / 3;
    let samples_length = points_length.min(20);
    let random_values = random_values_20();
    let mut min = Cartesian3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut max = Cartesian3::new(-f64::MAX, -f64::MAX, -f64::MAX);
    for value in random_values.iter().take(samples_length) {
        let index = (value * points_length as f64).floor() as usize;
        let position = Cartesian3::new(
            positions.get(index * 3).unwrap_or(0.0),
            positions.get(index * 3 + 1).unwrap_or(0.0),
            positions.get(index * 3 + 2).unwrap_or(0.0),
        );
        min = Cartesian3::new(min.x.min(position.x), min.y.min(position.y), min.z.min(position.z));
        max = Cartesian3::new(max.x.max(position.x), max.y.max(position.y), max.z.max(position.z));
    }
    let mut sphere = BoundingSphere::from_corner_points(min, max);
    sphere.radius += 0.01;
    sphere
}

/// Converts INT, UNSIGNED_INT and DOUBLE vertex attributes to floats.
pub fn prepare_vertex_attribute(typed_array: &TypedArray) -> TypedArray {
    match typed_array {
        TypedArray::Int32(_) | TypedArray::Uint32(_) | TypedArray::Float64(_) => {
            TypedArray::Float32(typed_array.to_f64_vec().iter().map(|v| *v as f32).collect())
        }
        other => other.clone(),
    }
}

impl<G: Gl> PointCloud<G> {
    pub fn new(options: PointCloudOptions<G>) -> Result<Self> {
        if options.array_buffer.is_empty() && options.byte_offset == 0 {
            return Err(RuntimeError::new("options.arrayBuffer is required"));
        }
        let mut cloud = Self {
            parsed_content: None,
            draw_command: None,
            is_translucent: false,
            style_translucent: false,
            constant_color: Color::DARKGRAY,
            highlight_color: Color::WHITE,
            point_size: 1.0,
            rtc_center: None,
            quantized_volume_scale: None,
            quantized_volume_offset: None,
            styleable_shader_attributes: BTreeMap::new(),
            is_quantized: false,
            is_oct_encoded16p: false,
            is_rgb565: false,
            has_colors: false,
            has_normals: false,
            has_batch_ids: false,
            decoding_state: DecodingState::Ready,
            is_quantized_draco: false,
            is_oct_encoded_draco: false,
            quantized_range: 0.0,
            oct_encoded_range: 0.0,
            back_face_culling: false,
            back_face_culling_old: false,
            normal_shading: true,
            normal_shading_old: true,
            opaque_render_state: RenderState::default(),
            translucent_render_state: RenderState::default(),
            mode: None,
            ready: false,
            points_length: 0,
            geometry_byte_length: 0,
            callbacks: options.callbacks,
            opaque_pass: options.opaque_pass,
            cull: options.cull,
            style: None,
            style_old: None,
            style_dirty: false,
            model_matrix: Matrix4::IDENTITY,
            model_matrix_old: Matrix4::IDENTITY,
            computed_model_matrix: Matrix4::IDENTITY,
            time: 0.0,
            shadows: ShadowMode::Enabled,
            local_bounding_sphere: None,
            bounding_sphere: None,
            clipping_planes: None,
            is_clipped: false,
            clipping_planes_dirty: false,
            clipping_planes_origin_matrix: None,
            attenuation: false,
            attenuation_old: false,
            geometric_error: 0.0,
            geometric_error_scale: 1.0,
            maximum_attenuation: 1.0,
            split_direction: options.split_direction,
            splitting_enabled: false,
            error: None,
            uniforms: Rc::new(RefCell::new(PointCloudUniforms { values: Vec::new() })),
            vertex_array: None,
            shader_program: None,
            draw_command_translucent: false,
            style_attributes: Vec::new(),
            textures: Vec::new(),
            shader_dirty: true,
            shader_source: None,
            uniform_names: Vec::new(),
            destroyed: false,
        };
        cloud.initialize(options.array_buffer, options.byte_offset)?;
        Ok(cloud)
    }

    fn initialize(&mut self, array_buffer: Arc<Vec<u8>>, byte_offset: usize) -> Result<()> {
        let parsed = pnts_parser::parse(&array_buffer, byte_offset)?;
        self.rtc_center = parsed.rtc_center;
        self.has_normals = parsed.has_normals;
        self.has_colors = parsed.has_colors;
        self.has_batch_ids = parsed.has_batch_ids;
        self.is_translucent = parsed.is_translucent;
        if !parsed.has_batch_ids {
            // Per-point batch table properties become styleable attributes in
            // Cesium; the binary property reader is not ported here.
        }
        if parsed.draco.is_some() {
            self.decoding_state = DecodingState::NeedsDecode;
        }
        if let Some(positions) = &parsed.positions {
            self.is_quantized = positions.is_quantized.unwrap_or(false);
            self.quantized_volume_scale = positions.quantized_volume_scale;
            self.quantized_volume_offset = positions.quantized_volume_offset;
            self.quantized_range = positions.quantized_range.unwrap_or(0) as f64;
        }
        if let Some(normals) = &parsed.normals {
            self.is_oct_encoded16p = normals.oct_encoded.unwrap_or(false);
        }
        if let Some(colors) = &parsed.colors {
            if let Some(color) = colors.constant_color {
                self.constant_color = color;
                self.has_colors = false;
            }
            self.is_rgb565 = colors.is_rgb565.unwrap_or(false);
        }
        if parsed.has_batch_ids {
            if let Some(callback) = &self.callbacks.batch_table_loaded {
                let length = parsed
                    .batch_length
                    .as_ref()
                    .map_or(0, |length| length.to_number() as usize);
                callback(length, parsed.batch_table_json.clone(), parsed.batch_table_binary.clone());
            }
        }
        self.points_length = parsed.points_length.to_number() as usize;
        self.parsed_content = Some(parsed);
        Ok(())
    }

    pub fn points_length(&self) -> usize {
        self.points_length
    }
    pub fn geometry_byte_length(&self) -> usize {
        self.geometry_byte_length
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn color(&self) -> Color {
        self.highlight_color
    }
    pub fn set_color(&mut self, value: Color) {
        self.highlight_color = value;
    }
    pub fn bounding_sphere(&self) -> Option<BoundingSphere> {
        self.bounding_sphere
    }
    pub fn set_bounding_sphere(&mut self, value: Option<BoundingSphere>) {
        self.local_bounding_sphere = value;
        self.bounding_sphere = value;
    }
    pub fn computed_model_matrix(&self) -> Matrix4 {
        self.computed_model_matrix
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn is_translucent(&self) -> bool {
        self.is_translucent
    }
    pub fn style_translucent(&self) -> bool {
        self.style_translucent
    }
    pub fn draw_command(&self) -> Option<&SceneCommand> {
        self.draw_command.as_ref()
    }
    pub fn vertex_array(&self) -> Option<&Rc<VertexArray<G>>> {
        self.vertex_array.as_ref()
    }
    pub fn shader_source(&self) -> Option<(&str, &str)> {
        self.shader_source.as_ref().map(|(vs, fs)| (vs.as_str(), fs.as_str()))
    }
    pub fn uniform_names(&self) -> &[String] {
        &self.uniform_names
    }
    fn effective_translucent(&self) -> bool {
        self.is_translucent
            || self.highlight_color.alpha < 1.0
            || self.constant_color.alpha < 1.0
            || self.style_translucent
    }

    pub fn render_state(&self) -> &RenderState {
        if self.effective_translucent() {
            &self.translucent_render_state
        } else {
            &self.opaque_render_state
        }
    }

    fn create_vertex_buffer(
        &mut self,
        context: &Context<G>,
        typed_array: &TypedArray,
    ) -> Result<Buffer<G>> {
        let bytes = typed_array.to_le_bytes();
        let buffer = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
        self.geometry_byte_length += bytes.len();
        Ok(buffer)
    }

    fn create_resources(&mut self, context: &Context<G>) -> Result<()> {
        let parsed = self
            .parsed_content
            .take()
            .ok_or_else(|| RuntimeError::new("Point cloud content is unavailable"))?;
        let positions = parsed
            .positions
            .as_ref()
            .ok_or_else(|| RuntimeError::new("Point cloud requires POSITION"))?;
        let position_data = positions
            .typed_array
            .clone()
            .ok_or_else(|| RuntimeError::new("Point cloud POSITION has no data"))?;

        let mut attributes = Vec::new();
        let styleable: BTreeMap<String, (u32, u32)> = BTreeMap::new();

        let positions_buffer = self.create_vertex_buffer(context, &position_data)?;
        let position_datatype = if self.is_quantized {
            ComponentDatatype::UnsignedShort
        } else if self.is_quantized_draco {
            if self.quantized_range <= 255.0 {
                ComponentDatatype::UnsignedByte
            } else {
                ComponentDatatype::UnsignedShort
            }
        } else {
            ComponentDatatype::Float
        };
        attributes.push({
            let mut attribute = VertexAttribute::buffer(POSITION_LOCATION, positions_buffer, 3);
            attribute.datatype = position_datatype;
            attribute
        });
        if self.cull {
            let local = Some(if self.is_quantized || self.is_quantized_draco {
                BoundingSphere::from_corner_points(
                    Cartesian3::ZERO,
                    self.quantized_volume_scale.unwrap_or(Cartesian3::ONE),
                )
            } else {
                approximate_bounding_sphere_from_positions(&position_data)
            });
            self.local_bounding_sphere = local;
            self.bounding_sphere = local;
        }

        if self.has_colors {
            let colors = parsed.colors.as_ref().unwrap();
            let data = colors
                .typed_array
                .clone()
                .ok_or_else(|| RuntimeError::new("Point cloud COLOR has no data"))?;
            let buffer = self.create_vertex_buffer(context, &data)?;
            let mut attribute = if self.is_rgb565 {
                VertexAttribute::buffer(COLOR_LOCATION, buffer, 1)
            } else {
                let components = if self.is_translucent { 4 } else { 3 };
                VertexAttribute::buffer(COLOR_LOCATION, buffer, components)
            };
            attribute.datatype = if self.is_rgb565 {
                ComponentDatatype::UnsignedShort
            } else {
                ComponentDatatype::UnsignedByte
            };
            attribute.normalize = !self.is_rgb565;
            attributes.push(attribute);
        }

        if self.has_normals {
            let normals = parsed.normals.as_ref().unwrap();
            let data = normals
                .typed_array
                .clone()
                .ok_or_else(|| RuntimeError::new("Point cloud NORMAL has no data"))?;
            let buffer = self.create_vertex_buffer(context, &data)?;
            let (components, datatype) = if self.is_oct_encoded16p {
                (2, ComponentDatatype::UnsignedByte)
            } else if self.is_oct_encoded_draco {
                (
                    2,
                    if self.oct_encoded_range <= 255.0 {
                        ComponentDatatype::UnsignedByte
                    } else {
                        ComponentDatatype::UnsignedShort
                    },
                )
            } else {
                (3, ComponentDatatype::Float)
            };
            let mut attribute = VertexAttribute::buffer(NORMAL_LOCATION, buffer, components);
            attribute.datatype = datatype;
            attributes.push(attribute);
        }

        if self.has_batch_ids {
            let batch_ids = parsed.batch_ids.as_ref().unwrap();
            let data = prepare_vertex_attribute(
                batch_ids
                    .typed_array
                    .as_deref()
                    .ok_or_else(|| RuntimeError::new("Point cloud BATCH_ID has no data"))?,
            );
            let buffer = self.create_vertex_buffer(context, &data)?;
            let mut attribute = VertexAttribute::buffer(BATCH_ID_LOCATION, buffer, 1);
            attribute.datatype = data.component_datatype();
            attributes.push(attribute);
        }
        for (name, (location, components)) in &styleable {
            let _ = (name, location, components);
        }
        self.styleable_shader_attributes = styleable;
        self.style_attributes = attributes
            .iter()
            .skip(NUMBER_OF_ATTRIBUTES as usize)
            .map(|attribute| VertexAttributeState { enabled: attribute.enabled })
            .collect();

        self.opaque_render_state = RenderState {
            depth_test: DepthTest { enabled: true, ..DepthTest::default() },
            ..RenderState::default()
        };
        self.translucent_render_state = RenderState {
            depth_test: DepthTest { enabled: true, ..DepthTest::default() },
            depth_mask: false,
            blending: blending_state::alpha_blend(),
            ..RenderState::default()
        };
        if self.opaque_pass == Pass::Cesium3DTile {
            self.opaque_render_state.stencil_test = stencil_constants::set_cesium_3d_tile_bit();
            self.opaque_render_state.stencil_mask = stencil_constants::CESIUM_3D_TILE_MASK as i64;
            self.translucent_render_state.stencil_test = stencil_constants::set_cesium_3d_tile_bit();
            self.translucent_render_state.stencil_mask = stencil_constants::CESIUM_3D_TILE_MASK as i64;
        }
        let vertex_array = Rc::new(VertexArray::new(context, attributes, None)?);
        self.vertex_array = Some(vertex_array);
        self.ready = true;
        Ok(())
    }

    /// Rebuilds the VAO with current attribute-enabled flags.
    fn rebuild_vertex_array(&mut self, context: &Context<G>) -> Result<()> {
        let Some(vertex_array) = &self.vertex_array else { return Ok(()) };
        let mut attributes = Vec::new();
        for (index, attribute) in vertex_array.attributes().iter().enumerate() {
            let mut attribute = attribute.clone();
            if index >= NUMBER_OF_ATTRIBUTES as usize {
                if let Some(state) = self.style_attributes.get(index - NUMBER_OF_ATTRIBUTES as usize) {
                    attribute.enabled = state.enabled;
                }
            }
            attributes.push(attribute);
        }
        self.vertex_array = Some(Rc::new(VertexArray::new(context, attributes, None)?));
        Ok(())
    }

    fn create_uniform_map(&mut self, frame: &FrameState, context: &Context<G>) {
        let mut values = Vec::new();
        let point_size = if self.attenuation {
            self.maximum_attenuation
        } else {
            self.point_size
        } * frame.pixel_ratio;
        let depth_multiplier = if self.attenuation {
            if frame.mode == SceneMode::Scene2D
                || matches!(
                    frame.uniform.camera.frustum,
                    crate::renderer::UniformFrustum::Orthographic(_)
                        | crate::renderer::UniformFrustum::OrthographicOffCenter(_)
                )
            {
                f64::INFINITY
            } else {
                match frame.uniform.camera.frustum {
                    crate::renderer::UniformFrustum::Perspective(frustum) => {
                        frame.drawing_buffer_height as f64 / frustum.sse_denominator()
                    }
                    _ => f64::NAN,
                }
            }
        } else {
            0.0
        };
        values.push((
            "u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier".into(),
            UniformValue::Float(vec![
                point_size as f32,
                self.time as f32,
                (self.geometric_error * self.geometric_error_scale) as f32,
                depth_multiplier as f32,
            ]),
        ));
        values.push((
            "u_highlightColor".into(),
            UniformValue::Float(vec![
                self.highlight_color.red as f32,
                self.highlight_color.green as f32,
                self.highlight_color.blue as f32,
                self.highlight_color.alpha as f32,
            ]),
        ));
        values.push((
            "u_constantColor".into(),
            UniformValue::Float(vec![
                self.constant_color.red as f32,
                self.constant_color.green as f32,
                self.constant_color.blue as f32,
                self.constant_color.alpha as f32,
            ]),
        ));
        let clipping = self.clipping_planes.clone();
        let edge_style = clipping
            .as_ref()
            .map(|planes| planes.borrow().edge_color)
            .unwrap_or(Color::TRANSPARENT);
        let edge_width = clipping.as_ref().map_or(0.0, |planes| planes.borrow().edge_width);
        values.push((
            "u_clippingPlanes".into(),
            UniformValue::Int(vec![0]),
        ));
        values.push((
            "u_clippingPlanesMatrix".into(),
            UniformValue::Matrix { dimension: 4, values: Matrix4::IDENTITY.0.iter().map(|v| *v as f32).collect() },
        ));
        values.push((
            "u_clippingPlanesEdgeStyle".into(),
            UniformValue::Float(vec![
                edge_style.red as f32,
                edge_style.green as f32,
                edge_style.blue as f32,
                edge_width as f32,
            ]),
        ));
        if self.is_quantized || self.is_quantized_draco || self.is_oct_encoded_draco {
            let scale = self
                .quantized_volume_scale
                .map(|scale| {
                    Cartesian3::new(
                        scale.x / self.quantized_range,
                        scale.y / self.quantized_range,
                        scale.z / self.quantized_range,
                    )
                })
                .unwrap_or(Cartesian3::ZERO);
            values.push((
                "u_quantizedVolumeScaleAndOctEncodedRange".into(),
                UniformValue::Float(vec![
                    scale.x as f32,
                    scale.y as f32,
                    scale.z as f32,
                    self.oct_encoded_range as f32,
                ]),
            ));
        }
        values.push((
            "czm_splitDirection".into(),
            UniformValue::Float(vec![self.split_direction.value() as f32]),
        ));
        if let Some(callback) = &self.callbacks.uniform_map_loaded {
            values = callback(values);
        }
        self.uniforms.borrow_mut().values = values;
        let _ = context;
    }

    fn create_shaders(&mut self, frame: &FrameState, context: &Context<G>) -> Result<()> {
        let has_style = self.style.is_some();
        let is_quantized = self.is_quantized;
        let is_quantized_draco = self.is_quantized_draco;
        let is_oct_encoded16p = self.is_oct_encoded16p;
        let is_oct_encoded_draco = self.is_oct_encoded_draco;
        let is_rgb565 = self.is_rgb565;
        let is_translucent = self.is_translucent;
        let has_colors = self.has_colors;
        let has_normals = self.has_normals;
        let has_batch_ids = self.has_batch_ids;
        let back_face_culling = self.back_face_culling;
        let normal_shading = self.normal_shading;
        let attenuation = self.attenuation;
        let clipping_planes = self.clipping_planes.clone();
        let has_clipped_content = self.is_clipped;

        let mut variable_substitution_map = BTreeMap::new();
        variable_substitution_map.insert("POSITION".into(), "czm_3dtiles_builtin_property_POSITION".into());
        variable_substitution_map.insert(
            "POSITION_ABSOLUTE".into(),
            "czm_3dtiles_builtin_property_POSITION_ABSOLUTE".into(),
        );
        variable_substitution_map.insert("COLOR".into(), "czm_3dtiles_builtin_property_COLOR".into());
        variable_substitution_map.insert("NORMAL".into(), "czm_3dtiles_builtin_property_NORMAL".into());
        for (name, (location, _)) in &self.styleable_shader_attributes {
            variable_substitution_map.insert(name.clone(), format!("czm_3dtiles_property_{location}"));
        }

        let mut color_style_function = None;
        let mut show_style_function = None;
        let mut point_size_style_function = None;
        let mut style_translucent = is_translucent;
        if has_style {
            let style = self.style.as_ref().unwrap().clone();
            let mut style = style.borrow_mut();
            let mut shader_state = super::expression::ShaderState { translucent: false };
            let parameter_list = "(vec3 czm_3dtiles_builtin_property_POSITION, vec3 czm_3dtiles_builtin_property_POSITION_ABSOLUTE, vec4 czm_3dtiles_builtin_property_COLOR, vec3 czm_3dtiles_builtin_property_NORMAL)";
            color_style_function = style.get_color_shader_function(
                &format!("getColorFromStyle{parameter_list}"),
                &variable_substitution_map,
                &mut shader_state,
            )?;
            show_style_function = style.get_show_shader_function(
                &format!("getShowFromStyle{parameter_list}"),
                &variable_substitution_map,
                &mut shader_state,
            )?;
            point_size_style_function = style.get_point_size_shader_function(
                &format!("getPointSizeFromStyle{parameter_list}"),
                &variable_substitution_map,
                &mut shader_state,
            )?;
            if color_style_function.is_some() && shader_state.translucent {
                style_translucent = true;
            }
        }
        self.style_translucent = style_translucent;

        let has_color_style = color_style_function.is_some();
        let has_show_style = show_style_function.is_some();
        let has_point_size_style = point_size_style_function.is_some();

        let styleable_property_ids: Vec<u32> = Vec::new();
        let builtin_property_names: Vec<String> = Vec::new();
        let uses_color_semantic = builtin_property_names.iter().any(|name| name == "COLOR");
        let uses_normal_semantic = builtin_property_names.iter().any(|name| name == "NORMAL");
        if uses_normal_semantic && !has_normals {
            return Err(RuntimeError::new(
                "Style references the NORMAL semantic but the point cloud does not have normals",
            ));
        }
        for (location, _) in self.styleable_shader_attributes.values() {
            if let Some(state) = self
                .style_attributes
                .get(*location as usize - NUMBER_OF_ATTRIBUTES as usize)
            {
                let _ = state;
            }
        }
        let uses_colors = has_colors && (!has_color_style || uses_color_semantic);
        let uses_normals = has_normals && (normal_shading || back_face_culling || uses_normal_semantic);

        let mut attribute_locations = BTreeMap::new();
        attribute_locations.insert("a_position".into(), POSITION_LOCATION);
        if uses_colors {
            attribute_locations.insert("a_color".into(), COLOR_LOCATION);
        }
        if uses_normals {
            attribute_locations.insert("a_normal".into(), NORMAL_LOCATION);
        }
        if has_batch_ids {
            attribute_locations.insert("a_batchId".into(), BATCH_ID_LOCATION);
        }
        let mut attribute_declarations = String::new();
        for property_id in &styleable_property_ids {
            let attribute_name = format!("czm_3dtiles_property_{property_id}");
            let components = 1;
            let datatype = if components == 1 { "float".to_string() } else { format!("vec{components}") };
            attribute_declarations.push_str(&format!("in {datatype} {attribute_name}; \n"));
            attribute_locations.insert(attribute_name, *property_id);
        }

        let mut vs = String::from(
            "in vec3 a_position; \n\
             out vec4 v_color; \n\
             uniform vec4 u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier; \n\
             uniform vec4 u_constantColor; \n\
             uniform vec4 u_highlightColor; \n\
             float u_pointSize; \n\
             float tiles3d_tileset_time; \n",
        );
        if attenuation {
            vs.push_str("float u_geometricError; \nfloat u_depthMultiplier; \n");
        }
        vs.push_str(&attribute_declarations);
        if uses_colors {
            if is_translucent {
                vs.push_str("in vec4 a_color; \n");
            } else if is_rgb565 {
                vs.push_str(
                    "in float a_color; \n\
                     const float SHIFT_RIGHT_11 = 1.0 / 2048.0; \n\
                     const float SHIFT_RIGHT_5 = 1.0 / 32.0; \n\
                     const float SHIFT_LEFT_11 = 2048.0; \n\
                     const float SHIFT_LEFT_5 = 32.0; \n\
                     const float NORMALIZE_6 = 1.0 / 64.0; \n\
                     const float NORMALIZE_5 = 1.0 / 32.0; \n",
                );
            } else {
                vs.push_str("in vec3 a_color; \n");
            }
        }
        if uses_normals {
            if is_oct_encoded16p || is_oct_encoded_draco {
                vs.push_str("in vec2 a_normal; \n");
            } else {
                vs.push_str("in vec3 a_normal; \n");
            }
        }
        if has_batch_ids {
            vs.push_str("in float a_batchId; \n");
        }
        if is_quantized || is_quantized_draco || is_oct_encoded_draco {
            vs.push_str("uniform vec4 u_quantizedVolumeScaleAndOctEncodedRange; \n");
        }
        if let Some(function) = &color_style_function {
            vs.push_str(function);
        }
        if let Some(function) = &show_style_function {
            vs.push_str(function);
        }
        if let Some(function) = &point_size_style_function {
            vs.push_str(function);
        }
        vs.push_str(
            "void main() \n\
             { \n\
                 u_pointSize = u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier.x; \n\
                     tiles3d_tileset_time = u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier.y; \n",
        );
        if attenuation {
            vs.push_str(
                "    u_geometricError = u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier.z; \n\
                 u_depthMultiplier = u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier.w; \n",
            );
        }
        if uses_colors {
            if is_translucent {
                vs.push_str("    vec4 color = a_color; \n");
            } else if is_rgb565 {
                vs.push_str(
                    "    float compressed = a_color; \n\
                     float r = floor(compressed * SHIFT_RIGHT_11); \n\
                     compressed -= r * SHIFT_LEFT_11; \n\
                     float g = floor(compressed * SHIFT_RIGHT_5); \n\
                     compressed -= g * SHIFT_LEFT_5; \n\
                     float b = compressed; \n\
                     vec3 rgb = vec3(r * NORMALIZE_5, g * NORMALIZE_6, b * NORMALIZE_5); \n\
                     vec4 color = vec4(rgb, 1.0); \n",
                );
            } else {
                vs.push_str("    vec4 color = vec4(a_color, 1.0); \n");
            }
        } else {
            vs.push_str("    vec4 color = u_constantColor; \n");
        }
        if is_quantized || is_quantized_draco {
            vs.push_str("    vec3 position = a_position * u_quantizedVolumeScaleAndOctEncodedRange.xyz; \n");
        } else {
            vs.push_str("    vec3 position = a_position; \n");
        }
        vs.push_str("    vec3 position_absolute = vec3(czm_model * vec4(position, 1.0)); \n");
        if uses_normals {
            if is_oct_encoded16p {
                vs.push_str("    vec3 normal = czm_octDecode(a_normal); \n");
            } else if is_oct_encoded_draco {
                vs.push_str(
                    "    vec3 normal = czm_octDecode(a_normal, u_quantizedVolumeScaleAndOctEncodedRange.w).zxy; \n",
                );
            } else {
                vs.push_str("    vec3 normal = a_normal; \n");
            }
            vs.push_str("    vec3 normalEC = czm_normal * normal; \n");
        } else {
            vs.push_str("    vec3 normal = vec3(1.0); \n");
        }
        if has_color_style {
            vs.push_str("    color = getColorFromStyle(position, position_absolute, color, normal); \n");
        }
        if has_show_style {
            vs.push_str("    float show = float(getShowFromStyle(position, position_absolute, color, normal)); \n");
        }
        if has_point_size_style {
            vs.push_str(
                "    gl_PointSize = getPointSizeFromStyle(position, position_absolute, color, normal) * czm_pixelRatio; \n",
            );
        } else if attenuation {
            vs.push_str(
                "    vec4 positionEC = czm_modelView * vec4(position, 1.0); \n\
                 float depth = -positionEC.z; \n\
                 gl_PointSize = min((u_geometricError / depth) * u_depthMultiplier, u_pointSize); \n",
            );
        } else {
            vs.push_str("    gl_PointSize = u_pointSize; \n");
        }
        vs.push_str("    color = color * u_highlightColor; \n");
        if uses_normals && normal_shading {
            vs.push_str(
                "    float diffuseStrength = czm_getLambertDiffuse(czm_lightDirectionEC, normalEC); \n\
                 diffuseStrength = max(diffuseStrength, 0.4); \n\
                 color.xyz *= diffuseStrength * czm_lightColor; \n",
            );
        }
        vs.push_str(
            "    v_color = color; \n\
                 gl_Position = czm_modelViewProjection * vec4(position, 1.0); \n",
        );
        if uses_normals && back_face_culling {
            vs.push_str(
                "    float visible = step(-normalEC.z, 0.0); \n\
                 gl_Position *= visible; \n\
                 gl_PointSize *= visible; \n",
            );
        }
        if has_show_style {
            vs.push_str(
                "    gl_Position.w *= float(show); \n\
                 gl_PointSize *= float(show); \n",
            );
        }
        vs.push_str("} \n");

        let mut fs = String::from("in vec4 v_color; \n");
        if has_clipped_content {
            fs.push_str(
                "uniform highp sampler2D u_clippingPlanes; \n\
                 uniform mat4 u_clippingPlanesMatrix; \n\
                 uniform vec4 u_clippingPlanesEdgeStyle; \n\n",
            );
            if let Some(planes) = &clipping_planes {
                fs.push_str(&get_clipping_function(&planes.borrow(), context)?);
            }
            fs.push('\n');
        }
        fs.push_str(
            "void main() \n\
             { \n\
                 out_FragColor = czm_gammaCorrect(v_color); \n",
        );
        if has_clipped_content {
            fs.push_str(&get_clip_and_style_code(
                "u_clippingPlanes",
                "u_clippingPlanesMatrix",
                "u_clippingPlanesEdgeStyle",
            ));
        }
        fs.push_str("} \n");
        if self.split_direction != SplitDirection::None {
            fs = super::splitter::modify_fragment_shader(&fs);
        }
        if let Some(callback) = &self.callbacks.vertex_shader_loaded {
            vs = callback(&vs);
        }
        if let Some(callback) = &self.callbacks.fragment_shader_loaded {
            fs = callback(&fs);
        }

        self.shader_source = Some((vs.clone(), fs.clone()));
        self.uniform_names = self.uniforms.borrow().values.iter().map(|(name, _)| name.clone()).collect();
        let options = ShaderProgramOptions {
            vertex_shader_source: ShaderSource { sources: vec![vs], ..Default::default() },
            fragment_shader_source: ShaderSource { sources: vec![fs], ..Default::default() },
            attribute_locations,
        };
        let program = Rc::new(ShaderProgram::from_sources(context, &options).map_err(|_| {
            RuntimeError::new(
                "Error generating style shader: this may be caused by a type mismatch, index out-of-bounds, or other syntax error.",
            )
        })?);
        self.shader_program = Some(program);
        self.create_uniform_map(frame, context);
        self.uniform_names = self.uniforms.borrow().values.iter().map(|(name, _)| name.clone()).collect();
        let is_translucent = self.effective_translucent();
        self.build_draw_command(context, is_translucent)?;
        Ok(())
    }

    fn build_draw_command(&mut self, context: &Context<G>, is_translucent: bool) -> Result<()> {
        let vertex_array = self
            .vertex_array
            .clone()
            .ok_or_else(|| RuntimeError::new("Point cloud vertex array is missing"))?;
        let program = self
            .shader_program
            .clone()
            .ok_or_else(|| RuntimeError::new("Point cloud shader program is missing"))?;
        let mut draw = SceneDrawOptions::new(vertex_array, program);
        draw.state = if is_translucent {
            self.translucent_render_state.clone()
        } else {
            self.opaque_render_state.clone()
        };
        draw.primitive_type = PrimitiveType::Points;
        draw.count = Some(self.points_length);
        draw.uniform_map = Some(Rc::new({
            let uniforms = self.uniforms.clone();
            move || Ok(uniforms.borrow().values.clone())
        }));
        draw.textures = self.textures.clone();
        let metadata = DrawMetadata {
            bounding_volume: self.bounding_sphere,
            model_matrix: Some(self.computed_model_matrix),
            cull: self.cull,
            cast_shadows: self.shadows.cast_shadows(),
            receive_shadows: self.shadows.receive_shadows(),
            owner: Some("PointCloud".into()),
            ..Default::default()
        };
        self.draw_command = Some(SceneCommand::from_draw(
            "PointCloud",
            context.clone(),
            if is_translucent { Pass::Translucent } else { self.opaque_pass },
            metadata,
            draw,
        ));
        self.draw_command_translucent = is_translucent;
        Ok(())
    }

    pub fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        let context = frame
            .context
            .as_ref()
            .and_then(|resource| resource.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("PointCloud requires the frame's renderer context"))?;
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        if self.decoding_state == DecodingState::NeedsDecode {
            return Err(RuntimeError::new("Point cloud Draco decoding is not available in this port"));
        }
        if self.decoding_state == DecodingState::Decoding {
            return Ok(());
        }

        let mut shaders_dirty = false;
        let mut model_matrix_dirty = self.model_matrix != self.model_matrix_old;
        if self.mode != Some(frame.mode) {
            self.mode = Some(frame.mode);
            model_matrix_dirty = true;
        }
        if self.draw_command.is_none() {
            self.create_resources(&context)?;
            model_matrix_dirty = true;
            shaders_dirty = true;
            self.ready = true;
        }
        if model_matrix_dirty {
            self.model_matrix_old = self.model_matrix;
            let mut computed = self.model_matrix;
            if let Some(rtc) = self.rtc_center {
                computed = computed.multiply_by_translation(rtc);
            }
            if let Some(offset) = self.quantized_volume_offset {
                computed = computed.multiply_by_translation(offset);
            }
            if frame.mode != SceneMode::Scene3D {
                let translation = computed.get_column(3);
                if translation != Cartesian4::UNIT_W {
                    computed = crate::core::Transforms::basis_to_2d(
                        frame.uniform.map_projection.as_ref(),
                        &computed,
                    );
                }
            }
            self.computed_model_matrix = computed;
            self.bounding_sphere = self.local_bounding_sphere.map(|mut sphere| {
                if self.cull {
                    sphere.center = computed.multiply_by_point(sphere.center);
                    sphere.radius *= computed.get_scale().maximum_component();
                }
                sphere
            });
        }
        if self.clipping_planes_dirty {
            self.clipping_planes_dirty = false;
            shaders_dirty = true;
        }
        if self.attenuation != self.attenuation_old {
            self.attenuation_old = self.attenuation;
            shaders_dirty = true;
        }
        if self.back_face_culling != self.back_face_culling_old {
            self.back_face_culling_old = self.back_face_culling;
            shaders_dirty = true;
        }
        if self.normal_shading != self.normal_shading_old {
            self.normal_shading_old = self.normal_shading;
            shaders_dirty = true;
        }
        let style_changed = match (&self.style, &self.style_old) {
            (Some(style), Some(old)) => !Rc::ptr_eq(style, old),
            (None, None) => false,
            _ => true,
        };
        if style_changed || self.style_dirty {
            self.style_old = self.style.clone();
            self.style_dirty = false;
            shaders_dirty = true;
        }
        let splitting_enabled = self.split_direction != SplitDirection::None;
        if self.splitting_enabled != splitting_enabled {
            self.splitting_enabled = splitting_enabled;
            shaders_dirty = true;
        }
        if shaders_dirty || self.shader_dirty {
            self.rebuild_vertex_array(&context)?;
            self.create_shaders(frame, &context)?;
            self.shader_dirty = false;
        } else {
            self.create_uniform_map(frame, &context);
        }

        let is_translucent = self.effective_translucent();
        if self.draw_command.is_none() || self.draw_command_translucent != is_translucent {
            self.build_draw_command(&context, is_translucent)?;
        }
        if let Some(draw) = &mut self.draw_command {
            draw.pass = if is_translucent { Pass::Translucent } else { self.opaque_pass };
            draw.metadata.cast_shadows = self.shadows.cast_shadows();
            draw.metadata.receive_shadows = self.shadows.receive_shadows();
            draw.metadata.model_matrix = Some(self.computed_model_matrix);
            draw.metadata.bounding_volume = self.bounding_sphere;
        }
        if frame.passes.render || frame.passes.pick {
            if let Some(draw) = &self.draw_command {
                frame.command_list.push(draw.clone());
            }
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.draw_command = None;
        self.shader_program = None;
        self.vertex_array = None;
        self.textures.clear();
        self.destroyed = true;
        Ok(())
    }
}

#[allow(dead_code)]
fn unused(_: &HashMap<String, f64>) {}
