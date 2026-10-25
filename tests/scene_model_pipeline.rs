mod common;
use cesium_rs::{
    core::*,
    renderer::*,
    scene::{model::*, model_components as mc, *},
};
use common::*;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
struct Host;
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, _: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        unreachable!()
    }
}
fn snapshot(r: &PipelineResources) -> Value {
    let (v, f) = r.shader_builder.sources();
    let b = &r.render_state_options.blending;
    let uniforms = r
        .uniform_map
        .iter()
        .map(|(n, f)| {
            let v = match f() {
                UniformValue::Float(v) if v.len() == 1 => json!(v[0]),
                UniformValue::Float(v) => json!(v),
                UniformValue::FloatArray { values, .. } => json!(values),
                UniformValue::MatrixArray { values, .. } => json!(values.chunks(16).collect::<Vec<_>>()),
                _ => panic!(),
            };
            (n.clone(), v)
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"pass":r.alpha_options.borrow().pass.map(|p|p as u32),"cutoff":r.alpha_options.borrow().alpha_cutoff,"depth":r.render_state_options.depth_test,"depthMask":r.render_state_options.depth_mask,"cull":r.render_state_options.cull,"colorMask":r.render_state_options.color_mask,"blending":{"enabled":b.enabled,"equationRgb":b.equation_rgb,"equationAlpha":b.equation_alpha,"functionSourceRgb":b.function_source_rgb,"functionSourceAlpha":b.function_source_alpha,"functionDestinationRgb":b.function_destination_rgb,"functionDestinationAlpha":b.function_destination_alpha},"vertex":{"defines":v.defines,"source":v.sources[0]},"fragment":{"defines":f.defines,"source":f.sources[0]},"uniforms":uniforms})
}
#[test]
fn pipeline_resources_and_stages_match_cesium() {
    run(
        "scene",
        "ModelPipeline",
        |_| Tol { abs: 1e-6, rel: 1e-6, vector: false },
        |f, a| {
            let a = &a[0];
            let state = Rc::new(RefCell::new(ModelUniformState::default()));
            let mut model = ModelRenderResources::<Host>::new(state.clone());
            match f {
                "stages" => {
                    state.borrow_mut().color = Color::new(0.2, 0.4, 0.6, n(&a["alpha"]));
                    state.borrow_mut().color_blend_mode = ColorBlendMode::from_value(u(&a["mode"]) as i32).unwrap();
                    state.borrow_mut().color_blend_amount = 0.25;
                    state.borrow_mut().split_direction = -1.;
                    model.alpha_options.borrow_mut().alpha_cutoff = opt(&a["cutoff"]);
                    model_color_pipeline_stage::process(&mut model, a["silhouette"].as_bool().unwrap());
                    model_splitter_pipeline_stage::process(&mut model);
                    alpha_pipeline_stage::process(&mut model);
                    Some(Out::Ok(snapshot(&model)))
                }
                "resources" => {
                    model.shader_builder.add_define("MODEL", None, ShaderDestination::BOTH);
                    model.uniform_map.insert("test".into(), Rc::new(|| UniformValue::Float(vec![3.])));
                    model.has_silhouette = true;
                    let mut node = NodeRenderResources::new(&model, 0);
                    node.instance_count = if a["instanced"].as_bool().unwrap() { 8 } else { 0 };
                    node.shader_builder.add_define("NODE", None, ShaderDestination::BOTH);
                    let primitive = mc::Primitive {
                        attributes: vec![mc::Attribute::<Host> {
                            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                            count: Some(4),
                            min: Some(mc::MathValue::Cartesian3(-Cartesian3::ONE)),
                            max: Some(mc::MathValue::Cartesian3(Cartesian3::ONE)),
                            ..Default::default()
                        }],
                        indices: if a["indexed"].as_bool().unwrap() {
                            Some(mc::Indices { count: Some(6), ..Default::default() })
                        } else {
                            None
                        },
                        primitive_type: Some(PrimitiveType::Triangles),
                        ..Default::default()
                    };
                    let instanced = a["instanced"].as_bool().unwrap();
                    let mut p = PrimitiveRenderResources::new(
                        &node,
                        0,
                        &primitive,
                        instanced.then_some(Cartesian3::new(-2., -3., -4.)),
                        instanced.then_some(Cartesian3::new(2., 3., 4.)),
                    )
                    .unwrap();
                    p.shader_builder.add_define("PRIMITIVE", None, ShaderDestination::BOTH);
                    p.render_state_options.cull.enabled = true;
                    p.alpha_options.borrow_mut().alpha_cutoff = Some(0.3);
                    Some(Out::Ok(
                        json!({"model":snapshot(&model),"node":snapshot(&node),"primitive":snapshot(&p),"count":p.count,"instanceCount":p.instance_count,"attributeIndex":p.attribute_index,"featureIndex":p.feature_id_vertex_attribute_set_index,"min":p.position_min.map(|v|v.enc()),"max":p.position_max.map(|v|v.enc()),"bounds":p.bounding_sphere.enc(),"hasSilhouette":p.has_silhouette,"hasSkipLevelOfDetail":p.has_skip_level_of_detail,"lighting":p.lighting_options.lighting_model as u32,"propertyTable":p.has_property_table}),
                    ))
                }
                "skinning" => {
                    let a = &a;
                    let max = n(&a["maximumSet"]) as i32;
                    let primitive = mc::Primitive::<Host> {
                        attributes: if max < 0 {
                            vec![]
                        } else {
                            [VertexAttributeSemantic::Joints, VertexAttributeSemantic::Weights]
                                .into_iter()
                                .map(|s| mc::Attribute {
                                    semantic: Some(mc::AttributeSemantic::Vertex(s)),
                                    set_index: Some(max as u32),
                                    ..Default::default()
                                })
                                .collect()
                        },
                        ..Default::default()
                    };
                    skinning_pipeline_stage::process(
                        &mut model,
                        &primitive,
                        Rc::new(RefCell::new(vec![Matrix4::IDENTITY; u(&a["joints"]) as usize])),
                    )
                    .unwrap();
                    Some(Out::Ok(snapshot(&model)))
                }
                "morph" => {
                    let count = u(&a["count"]) as usize;
                    let node = NodeRenderResources::new(&model, 0);
                    let mut primitive = mc::Primitive::<Host> {
                        attributes: vec![mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                            count: Some(4),
                            min: Some(mc::MathValue::Cartesian3(-Cartesian3::ONE)),
                            max: Some(mc::MathValue::Cartesian3(Cartesian3::ONE)),
                            ..Default::default()
                        }],
                        primitive_type: Some(PrimitiveType::Triangles),
                        ..Default::default()
                    };
                    let mut p = PrimitiveRenderResources::new(&node, 0, &primitive, None, None).unwrap();
                    let semantics = if a["normals"].as_bool().unwrap() {
                        vec![
                            VertexAttributeSemantic::Position,
                            VertexAttributeSemantic::Normal,
                            VertexAttributeSemantic::Tangent,
                            VertexAttributeSemantic::Color,
                        ]
                    } else {
                        vec![VertexAttributeSemantic::Position]
                    };
                    primitive.morph_targets = (0..count)
                        .map(|_| mc::MorphTarget {
                            attributes: semantics
                                .iter()
                                .map(|&s| mc::Attribute {
                                    semantic: Some(mc::AttributeSemantic::Vertex(s)),
                                    attribute_type: Some(AttributeType::Vec3),
                                    component_datatype: Some(ComponentDatatype::Float),
                                    constant: Some(mc::MathValue::Cartesian3(Cartesian3::new(1., 2., 3.))),
                                    byte_stride: Some(12),
                                    ..Default::default()
                                })
                                .collect(),
                        })
                        .collect();
                    morph_targets_pipeline_stage::process(&mut p, &primitive, Rc::new(RefCell::new(vec![0.25; count])))
                        .unwrap();
                    let mut result = snapshot(&p);
                    result["attributeIndex"] = json!(p.attribute_index);
                    result["attributes"]=p.attributes.iter().map(|a|{let value=if let AttributeSource::Constant(v)=&a.source{json!(v)}else{Value::Null};json!({"index":a.index,"value":value,"components":a.components,"datatype":a.datatype as u32,"offset":a.offset,"stride":a.stride,"normalize":a.normalize})}).collect();
                    Some(Out::Ok(result))
                }
                _ => None,
            }
        },
    );
}
#[test]
fn uniforms_read_current_model_values() {
    let state = Rc::new(RefCell::new(ModelUniformState::default()));
    let mut r = ModelRenderResources::<Host>::new(state.clone());
    model_color_pipeline_stage::process(&mut r, false);
    model_splitter_pipeline_stage::process(&mut r);
    r.alpha_options.borrow_mut().alpha_cutoff = Some(0.5);
    alpha_pipeline_stage::process(&mut r);
    state.borrow_mut().color = Color::RED;
    state.borrow_mut().split_direction = 1.;
    r.alpha_options.borrow_mut().alpha_cutoff = Some(0.2);
    let snap = snapshot(&r);
    assert_eq!(snap["uniforms"]["model_color"], json!([1., 0., 0., 1.]));
    assert_eq!(snap["uniforms"]["model_splitDirection"], json!(1.));
    assert!((n(&snap["uniforms"]["u_alphaCutoff"]) - 0.2).abs() < 1e-6);
    assert_eq!((r.renderer_uniform_map())().unwrap().len(), 4);
}
