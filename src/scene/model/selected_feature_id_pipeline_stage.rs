//! CesiumJS `Scene/Model/SelectedFeatureIdPipelineStage`: configures the
//! `SelectedFeature` struct, `SELECTED_FEATURE_ID` defines, `model_nullFeatureId`
//! uniform, and `SelectedFeatureIdStageCommon` shader code.

use super::{model_utility, PrimitiveRenderResources};
use crate::core::RuntimeError;
use crate::renderer::{Gl, ShaderDestination, UniformValue};
use crate::scene::model_components as mc;
use crate::shaders;
use std::rc::Rc;

pub const STRUCT_ID_SELECTED_FEATURE: &str = "SelectedFeature";
pub const STRUCT_NAME_SELECTED_FEATURE: &str = "SelectedFeature";

fn get_selected_feature_id<'a, G: Gl>(
    resources: &PrimitiveRenderResources<G>,
    node: &'a mc::Node<G>,
    primitive: &'a mc::Primitive<G>,
) -> Option<&'a mc::FeatureId<G>> {
    let model = resources.model.borrow();
    if let Some(instances) = &node.instances {
        let feature_ids =
            model_utility::get_feature_ids_by_label(&instances.feature_ids, &model.instance_feature_id_label);
        if let Some(fid) = feature_ids {
            return Some(fid);
        }
    }
    model_utility::get_feature_ids_by_label(&primitive.feature_ids, &model.feature_id_label)
}

pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    node: &mc::Node<G>,
    primitive: &mc::Primitive<G>,
) -> Result<(), RuntimeError> {
    // If this pipeline stage is running, we already know the primitive uses a property table.
    resources.has_property_table = true;

    let Some(feature_ids) = get_selected_feature_id(resources, node, primitive) else {
        return Ok(());
    };
    if feature_ids.property_table_id().is_none() {
        return Ok(());
    }

    let is_texture = matches!(feature_ids, mc::FeatureId::Texture(_));
    let shader_direction = if is_texture { ShaderDestination::FRAGMENT } else { ShaderDestination::BOTH };

    let variable_name = feature_ids.label().or_else(|| feature_ids.positional_label()).unwrap_or("undefined");

    {
        let shader = &mut resources.shader_builder;
        shader.add_define("HAS_SELECTED_FEATURE_ID", None, shader_direction);
        shader.add_define("SELECTED_FEATURE_ID", Some(variable_name), shader_direction);

        if is_texture {
            shader.add_define("HAS_SELECTED_FEATURE_ID_TEXTURE", None, shader_direction);
        } else {
            shader.add_define("HAS_SELECTED_FEATURE_ID_ATTRIBUTE", None, shader_direction);
        }

        // Precision qualifiers not needed here as precision header is added prior to structs.
        shader.add_struct_field(STRUCT_ID_SELECTED_FEATURE, "int", "id")?;
        shader.add_struct_field(STRUCT_ID_SELECTED_FEATURE, "vec2", "st")?;
        shader.add_struct_field(STRUCT_ID_SELECTED_FEATURE, "vec4", "color")?;
    }

    if let Some(null_feature_id) = feature_ids.null_feature_id() {
        resources.shader_builder.add_define("HAS_NULL_FEATURE_ID", None, shader_direction);
        resources.shader_builder.add_uniform("int", "model_nullFeatureId", shader_direction);
        resources
            .uniform_map
            .insert("model_nullFeatureId".into(), Rc::new(move || UniformValue::Int(vec![null_feature_id as i32])));
    }

    resources.shader_builder.add_fragment_lines(&[shaders::model::SELECTED_FEATURE_ID_STAGE_COMMON.source]);
    if shader_direction == ShaderDestination::BOTH {
        resources.shader_builder.add_vertex_lines(&[shaders::model::SELECTED_FEATURE_ID_STAGE_COMMON.source]);
    }

    Ok(())
}
