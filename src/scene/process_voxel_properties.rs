//! CesiumJS `processVoxelProperties`: shader structs and functions that read
//! voxel metadata from the megatextures.
use super::{
    voxel_render_resources::VoxelRenderResources, MetadataComponentType, MetadataType,
};
use crate::{core::RuntimeError, renderer::ShaderDestination};
type Result<T> = std::result::Result<T, RuntimeError>;

/// The provider metadata layout used to generate the shader.
#[derive(Clone, Debug)]
pub struct VoxelPropertyInfo {
    pub names: Vec<String>,
    pub types: Vec<MetadataType>,
    pub component_types: Vec<MetadataComponentType>,
    pub minimum_values: Option<Vec<Vec<f64>>>,
    pub maximum_values: Option<Vec<Vec<f64>>>,
}

/// The GLSL type of a metadata type, as used by generated fields.
pub fn glsl_type(metadata_type: MetadataType) -> &'static str {
    match metadata_type {
        MetadataType::Scalar => "float",
        MetadataType::Vec2 => "vec2",
        MetadataType::Vec3 => "vec3",
        MetadataType::Vec4 => "vec4",
        // Upstream has no branch for the remaining metadata types and returns
        // `undefined`, which produces an invalid shader; this port rejects them
        // at construction instead.
        _ => "",
    }
}

fn glsl_texture_swizzle(metadata_type: MetadataType) -> &'static str {
    match metadata_type {
        MetadataType::Scalar => ".r",
        MetadataType::Vec2 => ".rg",
        MetadataType::Vec3 => ".rgb",
        MetadataType::Vec4 => "",
        _ => "",
    }
}

/// `getGlslNumberAsFloat`: append `.0` when the number is written as an integer.
fn glsl_number_as_float(number: f64) -> String {
    let text = number.to_string();
    if text.contains('.') { text } else { format!("{text}.0") }
}

fn glsl_field(metadata_type: MetadataType, index: usize) -> String {
    if metadata_type == MetadataType::Scalar {
        String::new()
    } else {
        format!("[{index}]")
    }
}

/// Adds the metadata statistics structs and property functions.
pub fn process_voxel_properties(resources: &mut VoxelRenderResources, provider: &VoxelPropertyInfo) -> Result<()> {
    let builder = &mut resources.shader_builder;
    validate_provider(provider)?;

    let attribute_length = provider.types.len();
    let has_statistics = provider.minimum_values.is_some() && provider.maximum_values.is_some();

    builder.add_define("METADATA_COUNT", Some(&attribute_length.to_string()), ShaderDestination::FRAGMENT);
    if has_statistics {
        builder.add_define("STATISTICS", None, ShaderDestination::FRAGMENT);
    }

    for index in 0..attribute_length {
        let name = &provider.names[index];
        let id = format!("PropertyStatistics_{name}");
        let struct_name = format!("PropertyStatistics_{name}");
        builder.add_struct(&id, &struct_name, ShaderDestination::FRAGMENT);
        let glsl = glsl_type(provider.types[index]);
        builder.add_struct_field(&id, glsl, "min")?;
        builder.add_struct_field(&id, glsl, "max")?;
    }

    builder.add_struct("MetadataStatistics", "MetadataStatistics", ShaderDestination::FRAGMENT);
    for name in &provider.names {
        builder.add_struct_field("MetadataStatistics", &format!("PropertyStatistics_{name}"), name)?;
    }

    builder.add_struct("Metadata", "Metadata", ShaderDestination::FRAGMENT);
    for index in 0..attribute_length {
        builder.add_struct_field("Metadata", glsl_type(provider.types[index]), &provider.names[index])?;
    }

    builder.add_struct("Attributes", "Attributes", ShaderDestination::FRAGMENT);
    builder.add_struct_field("Attributes", "vec3", "positionEC")?;
    builder.add_struct_field("Attributes", "vec3", "normalEC")?;

    builder.add_struct("Voxel", "Voxel", ShaderDestination::FRAGMENT);
    for (datatype, name) in [
        ("vec3", "viewDirUv"),
        ("float", "travelDistance"),
        ("int", "stepCount"),
        ("int", "tileIndex"),
        ("int", "sampleIndex"),
        ("float", "distanceToDepthBuffer"),
    ] {
        builder.add_struct_field("Voxel", datatype, name)?;
    }

    builder.add_struct("FragmentInput", "FragmentInput", ShaderDestination::FRAGMENT);
    builder.add_struct_field("FragmentInput", "MetadataStatistics", "metadataStatistics")?;
    builder.add_struct_field("FragmentInput", "Metadata", "metadata")?;
    builder.add_struct_field("FragmentInput", "Attributes", "attributes")?;
    builder.add_struct_field("FragmentInput", "Voxel", "voxel")?;

    builder.add_struct("Properties", "Properties", ShaderDestination::FRAGMENT);
    for index in 0..attribute_length {
        builder.add_struct_field("Properties", glsl_type(provider.types[index]), &provider.names[index])?;
    }

    // clearProperties
    builder.add_function("clearProperties", "Properties clearProperties()", ShaderDestination::FRAGMENT);
    builder.add_function_lines("clearProperties", &["Properties properties;"])?;
    for index in 0..attribute_length {
        let glsl = glsl_type(provider.types[index]);
        builder.add_function_lines(
            "clearProperties",
            &[format!("properties.{} = {}(0.0);", provider.names[index], glsl)],
        )?;
    }
    builder.add_function_lines("clearProperties", &["return properties;"])?;

    // sumProperties
    builder.add_function(
        "sumProperties",
        "Properties sumProperties(Properties propertiesA, Properties propertiesB)",
        ShaderDestination::FRAGMENT,
    );
    builder.add_function_lines("sumProperties", &["Properties properties;"])?;
    for name in &provider.names {
        builder.add_function_lines(
            "sumProperties",
            &[format!("properties.{name} = propertiesA.{name} + propertiesB.{name};")],
        )?;
    }
    builder.add_function_lines("sumProperties", &["return properties;"])?;

    // scaleProperties
    builder.add_function(
        "scaleProperties",
        "Properties scaleProperties(Properties properties, float scale)",
        ShaderDestination::FRAGMENT,
    );
    builder.add_function_lines("scaleProperties", &["Properties scaledProperties = properties;"])?;
    for name in &provider.names {
        builder.add_function_lines("scaleProperties", &[format!("scaledProperties.{name} *= scale;")])?;
    }
    builder.add_function_lines("scaleProperties", &["return scaledProperties;"])?;

    // mixProperties
    builder.add_function(
        "mixProperties",
        "Properties mixProperties(Properties propertiesA, Properties propertiesB, float mixFactor)",
        ShaderDestination::FRAGMENT,
    );
    builder.add_function_lines("mixProperties", &["Properties properties;"])?;
    for name in &provider.names {
        builder.add_function_lines(
            "mixProperties",
            &[format!(
                "properties.{name} = mix(propertiesA.{name}, propertiesB.{name}, mixFactor);"
            )],
        )?;
    }
    builder.add_function_lines("mixProperties", &["return properties;"])?;

    // copyPropertiesToMetadata
    builder.add_function(
        "copyPropertiesToMetadata",
        "void copyPropertiesToMetadata(in Properties properties, inout Metadata metadata)",
        ShaderDestination::FRAGMENT,
    );
    for name in &provider.names {
        builder.add_function_lines(
            "copyPropertiesToMetadata",
            &[format!("metadata.{name} = properties.{name};")],
        )?;
    }

    // setStatistics
    if has_statistics {
        builder.add_function(
            "setStatistics",
            "void setStatistics(inout MetadataStatistics metadataStatistics)",
            ShaderDestination::FRAGMENT,
        );
        let minimums = provider.minimum_values.as_ref().unwrap();
        let maximums = provider.maximum_values.as_ref().unwrap();
        for index in 0..attribute_length {
            let name = &provider.names[index];
            let metadata_type = provider.types[index];
            let component_count = metadata_type.get_component_count();
            for component in 0..component_count {
                let field = glsl_field(metadata_type, component);
                let minimum = minimums.get(index).and_then(|values| values.get(component)).copied();
                let maximum = maximums.get(index).and_then(|values| values.get(component)).copied();
                let (Some(minimum), Some(maximum)) = (minimum, maximum) else { continue };
                builder.add_function_lines(
                    "setStatistics",
                    &[
                        format!("metadataStatistics.{name}.min{field} = {};", glsl_number_as_float(minimum)),
                        format!("metadataStatistics.{name}.max{field} = {};", glsl_number_as_float(maximum)),
                    ],
                )?;
            }
        }
    }

    // getPropertiesFromMegatextureAtUv
    builder.add_function(
        "getPropertiesFromMegatextureAtUv",
        "Properties getPropertiesFromMegatextureAtUv(vec3 texcoord)",
        ShaderDestination::FRAGMENT,
    );
    builder.add_function_lines("getPropertiesFromMegatextureAtUv", &["Properties properties;"])?;
    for index in 0..attribute_length {
        let metadata_type = provider.types[index];
        let component_type = provider.component_types[index];
        let swizzle = glsl_texture_swizzle(metadata_type);
        builder.add_function_lines(
            "getPropertiesFromMegatextureAtUv",
            &[format!(
                "properties.{} = texture(u_megatextureTextures[{index}], texcoord){swizzle};",
                provider.names[index]
            )],
        )?;
        let _ = component_type;
    }
    builder.add_function_lines("getPropertiesFromMegatextureAtUv", &["return properties;"])?;

    Ok(())
}

/// Upstream builds these lines for any metadata type; the shader only supports
/// scalar and vector types, so reject the rest instead of emitting dead GLSL.
fn validate_provider(provider: &VoxelPropertyInfo) -> Result<()> {
    if provider.names.len() != provider.types.len() || provider.names.len() != provider.component_types.len() {
        return Err(RuntimeError::new("Voxel provider metadata arrays must have the same length"));
    }
    for metadata_type in &provider.types {
        if !matches!(
            metadata_type,
            MetadataType::Scalar | MetadataType::Vec2 | MetadataType::Vec3 | MetadataType::Vec4
        ) {
            return Err(RuntimeError::new(
                "Voxel metadata type must be SCALAR, VEC2, VEC3, or VEC4",
            ));
        }
    }
    Ok(())
}
