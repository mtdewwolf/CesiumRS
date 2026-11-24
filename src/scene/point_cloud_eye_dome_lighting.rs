//! CesiumJS `PointCloudEyeDomeLighting`: eye-dome lighting post-process for
//! point clouds, hijacking point draw commands into a 2-color+depth MRT
//! framebuffer and blending the shaded result into the main framebuffer.
use super::{
    blending_state, stencil_constants, PointCloudShading, SceneCommand, SceneDrawOptions,
};
use crate::{
    core::{BoundingSphere, PrimitiveType, RuntimeError},
    renderer::{
        ClearCommand, Context, DrawMetadata, Framebuffer, FramebufferManager, FramebufferManagerOptions, Gl, Pass,
        RenderState, SampledTexture, ShaderProgram, ShaderProgramOptions, ShaderSource, Texture, TextureBinding,
        UniformValue, ViewportCommand, ViewportCommandOptions,
    },
    shaders,
};
use std::rc::Rc;
type Result<T> = std::result::Result<T, RuntimeError>;

/// The offscreen color and depth-of-fragment targets and the blend/clear commands.
pub struct PointCloudEyeDomeLighting<G: Gl> {
    framebuffer: FramebufferManager<G>,
    draw_command: Option<ViewportCommand<G>>,
    clear_command: Option<ClearCommand<G>>,
    strength: f64,
    radius: f64,
    destroyed: bool,
}

impl<G: Gl + 'static> Default for PointCloudEyeDomeLighting<G> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: Gl + 'static> PointCloudEyeDomeLighting<G> {
    pub fn new() -> Self {
        Self {
            framebuffer: FramebufferManager::new(FramebufferManagerOptions {
                color_attachments_length: 2,
                depth: true,
                supports_depth_texture: true,
                ..Default::default()
            })
            .expect("valid eye dome lighting framebuffer options"),
            draw_command: None,
            clear_command: None,
            strength: 1.0,
            radius: 1.0,
            destroyed: false,
        }
    }

    pub fn framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.framebuffer.framebuffer()
    }
    pub fn color_g_buffer(&self) -> Option<&Rc<Texture<G>>> {
        self.framebuffer.color_texture(0).ok().flatten()
    }
    pub fn depth_g_buffer(&self) -> Option<&Rc<Texture<G>>> {
        self.framebuffer.color_texture(1).ok().flatten()
    }
    pub fn strength(&self) -> f64 {
        self.strength
    }
    pub fn radius(&self) -> f64 {
        self.radius
    }
    pub fn draw_command(&self) -> Option<&ViewportCommand<G>> {
        self.draw_command.as_ref()
    }
    pub fn clear_command(&self) -> Option<&ClearCommand<G>> {
        self.clear_command.as_ref()
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// `PointCloudEyeDomeLighting.isSupported`.
    pub fn is_supported(context: &Context<G>) -> bool {
        context
            .capabilities()
            .map(|capabilities| capabilities.draw_buffers && capabilities.fragment_depth)
            .unwrap_or(false)
    }

    fn create_commands(&mut self, context: &Context<G>) -> Result<()> {
        let fragment = ShaderSource {
            defines: vec!["LOG_DEPTH_WRITE".into()],
            sources: vec![shaders::post_process_stages::POINT_CLOUD_EYE_DOME_LIGHTING.source.into()],
            ..Default::default()
        };
        let mut draw_command = context.create_viewport_quad_command(
            fragment,
            ViewportCommandOptions {
                render_state: RenderState {
                    blending: blending_state::alpha_blend(),
                    depth_mask: true,
                    depth_test: crate::renderer::render_state::DepthTest {
                        enabled: true,
                        ..Default::default()
                    },
                    stencil_test: stencil_constants::set_cesium_3d_tile_bit(),
                    stencil_mask: stencil_constants::CESIUM_3D_TILE_MASK as i64,
                    ..Default::default()
                },
                pass: Some(Pass::Cesium3DTile),
                owner: Some("PointCloudEyeDomeLighting".into()),
                ..Default::default()
            },
        )?;
        draw_command.uniforms = self.uniforms();
        if let Some(color_g_buffer) = self.color_g_buffer().cloned() {
            draw_command
                .textures
                .push(("u_pointCloud_colorGBuffer".into(), TextureBinding::Texture(color_g_buffer)));
        }
        if let Some(depth_g_buffer) = self.depth_g_buffer().cloned() {
            draw_command
                .textures
                .push(("u_pointCloud_depthGBuffer".into(), TextureBinding::Texture(depth_g_buffer)));
        }
        self.draw_command = Some(draw_command);
        self.clear_command = Some(ClearCommand {
            framebuffer: self.framebuffer.framebuffer().cloned(),
            color: Some([0.0, 0.0, 0.0, 0.0]),
            depth: Some(1.0),
            pass: Some(Pass::Cesium3DTile),
            owner: Some("PointCloudEyeDomeLighting".into()),
            ..Default::default()
        });
        Ok(())
    }

    /// `PointCloudEyeDomeLighting.prototype.update`: creates/updates the 2-color+depth
    /// MRT framebuffer, replaces point draw commands in `frame.command_list[command_start..]`
    /// with derived EC MRT commands, and appends the EDL blend and clear commands.
    pub fn update(
        &mut self,
        frame: &mut super::FrameState,
        command_start: usize,
        point_cloud_shading: &PointCloudShading,
        bounding_volume: Option<BoundingSphere>,
    ) -> Result<()> {
        let context = frame
            .context
            .as_ref()
            .and_then(|resource| resource.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("Eye dome lighting requires the frame's renderer context"))?;
        if !Self::is_supported(&context) {
            return Ok(());
        }
        self.strength = point_cloud_shading.eye_dome_lighting_strength;
        self.radius = point_cloud_shading.eye_dome_lighting_radius * frame.pixel_ratio;
        let (width, height) = if frame.drawing_buffer_width > 0 && frame.drawing_buffer_height > 0 {
            (frame.drawing_buffer_width, frame.drawing_buffer_height)
        } else {
            let size = context.drawing_buffer_size()?;
            (size[0], size[1])
        };
        self.framebuffer.update(&context, width, height, None, None, None)?;
        self.create_commands(&context)?;

        let framebuffer = self
            .framebuffer()
            .cloned()
            .ok_or_else(|| RuntimeError::new("Eye dome lighting framebuffer was not created"))?;

        // Hijack existing opaque point commands to render into the offscreen MRT FBO.
        let command_end = frame.command_list.len();
        for command in &mut frame.command_list[command_start.min(command_end)..command_end] {
            if command.primitive_type != Some(PrimitiveType::Points) || command.pass == Pass::Translucent {
                continue;
            }
            let Some(original_program) = command.draw_options::<G>().map(|opts| Rc::clone(&opts.program)) else {
                continue;
            };
            let ec_program = self.get_ec_shader_program(&context, &original_program)?;
            if let Some(derived) = command.derive_draw(context.clone(), |opts, meta| {
                opts.framebuffer = Some(Rc::clone(&framebuffer));
                opts.program = ec_program;
                meta.cast_shadows = false;
                meta.receive_shadows = false;
            }) {
                *command = derived;
            }
        }

        // Append EDL fullscreen blend command, then clear command for the offscreen FBO,
        // matching CesiumJS `PointCloudEyeDomeLighting.prototype.update`.
        if let Some(blend) = &self.draw_command {
            let mut draw_options = SceneDrawOptions::new(Rc::clone(&blend.vertex_array), Rc::clone(&blend.shader_program));
            draw_options.state = blend.options.render_state.clone();
            draw_options.primitive_type = PrimitiveType::Triangles;
            draw_options.uniforms = self.uniforms();
            if let Some(color_g_buffer) = self.color_g_buffer().cloned() {
                draw_options
                    .textures
                    .push(("u_pointCloud_colorGBuffer".into(), color_g_buffer as Rc<dyn SampledTexture<G>>));
            }
            if let Some(depth_g_buffer) = self.depth_g_buffer().cloned() {
                draw_options
                    .textures
                    .push(("u_pointCloud_depthGBuffer".into(), depth_g_buffer as Rc<dyn SampledTexture<G>>));
            }
            let metadata = DrawMetadata {
                bounding_volume,
                owner: Some("PointCloudEyeDomeLighting".into()),
                ..Default::default()
            };
            frame.command_list.push(SceneCommand::from_draw(
                "PointCloudEyeDomeLighting",
                context.clone(),
                Pass::Cesium3DTile,
                metadata,
                draw_options,
            ));
        }

        if let Some(clear) = self.clear_command.clone() {
            let mut clear_cmd = SceneCommand::clear("PointCloudEyeDomeLighting.clear", context, clear);
            clear_cmd.metadata.owner = Some("PointCloudEyeDomeLighting".into());
            frame.command_list.push(clear_cmd);
        }

        Ok(())
    }

    /// Returns the blend command's uniforms for a draw.
    pub fn uniforms(&self) -> Vec<(String, UniformValue)> {
        vec![(
            "u_distanceAndEdlStrength".into(),
            UniformValue::Float(vec![self.radius as f32, self.strength as f32]),
        )]
    }

    /// Builds the derived EC MRT shader source options from a base shader program.
    pub fn ec_shader_options(shader_program: &ShaderProgram<G>) -> Result<ShaderProgramOptions> {
        let options = shader_program
            .source_options()
            .ok_or_else(|| RuntimeError::new("Eye dome lighting requires ShaderProgram::from_sources"))?;
        let mut fragment = options.fragment_shader_source.clone();
        fragment.sources.insert(
            0,
            "layout (location = 0) out vec4 out_FragData_0;\nlayout (location = 1) out vec4 out_FragData_1;".into(),
        );
        for source in &mut fragment.sources {
            *source = ShaderSource::replace_main(source, "czm_point_cloud_post_process_main");
            *source = source.replace("out_FragColor", "out_FragData_0");
        }
        fragment.sources.push(
            concat!(
                "void main() \n",
                "{ \n",
                "    czm_point_cloud_post_process_main(); \n",
                "#ifdef LOG_DEPTH\n",
                "    czm_writeLogDepth();\n",
                "    out_FragData_1 = czm_packDepth(gl_FragDepth); \n",
                "#else\n",
                "    out_FragData_1 = czm_packDepth(gl_FragCoord.z);\n",
                "#endif\n",
                "}",
            )
            .into(),
        );
        Ok(ShaderProgramOptions {
            vertex_shader_source: options.vertex_shader_source.clone(),
            fragment_shader_source: fragment,
            attribute_locations: options.attribute_locations.clone(),
        })
    }

    /// Derives the MRT shader that writes packed depth alongside the color.
    /// Uses `context.shader_cache` ("EC" derived key) when the base shader is
    /// cache-managed, falling back to standalone compilation otherwise.
    pub fn get_ec_shader_program(
        &self,
        context: &Context<G>,
        shader_program: &Rc<ShaderProgram<G>>,
    ) -> Result<Rc<ShaderProgram<G>>> {
        if let Some(cached) = context.with_shader_cache(|cache| Ok(cache.get_derived_shader_program(shader_program, "EC")))? {
            return Ok(cached);
        }
        let derived = Self::ec_shader_options(shader_program)?;
        match context.with_shader_cache(|cache| cache.create_derived_shader_program(shader_program, "EC", &derived)) {
            Ok(program) => Ok(program),
            Err(_) => Ok(Rc::new(ShaderProgram::from_sources(context, &derived)?)),
        }
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.framebuffer.destroy()?;
        self.draw_command = None;
        self.clear_command = None;
        self.destroyed = true;
        Ok(())
    }
}
