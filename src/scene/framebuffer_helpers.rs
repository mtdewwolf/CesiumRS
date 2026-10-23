use crate::{core::RuntimeError, renderer::*};
use std::rc::Rc;
pub(crate) type Result<T> = std::result::Result<T, RuntimeError>;

pub(crate) fn hdr_datatype<G: Gl>(context: &Context<G>, hdr: bool) -> Result<PixelDatatype> {
    Ok(if hdr {
        if context.capabilities()?.half_floating_point_texture {
            PixelDatatype::HalfFloat
        } else {
            PixelDatatype::Float
        }
    } else {
        PixelDatatype::UnsignedByte
    })
}
pub(crate) fn color<G: Gl>(manager: &FramebufferManager<G>, index: usize) -> Result<Rc<Texture<G>>> {
    manager.color_texture(index)?.cloned().ok_or_else(|| RuntimeError::new("Framebuffer has not been updated"))
}
pub(crate) fn depth<G: Gl>(manager: &FramebufferManager<G>) -> Result<Rc<Texture<G>>> {
    manager.depth_stencil_texture().cloned().ok_or_else(|| RuntimeError::new("Depth texture is unavailable"))
}
pub(crate) fn copy<G: Gl>(
    context: &Context<G>,
    command: &mut Option<ViewportCommand<G>>,
    source: &str,
    texture_name: &str,
    texture: Rc<Texture<G>>,
    target: (Option<Rc<Framebuffer<G>>>, RenderState),
    pass: &PassState<G>,
) -> Result<()> {
    if command.is_none() {
        *command =
            Some(context.create_viewport_quad_command(ShaderSource::new(source), ViewportCommandOptions::default())?);
    }
    let command = command.as_mut().expect("created above");
    command.options.framebuffer = target.0;
    command.options.render_state = target.1;
    command.textures = vec![(texture_name.into(), TextureBinding::Texture(texture))];
    command.execute(context, pass)
}
pub(crate) fn copy_state<G: Gl>(width: u32, height: u32, pass: &PassState<G>) -> RenderState {
    let viewport = Viewport { x: 0, y: 0, width, height };
    let rectangle = pass.viewport.unwrap_or_default();
    RenderState {
        viewport: Some(viewport),
        scissor_test: ScissorTest { enabled: Some(viewport) != pass.viewport, rectangle },
        ..Default::default()
    }
}
pub(crate) fn destroy_command<G: Gl>(command: &mut Option<ViewportCommand<G>>) -> Result<()> {
    if let Some(command) = command.take() {
        command.shader_program.destroy()?;
    }
    Ok(())
}
pub(crate) fn alive(destroyed: bool) -> Result<()> {
    if destroyed {
        Err(RuntimeError::new("Scene framebuffer resource is destroyed"))
    } else {
        Ok(())
    }
}
