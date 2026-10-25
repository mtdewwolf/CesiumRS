//! CesiumJS `Scene/Model/WireframePipelineStage`: generates wireframe line indices
//! for triangle primitives when debug wireframe mode is enabled.

use super::{model_utility, ModelStatistics, PrimitiveRenderResources};
use crate::core::{
    wireframe_index_generator, ComponentDatatype, IndexDatatype, PrimitiveType, RuntimeError, TypedArray,
};
use crate::renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, ShaderDestination};
use crate::scene::{model_components as mc, VertexAttributeSemantic};

fn typed_array_to_u32_vec(typed: &TypedArray) -> Vec<u32> {
    (0..typed.len()).map(|i| typed.get(i).unwrap_or(0.0) as u32).collect()
}

pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
) -> Result<(), RuntimeError> {
    let position_attribute = model_utility::get_attribute_by_semantic(
        &primitive.attributes,
        mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position),
        None,
    )
    .ok_or_else(|| RuntimeError::new("Missing position attribute"))?;
    let vertex_count = position_attribute.count.ok_or_else(|| RuntimeError::new("Missing position attribute count"))?;

    let primitive_type = primitive.primitive_type.ok_or_else(|| RuntimeError::new("Missing primitiveType"))?;
    let original_count = resources.count.ok_or_else(|| RuntimeError::new("Missing primitive count"))?;

    if !matches!(primitive_type, PrimitiveType::Triangles | PrimitiveType::TriangleStrip | PrimitiveType::TriangleFan) {
        return Ok(());
    }

    resources.shader_builder.add_define("HAS_WIREFRAME", None, ShaderDestination::FRAGMENT);

    let original_indices_u32 = if let Some(indices) = &primitive.indices {
        if let Some(buffer) = &indices.buffer {
            if context.webgl2() {
                let index_datatype = buffer
                    .index_datatype()
                    .or(indices.index_datatype)
                    .ok_or_else(|| RuntimeError::new("Missing index datatype"))?;
                let count = indices.count.unwrap_or_else(|| buffer.number_of_indices().unwrap_or(0));
                let byte_length = count * index_datatype.get_size_in_bytes();
                let bytes = buffer.get_buffer_data(0, byte_length)?;
                let component_datatype = match index_datatype {
                    IndexDatatype::UnsignedByte => ComponentDatatype::UnsignedByte,
                    IndexDatatype::UnsignedShort => ComponentDatatype::UnsignedShort,
                    IndexDatatype::UnsignedInt => ComponentDatatype::UnsignedInt,
                };
                let typed = component_datatype
                    .create_array_buffer_view(&bytes, 0, Some(count))
                    .ok_or_else(|| RuntimeError::new("Failed to read index buffer view"))?;
                Some(typed_array_to_u32_vec(&typed))
            } else {
                indices.typed_array.as_ref().map(typed_array_to_u32_vec)
            }
        } else {
            indices.typed_array.as_ref().map(typed_array_to_u32_vec)
        }
    } else {
        None
    };

    let wireframe_indices = wireframe_index_generator::create_wireframe_indices(
        primitive_type,
        vertex_count,
        original_indices_u32.as_deref(),
    )
    .ok_or_else(|| RuntimeError::new("Failed to generate wireframe indices"))?;

    let index_datatype = match &wireframe_indices {
        TypedArray::Uint8(_) => IndexDatatype::UnsignedByte,
        TypedArray::Uint16(_) => IndexDatatype::UnsignedShort,
        TypedArray::Uint32(_) => IndexDatatype::UnsignedInt,
        _ => return Err(RuntimeError::new("Invalid wireframe index typed array")),
    };

    let bytes = wireframe_indices.to_le_bytes();
    let wireframe_index_buffer =
        Buffer::new(context, BufferKind::Index(index_datatype), BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
    statistics.add_buffer(&wireframe_index_buffer, false);

    resources.wireframe_index_buffer = Some(wireframe_index_buffer);
    resources.primitive_type = Some(PrimitiveType::Lines);
    resources.count = Some(wireframe_index_generator::get_wireframe_indices_count(primitive_type, original_count));

    Ok(())
}
