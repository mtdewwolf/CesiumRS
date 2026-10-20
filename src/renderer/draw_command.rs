//! Executable draw/clear commands, with explicit uniforms and texture units.
//! Scene scheduling, derived commands and automatic uniforms are deferred.
use super::{context::*, *};
use crate::core::{webgl_constants as gl, PrimitiveType};

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
    pub pass: Option<Pass>,
    pub uniforms: Vec<(String, UniformValue)>,
    pub textures: Vec<(String, &'a Texture<G>)>,
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
            uniforms: vec![],
            textures: vec![],
        }
    }
    pub fn execute(&self, context: &Context<G>, pass: &PassState) -> Result<()> {
        context.draw(self, pass)
    }
}
#[derive(Clone, Debug, Default)]
pub struct ClearCommand {
    pub color: Option<[f64; 4]>,
    pub depth: Option<f64>,
    pub stencil: Option<i32>,
    pub render_state: RenderState,
}
impl ClearCommand {
    pub fn all() -> Self {
        Self { color: Some([0.; 4]), depth: Some(1.), stencil: Some(0), render_state: RenderState::default() }
    }
    pub fn execute<G: Gl>(&self, context: &Context<G>, pass: &PassState) -> Result<()> {
        context.clear(self, pass)
    }
}
impl<G: Gl> Context<G> {
    pub fn draw(&self, command: &DrawCommand<'_, G>, pass: &PassState) -> Result<()> {
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
        self.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), GlValue::Null])?;
        command.render_state.apply(self, pass, self.gl.drawing_buffer_size()?)?;
        program.bind()?;
        for (name, uniform) in &command.uniforms {
            program.set_uniform(name, uniform)?;
        }
        for (unit, (name, texture)) in command.textures.iter().enumerate() {
            texture.bind(unit as u32)?;
            program.set_uniform(name, &UniformValue::Int(vec![unit as i32]))?;
        }
        va.bind()?;
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
    pub fn clear(&self, command: &ClearCommand, pass: &PassState) -> Result<()> {
        self.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), GlValue::Null])?;
        command.render_state.apply(self, pass, self.gl.drawing_buffer_size()?)?;
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
        let [width, height] = self.gl.drawing_buffer_size()?;
        range(region.x as usize, region.width as usize, width as usize)?;
        range(region.y as usize, region.height as usize, height as usize)?;
        let length = (region.width as usize)
            .checked_mul(region.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| crate::core::RuntimeError::new("Readback size overflow"))?;
        size::<G::Object>(length)?;
        self.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), GlValue::Null])?;
        self.call("bindBuffer", &[gl::PIXEL_PACK_BUFFER.into(), GlValue::Null])?;
        self.call("pixelStorei", &[gl::PACK_ALIGNMENT.into(), 1.into()])?;
        match self.call(
            "readPixels",
            &[
                region.x.into(),
                region.y.into(),
                region.width.into(),
                region.height.into(),
                gl::RGBA.into(),
                gl::UNSIGNED_BYTE.into(),
                GlValue::Bytes(vec![0; length]),
            ],
        )? {
            GlValue::Bytes(bytes) if bytes.len() == length => Ok(bytes),
            _ => Err(crate::core::RuntimeError::new("Invalid pixel readback from GL host")),
        }
    }
}
