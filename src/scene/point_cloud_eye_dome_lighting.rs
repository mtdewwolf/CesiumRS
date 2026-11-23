//! CesiumJS `PointCloudEyeDomeLighting`: eye-dome lighting post-process for
//! point clouds. The CPU state, framebuffer and shader derivation are ported;
//! hijacking already-built scene commands for the offscreen pass is limited by
//! this port's owned [`SceneCommand`](super::SceneCommand) closure model.
use super::{
    blending_state, stencil_constants, PointCloudShading,
};
use crate::{
    core::{Color, RuntimeError},
    renderer::{
        Context, FramebufferManager, FramebufferManagerOptions, Gl, Pass, RenderState, ShaderProgram,
        ShaderProgramOptions, ShaderSource, Texture, UniformValue,
    },
    shaders,
};
use std::rc::Rc;
type Result<T> = std::result::Result<T, RuntimeError>;

/// The offscreen color and depth-of-fragment targets and the blend command.
pub struct PointCloudEyeDomeLighting<G: Gl> {
    framebuffer: FramebufferManager<G>,
    draw_command: Option<crate::renderer::ViewportCommand<G>>,
    clear_command: Option<crate::renderer::ClearCommand<G>>,
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

    pub fn framebuffer(&self) -> Option<&Rc<crate::renderer::Framebuffer<G>>> {
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
    pub fn draw_command(&self) -> Option<&crate::renderer::ViewportCommand<G>> {
        self.draw_command.as_ref()
    }
    pub fn clear_command(&self) -> Option<&crate::renderer::ClearCommand<G>> {
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
        let clear = Color::new(0.0, 0.0, 0.0, 0.0);
        let _ = clear;
        self.draw_command = Some(context.create_viewport_quad_command(
            fragment,
            crate::renderer::ViewportCommandOptions {
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
                ..Default::default()
            },
        )?);
        self.clear_command = Some(crate::renderer::ClearCommand {
            color: Some([0.0, 0.0, 0.0, 0.0]),
            depth: Some(1.0),
            ..Default::default()
        });
        Ok(())
    }

    /// Creates the framebuffer and commands for this frame. `command_start` and
    /// the shading are accepted for upstream parity; command hijacking is not
    /// performed because SceneCommand owns its execution closure.
    pub fn update(
        &mut self,
        frame: &mut super::FrameState,
        command_start: usize,
        point_cloud_shading: &PointCloudShading,
        _bounding_volume: Option<crate::core::BoundingSphere>,
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
        self.framebuffer.update(
            &context,
            frame.drawing_buffer_width,
            frame.drawing_buffer_height,
            None,
            None,
            None,
        )?;
        self.create_commands(&context)?;
        let _ = command_start;
        Ok(())
    }

    /// Returns the blend command's uniforms for a draw.
    pub fn uniforms(&self) -> Vec<(String, UniformValue)> {
        vec![(
            "u_distanceAndEdlStrength".into(),
            UniformValue::Float(vec![self.radius as f32, self.strength as f32]),
        )]
    }

    /// Derives the MRT shader that writes packed depth alongside the color.
    pub fn get_ec_shader_program(
        &self,
        context: &Context<G>,
        shader_program: &ShaderProgram<G>,
    ) -> Result<Rc<ShaderProgram<G>>> {
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
            "void main() \n\
             { \n\
                 czm_point_cloud_post_process_main(); \n\
             #ifdef LOG_DEPTH\n\
                 czm_writeLogDepth();\n\
                 out_FragData_1 = czm_packDepth(gl_FragDepth); \n\
             #else\n\
                 out_FragData_1 = czm_packDepth(gl_FragCoord.z);\n\
             #endif\n\
             }"
            .into(),
        );
        let derived = ShaderProgramOptions {
            vertex_shader_source: options.vertex_shader_source.clone(),
            fragment_shader_source: fragment,
            attribute_locations: options.attribute_locations.clone(),
        };
        Ok(Rc::new(ShaderProgram::from_sources(context, &derived)?))
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.framebuffer.destroy()?;
        self.draw_command = None;
        self.clear_command = None;
        self.destroyed = true;
        Ok(())
    }
}
