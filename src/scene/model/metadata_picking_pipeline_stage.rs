//! Default metadata picking defines and shader function, replaced by derived
//! commands when a metadata property is selected.
use super::PipelineResources;
use crate::core::RuntimeError;
use crate::renderer::ShaderDestination;

pub const FUNCTION_ID_METADATA_PICKING_STAGE: &str = "metadataPickingStage";
pub const METADATA_PICKING_VALUE_TYPE_DEFAULT: &str = "float";
pub const METADATA_PICKING_VALUE_COMPONENT_DEFAULT: &str = "0.0";
pub const METADATA_PICKING_VALUE_STRING_DEFAULT: &str = "0.0";

pub fn process(resources: &mut PipelineResources) -> Result<(), RuntimeError> {
    let shader = &mut resources.shader_builder;
    for (name, value) in [
        ("METADATA_PICKING_VALUE_TYPE", METADATA_PICKING_VALUE_TYPE_DEFAULT),
        ("METADATA_PICKING_VALUE_STRING", METADATA_PICKING_VALUE_STRING_DEFAULT),
        ("METADATA_PICKING_VALUE_COMPONENT_X", METADATA_PICKING_VALUE_COMPONENT_DEFAULT),
        ("METADATA_PICKING_VALUE_COMPONENT_Y", METADATA_PICKING_VALUE_COMPONENT_DEFAULT),
        ("METADATA_PICKING_VALUE_COMPONENT_Z", METADATA_PICKING_VALUE_COMPONENT_DEFAULT),
        ("METADATA_PICKING_VALUE_COMPONENT_W", METADATA_PICKING_VALUE_COMPONENT_DEFAULT),
    ] {
        shader.add_define(name, Some(value), ShaderDestination::FRAGMENT);
    }
    shader.add_function(
        FUNCTION_ID_METADATA_PICKING_STAGE,
        "void metadataPickingStage(Metadata metadata, MetadataClass metadataClass, inout vec4 metadataValues)",
        ShaderDestination::FRAGMENT,
    );
    shader.add_function_lines(
        FUNCTION_ID_METADATA_PICKING_STAGE,
        &[
            "METADATA_PICKING_VALUE_TYPE value = METADATA_PICKING_VALUE_TYPE(METADATA_PICKING_VALUE_STRING);",
            "metadataValues.x = METADATA_PICKING_VALUE_COMPONENT_X;",
            "metadataValues.y = METADATA_PICKING_VALUE_COMPONENT_Y;",
            "metadataValues.z = METADATA_PICKING_VALUE_COMPONENT_Z;",
            "metadataValues.w = METADATA_PICKING_VALUE_COMPONENT_W;",
        ],
    )?;
    Ok(())
}
