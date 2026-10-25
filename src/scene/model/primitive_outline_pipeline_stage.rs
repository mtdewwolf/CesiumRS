//! CesiumJS `Scene/Model/PrimitiveOutlinePipelineStage`: configures the
//! `CESIUM_primitive_outline` vertex attribute, lookup texture, uniforms, and shaders.

use super::{PrimitiveOutlineGenerator, PrimitiveRenderResources};
use crate::core::{ComponentDatatype, RuntimeError};
use crate::renderer::{Context, Gl, ShaderDestination, UniformValue, VertexAttribute};
use crate::scene::model_components as mc;
use crate::shaders;
use std::rc::Rc;

pub fn process<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    context: &Context<G>,
) -> Result<(), RuntimeError> {
    let outline_coordinates = primitive
        .outline_coordinates
        .as_ref()
        .ok_or_else(|| RuntimeError::new("Missing primitive.outlineCoordinates"))?;
    let buffer = outline_coordinates
        .buffer
        .as_ref()
        .ok_or_else(|| RuntimeError::new("Missing outlineCoordinates.buffer"))?
        .clone();

    let attribute_index = resources.attribute_index;
    resources.attribute_index += 1;

    let mut vertex_attribute = VertexAttribute::buffer(
        attribute_index,
        buffer,
        outline_coordinates
            .attribute_type
            .ok_or_else(|| RuntimeError::new("Missing outlineCoordinates.type"))?
            .get_number_of_components() as u32,
    );
    vertex_attribute.datatype = outline_coordinates.component_datatype.unwrap_or(ComponentDatatype::Float);
    vertex_attribute.offset = outline_coordinates.byte_offset.unwrap_or(0);
    vertex_attribute.stride = outline_coordinates.byte_stride.unwrap_or(0);
    vertex_attribute.normalize = outline_coordinates.normalized;
    resources.attributes.push(Rc::new(vertex_attribute));

    let outline_texture = PrimitiveOutlineGenerator::create_texture(context)?;
    resources.texture_uniform_map.insert("model_outlineTexture".into(), Rc::new(move || Some(outline_texture.clone())));

    let model = resources.model.clone();
    resources.uniform_map.insert(
        "model_outlineColor".into(),
        Rc::new(move || {
            let c = model.borrow().outline_color;
            UniformValue::Float(vec![c.red as f32, c.green as f32, c.blue as f32, c.alpha as f32])
        }),
    );

    let model = resources.model.clone();
    resources.uniform_map.insert(
        "model_showOutline".into(),
        Rc::new(move || UniformValue::Int(vec![i32::from(model.borrow().show_outline)])),
    );

    let shader = &mut resources.shader_builder;
    shader.add_define("HAS_PRIMITIVE_OUTLINE", None, ShaderDestination::BOTH);
    shader.add_attribute("vec3", "a_outlineCoordinates");
    shader.add_varying("vec3", "v_outlineCoordinates", None);
    shader.add_uniform("sampler2D", "model_outlineTexture", ShaderDestination::FRAGMENT);
    shader.add_uniform("bool", "model_showOutline", ShaderDestination::FRAGMENT);
    shader.add_uniform("vec4", "model_outlineColor", ShaderDestination::FRAGMENT);
    shader.add_vertex_lines(&[shaders::model::PRIMITIVE_OUTLINE_STAGE_VS.source]);
    shader.add_fragment_lines(&[shaders::model::PRIMITIVE_OUTLINE_STAGE_FS.source]);

    Ok(())
}
