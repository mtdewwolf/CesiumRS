//! CesiumJS `Scene/Model/CustomShaderPipelineStage`: inserts the user's GLSL
//! callbacks, the generated `VertexInput`/`FragmentInput` structs, extra
//! uniforms and varyings, and the custom uniform map into a primitive's render
//! resources.
//!
//! The stage fails gracefully where upstream does: a primitive missing an
//! attribute the shader reads gets the documented default value, and a
//! primitive whose metadata cannot satisfy the shader disables the custom
//! shader for that primitive only.

use super::{
    feature_id_pipeline_stage::STRUCT_NAME_FEATURE_IDS,
    metadata_pipeline_stage::{STRUCT_NAME_METADATA, STRUCT_NAME_METADATA_CLASS, STRUCT_NAME_METADATA_STATISTICS},
    model_utility, CustomShader, CustomShaderMode, CustomShaderTranslucencyMode, PrimitiveRenderResources,
};
use crate::{
    core::RuntimeError,
    renderer::{Gl, Pass, ShaderBuilder, ShaderDestination as D},
    scene::{model_components as mc, StructuralMetadata},
    shaders,
};
use std::{
    collections::{BTreeSet, HashSet},
    rc::Rc,
};

type Result<T> = std::result::Result<T, RuntimeError>;

pub const STRUCT_ID_ATTRIBUTES_VS: &str = "AttributesVS";
pub const STRUCT_ID_ATTRIBUTES_FS: &str = "AttributesFS";
pub const STRUCT_NAME_ATTRIBUTES: &str = "Attributes";
pub const STRUCT_ID_VERTEX_INPUT: &str = "VertexInput";
pub const STRUCT_NAME_VERTEX_INPUT: &str = "VertexInput";
pub const STRUCT_ID_FRAGMENT_INPUT: &str = "FragmentInput";
pub const STRUCT_NAME_FRAGMENT_INPUT: &str = "FragmentInput";
pub const FUNCTION_ID_INITIALIZE_INPUT_STRUCT_VS: &str = "initializeInputStructVS";
pub const FUNCTION_SIGNATURE_INITIALIZE_INPUT_STRUCT_VS: &str =
    "void initializeInputStruct(out VertexInput vsInput, ProcessedAttributes attributes)";
pub const FUNCTION_ID_INITIALIZE_INPUT_STRUCT_FS: &str = "initializeInputStructFS";
pub const FUNCTION_SIGNATURE_INITIALIZE_INPUT_STRUCT_FS: &str =
    "void initializeInputStruct(out FragmentInput fsInput, ProcessedAttributes attributes)";

/// A warning that disables a custom shader for one primitive. Upstream routes
/// these through `oneTimeWarning`; callers own the one-time de-duplication.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CustomShaderWarning {
    IncompatiblePrimitiveVertex { attribute: String },
    IncompatiblePrimitiveFragment { attribute: String },
    MetadataPropertyMissing { property: String },
}

/// One-time warning registry. `warn` returns true the first time a warning is
/// seen, matching `oneTimeWarning`'s logging behavior without global state.
#[derive(Default)]
pub struct CustomShaderWarnings {
    seen: HashSet<CustomShaderWarning>,
}
impl CustomShaderWarnings {
    pub fn warn(&mut self, warning: CustomShaderWarning) -> bool {
        self.seen.insert(warning)
    }
    pub fn len(&self) -> usize {
        self.seen.len()
    }
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

struct GeneratedLines {
    enabled: bool,
    attribute_fields: Vec<(&'static str, String)>,
    initialization_lines: Vec<String>,
}
impl GeneratedLines {
    fn disabled() -> Self {
        Self { enabled: false, attribute_fields: vec![], initialization_lines: vec![] }
    }
}

/// Process a primitive. Returns the warnings the caller should emit once.
pub fn process<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    structural_metadata: Option<&StructuralMetadata<G>>,
    warnings: &mut CustomShaderWarnings,
) -> Result<Vec<CustomShaderWarning>> {
    let Some(custom_shader) = resources.custom_shader.clone() else {
        return Ok(Vec::new());
    };
    let mut emitted = Vec::new();

    // Lighting and translucency apply even without custom shader text.
    if let Some(lighting_model) = custom_shader.lighting_model {
        resources.lighting_options.lighting_model = lighting_model;
    }
    match custom_shader.translucency_mode {
        CustomShaderTranslucencyMode::Translucent => {
            resources.alpha_options.borrow_mut().pass = Some(Pass::Translucent);
        }
        CustomShaderTranslucencyMode::Opaque => {
            // Upstream clears the material-selected pass so the default
            // (OPAQUE or 3D_TILES) applies; AlphaPipelineStage owns that.
            resources.alpha_options.borrow_mut().pass = None;
        }
        CustomShaderTranslucencyMode::Inherit => {}
    }

    let attributes_by_name = get_attributes_by_name(&primitive.attributes)?;
    let primitive_property_ids = get_all_property_ids(primitive, structural_metadata);
    let vertex_lines = generate_vertex_shader_lines(&custom_shader, &attributes_by_name, &primitive_property_ids);
    let fragment_lines = generate_fragment_shader_lines(&custom_shader, &attributes_by_name, &primitive_property_ids);
    for warning in vertex_lines.1.iter().chain(fragment_lines.1.iter()) {
        if warnings.warn(warning.clone()) {
            emitted.push(warning.clone());
        }
    }
    let vertex_lines = vertex_lines.0;
    let fragment_lines = fragment_lines.0;
    let should_compute_position_wc =
        custom_shader.used_variables_fragment.attribute_set.contains("positionWC") && fragment_lines.enabled;
    if !vertex_lines.enabled && !fragment_lines.enabled {
        return Ok(emitted);
    }
    add_lines_to_shader(&mut resources.shader_builder, &custom_shader, &vertex_lines, &fragment_lines)?;
    if should_compute_position_wc {
        resources.shader_builder.add_define("COMPUTE_POSITION_WC_CUSTOM_SHADER", None, D::BOTH);
    }
    if custom_shader.vertex_shader_text.is_some() {
        resources.shader_builder.add_define("HAS_CUSTOM_VERTEX_SHADER", None, D::VERTEX);
    }
    if custom_shader.fragment_shader_text.is_some() {
        resources.shader_builder.add_define("HAS_CUSTOM_FRAGMENT_SHADER", None, D::FRAGMENT);
        resources.shader_builder.add_define(&CustomShaderMode::get_define_name(custom_shader.mode), None, D::FRAGMENT);
    }
    for (name, uniform) in &custom_shader.uniforms {
        resources.pipeline.shader_builder.add_uniform(uniform.uniform_type.as_str(), name, D::BOTH);
        if uniform.uniform_type == super::UniformType::Sampler2D {
            let manager = custom_shader.texture_manager.clone();
            let name = name.clone();
            resources.texture_uniform_map.insert(name.clone(), Rc::new(move || manager.get_texture_or_default(&name)));
        } else {
            let value = uniform.value.clone();
            let uniform_type = uniform.uniform_type;
            resources.pipeline.uniform_map.insert(
                name.clone(),
                Rc::new(move || {
                    value.borrow().to_renderer(uniform_type).unwrap_or(crate::renderer::UniformValue::Float(vec![0.]))
                }),
            );
        }
    }
    for (name, varying_type) in &custom_shader.varyings {
        resources.pipeline.shader_builder.add_varying(varying_type.as_str(), name, None);
    }
    Ok(emitted)
}

/// `[name, glslType]` pairs keyed by shader variable name, in primitive order.
fn get_attributes_by_name<G: Gl>(attributes: &[mc::Attribute<G>]) -> Result<Vec<(String, &'static str)>> {
    let mut names = Vec::with_capacity(attributes.len());
    for attribute in attributes {
        let info = model_utility::get_attribute_info(attribute)?;
        names.push((info.variable_name, info.glsl_type));
    }
    Ok(names)
}

fn attribute_type_lut(name: &str) -> Option<&'static str> {
    Some(match name {
        "position" | "normal" | "tangent" | "bitangent" => "vec3",
        "texCoord" => "vec2",
        "color" | "weights" => "vec4",
        "joints" => "ivec4",
        _ => return None,
    })
}

fn attribute_default_value_lut(name: &str) -> Option<&'static str> {
    Some(match name {
        "position" => "vec3(0.0)",
        "normal" => "vec3(0.0, 0.0, 1.0)",
        "tangent" => "vec3(1.0, 0.0, 0.0)",
        "bitangent" => "vec3(0.0, 1.0, 0.0)",
        "texCoord" => "vec2(0.0)",
        "color" => "vec4(1.0)",
        "joints" => "ivec4(0)",
        "weights" => "vec4(0.0)",
        _ => return None,
    })
}

/// `attributeName.replace(/_[0-9]+$/, "").replace(/(MC|EC)$/, "")`.
fn trim_attribute_name(attribute_name: &str) -> String {
    let without_set_index = match attribute_name.rfind('_') {
        Some(index)
            if index + 1 < attribute_name.len() && attribute_name[index + 1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            &attribute_name[..index]
        }
        _ => attribute_name,
    };
    without_set_index
        .strip_suffix("MC")
        .or_else(|| without_set_index.strip_suffix("EC"))
        .unwrap_or(without_set_index)
        .to_string()
}

fn infer_attribute_defaults(attribute_name: &str) -> Option<(&'static str, &'static str)> {
    let trimmed = trim_attribute_name(attribute_name);
    let glsl_type = attribute_type_lut(&trimmed)?;
    let value = attribute_default_value_lut(&trimmed)?;
    Some((glsl_type, value))
}

fn get_primitive_attributes_used_in_shader(
    primitive_attributes: &[(String, &'static str)],
    shader_attribute_set: &super::custom_shader::VariableSet,
    is_fragment_shader: bool,
) -> Vec<(String, &'static str)> {
    let mut add_to_shader = Vec::new();
    for (attribute_name, glsl_type) in primitive_attributes {
        // Normals and tangents are in model coordinates in the attributes but
        // in eye coordinates in the fragment shader; a `vec4` tangent loses
        // its sign in eye space, so upstream declares it `vec3` there.
        let (renamed, glsl_type) = if is_fragment_shader && attribute_name == "normalMC" {
            ("normalEC".to_string(), *glsl_type)
        } else if is_fragment_shader && attribute_name == "tangentMC" {
            ("tangentEC".to_string(), "vec3")
        } else {
            (attribute_name.clone(), *glsl_type)
        };
        if shader_attribute_set.contains(&renamed) {
            add_to_shader.push((renamed, glsl_type));
        }
    }
    add_to_shader
}

const BUILTIN_ATTRIBUTES: [&str; 2] = ["positionWC", "positionEC"];

fn get_attributes_needing_defaults(
    primitive_attributes: &[(String, &'static str)],
    shader_attribute_set: &super::custom_shader::VariableSet,
    is_fragment_shader: bool,
) -> Vec<String> {
    let mut need_defaults = Vec::new();
    for attribute_name in shader_attribute_set.iter() {
        if BUILTIN_ATTRIBUTES.contains(&attribute_name) {
            continue;
        }
        let renamed = if is_fragment_shader && attribute_name == "normalEC" {
            "normalMC"
        } else if is_fragment_shader && attribute_name == "tangentEC" {
            "tangentMC"
        } else {
            attribute_name
        };
        if !primitive_attributes.iter().any(|(name, _)| name == renamed) {
            need_defaults.push(attribute_name.to_string());
        }
    }
    need_defaults
}

fn generate_vertex_shader_lines(
    custom_shader: &CustomShader<impl Gl>,
    attributes_by_name: &[(String, &'static str)],
    primitive_property_ids: &BTreeSet<String>,
) -> (GeneratedLines, Vec<CustomShaderWarning>) {
    if custom_shader.vertex_shader_text.is_none() {
        return (GeneratedLines::disabled(), Vec::new());
    }
    let used_variables = &custom_shader.used_variables_vertex;
    if !check_metadata_compatibility(&used_variables.metadata_set, primitive_property_ids) {
        return (GeneratedLines::disabled(), metadata_warnings(&used_variables.metadata_set, primitive_property_ids));
    }
    let add_to_shader =
        get_primitive_attributes_used_in_shader(attributes_by_name, &used_variables.attribute_set, false);
    let needs_default = get_attributes_needing_defaults(attributes_by_name, &used_variables.attribute_set, false);
    let mut attribute_fields = Vec::new();
    let mut initialization_lines = Vec::new();
    for (variable_name, glsl_type) in add_to_shader {
        attribute_fields.push((glsl_type, variable_name.clone()));
        initialization_lines.push(format!("vsInput.attributes.{variable_name} = attributes.{variable_name};"));
    }
    let mut warnings = Vec::new();
    for variable_name in needs_default {
        let Some((glsl_type, value)) = infer_attribute_defaults(&variable_name) else {
            warnings.push(CustomShaderWarning::IncompatiblePrimitiveVertex { attribute: variable_name });
            return (GeneratedLines::disabled(), warnings);
        };
        attribute_fields.push((glsl_type, variable_name.clone()));
        initialization_lines.push(format!("vsInput.attributes.{variable_name} = {value};"));
    }
    (GeneratedLines { enabled: true, attribute_fields, initialization_lines }, warnings)
}

fn generate_position_builtins(custom_shader: &CustomShader<impl Gl>) -> (Vec<(&'static str, String)>, Vec<String>) {
    let mut attribute_fields = Vec::new();
    let mut initialization_lines = Vec::new();
    let used_variables = &custom_shader.used_variables_fragment.attribute_set;
    if used_variables.contains("positionWC") {
        attribute_fields.push(("vec3", "positionWC".into()));
        initialization_lines.push("fsInput.attributes.positionWC = attributes.positionWC;".into());
    }
    if used_variables.contains("positionEC") {
        attribute_fields.push(("vec3", "positionEC".into()));
        initialization_lines.push("fsInput.attributes.positionEC = attributes.positionEC;".into());
    }
    (attribute_fields, initialization_lines)
}

fn generate_fragment_shader_lines(
    custom_shader: &CustomShader<impl Gl>,
    attributes_by_name: &[(String, &'static str)],
    primitive_property_ids: &BTreeSet<String>,
) -> (GeneratedLines, Vec<CustomShaderWarning>) {
    if custom_shader.fragment_shader_text.is_none() {
        return (GeneratedLines::disabled(), Vec::new());
    }
    let used_variables = &custom_shader.used_variables_fragment;
    if !check_metadata_compatibility(&used_variables.metadata_set, primitive_property_ids) {
        return (GeneratedLines::disabled(), metadata_warnings(&used_variables.metadata_set, primitive_property_ids));
    }
    let add_to_shader =
        get_primitive_attributes_used_in_shader(attributes_by_name, &used_variables.attribute_set, true);
    let needs_default = get_attributes_needing_defaults(attributes_by_name, &used_variables.attribute_set, true);
    let mut attribute_fields = Vec::new();
    let mut initialization_lines = Vec::new();
    for (variable_name, glsl_type) in add_to_shader {
        attribute_fields.push((glsl_type, variable_name.clone()));
        initialization_lines.push(format!("fsInput.attributes.{variable_name} = attributes.{variable_name};"));
    }
    let mut warnings = Vec::new();
    for variable_name in needs_default {
        let Some((glsl_type, value)) = infer_attribute_defaults(&variable_name) else {
            warnings.push(CustomShaderWarning::IncompatiblePrimitiveFragment { attribute: variable_name });
            return (GeneratedLines::disabled(), warnings);
        };
        attribute_fields.push((glsl_type, variable_name.clone()));
        initialization_lines.push(format!("fsInput.attributes.{variable_name} = {value};"));
    }
    let (builtin_fields, builtin_lines) = generate_position_builtins(custom_shader);
    attribute_fields.extend(builtin_fields);
    let mut lines = builtin_lines;
    lines.extend(initialization_lines);
    (GeneratedLines { enabled: true, attribute_fields, initialization_lines: lines }, warnings)
}

fn metadata_warnings(
    used: &super::custom_shader::VariableSet,
    primitive_property_ids: &BTreeSet<String>,
) -> Vec<CustomShaderWarning> {
    used.iter()
        .filter(|name| !primitive_property_ids.contains(*name))
        .map(|property| CustomShaderWarning::MetadataPropertyMissing { property: property.into() })
        .collect()
}

fn add_vertex_lines_to_shader(shader_builder: &mut ShaderBuilder, lines: &GeneratedLines) -> Result<()> {
    shader_builder.add_struct(STRUCT_ID_ATTRIBUTES_VS, STRUCT_NAME_ATTRIBUTES, D::VERTEX);
    for (glsl_type, variable_name) in &lines.attribute_fields {
        shader_builder.add_struct_field(STRUCT_ID_ATTRIBUTES_VS, glsl_type, variable_name)?;
    }
    shader_builder.add_struct(STRUCT_ID_VERTEX_INPUT, STRUCT_NAME_VERTEX_INPUT, D::VERTEX);
    for (glsl_type, field) in [
        (STRUCT_NAME_ATTRIBUTES, "attributes"),
        (STRUCT_NAME_FEATURE_IDS, "featureIds"),
        (STRUCT_NAME_METADATA, "metadata"),
        (STRUCT_NAME_METADATA_CLASS, "metadataClass"),
        (STRUCT_NAME_METADATA_STATISTICS, "metadataStatistics"),
    ] {
        shader_builder.add_struct_field(STRUCT_ID_VERTEX_INPUT, glsl_type, field)?;
    }
    shader_builder.add_function(
        FUNCTION_ID_INITIALIZE_INPUT_STRUCT_VS,
        FUNCTION_SIGNATURE_INITIALIZE_INPUT_STRUCT_VS,
        D::VERTEX,
    );
    shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_INPUT_STRUCT_VS, &lines.initialization_lines)
}

fn add_fragment_lines_to_shader(shader_builder: &mut ShaderBuilder, lines: &GeneratedLines) -> Result<()> {
    shader_builder.add_struct(STRUCT_ID_ATTRIBUTES_FS, STRUCT_NAME_ATTRIBUTES, D::FRAGMENT);
    for (glsl_type, variable_name) in &lines.attribute_fields {
        shader_builder.add_struct_field(STRUCT_ID_ATTRIBUTES_FS, glsl_type, variable_name)?;
    }
    shader_builder.add_struct(STRUCT_ID_FRAGMENT_INPUT, STRUCT_NAME_FRAGMENT_INPUT, D::FRAGMENT);
    for (glsl_type, field) in [
        (STRUCT_NAME_ATTRIBUTES, "attributes"),
        (STRUCT_NAME_FEATURE_IDS, "featureIds"),
        (STRUCT_NAME_METADATA, "metadata"),
        (STRUCT_NAME_METADATA_CLASS, "metadataClass"),
        (STRUCT_NAME_METADATA_STATISTICS, "metadataStatistics"),
    ] {
        shader_builder.add_struct_field(STRUCT_ID_FRAGMENT_INPUT, glsl_type, field)?;
    }
    shader_builder.add_function(
        FUNCTION_ID_INITIALIZE_INPUT_STRUCT_FS,
        FUNCTION_SIGNATURE_INITIALIZE_INPUT_STRUCT_FS,
        D::FRAGMENT,
    );
    shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_INPUT_STRUCT_FS, &lines.initialization_lines)
}

fn add_lines_to_shader(
    shader_builder: &mut ShaderBuilder,
    custom_shader: &CustomShader<impl Gl>,
    vertex_lines: &GeneratedLines,
    fragment_lines: &GeneratedLines,
) -> Result<()> {
    if vertex_lines.enabled {
        add_vertex_lines_to_shader(shader_builder, vertex_lines)?;
        let text = custom_shader.vertex_shader_text.clone().unwrap_or_default();
        shader_builder.add_vertex_lines(&["#line 0", text.as_str(), shaders::model::CUSTOM_SHADER_STAGE_VS.source]);
    }
    if fragment_lines.enabled {
        add_fragment_lines_to_shader(shader_builder, fragment_lines)?;
        let text = custom_shader.fragment_shader_text.clone().unwrap_or_default();
        shader_builder.add_fragment_lines(&["#line 0", text.as_str(), shaders::model::CUSTOM_SHADER_STAGE_FS.source]);
    }
    Ok(())
}

/// All property IDs a primitive references, across property textures, property
/// attributes and property tables.
pub fn get_all_property_ids<G: Gl>(
    primitive: &mc::Primitive<G>,
    structural_metadata: Option<&StructuralMetadata<G>>,
) -> BTreeSet<String> {
    let Some(metadata) = structural_metadata else {
        return BTreeSet::new();
    };
    let mut ids = BTreeSet::new();
    for id in &primitive.property_texture_ids {
        if let Some(texture) = metadata.property_textures().get(*id) {
            ids.extend(texture.properties().keys().map(str::to_string));
        }
    }
    for id in &primitive.property_attribute_ids {
        if let Some(attribute) = metadata.property_attributes().get(*id) {
            ids.extend(attribute.properties().keys().map(str::to_string));
        }
    }
    let table_ids: Vec<usize> = primitive
        .feature_ids
        .iter()
        .filter_map(|feature_id| match feature_id {
            mc::FeatureId::Attribute(f) => f.property_table_id,
            mc::FeatureId::ImplicitRange(f) => f.property_table_id,
            mc::FeatureId::Texture(f) => f.property_table_id,
        })
        .collect();
    for id in table_ids {
        if let Some(table) = metadata.property_tables().get(id) {
            if let Some(properties) = table.properties() {
                ids.extend(properties.keys().map(str::to_string));
            }
        }
    }
    ids
}

fn check_metadata_compatibility(
    metadata_properties_used_in_shader: &super::custom_shader::VariableSet,
    primitive_property_ids: &BTreeSet<String>,
) -> bool {
    metadata_properties_used_in_shader.iter().all(|name| primitive_property_ids.contains(name))
}
