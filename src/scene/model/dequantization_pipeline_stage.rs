//! CesiumJS `DequantizationPipelineStage`: generates the vertex shader
//! function that expands quantized attributes and the uniforms it reads.
use super::{model_utility, PrimitiveRenderResources};
use crate::{
    core::RuntimeError,
    renderer::{Gl, ShaderDestination, UniformValue},
    scene::{model_components as mc, VertexAttributeSemantic},
};
use std::rc::Rc;

const FUNCTION_ID: &str = "dequantizationStage";
const FUNCTION_SIGNATURE: &str = "void dequantizationStage(inout ProcessedAttributes attributes)";

/// Process a primitive with quantized attributes.
pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
) -> Result<(), RuntimeError> {
    let has_classification = resources.model.borrow().classification_type.is_some();
    resources.shader_builder.add_define("USE_DEQUANTIZATION", None, ShaderDestination::VERTEX);
    resources.shader_builder.add_function(FUNCTION_ID, FUNCTION_SIGNATURE, ShaderDestination::VERTEX);

    for attribute in &primitive.attributes {
        let Some(quantization) = &attribute.quantization else { continue };
        let is_position =
            matches!(attribute.semantic, Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)));
        let is_texcoord =
            matches!(attribute.semantic, Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Texcoord)));
        if has_classification && !is_position && !is_texcoord {
            continue;
        }
        let info = model_utility::get_attribute_info(attribute)?;
        let variable = &info.variable_name;
        let line = if quantization.oct_encoded {
            let swizzle = if quantization.oct_encoded_zxy { ".zxy" } else { ".xyz" };
            format!(
                "attributes.{variable} = czm_octDecode(a_quantized_{variable}, model_normalizationRange_{variable}){swizzle};"
            )
        } else {
            format!(
                "attributes.{variable} = model_quantizedVolumeOffset_{variable} + a_quantized_{variable} * model_quantizedVolumeStepSize_{variable};"
            )
        };
        resources.shader_builder.add_function_lines(FUNCTION_ID, &[line])?;
        add_uniforms(resources, attribute, &info)?;
    }
    Ok(())
}

fn add_uniforms<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    attribute: &mc::Attribute<G>,
    info: &model_utility::AttributeInfo,
) -> Result<(), RuntimeError> {
    let variable = &info.variable_name;
    let quantization = attribute.quantization.as_ref().expect("checked above");
    if quantization.oct_encoded {
        let name = format!("model_normalizationRange_{variable}");
        resources.shader_builder.add_uniform("float", &name, ShaderDestination::VERTEX);
        let value = quantization
            .normalization_range
            .map(mc::MathValue::to_f32_vec)
            .and_then(|v| v.first().copied())
            .unwrap_or(f32::NAN);
        resources.uniform_map.insert(name, Rc::new(move || UniformValue::Float(vec![value])));
        return Ok(());
    }

    let offset_name = format!("model_quantizedVolumeOffset_{variable}");
    let step_name = format!("model_quantizedVolumeStepSize_{variable}");
    resources.shader_builder.add_uniform(info.glsl_type, &offset_name, ShaderDestination::VERTEX);
    resources.shader_builder.add_uniform(info.glsl_type, &step_name, ShaderDestination::VERTEX);

    let mut offset = quantization.quantized_volume_offset.map(mc::MathValue::to_f32_vec).unwrap_or_default();
    let mut step = quantization.quantized_volume_step_size.map(mc::MathValue::to_f32_vec).unwrap_or_default();
    if is_color(variable) {
        promote_to_vec4(&mut offset, 0.);
        promote_to_vec4(&mut step, 1.);
    }
    resources.uniform_map.insert(offset_name, Rc::new(move || UniformValue::Float(offset.clone())));
    resources.uniform_map.insert(step_name, Rc::new(move || UniformValue::Float(step.clone())));
    Ok(())
}

fn is_color(variable: &str) -> bool {
    variable.strip_prefix("color_").is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

fn promote_to_vec4(value: &mut Vec<f32>, alpha: f32) {
    if value.len() == 3 {
        value.push(alpha);
    }
}
