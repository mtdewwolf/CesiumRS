//! CesiumJS SkinningPipelineStage: weighted joint matrices in the vertex shader.
use super::PipelineResources;
use crate::{
    core::{Matrix4, RuntimeError},
    renderer::{Gl, ShaderDestination, UniformValue},
    scene::{model_components as mc, VertexAttributeSemantic},
    shaders,
};
use std::{cell::RefCell, rc::Rc};
pub fn process<G: Gl>(
    resources: &mut PipelineResources,
    primitive: &mc::Primitive<G>,
    joint_matrices: Rc<RefCell<Vec<Matrix4>>>,
) -> Result<(), RuntimeError> {
    let shader = &mut resources.shader_builder;
    shader.add_define("HAS_SKINNING", None, ShaderDestination::VERTEX);
    shader.add_function("getSkinningMatrix", "mat4 getSkinningMatrix()", ShaderDestination::VERTEX);
    shader.add_function_lines("getSkinningMatrix", &["mat4 skinnedMatrix = mat4(0);"])?;
    let maximum = primitive
        .attributes
        .iter()
        .filter(|a| {
            matches!(
                a.semantic,
                Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Joints | VertexAttributeSemantic::Weights))
            )
        })
        .filter_map(|a| a.set_index)
        .max();
    if let Some(maximum) = maximum {
        for set in 0..=maximum {
            for component in ["x", "y", "z", "w"] {
                shader.add_function_lines("getSkinningMatrix",&[format!("skinnedMatrix += a_weights_{set}.{component} * u_jointMatrices[int(a_joints_{set}.{component})];")])?;
            }
        }
    }
    shader.add_function_lines("getSkinningMatrix", &["return skinnedMatrix;"])?;
    shader.add_uniform(
        "mat4",
        &format!("u_jointMatrices[{}]", joint_matrices.borrow().len()),
        ShaderDestination::VERTEX,
    );
    shader.add_vertex_lines(&[shaders::model::SKINNING_STAGE_VS.source]);
    resources.uniform_map.insert(
        "u_jointMatrices".into(),
        Rc::new(move || UniformValue::MatrixArray {
            columns: 4,
            rows: 4,
            values: joint_matrices.borrow().iter().flat_map(|m| m.0.iter().map(|v| *v as f32)).collect(),
        }),
    );
    Ok(())
}
