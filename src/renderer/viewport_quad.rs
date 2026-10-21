//! Explicit owner for Cesium's per-context viewport-quad geometry cache.
use super::{context::*, *};
use crate::core::IndexDatatype;
use std::{collections::BTreeMap, rc::Rc};

pub const VIEWPORT_QUAD_VERTEX_SHADER: &str = "in vec4 position;\nin vec2 textureCoordinates;\n\nout vec2 v_textureCoordinates;\n\nvoid main() \n{\n    gl_Position = position;\n    v_textureCoordinates = textureCoordinates;\n}\n";

pub struct ViewportQuad<G: Gl> {
    vertex_array: Rc<VertexArray<G>>,
}
impl<G: Gl> ViewportQuad<G> {
    pub fn new(context: &Context<G>) -> Result<Self> {
        let values: [f32; 16] = [-1., -1., 0., 0., 1., -1., 1., 0., 1., 1., 1., 1., -1., 1., 0., 1.];
        let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let b = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
        let mut position = VertexAttribute::buffer(0, b.clone(), 2);
        position.stride = 16;
        let mut uv = VertexAttribute::buffer(1, b, 2);
        uv.offset = 8;
        uv.stride = 16;
        let indices: Vec<_> = [0u16, 1, 2, 0, 2, 3].iter().flat_map(|v| v.to_le_bytes()).collect();
        let indices = Buffer::new(
            context,
            BufferKind::Index(IndexDatatype::UnsignedShort),
            BufferUsage::StaticDraw,
            BufferData::Bytes(&indices),
        )?;
        Ok(Self { vertex_array: Rc::new(VertexArray::new(context, vec![position, uv], Some(indices))?) })
    }
    pub fn vertex_array(&self) -> &Rc<VertexArray<G>> {
        &self.vertex_array
    }
    pub fn shader_options(fragment_shader_source: ShaderSource) -> ShaderProgramOptions {
        ShaderProgramOptions {
            vertex_shader_source: ShaderSource {
                sources: vec![VIEWPORT_QUAD_VERTEX_SHADER.into()],
                ..Default::default()
            },
            fragment_shader_source,
            attribute_locations: BTreeMap::from([("position".into(), 0), ("textureCoordinates".into(), 1)]),
        }
    }
    pub fn create_command<'a>(&'a self, program: &'a ShaderProgram<G>) -> DrawCommand<'a, G> {
        DrawCommand::new(&self.vertex_array, program)
    }
    pub fn is_destroyed(&self) -> bool {
        self.vertex_array.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        self.vertex_array.destroy()
    }
}
