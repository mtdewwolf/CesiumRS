//! Executable draw/clear commands, with explicit uniforms and texture units.
//! Scene metadata, derived-command maps and uniform providers travel with
//! executable commands; scheduling is owned by Scene.
use super::{context::*, *};
use crate::core::{webgl_constants as gl, PrimitiveType};

pub type UniformMap = std::rc::Rc<dyn Fn() -> Result<Vec<(String, UniformValue)>>>;

pub struct DrawCommand<'a, G: Gl> {
    pub vertex_array: &'a VertexArray<G>,
    pub shader_program: &'a ShaderProgram<G>,
    pub primitive_type: PrimitiveType,
    pub offset: usize,
    pub count: Option<usize>,
    /// Zero selects the ordinary draw path, as in CesiumJS. Positive values
    /// select instanced drawing, even when the instance count is one.
    pub instance_count: u32,
    pub render_state: RenderState,
    pub framebuffer: Option<std::rc::Rc<Framebuffer<G>>>,
    pub pass: Option<Pass>,
    pub uniforms: Vec<(String, UniformValue)>,
    /// Evaluated for every submission, matching Cesium uniform-map callbacks.
    pub uniform_map: Option<UniformMap>,
    pub textures: Vec<(String, &'a Texture<G>)>,
    pub samplers: Vec<(String, &'a dyn SampledTexture<G>)>,
    pub metadata: super::command_metadata::DrawMetadata,
    pub last_dirty_time: f64,
    pub derived_commands: std::collections::HashMap<String, std::rc::Rc<DrawCommand<'a, G>>>,
    pub(crate) tracking: super::command_metadata::CommandTracking,
}
impl<'a, G: Gl> DrawCommand<'a, G> {
    pub fn new(vertex_array: &'a VertexArray<G>, shader_program: &'a ShaderProgram<G>) -> Self {
        Self {
            vertex_array,
            shader_program,
            primitive_type: PrimitiveType::Triangles,
            offset: 0,
            count: None,
            instance_count: 0,
            render_state: RenderState::default(),
            pass: None,
            framebuffer: None,
            uniforms: vec![],
            uniform_map: None,
            textures: vec![],
            samplers: vec![],
            metadata: Default::default(),
            last_dirty_time: 0.,
            derived_commands: super::command_metadata::empty_derived(),
            tracking: Default::default(),
        }
    }
    pub fn execute(&self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        context.draw(self, pass)
    }
}
pub struct ClearCommand<G: Gl> {
    pub framebuffer: Option<std::rc::Rc<Framebuffer<G>>>,
    pub owner: Option<String>,
    pub pass: Option<Pass>,
    pub color: Option<[f64; 4]>,
    pub depth: Option<f64>,
    pub stencil: Option<i32>,
    pub render_state: RenderState,
}
impl<G: Gl> Default for ClearCommand<G> {
    fn default() -> Self {
        Self {
            color: None,
            depth: None,
            stencil: None,
            render_state: RenderState::default(),
            framebuffer: None,
            owner: None,
            pass: None,
        }
    }
}
impl<G: Gl> Clone for ClearCommand<G> {
    fn clone(&self) -> Self {
        Self {
            color: self.color,
            depth: self.depth,
            stencil: self.stencil,
            render_state: self.render_state.clone(),
            framebuffer: self.framebuffer.clone(),
            owner: self.owner.clone(),
            pass: self.pass,
        }
    }
}
impl<G: Gl> ClearCommand<G> {
    pub fn all() -> Self {
        Self {
            color: Some([0.; 4]),
            depth: Some(1.),
            stencil: Some(0),
            render_state: RenderState::default(),
            framebuffer: None,
            owner: None,
            pass: None,
        }
    }
    pub fn execute(&self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        context.clear(self, pass)
    }
}
impl<G: Gl> Context<G> {
    pub fn draw(&self, command: &DrawCommand<'_, G>, pass: &PassState<G>) -> Result<()> {
        let va = command.vertex_array;
        let program = command.shader_program;
        self.same(&va.resource.context)?;
        self.same(&program.resource.context)?;
        va.resource.value()?;
        program.resource.value()?;
        for (_, texture) in &command.textures {
            self.same(&texture.resource.context)?;
            texture.resource.value()?;
        }
        for (_, texture) in &command.samplers {
            self.same(texture.context())?;
            ensure(!texture.is_destroyed(), "Sampler texture is destroyed")?;
        }
        let mut offset = size(command.offset)?;
        let available =
            va.index_buffer().and_then(Buffer::number_of_indices).unwrap_or_else(|| va.number_of_vertices());
        // Preserve Cesium's clamp against the full allocation, rather than
        // subtracting offset. An invalid final GL range is reported by WebGL.
        let count = size(command.count.unwrap_or(available).min(available))?;
        size::<G::Object>(command.instance_count as usize)?;
        if let Some(b) = va.index_buffer() {
            if let BufferKind::Index(datatype) = b.kind() {
                offset = size(
                    command
                        .offset
                        .checked_mul(datatype.get_size_in_bytes())
                        .ok_or_else(|| crate::core::RuntimeError::new("Index offset overflow"))?,
                )?;
            }
        }
        self.bind_framebuffer(command.framebuffer.as_deref().or(pass.framebuffer.as_deref()))?;
        self.apply_render_state(&command.render_state, pass, false)?;
        program.bind()?;
        for (name, uniform) in &command.uniforms {
            program.set_uniform(name, uniform)?;
        }
        if let Some(uniforms) = &command.uniform_map {
            for (name, value) in uniforms()? {
                program.set_uniform(&name, &value)?;
            }
        }
        for (unit, (name, texture)) in command.textures.iter().enumerate() {
            texture.bind(unit as u32)?;
            program.set_uniform(name, &UniformValue::Int(vec![unit as i32]))?;
        }
        for (i, (name, texture)) in command.samplers.iter().enumerate() {
            let unit = command.textures.len() + i;
            texture.bind(unit as u32)?;
            program.set_uniform(name, &UniformValue::Int(vec![unit as i32]))?;
        }
        self.frame_texture_units.set(
            self.frame_texture_units
                .get()
                .max((command.textures.len() + command.samplers.len()) as u32)
                .max(program.maximum_texture_unit_index() as u32),
        );
        {
            let mut state = self.uniform_state.borrow_mut();
            state.model = command.metadata.model_matrix.unwrap_or(crate::core::Matrix4::IDENTITY);
            let size = self.drawing_buffer_size()?;
            state.viewport = command.render_state.viewport.or(pass.viewport).unwrap_or(Viewport {
                x: 0,
                y: 0,
                width: size[0],
                height: size[1],
            });
            if let Some(p) = command.pass {
                state.update_pass(p);
            }
            let used = program
                .set_automatic_uniforms_from(&state, (command.textures.len() + command.samplers.len()) as u32)?;
            self.frame_texture_units.set(self.frame_texture_units.get().max(used));
        }
        va.bind()?;
        if self.validate_shader_program.get() {
            if let Err(error) = program.validate() {
                self.call("bindVertexArray", &[GlValue::Null])?;
                return Err(error);
            }
        }
        let draw = if let Some(b) = va.index_buffer() {
            let datatype = match b.kind() {
                BufferKind::Index(d) => d as u32,
                _ => unreachable!(),
            };
            let mut args = vec![(command.primitive_type as u32).into(), count, datatype.into(), offset];
            if command.instance_count > 0 {
                args.push(command.instance_count.into());
            }
            self.call(if command.instance_count == 0 { "drawElements" } else { "drawElementsInstanced" }, &args)
        } else {
            let mut args = vec![(command.primitive_type as u32).into(), offset, count];
            if command.instance_count > 0 {
                args.push(command.instance_count.into());
            }
            self.call(if command.instance_count == 0 { "drawArrays" } else { "drawArraysInstanced" }, &args)
        };
        self.call("bindVertexArray", &[GlValue::Null])?;
        draw.map(|_| ())
    }
    pub fn clear(&self, command: &ClearCommand<G>, pass: &PassState<G>) -> Result<()> {
        self.bind_framebuffer(command.framebuffer.as_deref().or(pass.framebuffer.as_deref()))?;
        self.apply_render_state(&command.render_state, pass, true)?;
        let mut mask = 0;
        if let Some(c) = command.color {
            self.call("clearColor", &[c[0].into(), c[1].into(), c[2].into(), c[3].into()])?;
            mask |= gl::COLOR_BUFFER_BIT;
        }
        if let Some(d) = command.depth {
            self.call("clearDepth", &[d.into()])?;
            mask |= gl::DEPTH_BUFFER_BIT;
        }
        if let Some(s) = command.stencil {
            self.call("clearStencil", &[s.into()])?;
            mask |= gl::STENCIL_BUFFER_BIT;
        }
        self.call("clear", &[mask.into()]).map(|_| ())
    }
    /// RGBA8 readback from the default framebuffer in GL's bottom-up row order.
    pub fn read_pixels(&self, region: TextureRegion) -> Result<Vec<u8>> {
        self.read_pixels_from(super::framebuffer::ReadState { region, framebuffer: None }).map(|v| v.bytes)
    }
}
