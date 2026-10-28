//! CesiumJS `BrdfLutGenerator`: generates a 256x256 2D BRDF look-up texture
//! for image-based lighting and physically-based materials.

use crate::core::RuntimeError;
use crate::core::PixelFormat;
use crate::renderer::{
    Attachment, Context, Framebuffer, FramebufferOptions, Gl, PassState,
    PixelDatatype, RenderState, Sampler, ShaderSource, Texture, TextureOptions,
    Viewport, ViewportCommand, ViewportCommandOptions,
};
use crate::shaders::root::BRDF_LUT_GENERATOR_FS;
use std::rc::Rc;

/// Generates a 2D BRDF split-sum lookup texture for image-based lighting.
pub struct BrdfLutGenerator<G: Gl> {
    color_texture: Option<Rc<Texture<G>>>,
    draw_command: Option<ViewportCommand<G>>,
    destroyed: bool,
}

impl<G: Gl> Default for BrdfLutGenerator<G> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: Gl> BrdfLutGenerator<G> {
    pub const WIDTH: u32 = 256;
    pub const HEIGHT: u32 = 256;
    pub const PIXEL_FORMAT: PixelFormat = PixelFormat::Rgba;
    pub const PIXEL_DATATYPE: PixelDatatype = PixelDatatype::UnsignedByte;

    pub fn new() -> Self {
        Self {
            color_texture: None,
            draw_command: None,
            destroyed: false,
        }
    }

    pub fn color_texture(&self) -> Option<&Rc<Texture<G>>> {
        self.color_texture.as_ref()
    }

    pub fn draw_command(&self) -> Option<&ViewportCommand<G>> {
        self.draw_command.as_ref()
    }

    pub fn shader_source() -> &'static str {
        BRDF_LUT_GENERATOR_FS.source
    }

    pub fn update(&mut self, context: &Context<G>) -> Result<(), RuntimeError> {
        if self.color_texture.is_none() {
            let mut options = TextureOptions::rgba(Self::WIDTH, Self::HEIGHT);
            options.pixel_format = Self::PIXEL_FORMAT;
            options.pixel_datatype = Self::PIXEL_DATATYPE;
            options.sampler = Sampler::NEAREST;

            let texture = Rc::new(Texture::new(context, options, None)?);
            self.color_texture = Some(texture.clone());

            let fb = Rc::new(Framebuffer::new(
                context,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(texture)],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )?);

            let command = context.create_viewport_quad_command(
                ShaderSource::new(Self::shader_source()),
                ViewportCommandOptions {
                    framebuffer: Some(fb.clone()),
                    render_state: RenderState {
                        viewport: Some(Viewport {
                            x: 0,
                            y: 0,
                            width: Self::WIDTH,
                            height: Self::HEIGHT,
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )?;

            command.execute(context, &PassState::default())?;
            let _ = fb.destroy();
            self.draw_command = Some(command);
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        if let Some(t) = self.color_texture.take() {
            let _ = t.destroy();
        }
        self.draw_command = None;
        self.destroyed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::GlValue;

    #[derive(Default)]
    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([256, 256])
        }
        fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            if method.starts_with("create") {
                Ok(GlValue::Object(1))
            } else {
                Ok(GlValue::Null)
            }
        }
    }

    #[test]
    fn brdf_lut_generator_constants_and_defaults() {
        let mut gen = BrdfLutGenerator::<TestGl>::default();
        assert_eq!(BrdfLutGenerator::<TestGl>::WIDTH, 256);
        assert_eq!(BrdfLutGenerator::<TestGl>::HEIGHT, 256);
        assert_eq!(
            BrdfLutGenerator::<TestGl>::PIXEL_FORMAT,
            PixelFormat::Rgba
        );
        assert_eq!(
            BrdfLutGenerator::<TestGl>::PIXEL_DATATYPE,
            PixelDatatype::UnsignedByte
        );
        assert!(gen.color_texture().is_none());
        assert!(gen.draw_command().is_none());
        assert_eq!(gen.is_destroyed(), false);

        assert!(BrdfLutGenerator::<TestGl>::shader_source().contains("integrateBrdf"));

        gen.destroy();
        assert_eq!(gen.is_destroyed(), true);
    }
}
