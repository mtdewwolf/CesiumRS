//! Cesium VertexArrayFacade: usage-bucketed interleaved writers and GPU commits.
use super::{context::*, *};
use crate::core::ComponentDatatype;
use std::{
    collections::{BTreeMap, HashSet},
    rc::Rc,
};
pub struct FacadeAttribute<G: Gl> {
    pub index: u32,
    pub enabled: bool,
    pub components: u32,
    pub datatype: ComponentDatatype,
    pub normalize: bool,
    pub usage: BufferUsage,
    pub vertex_buffer: Option<Buffer<G>>,
}
impl<G: Gl> Clone for FacadeAttribute<G> {
    fn clone(&self) -> Self {
        Self {
            index: self.index,
            enabled: self.enabled,
            components: self.components,
            datatype: self.datatype,
            normalize: self.normalize,
            usage: self.usage,
            vertex_buffer: self.vertex_buffer.clone(),
        }
    }
}
impl<G: Gl> FacadeAttribute<G> {
    pub fn new(index: u32, components: u32) -> Self {
        Self {
            index,
            components,
            enabled: true,
            datatype: ComponentDatatype::Float,
            normalize: false,
            usage: BufferUsage::StaticDraw,
            vertex_buffer: None,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FacadeWriter {
    index: u32,
    generation: u64,
}
struct View {
    index: u32,
    enabled: bool,
    components: u32,
    datatype: ComponentDatatype,
    normalize: bool,
    offset: usize,
}
struct Storage<G: Gl> {
    stride: usize,
    usage: BufferUsage,
    views: Vec<View>,
    bytes: Vec<u8>,
    gpu: Option<Buffer<G>>,
    dirty: bool,
}
pub struct FacadeVertexArray<G: Gl> {
    pub vertex_array: Rc<VertexArray<G>>,
    pub indices_count: f64,
}
pub struct VertexArrayFacade<G: Gl> {
    context: Context<G>,
    buffers: Vec<Storage<G>>,
    precreated: Vec<VertexAttribute<G>>,
    size: usize,
    instanced: bool,
    generation: u64,
    arrays: Option<Vec<FacadeVertexArray<G>>>,
    destroyed: bool,
}
impl<G: Gl> VertexArrayFacade<G> {
    pub fn new(
        context: &Context<G>,
        attributes: Vec<FacadeAttribute<G>>,
        size: usize,
        instanced: bool,
    ) -> Result<Self> {
        ensure(!attributes.is_empty(), "At least one facade attribute is required")?;
        let mut indices = HashSet::new();
        let mut groups: BTreeMap<u32, Vec<FacadeAttribute<G>>> = BTreeMap::new();
        let mut precreated = vec![];
        for a in attributes {
            ensure((1..=4).contains(&a.components), "Attribute component count must be 1 through 4")?;
            ensure(indices.insert(a.index), "Duplicate facade attribute index")?;
            if let Some(buffer) = a.vertex_buffer {
                context.same(&buffer.resource.context)?;
                let mut v = VertexAttribute::buffer(a.index, buffer, a.components);
                v.enabled = a.enabled;
                v.datatype = a.datatype;
                v.normalize = a.normalize;
                precreated.push(v);
            } else {
                groups.entry(a.usage as u32).or_default().push(a);
            }
        }
        let mut buffers = vec![];
        for (_, mut group) in groups {
            group.sort_by_key(|a| std::cmp::Reverse(a.datatype.get_size_in_bytes()));
            let usage = group[0].usage;
            let alignment = group[0].datatype.get_size_in_bytes();
            let mut offset = 0;
            let views = group
                .into_iter()
                .map(|a| {
                    let view = View {
                        index: a.index,
                        enabled: a.enabled,
                        components: a.components,
                        datatype: a.datatype,
                        normalize: a.normalize,
                        offset,
                    };
                    offset += a.components as usize * a.datatype.get_size_in_bytes();
                    view
                })
                .collect();
            let stride = offset.div_ceil(alignment) * alignment;
            buffers.push(Storage { stride, usage, views, bytes: vec![], gpu: None, dirty: false });
        }
        let mut facade = Self {
            context: context.clone(),
            buffers,
            precreated,
            size: 0,
            instanced,
            generation: 0,
            arrays: None,
            destroyed: false,
        };
        facade.resize(size)?;
        Ok(facade)
    }
    pub fn size(&self) -> usize {
        self.size
    }
    pub fn vertex_arrays(&self) -> Option<&[FacadeVertexArray<G>]> {
        self.arrays.as_deref()
    }
    pub fn writer(&self, index: u32) -> Result<FacadeWriter> {
        ensure(
            self.buffers.iter().any(|b| b.views.iter().any(|v| v.index == index)),
            "No managed writer for attribute",
        )?;
        Ok(FacadeWriter { index, generation: self.generation })
    }
    pub fn write_with(&mut self, writer: FacadeWriter, vertex: usize, values: &[f64]) -> Result<()> {
        ensure(writer.generation == self.generation, "Writer was invalidated by resize")?;
        self.write(writer.index, vertex, values)
    }
    pub fn write(&mut self, index: u32, vertex: usize, values: &[f64]) -> Result<()> {
        ensure(!self.destroyed, "Vertex array facade is destroyed")?;
        ensure(vertex < self.size, "Writer vertex index is out of range")?;
        for b in &mut self.buffers {
            if let Some(view) = b.views.iter().find(|v| v.index == index) {
                ensure(values.len() == view.components as usize, "Writer component count does not match attribute")?;
                let bytes = super::geometry_upload::typed_array_bytes(&view.datatype.create_typed_array(values));
                let offset = vertex * b.stride + view.offset;
                b.bytes[offset..offset + bytes.len()].copy_from_slice(&bytes);
                b.dirty = true;
                return Ok(());
            }
        }
        Err(crate::core::RuntimeError::new("No managed writer for attribute"))
    }
    pub fn resize(&mut self, size: usize) -> Result<()> {
        ensure(!self.destroyed, "Vertex array facade is destroyed")?;
        for b in &self.buffers {
            super::context::size::<()>(
                size.checked_mul(b.stride).ok_or_else(|| crate::core::RuntimeError::new("Facade size overflow"))?,
            )?;
        }
        for b in &mut self.buffers {
            b.bytes.resize(size * b.stride, 0);
        }
        self.destroy_arrays()?;
        self.size = size;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }
    pub fn commit(&mut self, index_buffer: Option<Buffer<G>>) -> Result<()> {
        ensure(!self.destroyed, "Vertex array facade is destroyed")?;
        if let Some(b) = &index_buffer {
            self.context.same(&b.resource.context)?;
            ensure(matches!(b.kind(), BufferKind::Index(_)), "Facade index buffer must be an index buffer")?;
        }
        if self.size == 0 {
            self.destroy_arrays()?;
            self.arrays = Some(vec![]);
            return Ok(());
        }
        let mut recreate = self.arrays.is_none();
        for b in &mut self.buffers {
            if b.dirty && b.stride > 0 {
                if b.gpu.as_ref().is_none_or(|gpu| gpu.size_in_bytes() < b.bytes.len()) {
                    b.gpu = Some(Buffer::new(&self.context, BufferKind::Vertex, b.usage, BufferData::Bytes(&b.bytes))?);
                    recreate = true;
                } else {
                    b.gpu.as_ref().unwrap().copy_from_array_view(&b.bytes, 0)?;
                }
                b.dirty = false;
            }
        }
        if recreate {
            const CHUNK: usize = 65532;
            let count = if index_buffer.is_some() && !self.instanced { self.size.div_ceil(CHUNK) } else { 1 };
            let mut arrays = vec![];
            for k in 0..count {
                let mut attributes = vec![];
                for b in &self.buffers {
                    let gpu = b.gpu.as_ref().ok_or_else(|| {
                        crate::core::RuntimeError::new("Write managed attributes before committing the facade")
                    })?;
                    for v in &b.views {
                        let mut attr = VertexAttribute::buffer(v.index, gpu.clone(), v.components);
                        attr.enabled = v.enabled;
                        attr.datatype = v.datatype;
                        attr.normalize = v.normalize;
                        attr.offset = k * b.stride * CHUNK + v.offset;
                        attr.stride = b.stride;
                        attr.instance_divisor = u32::from(self.instanced);
                        attributes.push(attr);
                    }
                }
                attributes.extend(self.precreated.clone());
                let va = Rc::new(VertexArray::new(&self.context, attributes, index_buffer.clone())?);
                arrays.push(FacadeVertexArray {
                    vertex_array: va,
                    indices_count: 1.5 * (if k != count - 1 { CHUNK } else { self.size % CHUNK }) as f64,
                });
            }
            self.destroy_arrays()?;
            self.arrays = Some(arrays);
        }
        Ok(())
    }
    pub fn sub_commit(&self, offset: usize, length: usize) -> Result<()> {
        ensure(!self.destroyed, "Vertex array facade is destroyed")?;
        ensure(offset < self.size, "Subcommit offset is out of range")?;
        range(offset, length, self.size)?;
        for b in &self.buffers {
            if b.dirty && b.stride > 0 {
                let gpu = b
                    .gpu
                    .as_ref()
                    .ok_or_else(|| crate::core::RuntimeError::new("Commit the facade before subcommitting"))?;
                let start = offset * b.stride;
                gpu.copy_from_array_view(&b.bytes[start..start + length * b.stride], start)?;
            }
        }
        Ok(())
    }
    pub fn end_sub_commits(&mut self) {
        for b in &mut self.buffers {
            b.dirty = false;
        }
    }
    pub fn cpu_buffers(&self) -> Vec<&[u8]> {
        self.buffers.iter().map(|b| b.bytes.as_slice()).collect()
    }
    fn destroy_arrays(&mut self) -> Result<()> {
        if let Some(arrays) = &self.arrays {
            for a in arrays {
                a.vertex_array.destroy()?;
            }
        }
        self.arrays = None;
        Ok(())
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        for b in &mut self.buffers {
            if let Some(gpu) = &b.gpu {
                gpu.destroy()?;
            }
            b.gpu = None;
        }
        self.destroy_arrays()?;
        self.destroyed = true;
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
impl<G: Gl> Drop for VertexArrayFacade<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
