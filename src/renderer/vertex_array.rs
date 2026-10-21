//! CesiumJS VertexArray: buffer layouts, constant attributes and instancing.
use super::{context::*, Buffer, BufferKind};
use crate::core::{webgl_constants as gl, ComponentDatatype};
use std::{collections::HashSet, rc::Rc};
pub enum AttributeSource<G: Gl> {
    Buffer(Buffer<G>),
    Constant(Vec<f32>),
}
pub struct VertexAttribute<G: Gl> {
    pub index: u32,
    pub enabled: bool,
    pub source: AttributeSource<G>,
    pub components: u32,
    pub datatype: ComponentDatatype,
    pub normalize: bool,
    pub offset: usize,
    pub stride: usize,
    pub instance_divisor: u32,
}
impl<G: Gl> VertexAttribute<G> {
    pub fn buffer(index: u32, buffer: Buffer<G>, components: u32) -> Self {
        Self {
            index,
            enabled: true,
            source: AttributeSource::Buffer(buffer),
            components,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        }
    }
    pub fn constant(index: u32, value: Vec<f32>) -> Self {
        Self {
            index,
            components: value.len() as u32,
            source: AttributeSource::Constant(value),
            enabled: true,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        }
    }
}
pub struct VertexArray<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    attributes: Vec<VertexAttribute<G>>,
    index_buffer: Option<Buffer<G>>,
    number_of_vertices: usize,
}
impl<G: Gl> VertexArray<G> {
    pub fn new(
        context: &Context<G>,
        attributes: Vec<VertexAttribute<G>>,
        index_buffer: Option<Buffer<G>>,
    ) -> Result<Self> {
        let mut indices = HashSet::new();
        let mut number = None;
        for a in &attributes {
            ensure(indices.insert(a.index), "Duplicate vertex attribute index")?;
            ensure(
                a.instance_divisor == 0 || context.webgl2() || context.extension_supported("ANGLE_instanced_arrays")?,
                "Instanced arrays unsupported",
            )?;
            ensure((1..=4).contains(&a.components), "Attribute components must be in [1,4]")?;
            ensure(a.stride <= 255, "Attribute stride exceeds WebGL's 255-byte limit")?;
            size::<G::Object>(a.offset)?;
            match &a.source {
                AttributeSource::Buffer(b) => {
                    context.same(&b.resource.context)?;
                    b.resource.value()?;
                    ensure(b.kind() == BufferKind::Vertex, "Attribute requires a vertex buffer")?;
                    ensure(
                        a.datatype != ComponentDatatype::Double,
                        "Split double attributes into float high/low first",
                    )?;
                    let width = a.components as usize * a.datatype.get_size_in_bytes();
                    ensure(
                        a.offset % a.datatype.get_size_in_bytes() == 0
                            && a.stride % a.datatype.get_size_in_bytes() == 0,
                        "Misaligned attribute layout",
                    )?;
                    ensure(a.instance_divisor == 0 || a.index != 0, "Attribute zero cannot be instanced")?;
                    if number.is_none() && a.instance_divisor == 0 {
                        number = Some(b.size_in_bytes() / if a.stride == 0 { width } else { a.stride });
                    }
                }
                AttributeSource::Constant(v) => {
                    ensure(v.len() == a.components as usize && a.instance_divisor == 0, "Invalid constant attribute")?;
                }
            }
        }
        if let Some(b) = &index_buffer {
            context.same(&b.resource.context)?;
            b.resource.value()?;
            ensure(matches!(b.kind(), BufferKind::Index(_)), "Vertex array requires an index buffer")?;
        }
        let resource = Rc::new(Resource::create(context, "createVertexArray", &[], "deleteVertexArray")?);
        context.call("bindVertexArray", &[resource.value()?])?;
        let result = (|| {
            for a in &attributes {
                if !a.enabled {
                    continue;
                }
                if let AttributeSource::Buffer(b) = &a.source {
                    context.call("bindBuffer", &[gl::ARRAY_BUFFER.into(), b.resource.value()?])?;
                    context.call(
                        "vertexAttribPointer",
                        &[
                            a.index.into(),
                            a.components.into(),
                            (a.datatype as u32).into(),
                            a.normalize.into(),
                            size(a.stride)?,
                            size(a.offset)?,
                        ],
                    )?;
                    context.call("enableVertexAttribArray", &[a.index.into()])?;
                    context.call("vertexAttribDivisor", &[a.index.into(), a.instance_divisor.into()])?;
                }
            }
            if let Some(b) = &index_buffer {
                context.call("bindBuffer", &[gl::ELEMENT_ARRAY_BUFFER.into(), b.resource.value()?])?;
            }
            Ok(())
        })();
        context.call("bindVertexArray", &[GlValue::Null])?;
        context.call("bindBuffer", &[gl::ARRAY_BUFFER.into(), GlValue::Null])?;
        result?;
        Ok(Self { resource, attributes, index_buffer, number_of_vertices: number.unwrap_or(1) })
    }
    pub fn number_of_vertices(&self) -> usize {
        self.number_of_vertices
    }
    pub fn attributes(&self) -> &[VertexAttribute<G>] {
        &self.attributes
    }
    pub fn index_buffer(&self) -> Option<&Buffer<G>> {
        self.index_buffer.as_ref()
    }
    pub(crate) fn bind(&self) -> Result<()> {
        for a in &self.attributes {
            if let AttributeSource::Buffer(b) = &a.source {
                b.resource.value()?;
            }
        }
        if let Some(b) = &self.index_buffer {
            b.resource.value()?;
        }
        let ctx = &self.resource.context;
        ctx.call("bindVertexArray", &[self.resource.value()?])?;
        // Constant attributes are global state, not stored in the VAO.
        for a in &self.attributes {
            if a.enabled {
                if let AttributeSource::Constant(v) = &a.source {
                    ctx.call(
                        &format!("vertexAttrib{}fv", a.components),
                        &[a.index.into(), GlValue::Floats(v.clone())],
                    )?;
                }
            }
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
    /// Buffers are shared and survive while any Rust owner retains them.
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
}

impl<G: Gl> Clone for AttributeSource<G> {
    fn clone(&self) -> Self {
        match self {
            Self::Buffer(b) => Self::Buffer(b.clone()),
            Self::Constant(v) => Self::Constant(v.clone()),
        }
    }
}
impl<G: Gl> Clone for VertexAttribute<G> {
    fn clone(&self) -> Self {
        Self {
            index: self.index,
            enabled: self.enabled,
            source: self.source.clone(),
            components: self.components,
            datatype: self.datatype,
            normalize: self.normalize,
            offset: self.offset,
            stride: self.stride,
            instance_divisor: self.instance_divisor,
        }
    }
}
impl<G: Gl> VertexArray<G> {
    pub fn number_of_attributes(&self) -> usize {
        self.attributes.len()
    }
    pub fn get_attribute(&self, index: usize) -> Option<&VertexAttribute<G>> {
        self.attributes.get(index)
    }
    pub fn set_constant_value(&mut self, index: usize, value: Vec<f32>) -> Result<()> {
        let attribute =
            self.attributes.get_mut(index).ok_or_else(|| crate::core::RuntimeError::new("Invalid attribute index"))?;
        ensure(value.len() == attribute.components as usize, "Constant component count cannot change")?;
        match &mut attribute.source {
            AttributeSource::Constant(v) => {
                *v = value;
                Ok(())
            }
            _ => Err(crate::core::RuntimeError::new("Attribute is backed by a buffer")),
        }
    }
    /// Replace a layout atomically. GPU storage survives through its Rust owners.
    pub fn replace_attribute(&mut self, index: usize, attribute: VertexAttribute<G>) -> Result<()> {
        ensure(index < self.attributes.len(), "Invalid attribute index")?;
        let mut attributes = self.attributes.clone();
        attributes[index] = attribute;
        let replacement = Self::new(&self.resource.context, attributes, self.index_buffer.clone())?;
        *self = replacement;
        Ok(())
    }
}
