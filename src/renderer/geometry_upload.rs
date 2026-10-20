//! CPU packing for VertexArray.fromGeometry, matching Cesium's attribute order.
use super::{context::*, *};
use crate::core::{ComponentDatatype as D, Geometry, IndexDatatype, TypedArray};
use std::collections::HashMap;
pub fn typed_array_bytes(values: &TypedArray) -> Vec<u8> {
    macro_rules! bytes {
        ($v:expr) => {
            $v.iter().flat_map(|v| v.to_le_bytes()).collect()
        };
    }
    match values {
        TypedArray::Int8(v) => bytes!(v),
        TypedArray::Uint8(v) => v.clone(),
        TypedArray::Int16(v) => bytes!(v),
        TypedArray::Uint16(v) => bytes!(v),
        TypedArray::Int32(v) => bytes!(v),
        TypedArray::Uint32(v) => bytes!(v),
        TypedArray::Float32(v) => bytes!(v),
        TypedArray::Float64(v) => bytes!(v),
    }
}
pub struct GeometryUploadOptions<G: Gl> {
    pub buffer_usage: BufferUsage,
    pub attribute_locations: HashMap<String, u32>,
    pub interleave: bool,
    pub vertex_array_attributes: Vec<VertexAttribute<G>>,
}
impl<G: Gl> Default for GeometryUploadOptions<G> {
    fn default() -> Self {
        Self {
            buffer_usage: BufferUsage::DynamicDraw,
            attribute_locations: HashMap::new(),
            interleave: false,
            vertex_array_attributes: vec![],
        }
    }
}
impl<G: Gl> VertexArray<G> {
    pub fn from_geometry(context: &Context<G>, geometry: &Geometry, options: GeometryUploadOptions<G>) -> Result<Self> {
        let mut attributes = options.vertex_array_attributes;
        let mut converted = vec![];
        for (name, a) in geometry.attributes.iter() {
            ensure((1..=4).contains(&a.components_per_attribute), "Invalid geometry attribute component count")?;
            let datatype = if a.component_datatype == D::Double { D::Float } else { a.component_datatype };
            let values = TypedArray::from_values(datatype, &a.values.to_f64_vec());
            ensure(
                values.len().is_multiple_of(a.components_per_attribute as usize),
                "Geometry attribute has an incomplete vertex",
            )?;
            converted.push((name, a, datatype, typed_array_bytes(&values)));
        }
        if options.interleave && !converted.is_empty() {
            let count = converted[0].1.values.len() / converted[0].1.components_per_attribute as usize;
            ensure(
                converted.iter().all(|(_, a, _, _)| a.values.len() / a.components_per_attribute as usize == count),
                "Each attribute list must have the same number of vertices",
            )?;
            let mut sorted: Vec<_> = (0..converted.len()).collect();
            sorted.sort_by_key(|i| std::cmp::Reverse(converted[*i].2.get_size_in_bytes()));
            let mut offsets = vec![0; converted.len()];
            let mut stride = 0;
            for i in &sorted {
                offsets[*i] = stride;
                stride += converted[*i].1.components_per_attribute as usize * converted[*i].2.get_size_in_bytes();
            }
            let alignment = converted[sorted[0]].2.get_size_in_bytes();
            stride = stride.div_ceil(alignment) * alignment;
            ensure(stride <= 255, "Interleaved geometry stride exceeds the WebGL limit")?;
            let length = count
                .checked_mul(stride)
                .ok_or_else(|| crate::core::RuntimeError::new("Interleaved buffer length overflow"))?;
            size::<G::Object>(length)?;
            let mut bytes = vec![0; length];
            for (i, (_, a, d, data)) in converted.iter().enumerate() {
                let width = a.components_per_attribute as usize * d.get_size_in_bytes();
                for vertex in 0..count {
                    let offset = vertex * stride + offsets[i];
                    bytes[offset..offset + width].copy_from_slice(&data[vertex * width..(vertex + 1) * width]);
                }
            }
            let buffer = Buffer::new(context, BufferKind::Vertex, options.buffer_usage, BufferData::Bytes(&bytes))?;
            for (i, (name, a, d, _)) in converted.iter().enumerate() {
                let index = options.attribute_locations.get(*name).copied().unwrap_or(attributes.len() as u32);
                let mut attribute = VertexAttribute::buffer(index, buffer.clone(), a.components_per_attribute as u32);
                attribute.datatype = *d;
                attribute.normalize = a.normalize;
                attribute.offset = offsets[i];
                attribute.stride = stride;
                attributes.push(attribute);
            }
        } else {
            for (name, a, d, bytes) in converted {
                let index = options.attribute_locations.get(name).copied().unwrap_or(attributes.len() as u32);
                let b = Buffer::new(context, BufferKind::Vertex, options.buffer_usage, BufferData::Bytes(&bytes))?;
                let mut attribute = VertexAttribute::buffer(index, b, a.components_per_attribute as u32);
                attribute.datatype = d;
                attribute.normalize = a.normalize;
                attributes.push(attribute);
            }
        }
        let indices = if let Some(values) = &geometry.indices {
            let count = geometry
                .compute_number_of_vertices()
                .ok_or_else(|| crate::core::RuntimeError::new("Geometry attributes disagree on vertex count"))?;
            let d = if count >= 65536. { IndexDatatype::UnsignedInt } else { IndexDatatype::UnsignedShort };
            let converted = TypedArray::from_values(
                if d == IndexDatatype::UnsignedInt { D::UnsignedInt } else { D::UnsignedShort },
                &values.to_f64_vec(),
            );
            Some(Buffer::new(
                context,
                BufferKind::Index(d),
                options.buffer_usage,
                BufferData::Bytes(&typed_array_bytes(&converted)),
            )?)
        } else {
            None
        };
        Self::new(context, attributes, indices)
    }
}
