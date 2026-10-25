//! CesiumJS `GeometryPipelineStage`: declares a primitive's vertex attributes,
//! fills the `ProcessedAttributes` structs and generates the dynamic varying
//! assignments. Positions, normals and tangents are handled statically by
//! `GeometryStageVS`; everything else is generated here.
use super::{model_utility, ModelType, PrimitiveRenderResources};
use crate::{
    core::{ComponentDatatype, RuntimeError},
    renderer::{AttributeSource, Gl, ShaderDestination, VertexAttribute},
    scene::{model_components as mc, AttributeType, SceneMode, VertexAttributeSemantic},
    shaders,
};
use std::rc::Rc;

const STRUCT_ID_PROCESSED_ATTRIBUTES_VS: &str = "ProcessedAttributesVS";
const STRUCT_ID_PROCESSED_ATTRIBUTES_FS: &str = "ProcessedAttributesFS";
const STRUCT_NAME_PROCESSED_ATTRIBUTES: &str = "ProcessedAttributes";
const STRUCT_ID_SELECTED_FEATURE: &str = "SelectedFeature";
const STRUCT_NAME_SELECTED_FEATURE: &str = "SelectedFeature";
const FUNCTION_ID_INITIALIZE_ATTRIBUTES: &str = "initializeAttributes";
const FUNCTION_SIGNATURE_INITIALIZE_ATTRIBUTES: &str = "void initializeAttributes(out ProcessedAttributes attributes)";
const FUNCTION_ID_SET_DYNAMIC_VARYINGS_VS: &str = "setDynamicVaryingsVS";
const FUNCTION_ID_SET_DYNAMIC_VARYINGS_FS: &str = "setDynamicVaryingsFS";
const FUNCTION_SIGNATURE_SET_DYNAMIC_VARYINGS: &str = "void setDynamicVaryings(inout ProcessedAttributes attributes)";

fn attribute_type(attribute: &mc::Attribute<impl Gl>) -> Result<AttributeType, RuntimeError> {
    match &attribute.quantization {
        Some(quantization) => quantization.attribute_type,
        None => attribute.attribute_type,
    }
    .ok_or_else(|| RuntimeError::new("Missing attribute type"))
}

fn component_datatype(attribute: &mc::Attribute<impl Gl>) -> Result<ComponentDatatype, RuntimeError> {
    match &attribute.quantization {
        Some(quantization) => quantization.component_datatype,
        None => attribute.component_datatype,
    }
    .ok_or_else(|| RuntimeError::new("Missing attribute component datatype"))
}

fn is_position(attribute: &mc::Attribute<impl Gl>) -> bool {
    matches!(attribute.semantic, Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)))
}

fn set_index_text(attribute: &mc::Attribute<impl Gl>) -> String {
    attribute.set_index.map_or_else(|| "undefined".into(), |v| v.to_string())
}

/// Process a primitive's attributes into render resources and shader source.
pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    mode: SceneMode,
    scene_3d_only: bool,
    webgl2: bool,
) -> Result<(), RuntimeError> {
    let is_tile_pnts = resources.model.borrow().model_type == ModelType::TilePnts;
    {
        let shader = &mut resources.shader_builder;
        shader.add_struct(
            STRUCT_ID_PROCESSED_ATTRIBUTES_VS,
            STRUCT_NAME_PROCESSED_ATTRIBUTES,
            ShaderDestination::VERTEX,
        );
        shader.add_struct(
            STRUCT_ID_PROCESSED_ATTRIBUTES_FS,
            STRUCT_NAME_PROCESSED_ATTRIBUTES,
            ShaderDestination::FRAGMENT,
        );
        // The Feature struct is always declared; it may be unused.
        shader.add_struct(STRUCT_ID_SELECTED_FEATURE, STRUCT_NAME_SELECTED_FEATURE, ShaderDestination::BOTH);
        shader.add_function(
            FUNCTION_ID_INITIALIZE_ATTRIBUTES,
            FUNCTION_SIGNATURE_INITIALIZE_ATTRIBUTES,
            ShaderDestination::VERTEX,
        );
        shader.add_varying("vec3", "v_positionWC", None);
        shader.add_varying("vec3", "v_positionEC", None);
        shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_FS, "vec3", "positionWC")?;
        shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_FS, "vec3", "positionEC")?;
        shader.add_function(
            FUNCTION_ID_SET_DYNAMIC_VARYINGS_VS,
            FUNCTION_SIGNATURE_SET_DYNAMIC_VARYINGS,
            ShaderDestination::VERTEX,
        );
        shader.add_function(
            FUNCTION_ID_SET_DYNAMIC_VARYINGS_FS,
            FUNCTION_SIGNATURE_SET_DYNAMIC_VARYINGS,
            ShaderDestination::FRAGMENT,
        );
        if is_tile_pnts {
            shader.add_define("HAS_SRGB_COLOR", None, ShaderDestination::FRAGMENT);
        }
    }

    let use_2d = mode != SceneMode::Scene3D && !scene_3d_only && resources.model.borrow().project_to_2d;
    let instanced = resources.has_instances;
    let increment_index_for_2d = use_2d && !instanced;

    for attribute in &primitive.attributes {
        let location_count = attribute
            .attribute_type
            .ok_or_else(|| RuntimeError::new("Missing attribute type"))?
            .get_attribute_location_count();
        if attribute.buffer.is_none() && attribute.constant.is_none() {
            return Err(RuntimeError::new("Attributes must be provided as a Buffer or constant value"));
        }
        let index = if location_count > 1 {
            let index = resources.attribute_index;
            resources.attribute_index += location_count as u32;
            index
        } else if is_position(attribute) && !increment_index_for_2d {
            0
        } else {
            let index = resources.attribute_index;
            resources.attribute_index += 1;
            index
        };
        let interpolation =
            (!is_position(attribute) && primitive.primitive_type == Some(crate::core::PrimitiveType::Points) && webgl2)
                .then_some("flat");
        process_attribute(resources, attribute, index, location_count, use_2d, instanced, interpolation)?;
    }

    handle_bitangents(resources, &primitive.attributes)?;
    if primitive.primitive_type == Some(crate::core::PrimitiveType::Points) {
        resources.shader_builder.add_define("PRIMITIVE_TYPE_POINTS", None, ShaderDestination::BOTH);
    }
    resources.shader_builder.add_vertex_lines(&[shaders::model::GEOMETRY_STAGE_VS.source]);
    resources.shader_builder.add_fragment_lines(&[shaders::model::GEOMETRY_STAGE_FS.source]);
    Ok(())
}

fn process_attribute<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    attribute: &mc::Attribute<G>,
    index: u32,
    location_count: usize,
    use_2d: bool,
    instanced: bool,
    interpolation: Option<&str>,
) -> Result<(), RuntimeError> {
    let info = model_utility::get_attribute_info(attribute)?;
    let modify_for_2d = use_2d && !instanced;

    if location_count > 1 {
        add_matrix_attribute(resources, attribute, index, location_count)?;
    } else {
        add_attribute(resources, attribute, index, modify_for_2d)?;
    }

    add_attribute_declaration(resources, attribute, &info, modify_for_2d)?;
    add_varying_declaration(resources, &info, interpolation);
    if attribute.semantic.is_some() {
        add_semantic_define(resources, attribute);
    }
    update_attributes_struct(resources, &info, use_2d)?;
    update_initialize_attributes(resources, &info, modify_for_2d)?;
    update_set_dynamic_varyings(resources, attribute, &info)?;
    Ok(())
}

fn add_semantic_define<G: Gl>(resources: &mut PrimitiveRenderResources<G>, attribute: &mc::Attribute<G>) {
    let shader = &mut resources.shader_builder;
    let set_index = set_index_text(attribute);
    match attribute.semantic {
        Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Normal)) => {
            shader.add_define("HAS_NORMALS", None, ShaderDestination::BOTH)
        }
        Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Tangent)) => {
            shader.add_define("HAS_TANGENTS", None, ShaderDestination::BOTH)
        }
        Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId)) => {
            shader.add_define(&format!("HAS_FEATURE_ID_{set_index}"), None, ShaderDestination::BOTH)
        }
        Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Texcoord | VertexAttributeSemantic::Color)) => {
            shader.add_define(
                &format!("HAS_{}_{set_index}", attribute.semantic.unwrap().as_str()),
                None,
                ShaderDestination::BOTH,
            )
        }
        _ => {}
    }
}

fn add_attribute<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    attribute: &mc::Attribute<G>,
    attribute_index: u32,
    modify_for_2d: bool,
) -> Result<(), RuntimeError> {
    let semantic = attribute.semantic;
    let set_index = attribute.set_index;
    let position = is_position(attribute);
    if matches!(semantic, Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId))) {
        if let Some(set_index) = set_index {
            if set_index >= resources.feature_id_vertex_attribute_set_index {
                resources.feature_id_vertex_attribute_set_index = set_index + 1;
            }
        }
    }

    let index = if position { 0 } else { attribute_index };
    let components = attribute_type(attribute)?.get_number_of_components() as u32;
    let source = match (&attribute.buffer, attribute.constant) {
        (Some(buffer), _) => AttributeSource::Buffer(buffer.clone()),
        (None, Some(value)) => AttributeSource::Constant(value.to_f32_vec()),
        (None, None) => return Err(RuntimeError::new("Attributes must be provided as a Buffer or constant value")),
    };
    resources.attributes.push(Rc::new(VertexAttribute {
        index,
        enabled: true,
        source,
        components,
        datatype: component_datatype(attribute)?,
        normalize: attribute.normalized,
        offset: attribute.byte_offset.unwrap_or(0),
        stride: attribute.byte_stride.unwrap_or(0),
        instance_divisor: 0,
    }));

    if position && modify_for_2d {
        let buffer = resources
            .position_buffer_2d
            .clone()
            .ok_or_else(|| RuntimeError::new("Missing runtimePrimitive.positionBuffer2D"))?;
        resources.attributes.push(Rc::new(VertexAttribute {
            index: attribute_index,
            enabled: true,
            source: AttributeSource::Buffer(buffer),
            components,
            datatype: ComponentDatatype::Float,
            normalize: attribute.normalized,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        }));
    }
    Ok(())
}

fn add_matrix_attribute<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    attribute: &mc::Attribute<G>,
    attribute_index: u32,
    column_count: usize,
) -> Result<(), RuntimeError> {
    let attribute_type = attribute_type(attribute)?;
    let datatype = component_datatype(attribute)?;
    let component_count = attribute_type.get_number_of_components();
    let components_per_column = component_count / column_count;
    let column_length = components_per_column * datatype.get_size_in_bytes();
    for column in 0..column_count {
        let source = match (&attribute.buffer, attribute.constant) {
            (Some(buffer), _) => AttributeSource::Buffer(buffer.clone()),
            (None, Some(value)) => {
                let values = value.to_f32_vec();
                let start = column * components_per_column;
                AttributeSource::Constant(values[start..start + components_per_column].to_vec())
            }
            (None, None) => return Err(RuntimeError::new("Attributes must be provided as a Buffer or constant value")),
        };
        resources.attributes.push(Rc::new(VertexAttribute {
            index: attribute_index + column as u32,
            enabled: true,
            source,
            components: components_per_column as u32,
            datatype,
            normalize: attribute.normalized,
            offset: attribute.byte_offset.unwrap_or(0) + column * column_length,
            stride: attribute.byte_stride.unwrap_or(0),
            instance_divisor: 0,
        }));
    }
    Ok(())
}

fn add_varying_declaration<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    info: &model_utility::AttributeInfo,
    interpolation: Option<&str>,
) {
    let (name, glsl_type) = if info.variable_name == "normalMC" {
        ("v_normalEC".to_string(), info.glsl_type)
    } else if info.variable_name == "tangentMC" {
        ("v_tangentEC".to_string(), "vec3")
    } else {
        (format!("v_{}", info.variable_name), info.glsl_type)
    };
    resources.shader_builder.add_varying(glsl_type, &name, interpolation);
}

fn add_attribute_declaration<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    attribute: &mc::Attribute<G>,
    info: &model_utility::AttributeInfo,
    modify_for_2d: bool,
) -> Result<(), RuntimeError> {
    let (name, glsl_type) = if info.is_quantized {
        (
            format!("a_quantized_{}", info.variable_name),
            info.quantized_glsl_type.ok_or_else(|| RuntimeError::new("Missing quantized attribute type"))?,
        )
    } else {
        (format!("a_{}", info.variable_name), info.glsl_type)
    };
    let position = is_position(attribute);
    if position {
        resources.shader_builder.set_position_attribute(glsl_type, &name)?;
    } else {
        resources.shader_builder.add_attribute(glsl_type, &name);
    }
    if position && modify_for_2d {
        resources.shader_builder.add_attribute("vec3", "a_position2D");
    }
    Ok(())
}

fn update_attributes_struct<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    info: &model_utility::AttributeInfo,
    use_2d: bool,
) -> Result<(), RuntimeError> {
    let shader = &mut resources.shader_builder;
    match info.variable_name.as_str() {
        "tangentMC" => {
            shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_VS, "vec3", "tangentMC")?;
            shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_VS, "float", "tangentSignMC")?;
            shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_FS, "vec3", "tangentEC")?;
        }
        "normalMC" => {
            shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_VS, "vec3", "normalMC")?;
            shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_FS, "vec3", "normalEC")?;
        }
        _ => {
            shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_VS, info.glsl_type, &info.variable_name)?;
            shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_FS, info.glsl_type, &info.variable_name)?;
        }
    }
    if info.variable_name == "positionMC" && use_2d {
        shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_VS, "vec3", "position2D")?;
    }
    Ok(())
}

fn update_initialize_attributes<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    info: &model_utility::AttributeInfo,
    use_2d: bool,
) -> Result<(), RuntimeError> {
    if info.variable_name == "positionMC" && use_2d {
        resources
            .shader_builder
            .add_function_lines(FUNCTION_ID_INITIALIZE_ATTRIBUTES, &["attributes.position2D = a_position2D;"])?;
    }
    if info.is_quantized {
        return Ok(());
    }
    let lines: Vec<String> = if info.variable_name == "tangentMC" {
        vec!["attributes.tangentMC = a_tangentMC.xyz;".into(), "attributes.tangentSignMC = a_tangentMC.w;".into()]
    } else {
        vec![format!("attributes.{} = a_{};", info.variable_name, info.variable_name)]
    };
    resources.shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_ATTRIBUTES, &lines)?;
    Ok(())
}

fn update_set_dynamic_varyings<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    attribute: &mc::Attribute<G>,
    info: &model_utility::AttributeInfo,
) -> Result<(), RuntimeError> {
    if attribute.semantic.is_some() && attribute.set_index.is_none() {
        return Ok(());
    }
    let variable = &info.variable_name;
    resources
        .shader_builder
        .add_function_lines(FUNCTION_ID_SET_DYNAMIC_VARYINGS_VS, &[format!("v_{variable} = attributes.{variable};")])?;
    resources
        .shader_builder
        .add_function_lines(FUNCTION_ID_SET_DYNAMIC_VARYINGS_FS, &[format!("attributes.{variable} = v_{variable};")])?;
    Ok(())
}

fn handle_bitangents<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    attributes: &[mc::Attribute<G>],
) -> Result<(), RuntimeError> {
    let has = |semantic| attributes.iter().any(|a| a.semantic == Some(mc::AttributeSemantic::Vertex(semantic)));
    if !has(VertexAttributeSemantic::Normal) || !has(VertexAttributeSemantic::Tangent) {
        return Ok(());
    }
    let shader = &mut resources.shader_builder;
    shader.add_define("HAS_BITANGENTS", None, ShaderDestination::BOTH);
    shader.add_varying("vec3", "v_bitangentEC", None);
    shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_VS, "vec3", "bitangentMC")?;
    shader.add_struct_field(STRUCT_ID_PROCESSED_ATTRIBUTES_FS, "vec3", "bitangentEC")?;
    Ok(())
}
