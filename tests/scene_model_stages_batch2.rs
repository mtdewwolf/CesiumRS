mod common;

use std::{cell::RefCell, rc::Rc};

use cesium_rs::{
    core::*,
    renderer::*,
    scene::{
        model::{
            edge_visibility_pipeline_stage, image_based_lighting_pipeline_stage, instancing_pipeline_stage,
            point_cloud_styling_pipeline_stage, scene_mode_2d_pipeline_stage, ImageBasedLightingOptions,
            ModelRenderResources, ModelUniformState, NodeRenderResources, PipelineResources, PointCloudStylingOptions,
            PrimitiveRenderResources,
        },
        model_components as mc,
        point_cloud_shading::PointCloudShading,
        AttributeType, Camera, CameraOptions, FrameState, InstanceAttributeSemantic, SceneMode,
        VertexAttributeSemantic,
    },
};
use common::*;
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct TestGl;

impl Gl for TestGl {
    type Object = u32;

    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }

    fn invoke(&self, name: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            return Ok(GlValue::Object(1));
        }
        Ok(GlValue::Null)
    }
}

fn shader_snapshot(r: &PipelineResources) -> Value {
    let (v, f) = r.shader_builder.sources();
    let locations = r
        .shader_builder
        .attribute_locations()
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "vertex": { "defines": v.defines },
        "fragment": { "defines": f.defines },
        "attributeLocations": locations,
    })
}

fn enc_components(v: &[f32]) -> Value {
    if v.len() == 1 {
        num(v[0] as f64)
    } else {
        json!(v.iter().map(|&x| num(x as f64)).collect::<Vec<_>>())
    }
}

fn enc_uniform(v: UniformValue) -> Value {
    match v {
        UniformValue::Float(v) => enc_components(&v),
        UniformValue::Int(v) if v.len() == 1 => json!(v[0]),
        UniformValue::Int(v) => json!(v),
        UniformValue::FloatArray { values, .. } => {
            if values.len() == 27 {
                // 9 3D vectors
                let chunks: Vec<Value> = values.chunks(3).map(|c| json!([c[0], c[1], c[2]])).collect();
                json!(chunks)
            } else {
                json!(values)
            }
        }
        UniformValue::IntArray { values, .. } => json!(values),
        UniformValue::UIntArray { values, .. } => json!(values),
        UniformValue::MatrixArray { values, .. } => json!(values),
        UniformValue::Matrix { values, .. } => json!(values),
    }
}

fn uniforms(r: &PipelineResources) -> Value {
    let values = r
        .uniform_map
        .iter()
        .map(|(name, provider)| (name.clone(), enc_uniform(provider())))
        .collect::<serde_json::Map<_, _>>();
    Value::Object(values)
}

#[test]
fn model_stages_batch2_match_cesium() {
    let context = Context::new_webgl2(TestGl);

    run(
        "scene",
        "ModelStagesBatch2",
        |_| Tol { abs: 1e-5, rel: 1e-5, vector: true },
        |f, a| {
            let c = &a[0];
            match f {
                "instancing" => {
                    let tx = c.get("tx").and_then(Value::as_f64).unwrap_or(0.0);
                    let ty = c.get("ty").and_then(Value::as_f64).unwrap_or(0.0);
                    let tz = c.get("tz").and_then(Value::as_f64).unwrap_or(0.0);

                    let state = Rc::new(RefCell::new(ModelUniformState {
                        scene_graph_computed_model_matrix: Matrix4::from_translation(Cartesian3::new(tx, ty, tz)),
                        ..Default::default()
                    }));
                    let model_res = ModelRenderResources::<TestGl>::new(state);
                    let mut node_res = NodeRenderResources::new(&model_res, 0);

                    let mut attrs = Vec::new();
                    if c.get("hasTranslation") == Some(&json!(true)) {
                        attrs.push(mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Translation)),
                            attribute_type: Some(AttributeType::Vec3),
                            component_datatype: Some(ComponentDatatype::Float),
                            count: Some(2),
                            typed_array: Some(TypedArray::Float32(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])),
                            ..Default::default()
                        });
                    }
                    if c.get("hasRotation") == Some(&json!(true)) {
                        attrs.push(mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Rotation)),
                            attribute_type: Some(AttributeType::Vec4),
                            component_datatype: Some(ComponentDatatype::Float),
                            count: Some(2),
                            typed_array: Some(TypedArray::Float32(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])),
                            ..Default::default()
                        });
                    }
                    if c.get("hasScale") == Some(&json!(true)) {
                        attrs.push(mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Scale)),
                            attribute_type: Some(AttributeType::Vec3),
                            component_datatype: Some(ComponentDatatype::Float),
                            count: Some(2),
                            typed_array: Some(TypedArray::Float32(vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0])),
                            ..Default::default()
                        });
                    }
                    if c.get("hasFeatureId") == Some(&json!(true)) {
                        attrs.push(mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::FeatureId)),
                            set_index: Some(0),
                            attribute_type: Some(AttributeType::Scalar),
                            component_datatype: Some(ComponentDatatype::Float),
                            count: Some(2),
                            typed_array: Some(TypedArray::Float32(vec![0.0, 1.0])),
                            ..Default::default()
                        });
                    }

                    let mut node = mc::Node {
                        instances: Some(mc::Instances {
                            attributes: attrs,
                            transform_in_world_space: c.get("transformInWorldSpace") == Some(&json!(true)),
                            ..Default::default()
                        }),
                        ..Default::default()
                    };

                    let mut runtime_node = instancing_pipeline_stage::RuntimeNodeInstancingState::default();
                    let projection: Rc<dyn MapProjection> = Rc::new(GeographicProjection::default());
                    let view_matrix = Rc::new(RefCell::new(Matrix4::IDENTITY));
                    let is_2d = c.get("mode2D") == Some(&json!(true));
                    let mode = if is_2d { SceneMode::Scene2D } else { SceneMode::Scene3D };

                    let mut model_buffers = Vec::new();
                    instancing_pipeline_stage::process(
                        &mut node_res,
                        &mut node,
                        &mut runtime_node,
                        mode,
                        !is_2d,
                        1.0,
                        projection.clone(),
                        &context,
                        view_matrix.clone(),
                        view_matrix,
                        &mut model_buffers,
                    )
                    .unwrap();

                    let mut out = shader_snapshot(&node_res);
                    out["attributeCount"] = json!(node_res.attributes.len());
                    out["hasInstances"] = Value::Null;
                    out["instanceCount"] = json!(node_res.instance_count);
                    Some(Out::Ok(out))
                }
                "sceneMode2D" => {
                    let tx = c.get("tx").and_then(Value::as_f64).unwrap_or(6378137.0);
                    let state = Rc::new(RefCell::new(ModelUniformState {
                        scene_graph_computed_model_matrix: Matrix4::from_translation(Cartesian3::new(tx, 0.0, 0.0)),
                        ..Default::default()
                    }));
                    let model_res = ModelRenderResources::<TestGl>::new(state);
                    let mut node_res = NodeRenderResources::new(&model_res, 0);
                    let instanced = c.get("instanced") == Some(&json!(true));
                    node_res.has_instances = instanced;

                    let mut prim = mc::Primitive {
                        attributes: vec![mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                            attribute_type: Some(AttributeType::Vec3),
                            count: Some(2),
                            min: Some(mc::MathValue::Cartesian3(Cartesian3::new(-1.0, -1.0, -1.0))),
                            max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1.0, 1.0, 1.0))),
                            typed_array: Some(TypedArray::Float32(vec![-1.0, -1.0, -1.0, 1.0, 1.0, 1.0])),
                            ..Default::default()
                        }],
                        primitive_type: Some(PrimitiveType::Triangles),
                        ..Default::default()
                    };

                    let mut prim_res = PrimitiveRenderResources::new(&node_res, 0, &prim, None, None).unwrap();
                    let mut runtime_prim = scene_mode_2d_pipeline_stage::RuntimePrimitive2DState::default();
                    let projection = GeographicProjection::default();
                    let view_matrix = Rc::new(RefCell::new(Matrix4::IDENTITY));
                    let mut model_buffers = Vec::new();

                    scene_mode_2d_pipeline_stage::process(
                        &mut prim_res,
                        &mut prim,
                        &mut runtime_prim,
                        &Matrix4::IDENTITY,
                        SceneMode::Scene2D,
                        1.0,
                        &projection,
                        &context,
                        view_matrix,
                        &mut model_buffers,
                    )
                    .unwrap();

                    let mut out = shader_snapshot(&prim_res);
                    out["uniforms"] = uniforms(&prim_res);
                    out["hasPositionBuffer2D"] = json!(runtime_prim.position_buffer_2d.is_some());
                    out["boundingSphere2D"] = if let Some(bs) = runtime_prim.bounding_sphere_2d {
                        json!({
                            "center": [bs.center.x, bs.center.y, bs.center.z],
                            "radius": bs.radius,
                        })
                    } else {
                        Value::Null
                    };
                    Some(Out::Ok(out))
                }
                "ibl" => {
                    let state = Rc::new(RefCell::new(ModelUniformState::default()));
                    let mut model_res = ModelRenderResources::<TestGl>::new(state);

                    let custom_sh = c.get("customSH") == Some(&json!(true));
                    let sh = if custom_sh {
                        Some([
                            Cartesian3::new(1.0, 2.0, 3.0),
                            Cartesian3::new(4.0, 5.0, 6.0),
                            Cartesian3::new(7.0, 8.0, 9.0),
                            Cartesian3::new(1.0, 0.0, 0.0),
                            Cartesian3::new(0.0, 1.0, 0.0),
                            Cartesian3::new(0.0, 0.0, 1.0),
                            Cartesian3::new(1.0, 1.0, 0.0),
                            Cartesian3::new(0.0, 1.0, 1.0),
                            Cartesian3::new(1.0, 0.0, 1.0),
                        ])
                    } else {
                        None
                    };

                    let cube_map_ready = c.get("cubeMapReady") == Some(&json!(true));
                    let ibl = ImageBasedLightingOptions {
                        enabled: c.get("enabled") != Some(&json!(false)),
                        image_based_lighting_factor: Cartesian2::new(
                            c.get("factorX").and_then(Value::as_f64).unwrap_or(1.0),
                            c.get("factorY").and_then(Value::as_f64).unwrap_or(1.0),
                        ),
                        spherical_harmonic_coefficients: sh,
                        use_spherical_harmonics: c.get("useSphericalHarmonics") == Some(&json!(true)),
                        use_specular_environment_maps: false,
                        use_default_spherical_harmonics: c.get("useDefaultSH") == Some(&json!(true)),
                        use_default_specular_maps: c.get("useDefaultSpecular") == Some(&json!(true)),
                        reference_frame_matrix: Matrix3::IDENTITY,
                        specular_cube_map_ready: cube_map_ready,
                        specular_environment_map_atlas: false,
                        maximum_mipmap_level: if cube_map_ready { 5.0 } else { 0.0 },
                        environment_map_intensity: c.get("intensity").and_then(Value::as_f64).unwrap_or(1.0),
                        specular_supported: true,
                    };

                    image_based_lighting_pipeline_stage::process(&mut model_res, &ibl);

                    let mut out = shader_snapshot(&model_res);
                    let mut unifs = uniforms(&model_res);
                    if cube_map_ready {
                        unifs["model_specularEnvironmentMaps"] = json!({"fake": true});
                    } else {
                        unifs["model_specularEnvironmentMaps"] = Value::Null;
                    }
                    if !custom_sh {
                        unifs["model_sphericalHarmonicCoefficients"] = Value::Null;
                    }
                    if !cube_map_ready {
                        unifs["model_specularEnvironmentMapsMaximumLOD"] = Value::Null;
                    }
                    out["uniforms"] = unifs;
                    Some(Out::Ok(out))
                }
                "pointCloud" => {
                    let state = Rc::new(RefCell::new(ModelUniformState::default()));
                    let model_res = ModelRenderResources::<TestGl>::new(state);
                    let node_res = NodeRenderResources::new(&model_res, 0);

                    let prim = mc::Primitive {
                        attributes: vec![mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                            attribute_type: Some(AttributeType::Vec3),
                            count: Some(10),
                            min: Some(mc::MathValue::Cartesian3(Cartesian3::new(-1.0, -1.0, -1.0))),
                            max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1.0, 1.0, 1.0))),
                            ..Default::default()
                        }],
                        primitive_type: Some(PrimitiveType::Points),
                        ..Default::default()
                    };

                    let mut prim_res = PrimitiveRenderResources::new(&node_res, 0, &prim, None, None).unwrap();

                    let options = PointCloudStylingOptions {
                        shading: PointCloudShading {
                            attenuation: c.get("attenuation") == Some(&json!(true)),
                            back_face_culling: c.get("backFaceCulling") == Some(&json!(true)),
                            maximum_attenuation: c.get("maxAttenuation").and_then(Value::as_f64).or(Some(4.0)),
                            geometric_error_scale: c.get("geomScale").and_then(Value::as_f64).unwrap_or(1.0),
                            base_resolution: c.get("baseRes").and_then(Value::as_f64),
                            ..Default::default()
                        },
                        style_functions: None,
                        default_point_size: 1.0,
                        geometric_error: Some(0.05),
                        time_since_load: 0.0,
                        node_transform: Matrix4::IDENTITY,
                    };

                    let mut camera = Camera::new(CameraOptions::new(8.0, 8.0));
                    let uniform_cam = camera.uniform_camera();
                    let culling = camera.frustum.compute_culling_volume(
                        uniform_cam.position,
                        uniform_cam.direction,
                        uniform_cam.up,
                    );
                    let is_2d = c.get("mode2D") == Some(&json!(true));
                    let mut frame = FrameState::new(
                        UniformFrame::new(uniform_cam, JulianDate::new(2451545.0, 0.0, TimeStandard::Tai)),
                        culling,
                        [8, 1080],
                    );
                    frame.mode = if is_2d { SceneMode::Scene2D } else { SceneMode::Scene3D };
                    frame.pixel_ratio = c.get("pixelRatio").and_then(Value::as_f64).unwrap_or(1.0);

                    point_cloud_styling_pipeline_stage::process(&mut prim_res, &prim, &options, &frame).unwrap();

                    let mut out = shader_snapshot(&prim_res);
                    let mut unifs = uniforms(&prim_res);
                    if !is_2d {
                        unifs["model_pointCloudParameters"] = Value::Null;
                    }
                    out["uniforms"] = unifs;
                    Some(Out::Ok(out))
                }
                "edgeVisibility" => {
                    let state = Rc::new(RefCell::new(ModelUniformState::default()));
                    let model_res = ModelRenderResources::<TestGl>::new(state);
                    let node_res = NodeRenderResources::new(&model_res, 0);

                    let has_edges = c.get("hasEdges") == Some(&json!(true));
                    let edge_visibility = if has_edges {
                        Some(mc::EdgeVisibility {
                            visibility: vec![mc::MathValue::Number(7.0)],
                            material: None,
                            silhouette_normals: None,
                        })
                    } else {
                        None
                    };

                    let prim = mc::Primitive {
                        attributes: vec![mc::Attribute {
                            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                            attribute_type: Some(AttributeType::Vec3),
                            count: Some(3),
                            min: Some(mc::MathValue::Cartesian3(Cartesian3::new(0.0, 0.0, 0.0))),
                            max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1.0, 1.0, 0.0))),
                            typed_array: Some(TypedArray::Float32(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])),
                            ..Default::default()
                        }],
                        indices: Some(mc::Indices {
                            count: Some(3),
                            typed_array: Some(TypedArray::Uint16(vec![0, 1, 2])),
                            ..Default::default()
                        }),
                        edge_visibility,
                        primitive_type: Some(PrimitiveType::Triangles),
                        ..Default::default()
                    };

                    let mut prim_res = PrimitiveRenderResources::new(&node_res, 0, &prim, None, None).unwrap();
                    prim_res.shader_builder.add_function(
                        "setDynamicVaryingsVS",
                        "void setDynamicVaryingsVS()",
                        ShaderDestination::VERTEX,
                    );

                    let mut camera = Camera::new(CameraOptions::new(8.0, 8.0));
                    let uniform_cam = camera.uniform_camera();
                    let culling = camera.frustum.compute_culling_volume(
                        uniform_cam.position,
                        uniform_cam.direction,
                        uniform_cam.up,
                    );
                    let mut frame = FrameState::new(
                        UniformFrame::new(uniform_cam, JulianDate::new(2451545.0, 0.0, TimeStandard::Tai)),
                        culling,
                        [8, 8],
                    );

                    edge_visibility_pipeline_stage::process(&mut prim_res, &prim, &mut frame, &context).unwrap();

                    let mut out = shader_snapshot(&prim_res);
                    out["hasEdgeGeometry"] = json!(prim_res.edge_geometry.is_some());
                    out["edgePass"] =
                        if let Some(ref eg) = prim_res.edge_geometry { json!(eg.pass as u32) } else { Value::Null };
                    out["edgeIndexCount"] =
                        if let Some(ref eg) = prim_res.edge_geometry { json!(eg.index_count) } else { Value::Null };
                    out["edgeVisibilityRequested"] = json!(frame.edge_visibility_requested);
                    Some(Out::Ok(out))
                }
                other => panic!("unknown stage {other}"),
            }
        },
    );
}
