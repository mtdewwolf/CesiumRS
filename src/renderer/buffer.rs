//! CesiumJS Buffer: raw vertex storage, typed indices and pixel-pack buffers.
use super::{context::*, BufferUsage};
use crate::core::{webgl_constants as gl, IndexDatatype};
use std::rc::Rc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Vertex,
    Index(IndexDatatype),
    PixelPack,
}
impl BufferKind {
    pub(crate) fn target(self) -> u32 {
        match self {
            Self::Vertex => gl::ARRAY_BUFFER,
            Self::Index(_) => gl::ELEMENT_ARRAY_BUFFER,
            Self::PixelPack => gl::PIXEL_PACK_BUFFER,
        }
    }
}
pub enum BufferData<'a> {
    Bytes(&'a [u8]),
    Size(usize),
}
pub struct Buffer<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    kind: BufferKind,
    usage: BufferUsage,
    size: usize,
}
impl<G: Gl> Clone for Buffer<G> {
    fn clone(&self) -> Self {
        Self { resource: self.resource.clone(), kind: self.kind, usage: self.usage, size: self.size }
    }
}
impl<G: Gl> Buffer<G> {
    pub fn new(context: &Context<G>, kind: BufferKind, usage: BufferUsage, data: BufferData<'_>) -> Result<Self> {
        let length = match data {
            BufferData::Bytes(b) => b.len(),
            BufferData::Size(n) => n,
        };
        ensure(length > 0, "Buffer size must be positive")?;
        size::<G::Object>(length)?;
        if let BufferKind::Index(d) = kind {
            ensure(length % d.get_size_in_bytes() == 0, "Index buffer length is not aligned")?;
        }
        let resource = Rc::new(Resource::create(context, "createBuffer", &[], "deleteBuffer")?);
        let target: GlValue<G::Object> = kind.target().into();
        context.call("bindBuffer", &[target.clone(), resource.value()?])?;
        let result = context.call(
            "bufferData",
            &[
                target.clone(),
                match data {
                    BufferData::Bytes(b) => GlValue::Bytes(b.to_vec()),
                    BufferData::Size(n) => size(n)?,
                },
                (usage as u32).into(),
            ],
        );
        context.call("bindBuffer", &[target, GlValue::Null])?;
        result?;
        Ok(Self { resource, kind, usage, size: length })
    }
    pub fn same_allocation(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.resource, &other.resource)
    }
    pub fn size_in_bytes(&self) -> usize {
        self.size
    }
    pub fn usage(&self) -> BufferUsage {
        self.usage
    }
    pub fn kind(&self) -> BufferKind {
        self.kind
    }
    pub fn number_of_indices(&self) -> Option<usize> {
        if let BufferKind::Index(d) = self.kind {
            Some(self.size / d.get_size_in_bytes())
        } else {
            None
        }
    }
    pub fn copy_from_array_view(&self, bytes: &[u8], offset: usize) -> Result<()> {
        range(offset, bytes.len(), self.size)?;
        let ctx = &self.resource.context;
        let target: GlValue<G::Object> = self.kind.target().into();
        ctx.call("bindBuffer", &[target.clone(), self.resource.value()?])?;
        let result = ctx.call("bufferSubData", &[target.clone(), size(offset)?, GlValue::Bytes(bytes.to_vec())]);
        ctx.call("bindBuffer", &[target, GlValue::Null])?;
        result.map(|_| ())
    }
    pub fn copy_from_buffer(
        &self,
        source: &Self,
        read_offset: usize,
        write_offset: usize,
        length: usize,
    ) -> Result<()> {
        self.resource.context.same(&source.resource.context)?;
        range(read_offset, length, source.size)?;
        range(write_offset, length, self.size)?;
        ensure(length > 0, "Copy length must be positive")?;
        ensure(
            matches!(self.kind, BufferKind::Index(_)) == matches!(source.kind, BufferKind::Index(_)),
            "Cannot copy between index and non-index buffers",
        )?;
        ensure(
            !Rc::ptr_eq(&self.resource, &source.resource)
                || read_offset + length <= write_offset
                || write_offset + length <= read_offset,
            "Overlapping buffer copy",
        )?;
        let ctx = &self.resource.context;
        // Validate both handles before changing any bindings.
        let (src, dst) = (source.resource.value()?, self.resource.value()?);
        ctx.call("bindBuffer", &[gl::COPY_WRITE_BUFFER.into(), dst])?;
        ctx.call("bindBuffer", &[gl::COPY_READ_BUFFER.into(), src])?;
        let result = ctx.call(
            "copyBufferSubData",
            &[
                gl::COPY_READ_BUFFER.into(),
                gl::COPY_WRITE_BUFFER.into(),
                size(read_offset)?,
                size(write_offset)?,
                size(length)?,
            ],
        );
        ctx.call("bindBuffer", &[gl::COPY_WRITE_BUFFER.into(), GlValue::Null])?;
        ctx.call("bindBuffer", &[gl::COPY_READ_BUFFER.into(), GlValue::Null])?;
        result.map(|_| ())
    }
    /// Rust readback uses byte offsets and an owned byte vector, avoiding JS's
    /// inconsistent element/byte offset interpretation for typed-array views.
    pub fn get_buffer_data(&self, offset: usize, length: usize) -> Result<Vec<u8>> {
        range(offset, length, self.size)?;
        let ctx = &self.resource.context;
        ctx.call("bindBuffer", &[gl::COPY_READ_BUFFER.into(), self.resource.value()?])?;
        let result = ctx
            .call("getBufferSubData", &[gl::COPY_READ_BUFFER.into(), size(offset)?, GlValue::Bytes(vec![0; length])]);
        ctx.call("bindBuffer", &[gl::COPY_READ_BUFFER.into(), GlValue::Null])?;
        match result? {
            GlValue::Bytes(b) if b.len() == length => Ok(b),
            _ => Err(crate::core::RuntimeError::new("Invalid buffer readback from host")),
        }
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
}
