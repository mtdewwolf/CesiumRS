//! CesiumJS MorphTargetsPipelineStage: position, normal and tangent deltas.
use super::PrimitiveRenderResources;
use crate::{
    core::{ComponentDatatype, RuntimeError},
    renderer::{Gl, ShaderDestination, UniformValue, VertexAttribute},
    scene::{model_components as mc, VertexAttributeSemantic},
    shaders,
};
use std::{cell::RefCell, rc::Rc};
fn constant(v: Option<mc::MathValue>) -> Vec<f32> {
    match v {
        Some(mc::MathValue::Cartesian3(v)) => vec![v.x as f32, v.y as f32, v.z as f32],
        Some(mc::MathValue::Cartesian4(v)) => vec![v.x as f32, v.y as f32, v.z as f32, v.w as f32],
        _ => vec![0.; 3],
    }
}
pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    weights: Rc<RefCell<Vec<f64>>>,
) -> Result<(), RuntimeError> {
    resources.shader_builder.add_define("HAS_MORPH_TARGETS", None, ShaderDestination::VERTEX);
    for (title, name) in [("Position", "position"), ("Normal", "normal"), ("Tangent", "tangent")] {
        let id = format!("getMorphed{title}");
        resources.shader_builder.add_function(&id, &format!("vec3 {id}(in vec3 {name})"), ShaderDestination::VERTEX);
        resources.shader_builder.add_function_lines(&id, &[format!("vec3 morphed{title} = {name};")])?;
    }
    for (target, t) in primitive.morph_targets.iter().enumerate() {
        for attribute in &t.attributes {
            let title = match attribute.semantic {
                Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)) => "Position",
                Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Normal)) => "Normal",
                Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Tangent)) => "Tangent",
                _ => continue,
            };
            let mut vertex = if let Some(buffer) = &attribute.buffer {
                VertexAttribute::buffer(
                    resources.attribute_index,
                    buffer.clone(),
                    attribute
                        .attribute_type
                        .ok_or_else(|| RuntimeError::new("Missing morph attribute type"))?
                        .get_number_of_components() as u32,
                )
            } else {
                VertexAttribute::constant(resources.attribute_index, constant(attribute.constant))
            };
            vertex.datatype = attribute.component_datatype.unwrap_or(ComponentDatatype::Float);
            vertex.offset = attribute.byte_offset.unwrap_or(0);
            vertex.stride = attribute.byte_stride.unwrap_or(0);
            vertex.normalize = attribute.normalized;
            resources.attributes.push(Rc::new(vertex));
            resources.attribute_index += 1;
            let name = format!("a_target{title}_{target}");
            resources.shader_builder.add_attribute("vec3", &name);
            resources.shader_builder.add_function_lines(
                &format!("getMorphed{title}"),
                &[format!("morphed{title} += u_morphWeights[{target}] * {name};")],
            )?;
        }
    }
    for title in ["Position", "Normal", "Tangent"] {
        resources
            .shader_builder
            .add_function_lines(&format!("getMorphed{title}"), &[format!("return morphed{title};")])?;
    }
    resources.shader_builder.add_uniform(
        "float",
        &format!("u_morphWeights[{}]", weights.borrow().len()),
        ShaderDestination::VERTEX,
    );
    resources.shader_builder.add_vertex_lines(&[shaders::model::MORPH_TARGETS_STAGE_VS.source]);
    resources.uniform_map.insert(
        "u_morphWeights".into(),
        Rc::new(move || UniformValue::FloatArray {
            components: 1,
            values: weights.borrow().iter().map(|v| *v as f32).collect(),
        }),
    );
    Ok(())
}
