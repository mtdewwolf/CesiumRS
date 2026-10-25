//! CesiumJS `Scene/Model/FeatureIdPipelineStage`: processes primitive and instance
//! feature ID attributes, implicit ranges, and textures into shader structs,
//! functions, attributes, varyings, and uniforms.

use super::{model_utility, ModelStatistics, PrimitiveRenderResources};
use crate::core::{ComponentDatatype, Matrix3, RuntimeError, TypedArray};
use crate::renderer::{
    Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, ShaderBuilder, ShaderDestination, UniformValue,
    VertexAttribute,
};
use crate::scene::{model_components as mc, VertexAttributeSemantic};
use crate::shaders;
use std::rc::Rc;

pub const STRUCT_ID_FEATURE_IDS_VS: &str = "FeatureIdsVS";
pub const STRUCT_ID_FEATURE_IDS_FS: &str = "FeatureIdsFS";
pub const STRUCT_NAME_FEATURE_IDS: &str = "FeatureIds";
pub const FUNCTION_ID_INITIALIZE_FEATURE_IDS_VS: &str = "initializeFeatureIdsVS";
pub const FUNCTION_ID_INITIALIZE_FEATURE_IDS_FS: &str = "initializeFeatureIdsFS";
pub const FUNCTION_ID_INITIALIZE_FEATURE_ID_ALIASES_VS: &str = "initializeFeatureIdAliasesVS";
pub const FUNCTION_ID_INITIALIZE_FEATURE_ID_ALIASES_FS: &str = "initializeFeatureIdAliasesFS";
pub const FUNCTION_SIGNATURE_INITIALIZE_FEATURE_IDS: &str =
    "void initializeFeatureIds(out FeatureIds featureIds, ProcessedAttributes attributes)";
pub const FUNCTION_SIGNATURE_INITIALIZE_FEATURE_ID_ALIASES: &str =
    "void initializeFeatureIdAliases(inout FeatureIds featureIds)";
pub const FUNCTION_ID_SET_FEATURE_ID_VARYINGS: &str = "setFeatureIdVaryings";
pub const FUNCTION_SIGNATURE_SET_FEATURE_ID_VARYINGS: &str = "void setFeatureIdVaryings()";

pub fn process<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    node: &mc::Node<G>,
    primitive: &mc::Primitive<G>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
    pipeline_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    declare_structs_and_functions(&mut resources.shader_builder);

    if let Some(instances) = &node.instances {
        process_instance_feature_ids(resources, instances, context, statistics, pipeline_resources)?;
    }

    process_primitive_feature_ids(resources, primitive, context, statistics, pipeline_resources)?;

    resources.shader_builder.add_vertex_lines(&[shaders::model::FEATURE_ID_STAGE_VS.source]);
    resources.shader_builder.add_fragment_lines(&[shaders::model::FEATURE_ID_STAGE_FS.source]);

    Ok(())
}

fn declare_structs_and_functions(shader_builder: &mut ShaderBuilder) {
    // Declare the FeatureIds struct. The vertex shader will only use
    // feature ID attributes, while the fragment shader will also use
    // feature ID textures.
    shader_builder.add_struct(STRUCT_ID_FEATURE_IDS_VS, STRUCT_NAME_FEATURE_IDS, ShaderDestination::VERTEX);
    shader_builder.add_struct(STRUCT_ID_FEATURE_IDS_FS, STRUCT_NAME_FEATURE_IDS, ShaderDestination::FRAGMENT);

    // declare the initializeFeatureIds() function. The details may differ
    // between vertex and fragment shader
    shader_builder.add_function(
        FUNCTION_ID_INITIALIZE_FEATURE_IDS_VS,
        FUNCTION_SIGNATURE_INITIALIZE_FEATURE_IDS,
        ShaderDestination::VERTEX,
    );
    shader_builder.add_function(
        FUNCTION_ID_INITIALIZE_FEATURE_IDS_FS,
        FUNCTION_SIGNATURE_INITIALIZE_FEATURE_IDS,
        ShaderDestination::FRAGMENT,
    );

    // declare the initializeFeatureIdAliases() function. The details may differ
    // between vertex and fragment shader
    shader_builder.add_function(
        FUNCTION_ID_INITIALIZE_FEATURE_ID_ALIASES_VS,
        FUNCTION_SIGNATURE_INITIALIZE_FEATURE_ID_ALIASES,
        ShaderDestination::VERTEX,
    );
    shader_builder.add_function(
        FUNCTION_ID_INITIALIZE_FEATURE_ID_ALIASES_FS,
        FUNCTION_SIGNATURE_INITIALIZE_FEATURE_ID_ALIASES,
        ShaderDestination::FRAGMENT,
    );

    // declare the setFeatureIdVaryings() function in the vertex shader only
    shader_builder.add_function(
        FUNCTION_ID_SET_FEATURE_ID_VARYINGS,
        FUNCTION_SIGNATURE_SET_FEATURE_ID_VARYINGS,
        ShaderDestination::VERTEX,
    );
}

fn process_instance_feature_ids<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    instances: &mc::Instances<G>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
    pipeline_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    let count = instances.attributes.first().and_then(|a| a.count).unwrap_or(0);

    for feature_ids in &instances.feature_ids {
        let variable_name = feature_ids.positional_label().unwrap_or("undefined");

        match feature_ids {
            mc::FeatureId::Attribute(attr) => {
                process_instance_attribute(resources, attr, variable_name)?;
            }
            mc::FeatureId::ImplicitRange(implicit) => {
                process_implicit_range(
                    resources,
                    implicit,
                    variable_name,
                    count,
                    Some(1),
                    context,
                    statistics,
                    pipeline_resources,
                )?;
            }
            mc::FeatureId::Texture(_) => {}
        }

        if let Some(label) = feature_ids.label() {
            add_alias(resources, variable_name, label, ShaderDestination::BOTH)?;
        }
    }
    Ok(())
}

fn process_primitive_feature_ids<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
    pipeline_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    let position_attribute = model_utility::get_attribute_by_semantic(
        &primitive.attributes,
        mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position),
        None,
    )
    .ok_or_else(|| RuntimeError::new("Missing POSITION attribute"))?;
    let count = position_attribute.count.unwrap_or(0);

    for (i, feature_ids) in primitive.feature_ids.iter().enumerate() {
        let variable_name = feature_ids.positional_label().unwrap_or("undefined");

        let mut alias_destination = ShaderDestination::BOTH;
        match feature_ids {
            mc::FeatureId::Attribute(attr) => {
                process_attribute(resources, attr, variable_name)?;
            }
            mc::FeatureId::ImplicitRange(implicit) => {
                process_implicit_range(
                    resources,
                    implicit,
                    variable_name,
                    count,
                    None,
                    context,
                    statistics,
                    pipeline_resources,
                )?;
            }
            mc::FeatureId::Texture(tex) => {
                process_texture(resources, tex, variable_name, i, context)?;
                alias_destination = ShaderDestination::FRAGMENT;
            }
        }

        if let Some(label) = feature_ids.label() {
            add_alias(resources, variable_name, label, alias_destination)?;
        }
    }
    Ok(())
}

fn strip_numeric_suffix(variable_name: &str) -> String {
    if let Some((prefix, suffix)) = variable_name.rsplit_once('_') {
        if !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) {
            return format!("{prefix}_");
        }
    }
    variable_name.to_string()
}

fn process_instance_attribute<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    feature_id_attribute: &mc::FeatureIdAttribute,
    variable_name: &str,
) -> Result<(), RuntimeError> {
    let shader_builder = &mut resources.shader_builder;
    shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_VS, "int", variable_name)?;
    shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_FS, "int", variable_name)?;

    let set_index = feature_id_attribute.set_index.map_or_else(|| "undefined".to_string(), |s| s.to_string());
    let prefix = strip_numeric_suffix(variable_name);

    let attribute_name = format!("a_{prefix}{set_index}");
    let varying_name = format!("v_{prefix}{set_index}");
    let vertex_line = format!("featureIds.{variable_name} = int(czm_round({attribute_name}));");
    let fragment_line = format!("featureIds.{variable_name} = int(czm_round({varying_name}));");

    shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_FEATURE_IDS_VS, &[vertex_line])?;
    shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_FEATURE_IDS_FS, &[fragment_line])?;

    // Instanced attributes don't normally need varyings, so add one here
    shader_builder.add_varying("float", &varying_name, None);

    shader_builder
        .add_function_lines(FUNCTION_ID_SET_FEATURE_ID_VARYINGS, &[format!("{varying_name} = {attribute_name};")])?;

    Ok(())
}

fn process_attribute<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    feature_id_attribute: &mc::FeatureIdAttribute,
    variable_name: &str,
) -> Result<(), RuntimeError> {
    let shader_builder = &mut resources.shader_builder;
    shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_VS, "int", variable_name)?;
    shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_FS, "int", variable_name)?;

    let set_index = feature_id_attribute.set_index.map_or_else(|| "undefined".to_string(), |s| s.to_string());
    let prefix = strip_numeric_suffix(variable_name);

    let line = format!("featureIds.{variable_name} = int(czm_round(attributes.{prefix}{set_index}));");
    shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_FEATURE_IDS_VS, std::slice::from_ref(&line))?;
    shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_FEATURE_IDS_FS, &[line])?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_implicit_range<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    implicit_feature_ids: &mc::FeatureIdImplicitRange,
    variable_name: &str,
    count: usize,
    instance_divisor: Option<u32>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
    pipeline_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    generate_implicit_feature_id_attribute(
        resources,
        implicit_feature_ids,
        count,
        instance_divisor,
        context,
        statistics,
        pipeline_resources,
    )?;

    let shader_builder = &mut resources.shader_builder;
    let implicit_attribute_name = format!("a_implicit_{variable_name}");
    shader_builder.add_attribute("float", &implicit_attribute_name);

    let implicit_varying_name = format!("v_implicit_{variable_name}");
    shader_builder.add_varying("float", &implicit_varying_name, None);

    shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_VS, "int", variable_name)?;
    shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_FS, "int", variable_name)?;

    shader_builder.add_function_lines(
        FUNCTION_ID_SET_FEATURE_ID_VARYINGS,
        &[format!("{implicit_varying_name} = {implicit_attribute_name};")],
    )?;

    shader_builder.add_function_lines(
        FUNCTION_ID_INITIALIZE_FEATURE_IDS_VS,
        &[format!("featureIds.{variable_name} = int(czm_round({implicit_attribute_name}));")],
    )?;
    shader_builder.add_function_lines(
        FUNCTION_ID_INITIALIZE_FEATURE_IDS_FS,
        &[format!("featureIds.{variable_name} = int(czm_round({implicit_varying_name}));")],
    )?;

    Ok(())
}

fn process_texture<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    feature_id_texture: &mc::FeatureIdTexture<G>,
    variable_name: &str,
    index: usize,
    context: &Context<G>,
) -> Result<(), RuntimeError> {
    let uniform_name = format!("u_featureIdTexture_{index}");
    let texture_reader = feature_id_texture
        .texture_reader
        .as_ref()
        .ok_or_else(|| RuntimeError::new("Missing featureIdTexture.textureReader"))?;

    let default_texture = context.default_texture().ok();
    let tex = texture_reader.texture.clone();
    resources
        .texture_uniform_map
        .insert(uniform_name.clone(), Rc::new(move || tex.clone().or_else(|| default_texture.clone())));

    let channels = texture_reader.channels.as_deref().unwrap_or("r");

    resources.shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_FS, "int", variable_name)?;
    resources.shader_builder.add_uniform("sampler2D", &uniform_name, ShaderDestination::FRAGMENT);

    let tex_coord = texture_reader.tex_coord as u32;
    let tex_coord_variable = format!("v_texCoord_{tex_coord}");
    let mut tex_coord_variable_expression = tex_coord_variable.clone();

    if let Some(transform) = texture_reader.transform {
        if transform != Matrix3::IDENTITY {
            let transform_uniform_name = format!("{uniform_name}Transform");
            resources.shader_builder.add_uniform("mat3", &transform_uniform_name, ShaderDestination::FRAGMENT);
            resources.uniform_map.insert(
                transform_uniform_name.clone(),
                Rc::new(move || UniformValue::Matrix {
                    dimension: 3,
                    values: transform.0.iter().map(|&v| v as f32).collect(),
                }),
            );
            tex_coord_variable_expression = format!("vec2({transform_uniform_name} * vec3({tex_coord_variable}, 1.0))");
        }
    }

    let texture_read = format!("texture({uniform_name}, {tex_coord_variable_expression}).{channels}");
    let initialization_line = format!("featureIds.{variable_name} = czm_unpackUint({texture_read});");

    resources.shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_FEATURE_IDS_FS, &[initialization_line])?;

    Ok(())
}

fn add_alias<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    variable_name: &str,
    alias: &str,
    shader_destination: ShaderDestination,
) -> Result<(), RuntimeError> {
    let shader_builder = &mut resources.shader_builder;
    let update_vs = shader_destination.includes_vertex_shader();
    if update_vs {
        shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_VS, "int", alias)?;
    }
    shader_builder.add_struct_field(STRUCT_ID_FEATURE_IDS_FS, "int", alias)?;

    let line = format!("featureIds.{alias} = featureIds.{variable_name};");
    if update_vs {
        shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_FEATURE_ID_ALIASES_VS, std::slice::from_ref(&line))?;
    }
    shader_builder.add_function_lines(FUNCTION_ID_INITIALIZE_FEATURE_ID_ALIASES_FS, &[line])?;

    Ok(())
}

fn generate_implicit_feature_id_attribute<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    implicit_feature_ids: &mc::FeatureIdImplicitRange,
    count: usize,
    instance_divisor: Option<u32>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
    pipeline_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    let index = resources.attribute_index;
    resources.attribute_index += 1;

    let mut attr = if let Some(repeat) = implicit_feature_ids.repeat {
        let typed_array = generate_implicit_feature_id_typed_array(implicit_feature_ids.offset, repeat, count);
        let bytes = TypedArray::Float32(typed_array).to_le_bytes();
        let vertex_buffer =
            Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
        let has_cpu_copy = false;
        statistics.add_buffer(&vertex_buffer, has_cpu_copy);
        pipeline_resources.push(vertex_buffer.clone());
        VertexAttribute::buffer(index, vertex_buffer, 1)
    } else {
        VertexAttribute::constant(index, vec![implicit_feature_ids.offset as f32])
    };

    attr.instance_divisor = instance_divisor.unwrap_or(0);
    attr.normalize = false;
    attr.datatype = ComponentDatatype::Float;
    attr.stride = ComponentDatatype::Float.get_size_in_bytes();
    attr.offset = 0;

    resources.attributes.push(Rc::new(attr));
    Ok(())
}

fn generate_implicit_feature_id_typed_array(offset: f64, repeat: f64, count: usize) -> Vec<f32> {
    (0..count).map(|i| (offset + (i as f64 / repeat).floor()) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Cartesian3;
    use crate::renderer::GlValue;
    use crate::scene::model::{ModelRenderResources, ModelUniformState, NodeRenderResources};
    use crate::scene::AttributeType;
    use std::cell::RefCell;

    #[derive(Default)]
    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([8, 8])
        }
        fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            if method.starts_with("create") {
                Ok(GlValue::Object(1))
            } else {
                Ok(GlValue::Null)
            }
        }
    }

    fn make_primitive<G: Gl + 'static>() -> mc::Primitive<G> {
        mc::Primitive {
            attributes: vec![mc::Attribute {
                semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                attribute_type: Some(AttributeType::Vec3),
                count: Some(4),
                min: Some(mc::MathValue::Cartesian3(Cartesian3::ZERO)),
                max: Some(mc::MathValue::Cartesian3(Cartesian3::ONE)),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn processes_attribute_implicit_and_texture_feature_ids() {
        let ctx = Context::new_webgl2(TestGl);
        let state = Rc::new(RefCell::new(ModelUniformState::default()));
        let model_res = ModelRenderResources::<TestGl>::new(state);
        let node_res = NodeRenderResources::new(&model_res, 0);
        let mut primitive = make_primitive();
        primitive.feature_ids = vec![
            mc::FeatureId::Attribute(mc::FeatureIdAttribute {
                feature_count: Some(4),
                set_index: Some(0),
                label: Some("customAttr".into()),
                positional_label: Some("featureId_0".into()),
                ..Default::default()
            }),
            mc::FeatureId::ImplicitRange(mc::FeatureIdImplicitRange {
                feature_count: Some(2),
                offset: 10.0,
                repeat: Some(2.0),
                label: Some("customImplicit".into()),
                positional_label: Some("featureId_1".into()),
                ..Default::default()
            }),
            mc::FeatureId::Texture(mc::FeatureIdTexture {
                feature_count: Some(4),
                positional_label: Some("featureId_2".into()),
                label: Some("customTex".into()),
                texture_reader: Some(mc::TextureReader {
                    tex_coord: 0.0,
                    channels: Some("rg".into()),
                    transform: Some(Matrix3::from_scale(Cartesian3::new(2.0, 2.0, 1.0))),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        ];
        let node = mc::Node::default();
        let mut prim_res = PrimitiveRenderResources::new(&node_res, 0, &primitive, None, None).unwrap();
        let mut stats = ModelStatistics::default();
        let mut pipeline_buffers = Vec::new();

        process(&mut prim_res, &node, &primitive, &ctx, &mut stats, &mut pipeline_buffers).unwrap();

        let (vs, fs) = prim_res.shader_builder.sources();
        assert!(vs.sources[0].contains("in float a_implicit_featureId_1;"));
        assert!(vs.sources[0].contains("featureIds.featureId_0 = int(czm_round(attributes.featureId_0));"));
        assert!(vs.sources[0].contains("featureIds.featureId_1 = int(czm_round(a_implicit_featureId_1));"));
        assert!(fs.sources[0].contains("uniform sampler2D u_featureIdTexture_2;"));
        assert!(fs.sources[0].contains("uniform mat3 u_featureIdTexture_2Transform;"));
        assert!(fs.sources[0].contains(
            "featureIds.featureId_2 = czm_unpackUint(texture(u_featureIdTexture_2, vec2(u_featureIdTexture_2Transform * vec3(v_texCoord_0, 1.0))).rg);"
        ));
        assert!(fs.sources[0].contains("featureIds.customTex = featureIds.featureId_2;"));
        assert_eq!(pipeline_buffers.len(), 1);
    }
}
