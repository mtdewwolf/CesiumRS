mod common;
use cesium_rs::{
    core::*,
    renderer::Gl,
    scene::{model::model_utility as utility, model_components as mc, *},
};
use common::*;
use serde_json::json;
struct Host;
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(
        &self,
        _: &str,
        _: &[cesium_rs::renderer::GlValue<u32>],
    ) -> Result<cesium_rs::renderer::GlValue<u32>, RuntimeError> {
        unreachable!()
    }
}
#[test]
fn model_utility_matches_cesium() {
    run(
        "scene",
        "ModelUtility",
        |_| TIGHT,
        |f, a| match f {
            "sanitize" => ok(utility::sanitize_glsl_identifier(s(&a[0]))),
            "axis" => ok(utility::get_axis_correction_matrix(
                Axis::from_value(u(&a[0]) as i32).unwrap(),
                Axis::from_value(u(&a[1]) as i32).unwrap(),
            )),
            "cull" => ok(utility::get_cull_face(&m4(&a[0]), PrimitiveType::from_gl(u(&a[1])).unwrap()) as u32),
            "extensions" => {
                let required = a[0].as_array().unwrap().iter().map(|v| s(v).to_owned()).collect::<Vec<_>>();
                Some(match utility::check_supported_extensions(&required) {
                    Ok(()) => Out::Ok(json!(true)),
                    Err(e) => Out::Err(e.message),
                })
            }
            "info" => {
                let j = &a[0];
                let attr = mc::Attribute::<Host> {
                    name: j["name"].as_str().map(str::to_owned),
                    semantic: j["semantic"]
                        .as_str()
                        .and_then(VertexAttributeSemantic::from_str_value)
                        .map(mc::AttributeSemantic::Vertex),
                    set_index: j["setIndex"].as_u64().map(|v| v as u32),
                    attribute_type: AttributeType::from_str_value(s(&j["type"])),
                    quantization: j.get("quantization").map(|q| mc::Quantization {
                        attribute_type: AttributeType::from_str_value(s(&q["type"])),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let i = utility::get_attribute_info(&attr).unwrap();
                Some(Out::Ok(
                    json!({"isQuantized":i.is_quantized,"variableName":i.variable_name,"hasSemantic":i.has_semantic,"glslType":i.glsl_type,"quantizedGlslType":i.quantized_glsl_type}),
                ))
            }
            "transform" => {
                let j = &a[0];
                let node = mc::Node::<Host> {
                    matrix: j.get("matrix").map(m4),
                    translation: j.get("translation").map(c3),
                    rotation: j.get("rotation").map(q),
                    scale: j.get("scale").map(c3),
                    ..Default::default()
                };
                ok(utility::get_node_transform(&node))
            }
            _ => None,
        },
    );
}
#[test]
fn attribute_lookup_and_position_bounds() {
    let attrs = vec![
        mc::Attribute::<Host> {
            name: Some("POSITION".into()),
            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
            min: Some(mc::MathValue::Cartesian3(Cartesian3::new(-1., -2., -3.))),
            max: Some(mc::MathValue::Cartesian3(Cartesian3::ONE)),
            ..Default::default()
        },
        mc::Attribute {
            name: Some("_FEATURE_ID_2".into()),
            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId)),
            set_index: Some(2),
            quantization: Some(mc::Quantization::default()),
            ..Default::default()
        },
    ];
    assert!(utility::has_quantized_attributes(Some(&attrs)));
    assert!(!utility::has_quantized_attributes::<Host>(None));
    assert_eq!(utility::get_attribute_by_name(&attrs, "_FEATURE_ID_2").unwrap().set_index, Some(2));
    assert!(utility::get_attribute_by_semantic(
        &attrs,
        mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId),
        Some(0)
    )
    .is_none());
    let primitive = mc::Primitive { attributes: attrs, ..Default::default() };
    assert_eq!(
        utility::get_position_min_max(&primitive, Some(Cartesian3::ONE), Some(Cartesian3::new(2., 3., 4.))).unwrap(),
        (Some(Cartesian3::new(0., -1., -2.)), Some(Cartesian3::new(3., 4., 5.)))
    );
}
