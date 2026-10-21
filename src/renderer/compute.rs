//! Cesium ComputeCommand/ComputeEngine: fragment-shader computation into a texture.
use super::{context::*, *};
use std::rc::Rc;

pub type ComputePreExecute<G> = Rc<dyn Fn(&mut ComputeCommand<G>) -> Result<()>>;
pub type ComputePostExecute<G> = Box<dyn FnMut(&Rc<Texture<G>>) -> Result<()>>;
pub struct ComputeCommand<G: Gl> {
    pub vertex_array: Option<Rc<VertexArray<G>>>,
    pub fragment_shader_source: Option<ShaderSource>,
    pub shader_program: Option<Rc<ShaderProgram<G>>>,
    pub uniform_map: Option<UniformMap>,
    /// Explicit texture leases accompany sampler uniforms in the Rust API.
    pub textures: Vec<(String, Rc<Texture<G>>)>,
    pub output_texture: Option<Rc<Texture<G>>>,
    pub pre_execute: Option<ComputePreExecute<G>>,
    pub post_execute: Option<ComputePostExecute<G>>,
    pub canceled: Option<Box<dyn FnMut() -> Result<()>>>,
    pub persists: bool,
    pub owner: Option<String>,
}
impl<G: Gl> Default for ComputeCommand<G> {
    fn default() -> Self {
        Self {
            vertex_array: None,
            fragment_shader_source: None,
            shader_program: None,
            uniform_map: None,
            textures: vec![],
            output_texture: None,
            pre_execute: None,
            post_execute: None,
            canceled: None,
            persists: false,
            owner: None,
        }
    }
}
impl<G: Gl> ComputeCommand<G> {
    pub fn pass(&self) -> Pass {
        Pass::Compute
    }
    pub fn execute(&mut self, engine: &mut ComputeEngine<G>) -> Result<()> {
        engine.execute(self)
    }
    pub fn cancel(&mut self) -> Result<()> {
        if let Some(f) = &mut self.canceled {
            f()?;
        }
        Ok(())
    }
}
pub struct ComputeEngine<G: Gl> {
    context: Context<G>,
    quad: Option<ViewportQuad<G>>,
    shaders: ShaderCache<G>,
    destroyed: bool,
}
impl<G: Gl> ComputeEngine<G> {
    pub fn new(context: &Context<G>) -> Self {
        Self { context: context.clone(), quad: None, shaders: ShaderCache::new(context), destroyed: false }
    }
    pub fn execute(&mut self, command: &mut ComputeCommand<G>) -> Result<()> {
        ensure(!self.destroyed, "Compute engine is destroyed")?;
        if let Some(pre) = command.pre_execute.clone() {
            pre(command)?;
        }
        ensure(
            command.fragment_shader_source.is_some() || command.shader_program.is_some(),
            "Compute requires a fragment shader source or shader program",
        )?;
        let output = command
            .output_texture
            .as_ref()
            .ok_or_else(|| crate::core::RuntimeError::new("Compute requires an output texture"))?
            .clone();
        self.context.same(&output.resource.context)?;
        output.resource.value()?;
        let va = if let Some(v) = &command.vertex_array {
            v.clone()
        } else {
            if self.quad.is_none() {
                self.quad = Some(ViewportQuad::new(&self.context)?);
            }
            self.quad.as_ref().unwrap().vertex_array().clone()
        };
        let program = if let Some(p) = &command.shader_program {
            p.clone()
        } else {
            self.shaders.get_shader_program(&ViewportQuad::<G>::shader_options(
                command.fragment_shader_source.clone().unwrap(),
            ))?
        };
        let framebuffer = Rc::new(Framebuffer::new(
            &self.context,
            FramebufferOptions {
                colors: vec![Attachment::Texture(output.clone())],
                destroy_attachments: false,
                ..Default::default()
            },
        )?);
        let state = RenderState {
            viewport: Some(Viewport { x: 0, y: 0, width: output.width(), height: output.height() }),
            ..Default::default()
        };
        ClearCommand {
            color: Some([0.; 4]),
            framebuffer: Some(framebuffer.clone()),
            render_state: state.clone(),
            ..Default::default()
        }
        .execute(&self.context, &PassState::default())?;
        let mut draw = DrawCommand::new(&va, &program);
        draw.framebuffer = Some(framebuffer.clone());
        draw.render_state = state;
        draw.uniform_map = command.uniform_map.clone();
        draw.textures = command.textures.iter().map(|(n, t)| (n.clone(), t.as_ref())).collect();
        draw.execute(&self.context, &PassState::default())?;
        framebuffer.destroy()?;
        if !command.persists {
            program.destroy()?;
            if command.vertex_array.is_some() {
                va.destroy()?;
            }
        }
        if let Some(post) = &mut command.post_execute {
            post(&output)?;
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        self.shaders.destroy()?;
        if let Some(quad) = &self.quad {
            quad.destroy()?;
        }
        self.quad = None;
        self.destroyed = true;
        Ok(())
    }
}
impl<G: Gl> Drop for ComputeEngine<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
