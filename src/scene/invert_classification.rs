//! CesiumJS `InvertClassification`: recolors unclassified fragments and keeps
//! classified fragments unchanged. Two offscreen color targets hold the scene
//! and the classified pass; opaque and translucent shader variants select by
//! the classification stencil mask.

use super::{
    blending_state,
    scene::Result,
    stencil_constants::{set_cesium_3d_tile_bit, CESIUM_3D_TILE_MASK, CLASSIFICATION_MASK},
    StencilFunction,
};
use crate::{
    core::{Color, PixelDatatype, PixelFormat, RuntimeError},
    renderer::{
        render_state::{StencilOperation as StencilOps, StencilTest},
        ClearCommand, FramebufferManager, FramebufferManagerOptions, Gl, PassState, RenderState, Renderbuffer,
        RenderbufferFormat, RenderbufferOptions, ShaderSource, Texture, ViewportCommand, ViewportCommandOptions,
    },
    shaders::post_process_stages::PASS_THROUGH,
};
use std::rc::Rc;

const TRANSLUCENT_FS: &str = "uniform sampler2D colorTexture;\n\
uniform sampler2D depthTexture;\n\
uniform sampler2D classifiedTexture;\n\
in vec2 v_textureCoordinates;\n\
void main()\n\
{\n\
    vec4 color = texture(colorTexture, v_textureCoordinates);\n\
    if (color.a == 0.0)\n\
    {\n\
        discard;\n\
    }\n\
    bool isClassified = all(equal(texture(classifiedTexture, v_textureCoordinates), vec4(0.0)));\n\
#ifdef UNCLASSIFIED\n\
    vec4 highlightColor = czm_invertClassificationColor;\n\
    if (isClassified)\n\
    {\n\
        discard;\n\
    }\n\
#else\n\
    vec4 highlightColor = vec4(1.0);\n\
    if (!isClassified)\n\
    {\n\
        discard;\n\
    }\n\
#endif\n\
    out_FragColor = color * highlightColor;\n\
    gl_FragDepth = texture(depthTexture, v_textureCoordinates).r;\n\
}\n";

const OPAQUE_FS: &str = "uniform sampler2D colorTexture;\n\
in vec2 v_textureCoordinates;\n\
void main()\n\
{\n\
    vec4 color = texture(colorTexture, v_textureCoordinates);\n\
    if (color.a == 0.0)\n\
    {\n\
        discard;\n\
    }\n\
#ifdef UNCLASSIFIED\n\
    out_FragColor = color * czm_invertClassificationColor;\n\
#else\n\
    out_FragColor = color;\n\
#endif\n\
}\n";

/// Depth/stencil attachments borrowed from the globe or scene framebuffer.
pub struct InvertClassificationDepth<G: Gl> {
    pub depth_stencil_texture: Option<Rc<Texture<G>>>,
    pub depth_stencil_renderbuffer: Option<Rc<Renderbuffer<G>>>,
}

fn keep() -> StencilOps {
    StencilOps {
        fail: crate::core::webgl_constants::KEEP,
        z_fail: crate::core::webgl_constants::KEEP,
        z_pass: crate::core::webgl_constants::KEEP,
    }
}

fn unclassified_state() -> RenderState {
    RenderState {
        depth_mask: false,
        stencil_test: StencilTest {
            enabled: true,
            front_function: StencilFunction::Equal.value(),
            back_function: StencilFunction::Never.value(),
            reference: 0,
            mask: i64::from(CLASSIFICATION_MASK),
            front_operation: keep(),
            back_operation: keep(),
        },
        blending: blending_state::alpha_blend(),
        ..Default::default()
    }
}

fn classified_state() -> RenderState {
    let mut state = unclassified_state();
    state.stencil_test.front_function = StencilFunction::NotEqual.value();
    state
}

fn default_state() -> RenderState {
    RenderState {
        depth_mask: true,
        depth_test: crate::renderer::render_state::DepthTest { enabled: true, ..Default::default() },
        stencil_test: set_cesium_3d_tile_bit(),
        stencil_mask: i64::from(CESIUM_3D_TILE_MASK),
        blending: blending_state::alpha_blend(),
        ..Default::default()
    }
}

/// The invert-classification post-process.
pub struct InvertClassification<G: Gl> {
    num_samples: u32,
    previous_framebuffer: bool,
    previous_was_present: bool,
    depth_stencil_texture: Option<Rc<Texture<G>>>,
    depth_stencil_renderbuffer: Option<Rc<Renderbuffer<G>>>,
    fbo: FramebufferManager<G>,
    fbo_classified: FramebufferManager<G>,
    rs_unclassified: Option<RenderState>,
    rs_classified: Option<RenderState>,
    rs_default: Option<RenderState>,
    unclassified_command: Option<ViewportCommand<G>>,
    classified_command: Option<ViewportCommand<G>>,
    translucent_command: Option<ViewportCommand<G>>,
    clear_color_command: ClearCommand<G>,
    clear_command: ClearCommand<G>,
    destroyed: bool,
}

impl<G: Gl> InvertClassification<G> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            num_samples: 1,
            previous_framebuffer: false,
            previous_was_present: false,
            depth_stencil_texture: None,
            depth_stencil_renderbuffer: None,
            fbo: FramebufferManager::new(FramebufferManagerOptions {
                depth_stencil: true,
                create_depth_attachments: false,
                ..Default::default()
            })?,
            fbo_classified: FramebufferManager::new(FramebufferManagerOptions {
                depth_stencil: true,
                create_depth_attachments: false,
                ..Default::default()
            })?,
            rs_unclassified: None,
            rs_classified: None,
            rs_default: None,
            unclassified_command: None,
            classified_command: None,
            translucent_command: None,
            clear_color_command: ClearCommand {
                color: Some([0.0, 0.0, 0.0, 0.0]),
                owner: Some("InvertClassification".into()),
                ..Default::default()
            },
            clear_command: ClearCommand {
                color: Some([0.0, 0.0, 0.0, 0.0]),
                depth: Some(1.0),
                stencil: Some(0),
                ..Default::default()
            },
            destroyed: false,
        })
    }

    /// `InvertClassification.isTranslucencySupported`.
    pub fn is_translucency_supported(context: &crate::renderer::ContextCapabilities) -> bool {
        context.depth_texture && context.fragment_depth
    }

    /// The unclassified command, exposed for renderer integration.
    pub fn unclassified_command(&self) -> Option<&ViewportCommand<G>> {
        self.unclassified_command.as_ref()
    }

    pub fn classified_command(&self) -> Option<&ViewportCommand<G>> {
        self.classified_command.as_ref()
    }

    pub fn translucent_command(&self) -> Option<&ViewportCommand<G>> {
        self.translucent_command.as_ref()
    }

    /// The current color target holding the unclassified scene.
    pub fn color_texture(&self) -> Option<Rc<Texture<G>>> {
        self.fbo.color_texture(0).ok().flatten().cloned()
    }

    pub fn classified_texture(&self) -> Option<Rc<Texture<G>>> {
        self.fbo_classified.color_texture(0).ok().flatten().cloned()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// `InvertClassification.prototype.update`.
    ///
    /// `previous_framebuffer` tells Cesium whether classification ran against
    /// the scene's existing depth/stencil; `globe` supplies those attachments.
    pub fn update(
        &mut self,
        context: &crate::renderer::Context<G>,
        num_samples: u32,
        previous_framebuffer: bool,
        globe: Option<InvertClassificationDepth<G>>,
    ) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        let texture = self.color_texture();
        let previous_framebuffer_changed = previous_framebuffer != self.previous_framebuffer;
        self.previous_framebuffer = previous_framebuffer;
        let samples_changed = self.num_samples != num_samples;

        let [width, height] = context.drawing_buffer_size()?;
        let texture_changed =
            texture.as_ref().is_none_or(|texture| texture.width() != width || texture.height() != height);

        if texture_changed || previous_framebuffer_changed || samples_changed {
            self.num_samples = num_samples;
            if let Some(texture) = self.depth_stencil_texture.take() {
                let _ = texture.destroy();
            }
            if let Some(renderbuffer) = self.depth_stencil_renderbuffer.take() {
                let _ = renderbuffer.destroy();
            }
            if !previous_framebuffer {
                let mut options = crate::renderer::TextureOptions::rgba(width, height);
                options.pixel_format = PixelFormat::DepthStencil;
                options.pixel_datatype = PixelDatatype::UnsignedInt24_8;
                self.depth_stencil_texture = Some(Rc::new(Texture::new(context, options, None)?));
                if num_samples > 1 {
                    self.depth_stencil_renderbuffer = Some(Rc::new(Renderbuffer::new(
                        context,
                        RenderbufferOptions {
                            format: RenderbufferFormat::Depth24Stencil8,
                            width: Some(width),
                            height: Some(height),
                            num_samples,
                        },
                    )?));
                }
            }
        }

        if self.fbo.framebuffer().is_none() || texture_changed || previous_framebuffer_changed || samples_changed {
            self.fbo.destroy()?;
            self.fbo_classified.destroy()?;

            let (depth_texture, depth_renderbuffer) = if previous_framebuffer {
                globe.as_ref().map_or((None, None), |resources| {
                    (resources.depth_stencil_texture.clone(), resources.depth_stencil_renderbuffer.clone())
                })
            } else {
                (self.depth_stencil_texture.clone(), self.depth_stencil_renderbuffer.clone())
            };
            self.fbo.set_depth_stencil_texture(depth_texture)?;
            if let Some(renderbuffer) = depth_renderbuffer {
                self.fbo.set_depth_stencil_renderbuffer(Some(renderbuffer))?;
            }
            self.fbo.update(context, width, height, Some(num_samples), None, None)?;
            if !previous_framebuffer {
                self.fbo_classified.set_depth_stencil_texture(self.depth_stencil_texture.clone())?;
                self.fbo_classified.update(context, width, height, None, None, None)?;
            }
        }

        if self.rs_unclassified.is_none() {
            self.rs_unclassified = Some(unclassified_state());
            self.rs_classified = Some(classified_state());
            self.rs_default = Some(default_state());
        }

        if self.unclassified_command.is_none() || previous_framebuffer_changed || samples_changed {
            let source = if previous_framebuffer { OPAQUE_FS } else { TRANSLUCENT_FS };
            let unclassified = ShaderSource {
                defines: vec!["UNCLASSIFIED".into()],
                sources: vec![source.into()],
                ..Default::default()
            };
            let classified = ShaderSource::new(source);
            let render_state = if previous_framebuffer {
                self.rs_unclassified.clone().expect("initialized above")
            } else {
                self.rs_default.clone().expect("initialized above")
            };
            self.unclassified_command = Some(context.create_viewport_quad_command(
                unclassified,
                ViewportCommandOptions { render_state: render_state.clone(), ..Default::default() },
            )?);
            let classified_state = if previous_framebuffer {
                self.rs_classified.clone().expect("initialized above")
            } else {
                self.rs_default.clone().expect("initialized above")
            };
            self.classified_command = Some(context.create_viewport_quad_command(
                classified,
                ViewportCommandOptions { render_state: classified_state, ..Default::default() },
            )?);
            if !previous_framebuffer {
                self.translucent_command = Some(context.create_viewport_quad_command(
                    ShaderSource::new(PASS_THROUGH.source),
                    ViewportCommandOptions {
                        render_state: self.rs_unclassified.clone().expect("initialized above"),
                        ..Default::default()
                    },
                )?);
            }
        }
        self.previous_was_present = previous_framebuffer;
        Ok(())
    }

    /// `prepareTextures`: resolves multisample targets before a stencil copy.
    pub fn prepare_textures(&self, _context: &crate::renderer::Context<G>, blit_stencil: bool) -> Result<()> {
        if self.fbo.num_samples() > 1 {
            self.fbo.prepare_textures(blit_stencil)?;
        }
        Ok(())
    }

    /// `clear`.
    pub fn clear(&self, context: &crate::renderer::Context<G>, pass: &PassState<G>) -> Result<()> {
        if self.previous_was_present {
            self.fbo.clear(context, &self.clear_color_command, pass)
        } else {
            self.fbo.clear(context, &self.clear_command, pass)?;
            self.fbo_classified.clear(context, &self.clear_command, pass)
        }
    }

    /// `executeClassified`.
    pub fn execute_classified(&mut self, context: &crate::renderer::Context<G>, pass: &mut PassState<G>) -> Result<()> {
        if !self.previous_was_present {
            let framebuffer = pass.framebuffer.clone();
            self.prepare_textures(context, true)?;
            pass.framebuffer = self.fbo_classified.framebuffer().cloned();
            if let Some(command) = &self.translucent_command {
                command.execute(context, pass)?;
            }
            pass.framebuffer = framebuffer;
        }
        if let Some(command) = &self.classified_command {
            command.execute(context, pass)?;
        }
        Ok(())
    }

    /// `executeUnclassified`.
    pub fn execute_unclassified(&self, context: &crate::renderer::Context<G>, pass: &PassState<G>) -> Result<()> {
        if let Some(command) = &self.unclassified_command {
            command.execute(context, pass)?;
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.fbo.destroy()?;
        self.fbo_classified.destroy()?;
        if let Some(texture) = self.depth_stencil_texture.take() {
            let _ = texture.destroy();
        }
        if let Some(renderbuffer) = self.depth_stencil_renderbuffer.take() {
            let _ = renderbuffer.destroy();
        }
        self.unclassified_command = None;
        self.classified_command = None;
        self.translucent_command = None;
        self.destroyed = true;
        Ok(())
    }
}

impl<G: Gl> Default for InvertClassification<G> {
    fn default() -> Self {
        Self::new().expect("default invert-classification resources are valid")
    }
}

/// `Color` helper matching Cesium's transparent ID clear.
pub fn transparent_id_clear() -> Color {
    Color::new(0.0, 0.0, 0.0, 0.0)
}
