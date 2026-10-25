//! CesiumJS `ModelUtility`: transforms, component lookup, shader attribute
//! information, bounds and coordinate-system corrections for model pipelines.
use crate::{
    core::{Cartesian3, Matrix4, PrimitiveType, Quaternion, RuntimeError},
    renderer::Gl,
    scene::{model_components as mc, AttributeType, Axis, CullFace, VertexAttributeSemantic},
};

pub fn get_error(kind: &str, path: &str, cause: Option<&RuntimeError>) -> RuntimeError {
    RuntimeError::new(format!(
        "Failed to load {kind}: {path}{}",
        cause.map(|e| format!("\n{}", e.message)).unwrap_or_default()
    ))
}
pub fn get_node_transform<G: Gl>(node: &mc::Node<G>) -> Matrix4 {
    node.matrix.unwrap_or_else(|| {
        Matrix4::from_translation_quaternion_rotation_scale(
            node.translation.unwrap_or(Cartesian3::ZERO),
            &node.rotation.unwrap_or(Quaternion::IDENTITY),
            node.scale.unwrap_or(Cartesian3::ONE),
        )
    })
}
pub fn get_attribute_by_semantic<G: Gl>(
    attributes: &[mc::Attribute<G>],
    semantic: mc::AttributeSemantic,
    set_index: Option<u32>,
) -> Option<&mc::Attribute<G>> {
    attributes.iter().find(|a| a.semantic == Some(semantic) && set_index.is_none_or(|i| a.set_index == Some(i)))
}
pub fn get_attribute_by_name<'a, G: Gl>(
    attributes: &'a [mc::Attribute<G>],
    name: &str,
) -> Option<&'a mc::Attribute<G>> {
    attributes.iter().find(|a| a.name.as_deref() == Some(name))
}
pub fn get_feature_ids_by_label<'a, G: Gl>(
    features: &'a [mc::FeatureId<G>],
    label: &str,
) -> Option<&'a mc::FeatureId<G>> {
    features.iter().find(|f| {
        let (name, position) = match f {
            mc::FeatureId::Attribute(f) => (&f.label, &f.positional_label),
            mc::FeatureId::ImplicitRange(f) => (&f.label, &f.positional_label),
            mc::FeatureId::Texture(f) => (&f.label, &f.positional_label),
        };
        name.as_deref() == Some(label) || position.as_deref() == Some(label)
    })
}
pub fn has_quantized_attributes<G: Gl>(attributes: Option<&[mc::Attribute<G>]>) -> bool {
    attributes.is_some_and(|a| a.iter().any(|a| a.quantization.is_some()))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeInfo {
    pub is_quantized: bool,
    pub variable_name: String,
    pub has_semantic: bool,
    pub glsl_type: &'static str,
    pub quantized_glsl_type: Option<&'static str>,
}
pub fn get_attribute_info<G: Gl>(attribute: &mc::Attribute<G>) -> Result<AttributeInfo, RuntimeError> {
    let variable_name = match attribute.semantic {
        Some(mc::AttributeSemantic::Vertex(s)) => s.get_variable_name(attribute.set_index),
        Some(mc::AttributeSemantic::Instance(_)) => return Err(RuntimeError::new("Invalid vertex attribute semantic")),
        None => attribute
            .name
            .as_deref()
            .ok_or_else(|| RuntimeError::new("Missing attribute name"))?
            .strip_prefix('_')
            .unwrap_or(attribute.name.as_deref().unwrap())
            .to_lowercase(),
    };
    let color =
        variable_name.strip_prefix("color_").is_some_and(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()));
    let glsl_type = if color {
        "vec4"
    } else {
        attribute.attribute_type.ok_or_else(|| RuntimeError::new("Missing attribute type"))?.get_glsl_type()
    };
    let quantized_glsl_type = attribute
        .quantization
        .as_ref()
        .map(|q| {
            if color {
                Ok("vec4")
            } else {
                q.attribute_type
                    .map(AttributeType::get_glsl_type)
                    .ok_or_else(|| RuntimeError::new("Missing quantized attribute type"))
            }
        })
        .transpose()?;
    Ok(AttributeInfo {
        is_quantized: attribute.quantization.is_some(),
        variable_name,
        has_semantic: attribute.semantic.is_some(),
        glsl_type,
        quantized_glsl_type,
    })
}
pub fn get_position_min_max<G: Gl>(
    primitive: &mc::Primitive<G>,
    translation_min: Option<Cartesian3>,
    translation_max: Option<Cartesian3>,
) -> Result<(Option<Cartesian3>, Option<Cartesian3>), RuntimeError> {
    let a = get_attribute_by_semantic(
        &primitive.attributes,
        mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position),
        None,
    )
    .ok_or_else(|| RuntimeError::new("Missing POSITION attribute"))?;
    let get = |v| if let Some(mc::MathValue::Cartesian3(v)) = v { Some(v) } else { None };
    let (mut min, mut max) = (get(a.min), get(a.max));
    if let (Some(lo), Some(hi)) = (translation_min, translation_max) {
        min = Some(min.ok_or_else(|| RuntimeError::new("Missing position minimum"))? + lo);
        max = Some(max.ok_or_else(|| RuntimeError::new("Missing position maximum"))? + hi);
    }
    Ok((min, max))
}
pub fn get_axis_correction_matrix(up: Axis, forward: Axis) -> Matrix4 {
    let up = match up {
        Axis::Y => Axis::Y_UP_TO_Z_UP,
        Axis::X => Axis::X_UP_TO_Z_UP,
        Axis::Z => Matrix4::IDENTITY,
    };
    if forward == Axis::Z {
        up.multiply_transformation(&Axis::Z_UP_TO_X_UP)
    } else {
        up
    }
}
pub fn get_cull_face(matrix: &Matrix4, primitive_type: PrimitiveType) -> CullFace {
    if primitive_type.is_triangles() && matrix.get_matrix3().determinant() < 0. {
        CullFace::Front
    } else {
        CullFace::Back
    }
}
pub fn sanitize_glsl_identifier(identifier: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for c in identifier.chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
            separator = false;
        } else if !separator {
            result.push('_');
            separator = true;
        }
    }
    if result.starts_with("gl_") {
        result.drain(..3);
    }
    if result.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        result.insert(0, '_');
    }
    result
}
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "AGI_articulations",
    "CESIUM_primitive_outline",
    "CESIUM_RTC",
    "EXT_feature_metadata",
    "EXT_implicit_cylinder_region",
    "EXT_implicit_ellipsoid_region",
    "EXT_instance_features",
    "EXT_mesh_features",
    "EXT_mesh_gpu_instancing",
    "EXT_meshopt_compression",
    "EXT_primitive_voxels",
    "EXT_structural_metadata",
    "EXT_texture_webp",
    "KHR_blend",
    "KHR_draco_mesh_compression",
    "KHR_implicit_shapes",
    "KHR_materials_common",
    "KHR_materials_pbrSpecularGlossiness",
    "KHR_materials_specular",
    "KHR_materials_anisotropy",
    "KHR_materials_clearcoat",
    "KHR_materials_unlit",
    "KHR_mesh_quantization",
    "KHR_techniques_webgl",
    "KHR_texture_basisu",
    "KHR_texture_transform",
    "KHR_gaussian_splatting",
    "KHR_gaussian_splatting_compression_spz_2",
    "WEB3D_quantized_attributes",
];
pub fn check_supported_extensions(extensions: &[String]) -> Result<(), RuntimeError> {
    for e in extensions {
        if !SUPPORTED_EXTENSIONS.contains(&e.as_str()) {
            return Err(RuntimeError::new(format!("Unsupported glTF Extension: {e}")));
        }
    }
    Ok(())
}
