//! Cesium `PostProcessStage`: runs a post-process effect on a render target.
use crate::{
    core::{create_guid::create_guid, BoundingRectangle, Color, PixelFormat, RuntimeError},
    renderer::*,
    scene::PostProcessStageSampleMode,
};
use regex::Regex;
use std::{collections::HashMap, rc::Rc};

/// Uniform value that can be assigned to a post-process stage.
pub enum PostProcessUniformValue<G: Gl> {
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([f32; 9]),
    Mat4([f32; 16]),
    Texture(Rc<Texture<G>>),
    StageName(String),
    Dynamic(Rc<dyn Fn() -> Option<PostProcessUniformValue<G>>>),
}

impl<G: Gl> Clone for PostProcessUniformValue<G> {
    fn clone(&self) -> Self {
        match self {
            Self::Float(v) => Self::Float(*v),
            Self::Int(v) => Self::Int(*v),
            Self::Bool(v) => Self::Bool(*v),
            Self::Vec2(v) => Self::Vec2(*v),
            Self::Vec3(v) => Self::Vec3(*v),
            Self::Vec4(v) => Self::Vec4(*v),
            Self::Mat3(v) => Self::Mat3(*v),
            Self::Mat4(v) => Self::Mat4(*v),
            Self::Texture(t) => Self::Texture(t.clone()),
            Self::StageName(n) => Self::StageName(n.clone()),
            Self::Dynamic(d) => Self::Dynamic(d.clone()),
        }
    }
}

impl<G: Gl> std::fmt::Debug for PostProcessUniformValue<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Float(v) => write!(f, "Float({v})"),
            Self::Int(v) => write!(f, "Int({v})"),
            Self::Bool(v) => write!(f, "Bool({v})"),
            Self::Vec2(v) => write!(f, "Vec2({v:?})"),
            Self::Vec3(v) => write!(f, "Vec3({v:?})"),
            Self::Vec4(v) => write!(f, "Vec4({v:?})"),
            Self::Mat3(v) => write!(f, "Mat3({v:?})"),
            Self::Mat4(v) => write!(f, "Mat4({v:?})"),
            Self::Texture(t) => write!(f, "Texture(id={:?})", t.id()),
            Self::StageName(n) => write!(f, "StageName({n})"),
            Self::Dynamic(_) => write!(f, "Dynamic(<closure>)"),
        }
    }
}

impl<G: Gl> From<f32> for PostProcessUniformValue<G> {
    fn from(v: f32) -> Self {
        Self::Float(v)
    }
}

impl<G: Gl> From<f64> for PostProcessUniformValue<G> {
    fn from(v: f64) -> Self {
        Self::Float(v as f32)
    }
}

impl<G: Gl> From<i32> for PostProcessUniformValue<G> {
    fn from(v: i32) -> Self {
        Self::Int(v)
    }
}

impl<G: Gl> From<bool> for PostProcessUniformValue<G> {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl<G: Gl> From<[f32; 2]> for PostProcessUniformValue<G> {
    fn from(v: [f32; 2]) -> Self {
        Self::Vec2(v)
    }
}

impl<G: Gl> From<[f32; 3]> for PostProcessUniformValue<G> {
    fn from(v: [f32; 3]) -> Self {
        Self::Vec3(v)
    }
}

impl<G: Gl> From<[f32; 4]> for PostProcessUniformValue<G> {
    fn from(v: [f32; 4]) -> Self {
        Self::Vec4(v)
    }
}

impl<G: Gl> From<Color> for PostProcessUniformValue<G> {
    fn from(c: Color) -> Self {
        Self::Vec4([c.red as f32, c.green as f32, c.blue as f32, c.alpha as f32])
    }
}

impl<G: Gl> From<&str> for PostProcessUniformValue<G> {
    fn from(s: &str) -> Self {
        Self::StageName(s.to_string())
    }
}

impl<G: Gl> From<String> for PostProcessUniformValue<G> {
    fn from(s: String) -> Self {
        Self::StageName(s)
    }
}

impl<G: Gl> From<Rc<Texture<G>>> for PostProcessUniformValue<G> {
    fn from(t: Rc<Texture<G>>) -> Self {
        Self::Texture(t)
    }
}

/// A selected feature representation for post-processing silhouette/highlighting.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectedFeature {
    Color(Color),
    Colors(Vec<Color>),
}

impl From<Color> for SelectedFeature {
    fn from(c: Color) -> Self {
        Self::Color(c)
    }
}

impl From<Vec<Color>> for SelectedFeature {
    fn from(c: Vec<Color>) -> Self {
        Self::Colors(c)
    }
}

/// Options to initialize a [`PostProcessStage`].
pub struct PostProcessStageOptions<G: Gl> {
    pub name: Option<String>,
    pub fragment_shader: String,
    pub uniforms: HashMap<String, PostProcessUniformValue<G>>,
    pub texture_scale: Option<f64>,
    pub force_power_of_two: Option<bool>,
    pub sample_mode: Option<PostProcessStageSampleMode>,
    pub pixel_format: Option<PixelFormat>,
    pub pixel_datatype: Option<PixelDatatype>,
    pub clear_color: Option<Color>,
    pub scissor_rectangle: Option<BoundingRectangle>,
}

impl<G: Gl> Clone for PostProcessStageOptions<G> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            fragment_shader: self.fragment_shader.clone(),
            uniforms: self.uniforms.clone(),
            texture_scale: self.texture_scale,
            force_power_of_two: self.force_power_of_two,
            sample_mode: self.sample_mode,
            pixel_format: self.pixel_format,
            pixel_datatype: self.pixel_datatype,
            clear_color: self.clear_color,
            scissor_rectangle: self.scissor_rectangle,
        }
    }
}

impl<G: Gl> PostProcessStageOptions<G> {
    pub fn new(fragment_shader: impl Into<String>) -> Self {
        Self {
            name: None,
            fragment_shader: fragment_shader.into(),
            uniforms: HashMap::new(),
            texture_scale: None,
            force_power_of_two: None,
            sample_mode: None,
            pixel_format: None,
            pixel_datatype: None,
            clear_color: None,
            scissor_rectangle: None,
        }
    }
}

fn add_selected_id_to_shader(shader_source: &str, id_texture_width: usize) -> String {
    let re = Regex::new(r"in\s+vec2\s+v_textureCoordinates;").unwrap();
    let stripped = re.replace_all(shader_source, "");
    format!(
        "#define CZM_SELECTED_FEATURE\n\
uniform sampler2D czm_idTexture;\n\
uniform sampler2D czm_selectedIdTexture;\n\
uniform float czm_selectedIdTextureStep;\n\
in vec2 v_textureCoordinates;\n\
bool czm_selected(vec2 offset)\n\
{{\n\
    bool selected = false;\n\
    vec4 id = texture(czm_idTexture, v_textureCoordinates + offset);\n\
    for (int i = 0; i < {id_texture_width}; ++i)\n\
    {{\n\
        vec4 selectedId = texture(czm_selectedIdTexture, vec2((float(i) + 0.5) * czm_selectedIdTextureStep, 0.5));\n\
        if (all(equal(id, selectedId)))\n\
        {{\n\
            return true;\n\
        }}\n\
    }}\n\
    return false;\n\
}}\n\
bool czm_selected()\n\
{{\n\
    return czm_selected(vec2(0.0));\n\
}}\n\n\
{stripped}"
    )
}

/// Runs a post-process stage on either the texture rendered by the scene
/// or the output of a previous post-process stage.
pub struct PostProcessStage<G: Gl> {
    name: String,
    fragment_shader: String,
    uniforms: HashMap<String, PostProcessUniformValue<G>>,
    texture_scale: f64,
    force_power_of_two: bool,
    sample_mode: PostProcessStageSampleMode,
    pixel_format: PixelFormat,
    pixel_datatype: PixelDatatype,
    clear_color: Color,
    scissor_rectangle: Option<BoundingRectangle>,
    enabled: bool,
    ready: bool,

    selected: Option<Vec<SelectedFeature>>,
    parent_selected: Option<Vec<SelectedFeature>>,
    combined_selected_shadow: Option<Vec<Color>>,
    selected_dirty: bool,
    selected_id_texture: Option<Rc<Texture<G>>>,

    command: Option<ViewportCommand<G>>,
    framebuffer: Option<Rc<Framebuffer<G>>>,
    output_texture: Option<Rc<Texture<G>>>,

    use_log_depth: bool,
    log_depth_changed: bool,
    sampler: Option<Sampler>,
    destroyed: bool,
}

impl<G: Gl> PostProcessStage<G> {
    pub fn new(options: PostProcessStageOptions<G>) -> Result<Self, RuntimeError> {
        let texture_scale = options.texture_scale.unwrap_or(1.0);
        if texture_scale <= 0.0 || texture_scale > 1.0 {
            return Err(RuntimeError::new("options.texture_scale must be greater than 0.0 and <= 1.0"));
        }
        let pixel_format = options.pixel_format.unwrap_or(PixelFormat::Rgba);
        if !pixel_format.is_color_format() {
            return Err(RuntimeError::new("options.pixel_format must be a color format"));
        }

        let name = options.name.unwrap_or_else(create_guid);
        let sample_mode = options.sample_mode.unwrap_or(PostProcessStageSampleMode::Nearest);
        let pixel_datatype = options.pixel_datatype.unwrap_or(PixelDatatype::UnsignedByte);
        let clear_color = options.clear_color.unwrap_or(Color::BLACK);

        Ok(Self {
            name,
            fragment_shader: options.fragment_shader,
            uniforms: options.uniforms,
            texture_scale,
            force_power_of_two: options.force_power_of_two.unwrap_or(false),
            sample_mode,
            pixel_format,
            pixel_datatype,
            clear_color,
            scissor_rectangle: options.scissor_rectangle,
            enabled: true,
            ready: false,
            selected: None,
            parent_selected: None,
            combined_selected_shadow: None,
            selected_dirty: true,
            selected_id_texture: None,
            command: None,
            framebuffer: None,
            output_texture: None,
            use_log_depth: false,
            log_depth_changed: false,
            sampler: None,
            destroyed: false,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fragment_shader(&self) -> &str {
        &self.fragment_shader
    }

    pub fn texture_scale(&self) -> f64 {
        self.texture_scale
    }

    pub fn force_power_of_two(&self) -> bool {
        self.force_power_of_two
    }

    pub fn sample_mode(&self) -> PostProcessStageSampleMode {
        self.sample_mode
    }

    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    pub fn pixel_datatype(&self) -> PixelDatatype {
        self.pixel_datatype
    }

    pub fn clear_color(&self) -> Color {
        self.clear_color
    }

    pub fn scissor_rectangle(&self) -> Option<BoundingRectangle> {
        self.scissor_rectangle
    }

    pub fn set_scissor_rectangle(&mut self, rect: Option<BoundingRectangle>) {
        self.scissor_rectangle = rect;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled && !enabled {
            self.release_resources();
        }
        self.enabled = enabled;
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn selected(&self) -> Option<&[SelectedFeature]> {
        self.selected.as_deref()
    }

    pub fn set_selected(&mut self, selected: Option<Vec<SelectedFeature>>) {
        self.selected = selected;
        self.selected_dirty = true;
    }

    pub fn parent_selected(&self) -> Option<&[SelectedFeature]> {
        self.parent_selected.as_deref()
    }

    pub fn set_parent_selected(&mut self, parent_selected: Option<Vec<SelectedFeature>>) {
        self.parent_selected = parent_selected;
        self.selected_dirty = true;
    }

    pub fn uniforms(&self) -> &HashMap<String, PostProcessUniformValue<G>> {
        &self.uniforms
    }

    pub fn uniforms_mut(&mut self) -> &mut HashMap<String, PostProcessUniformValue<G>> {
        &mut self.uniforms
    }

    pub fn set_uniform(&mut self, name: impl Into<String>, value: impl Into<PostProcessUniformValue<G>>) {
        self.uniforms.insert(name.into(), value.into());
    }

    pub fn get_uniform(&self, name: &str) -> Option<&PostProcessUniformValue<G>> {
        self.uniforms.get(name)
    }

    pub fn output_texture(&self) -> Option<Rc<Texture<G>>> {
        self.output_texture.clone()
    }

    pub fn framebuffer(&self) -> Option<Rc<Framebuffer<G>>> {
        self.framebuffer.clone()
    }

    pub fn set_framebuffer(&mut self, fb: Option<Rc<Framebuffer<G>>>, output_texture: Option<Rc<Texture<G>>>) {
        self.framebuffer = fb;
        self.output_texture = output_texture;
    }

    pub fn is_supported(&self, context: &Context<G>) -> bool {
        let requires_depth = self.fragment_shader.contains("depthTexture");
        if requires_depth {
            context.capabilities().map(|c| c.depth_texture).unwrap_or(false)
        } else {
            true
        }
    }

    fn release_resources(&mut self) {
        if let Some(cmd) = self.command.take() {
            let _ = cmd.shader_program.destroy();
        }
        if let Some(tex) = self.selected_id_texture.take() {
            let _ = tex.destroy();
        }
        self.ready = false;
    }

    fn flatten_selected(&self) -> Vec<Color> {
        let mut list = Vec::new();
        if let Some(sel) = &self.selected {
            for f in sel {
                match f {
                    SelectedFeature::Color(c) => list.push(*c),
                    SelectedFeature::Colors(cs) => list.extend_from_slice(cs),
                }
            }
        }
        if let Some(parent) = &self.parent_selected {
            for f in parent {
                match f {
                    SelectedFeature::Color(c) => list.push(*c),
                    SelectedFeature::Colors(cs) => list.extend_from_slice(cs),
                }
            }
        }
        list
    }

    fn update_selected_texture(&mut self, context: &Context<G>) -> Result<(), RuntimeError> {
        let combined = self.flatten_selected();
        let changed = match &self.combined_selected_shadow {
            None => true,
            Some(prev) => prev != &combined,
        };

        if !changed && !self.selected_dirty {
            return Ok(());
        }

        if let Some(tex) = self.selected_id_texture.take() {
            let _ = tex.destroy();
        }

        if combined.is_empty() {
            let empty = [255u8, 255, 255, 255];
            let mut opts = TextureOptions::rgba(1, 1);
            opts.sampler = Sampler::NEAREST;
            let tex = Texture::new(context, opts, Some(&empty))?;
            self.selected_id_texture = Some(Rc::new(tex));
        } else {
            let count = combined.len();
            let mut bytes = Vec::with_capacity(count * 4);
            for c in &combined {
                bytes.push((c.red.clamp(0.0, 1.0) * 255.0) as u8);
                bytes.push((c.green.clamp(0.0, 1.0) * 255.0) as u8);
                bytes.push((c.blue.clamp(0.0, 1.0) * 255.0) as u8);
                bytes.push((c.alpha.clamp(0.0, 1.0) * 255.0) as u8);
            }
            let mut opts = TextureOptions::rgba(count as u32, 1);
            opts.sampler = Sampler::NEAREST;
            let tex = Texture::new(context, opts, Some(&bytes))?;
            self.selected_id_texture = Some(Rc::new(tex));
        }

        self.combined_selected_shadow = Some(combined);
        self.selected_dirty = false;
        Ok(())
    }

    pub fn update(&mut self, context: &Context<G>, use_log_depth: bool) -> Result<(), RuntimeError> {
        if !self.enabled {
            self.release_resources();
            return Ok(());
        }

        self.log_depth_changed = self.use_log_depth != use_log_depth;
        self.use_log_depth = use_log_depth;

        let prev_dirty = self.selected_dirty;
        self.update_selected_texture(context)?;

        let needs_cmd = self.command.is_none() || self.log_depth_changed || prev_dirty;
        if needs_cmd {
            let mut fs_source = self.fragment_shader.clone();
            if let Some(sel) = &self.selected_id_texture {
                if self.selected.is_some() || self.parent_selected.is_some() {
                    fs_source = add_selected_id_to_shader(&fs_source, sel.width() as usize);
                }
            }

            let mut shader_source = ShaderSource::new(&fs_source);
            if self.use_log_depth {
                shader_source.defines.push("LOG_DEPTH".into());
            }

            let cmd = context.create_viewport_quad_command(shader_source, ViewportCommandOptions::default())?;
            self.command = Some(cmd);
        }

        let min_mag = match self.sample_mode {
            PostProcessStageSampleMode::Linear => {
                (TextureMinificationFilter::Linear, TextureMagnificationFilter::Linear)
            }
            PostProcessStageSampleMode::Nearest => {
                (TextureMinificationFilter::Nearest, TextureMagnificationFilter::Nearest)
            }
        };
        self.sampler = Some(Sampler {
            wrap_s: TextureWrap::ClampToEdge,
            wrap_t: TextureWrap::ClampToEdge,
            minification_filter: min_mag.0,
            magnification_filter: min_mag.1,
            ..Default::default()
        });

        if let Some(cmd) = self.command.as_mut() {
            cmd.options.framebuffer = self.framebuffer.clone();
            if let Some(out_tex) = &self.output_texture {
                let db_size = context.drawing_buffer_size().unwrap_or([1, 1]);
                if out_tex.width() != db_size[0] || out_tex.height() != db_size[1] {
                    let viewport = Viewport {
                        x: 0,
                        y: 0,
                        width: out_tex.width(),
                        height: out_tex.height(),
                    };
                    cmd.options.render_state.viewport = Some(viewport);
                }
            }
        }

        self.ready = true;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    pub fn execute(
        &mut self,
        context: &Context<G>,
        color_texture: &Rc<Texture<G>>,
        depth_texture: Option<&Rc<Texture<G>>>,
        id_texture: Option<&Rc<Texture<G>>>,
        stage_texture_resolver: Option<&dyn Fn(&str) -> Option<Rc<Texture<G>>>>,
    ) -> Result<(), RuntimeError> {
        if !self.enabled || !self.ready || self.command.is_none() || self.framebuffer.is_none() {
            return Ok(());
        }

        let cmd = self.command.as_mut().expect("checked above");
        cmd.options.framebuffer = self.framebuffer.clone();

        cmd.textures.clear();
        cmd.uniforms.clear();

        // 1. colorTexture
        cmd.textures.push(("colorTexture".into(), TextureBinding::Texture(color_texture.clone())));
        cmd.uniforms.push((
            "colorTextureDimensions".into(),
            UniformValue::Float(vec![color_texture.width() as f32, color_texture.height() as f32]),
        ));

        // 2. depthTexture
        if let Some(dt) = depth_texture {
            cmd.textures.push(("depthTexture".into(), TextureBinding::Texture(dt.clone())));
            cmd.uniforms.push((
                "depthTextureDimensions".into(),
                UniformValue::Float(vec![dt.width() as f32, dt.height() as f32]),
            ));
        }

        // 3. czm_idTexture
        if let Some(idt) = id_texture {
            cmd.textures.push(("czm_idTexture".into(), TextureBinding::Texture(idt.clone())));
        }

        // 4. czm_selectedIdTexture
        if let Some(sel) = &self.selected_id_texture {
            cmd.textures.push(("czm_selectedIdTexture".into(), TextureBinding::Texture(sel.clone())));
            cmd.uniforms.push((
                "czm_selectedIdTextureStep".into(),
                UniformValue::Float(vec![1.0 / (sel.width() as f32)]),
            ));
        }

        // 5. Custom uniforms
        for (name, val) in &self.uniforms {
            Self::resolve_and_push_uniform(name, val, cmd, stage_texture_resolver);
        }

        // Scissor test
        let mut pass_state = PassState::default();
        if let Some(rect) = self.scissor_rectangle {
            if rect.width > 0.0 && rect.height > 0.0 {
                let vp = Viewport {
                    x: rect.x as i32,
                    y: rect.y as i32,
                    width: rect.width as u32,
                    height: rect.height as u32,
                };
                pass_state.scissor_test = Some(ScissorTest {
                    enabled: true,
                    rectangle: vp,
                });
                cmd.options.render_state.scissor_test = ScissorTest {
                    enabled: true,
                    rectangle: vp,
                };
            }
        }

        cmd.execute(context, &pass_state)?;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn resolve_and_push_uniform(
        name: &str,
        value: &PostProcessUniformValue<G>,
        cmd: &mut ViewportCommand<G>,
        resolver: Option<&dyn Fn(&str) -> Option<Rc<Texture<G>>>>,
    ) {
        match value {
            PostProcessUniformValue::Float(f) => {
                cmd.uniforms.push((name.to_string(), UniformValue::Float(vec![*f])));
            }
            PostProcessUniformValue::Int(i) => {
                cmd.uniforms.push((name.to_string(), UniformValue::Int(vec![*i])));
            }
            PostProcessUniformValue::Bool(b) => {
                cmd.uniforms.push((name.to_string(), UniformValue::Int(vec![if *b { 1 } else { 0 }])));
            }
            PostProcessUniformValue::Vec2(v) => {
                cmd.uniforms.push((name.to_string(), UniformValue::Float(v.to_vec())));
            }
            PostProcessUniformValue::Vec3(v) => {
                cmd.uniforms.push((name.to_string(), UniformValue::Float(v.to_vec())));
            }
            PostProcessUniformValue::Vec4(v) => {
                cmd.uniforms.push((name.to_string(), UniformValue::Float(v.to_vec())));
            }
            PostProcessUniformValue::Mat3(m) => {
                cmd.uniforms.push((
                    name.to_string(),
                    UniformValue::Matrix {
                        dimension: 3,
                        values: m.to_vec(),
                    },
                ));
            }
            PostProcessUniformValue::Mat4(m) => {
                cmd.uniforms.push((
                    name.to_string(),
                    UniformValue::Matrix {
                        dimension: 4,
                        values: m.to_vec(),
                    },
                ));
            }
            PostProcessUniformValue::Texture(tex) => {
                cmd.textures.push((name.to_string(), TextureBinding::Texture(tex.clone())));
                cmd.uniforms.push((
                    format!("{name}Dimensions"),
                    UniformValue::Float(vec![tex.width() as f32, tex.height() as f32]),
                ));
            }
            PostProcessUniformValue::StageName(target_stage) => {
                if let Some(res) = resolver {
                    if let Some(tex) = res(target_stage) {
                        cmd.textures.push((name.to_string(), TextureBinding::Texture(tex.clone())));
                        cmd.uniforms.push((
                            format!("{name}Dimensions"),
                            UniformValue::Float(vec![tex.width() as f32, tex.height() as f32]),
                        ));
                    }
                }
            }
            PostProcessUniformValue::Dynamic(f) => {
                if let Some(dyn_val) = f() {
                    Self::resolve_and_push_uniform(name, &dyn_val, cmd, resolver);
                }
            }
        }
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.release_resources();
        self.destroyed = true;
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
