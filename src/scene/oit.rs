//! CesiumJS `OIT`: weighted blended transparency, MRT and two-pass fallback.
use super::framebuffer_helpers::*;
use crate::{
    core::{webgl_constants as gl, Color, PixelFormat, RuntimeError},
    renderer::{render_state::Blending, *},
    shaders::root::{ADJUST_TRANSLUCENT_FS, COMPOSITE_OITFS},
};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OitDrawKind {
    Mrt,
    Color,
    Alpha,
}
impl OitDrawKind {
    /// The pinned OIT transform suppresses ShaderSource's automatic output
    /// declaration. MRT supplies its own declarations, while the two-pass
    /// variants omit theirs. Complete that declaration for actual rendering.
    pub fn shader_for_rendering(self, base: &ShaderProgramOptions) -> ShaderProgramOptions {
        let mut result = self.shader(base);
        if self != Self::Mrt {
            result.fragment_shader_source.sources.insert(0, "layout (location = 0) out vec4 out_FragColor;\n".into());
        }
        result
    }
    pub fn render_state(self, base: &RenderState) -> RenderState {
        let mut state = base.clone();
        state.depth_mask = false;
        state.blending = Blending {
            enabled: true,
            equation_rgb: gl::FUNC_ADD,
            equation_alpha: gl::FUNC_ADD,
            function_source_rgb: if self == Self::Alpha { gl::ZERO } else { gl::ONE },
            function_destination_rgb: if self == Self::Alpha { gl::ONE_MINUS_SRC_ALPHA } else { gl::ONE },
            function_source_alpha: if self == Self::Color { gl::ONE } else { gl::ZERO },
            function_destination_alpha: if self == Self::Color { gl::ONE } else { gl::ONE_MINUS_SRC_ALPHA },
            ..Default::default()
        };
        state
    }
    pub fn shader(self, base: &ShaderProgramOptions) -> ShaderProgramOptions {
        let mut result = base.clone();
        let fs = &mut result.fragment_shader_source;
        let layout =
            regex::Regex::new(r"layout\s*\(location\s*=\s*0\)\s*out\s+vec4\s+out_FragColor;").expect("constant regex");
        let discard = regex::Regex::new(r"\bdiscard\b").expect("constant regex");
        fs.sources = fs
            .sources
            .iter()
            .map(|s| {
                let renamed =
                    ShaderSource::replace_main(s, "czm_translucent_main").replace("out_FragColor", "czm_out_FragColor");
                discard
                    .replace_all(&layout.replace_all(&renamed, ""), "czm_discard = true")
                    .replace("czm_phong", "czm_translucentPhong")
            })
            .collect();
        fs.sources.insert(
            0,
            "#define CESIUM_REDIRECTED_COLOR_OUTPUT\nvec4 czm_out_FragColor;\nbool czm_discard = false;\n".into(),
        );
        fs.sources.push(
            if self == Self::Mrt {
                "layout (location = 1) out vec4 out_FragData_1;\nlayout (location = 0) out vec4 out_FragData_0;\n"
            } else {
                ""
            }
            .into(),
        );
        let output=match self{
            Self::Mrt=>"    vec3 Ci = czm_out_FragColor.rgb * czm_out_FragColor.a;\n    float ai = czm_out_FragColor.a;\n    float wzi = czm_alphaWeight(ai);\n    out_FragData_0 = vec4(Ci * wzi, ai);\n    out_FragData_1 = vec4(ai * wzi);\n",
            Self::Color=>"    vec3 Ci = czm_out_FragColor.rgb * czm_out_FragColor.a;\n    float ai = czm_out_FragColor.a;\n    float wzi = czm_alphaWeight(ai);\n    out_FragColor = vec4(Ci, ai) * wzi;\n",
            Self::Alpha=>"    float ai = czm_out_FragColor.a;\n    out_FragColor = vec4(ai);\n",
        };
        fs.sources.push(format!("void main()\n{{\n    czm_translucent_main();\n    if (czm_discard)\n    {{\n        discard;\n    }}\n{output}}}\n"));
        result
    }
}

pub struct Oit<G: Gl> {
    mrt: bool,
    multipass: bool,
    opaque: Option<Rc<Framebuffer<G>>>,
    opaque_texture: Option<Rc<Texture<G>>>,
    depth: Option<Rc<Texture<G>>>,
    accumulation: Option<Rc<Texture<G>>>,
    revealage: Option<Rc<Texture<G>>>,
    translucent: FramebufferManager<G>,
    alpha: FramebufferManager<G>,
    adjust_translucent: FramebufferManager<G>,
    adjust_alpha: FramebufferManager<G>,
    composite: Option<ViewportCommand<G>>,
    adjust_command: Option<ViewportCommand<G>>,
    adjust_alpha_command: Option<ViewportCommand<G>>,
    state: RenderState,
    use_hdr: bool,
    num_samples: u32,
    destroyed: bool,
}
impl<G: Gl> Oit<G> {
    pub fn new(context: &Context<G>) -> Result<Self> {
        let caps = context.capabilities()?;
        Self::with_capabilities(caps)
    }
    pub fn with_capabilities(caps: ContextCapabilities) -> Result<Self> {
        let supported = caps.color_buffer_float && caps.depth_texture && caps.float_blend;
        let mrt = caps.draw_buffers && supported;
        let target = |count: usize, depth: bool| {
            FramebufferManager::new(FramebufferManagerOptions {
                color_attachments_length: count,
                create_color_attachments: false,
                create_depth_attachments: !depth,
                depth,
                ..Default::default()
            })
        };
        Ok(Self {
            mrt,
            multipass: !mrt && supported,
            opaque: None,
            opaque_texture: None,
            depth: None,
            accumulation: None,
            revealage: None,
            translucent: target(if mrt { 2 } else { 1 }, true)?,
            alpha: target(1, true)?,
            adjust_translucent: target(if mrt { 2 } else { 1 }, false)?,
            adjust_alpha: target(1, false)?,
            composite: None,
            adjust_command: None,
            adjust_alpha_command: None,
            state: RenderState::default(),
            use_hdr: false,
            num_samples: 1,
            destroyed: false,
        })
    }
    pub fn is_supported(&self) -> bool {
        self.mrt || self.multipass
    }
    pub fn uses_mrt(&self) -> bool {
        self.mrt
    }
    pub fn draw_kind(&self, phase: u32) -> OitDrawKind {
        if self.mrt {
            OitDrawKind::Mrt
        } else if phase == 0 {
            OitDrawKind::Color
        } else {
            OitDrawKind::Alpha
        }
    }
    pub fn framebuffer(&self, phase: u32) -> Option<&Rc<Framebuffer<G>>> {
        if self.mrt || phase == 0 {
            self.translucent.framebuffer()
        } else {
            self.alpha.framebuffer()
        }
    }
    fn destroy_framebuffers(&mut self) -> Result<()> {
        self.translucent.destroy()?;
        self.alpha.destroy()?;
        self.adjust_translucent.destroy()?;
        self.adjust_alpha.destroy()?;
        Ok(())
    }
    fn destroy_textures(&mut self) -> Result<()> {
        for texture in [&mut self.accumulation, &mut self.revealage] {
            if let Some(t) = texture.take() {
                t.destroy()?;
            }
        }
        Ok(())
    }
    fn update_framebuffers(&mut self, context: &Context<G>, width: u32, height: u32) -> Result<bool> {
        self.destroy_framebuffers()?;
        let accumulation = self.accumulation.as_ref().expect("allocated").clone();
        let revealage = self.revealage.as_ref().expect("allocated").clone();
        let depth = self.depth.clone();
        self.translucent.set_color_texture(0, Some(accumulation.clone()))?;
        self.translucent.set_depth_stencil_texture(depth.clone())?;
        self.adjust_translucent.set_color_texture(0, Some(accumulation))?;
        if self.mrt {
            self.translucent.set_color_texture(1, Some(revealage.clone()))?;
            self.adjust_translucent.set_color_texture(1, Some(revealage.clone()))?;
            self.translucent.update(context, width, height, None, None, None)?;
            self.adjust_translucent.update(context, width, height, None, None, None)?;
            if self.translucent.status()? != gl::FRAMEBUFFER_COMPLETE
                || self.adjust_translucent.status()? != gl::FRAMEBUFFER_COMPLETE
            {
                self.destroy_framebuffers()?;
                self.mrt = false;
                self.translucent = FramebufferManager::new(FramebufferManagerOptions {
                    create_color_attachments: false,
                    create_depth_attachments: false,
                    depth: true,
                    ..Default::default()
                })?;
                self.adjust_translucent = FramebufferManager::new(FramebufferManagerOptions {
                    create_color_attachments: false,
                    ..Default::default()
                })?;
                self.translucent.set_color_texture(0, self.accumulation.clone())?;
                self.translucent.set_depth_stencil_texture(depth.clone())?;
                self.adjust_translucent.set_color_texture(0, self.accumulation.clone())?;
            }
        }
        if !self.mrt {
            self.alpha.set_color_texture(0, Some(revealage.clone()))?;
            self.alpha.set_depth_stencil_texture(depth)?;
            self.adjust_alpha.set_color_texture(0, Some(revealage))?;
            for target in [&mut self.translucent, &mut self.alpha, &mut self.adjust_translucent, &mut self.adjust_alpha]
            {
                target.update(context, width, height, None, None, None)?;
            }
            if [&self.translucent, &self.alpha, &self.adjust_translucent, &self.adjust_alpha]
                .into_iter()
                .any(|t| t.status().is_ok_and(|status| status != gl::FRAMEBUFFER_COMPLETE))
            {
                self.destroy_textures()?;
                self.destroy_framebuffers()?;
                self.multipass = false;
                return Ok(false);
            }
            self.multipass = true;
        }
        Ok(true)
    }
    pub fn update(
        &mut self,
        context: &Context<G>,
        pass: &PassState<G>,
        opaque: &FramebufferManager<G>,
        hdr: bool,
        num_samples: u32,
    ) -> Result<()> {
        alive(self.destroyed)?;
        if !self.is_supported() {
            return Ok(());
        }
        self.opaque = opaque.framebuffer().cloned();
        self.opaque_texture = Some(color(opaque, 0)?);
        self.depth = Some(depth(opaque)?);
        let opaque_texture = self.opaque_texture.as_ref().expect("assigned");
        let (width, height) = (opaque_texture.width(), opaque_texture.height());
        let changed = self.accumulation.as_ref().is_none_or(|t| t.width() != width || t.height() != height)
            || self.use_hdr != hdr
            || self.num_samples != num_samples;
        if changed {
            self.destroy_textures()?;
            let mut options = TextureOptions::rgba(width, height);
            options.pixel_format = PixelFormat::Rgba;
            options.pixel_datatype = PixelDatatype::Float;
            self.accumulation = Some(Rc::new(Texture::new(context, options.clone(), None)?));
            options.flip_y = false;
            self.revealage = Some(Rc::new(Texture::new(context, options, None)?));
            self.num_samples = num_samples;
        }
        if (self.translucent.framebuffer().is_none() || changed) && !self.update_framebuffers(context, width, height)? {
            return Ok(());
        }
        self.use_hdr = hdr;
        self.state = copy_state(width, height, pass);
        if self.composite.is_none() {
            let mut source = ShaderSource::new(COMPOSITE_OITFS.source);
            if self.mrt {
                source.defines.push("MRT".into());
            }
            self.composite = Some(context.create_viewport_quad_command(source, ViewportCommandOptions::default())?);
        }
        if self.adjust_command.is_none() {
            let mut source = ShaderSource::new(ADJUST_TRANSLUCENT_FS.source);
            if self.mrt {
                source.defines.push("MRT".into());
            }
            self.adjust_command =
                Some(context.create_viewport_quad_command(source, ViewportCommandOptions::default())?);
            if !self.mrt {
                self.adjust_alpha_command = Some(context.create_viewport_quad_command(
                    ShaderSource::new(ADJUST_TRANSLUCENT_FS.source),
                    ViewportCommandOptions::default(),
                )?);
            }
        }
        Ok(())
    }
    pub fn clear(&self, context: &Context<G>, pass: &PassState<G>, color: Color) -> Result<()> {
        alive(self.destroyed)?;
        if !self.is_supported() {
            return Ok(());
        }
        ClearCommand {
            framebuffer: self.opaque.clone(),
            color: Some([color.red, color.green, color.blue, color.alpha]),
            ..Default::default()
        }
        .execute(context, pass)?;
        self.translucent.clear(
            context,
            &ClearCommand { color: Some(if self.mrt { [0., 0., 0., 1.] } else { [0.; 4] }), ..Default::default() },
            pass,
        )?;
        if self.multipass {
            self.alpha.clear(context, &ClearCommand { color: Some([1.; 4]), ..Default::default() }, pass)?;
        }
        Ok(())
    }
    pub fn adjust(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        alive(self.destroyed)?;
        let depth = self.depth.clone().ok_or_else(|| RuntimeError::new("OIT.update must be called before drawing"))?;
        if let Some(command) = &mut self.adjust_command {
            command.options.render_state = self.state.clone();
            command.options.framebuffer = self.adjust_translucent.framebuffer().cloned();
            command.textures = vec![("u_depthTexture".into(), TextureBinding::Texture(depth.clone()))];
            command.uniforms = vec![(
                "u_bgColor".into(),
                UniformValue::Float(if self.mrt { vec![0., 0., 0., 1.] } else { vec![0.; 4] }),
            )];
            command.execute(context, pass)?;
        }
        if let Some(command) = &mut self.adjust_alpha_command {
            command.options.render_state = self.state.clone();
            command.options.framebuffer = self.adjust_alpha.framebuffer().cloned();
            command.textures = vec![("u_depthTexture".into(), TextureBinding::Texture(depth))];
            command.uniforms = vec![("u_bgColor".into(), UniformValue::Float(vec![1.; 4]))];
            command.execute(context, pass)?;
        }
        Ok(())
    }
    pub fn execute(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        alive(self.destroyed)?;
        let command =
            self.composite.as_mut().ok_or_else(|| RuntimeError::new("OIT.update must be called before compositing"))?;
        command.options.render_state = self.state.clone();
        command.textures = vec![
            ("u_opaque".into(), TextureBinding::Texture(self.opaque_texture.as_ref().expect("updated").clone())),
            ("u_accumulation".into(), TextureBinding::Texture(self.accumulation.as_ref().expect("updated").clone())),
            ("u_revealage".into(), TextureBinding::Texture(self.revealage.as_ref().expect("updated").clone())),
        ];
        command.execute(context, pass)
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.destroy_textures()?;
            self.destroy_framebuffers()?;
            destroy_command(&mut self.composite)?;
            destroy_command(&mut self.adjust_command)?;
            destroy_command(&mut self.adjust_alpha_command)?;
            self.opaque = None;
            self.opaque_texture = None;
            self.depth = None;
            self.destroyed = true;
        }
        Ok(())
    }
}
