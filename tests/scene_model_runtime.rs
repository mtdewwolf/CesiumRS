mod common;
use cesium_rs::{
    core::*,
    renderer::{Gl, UniformFrame},
    scene::{
        camera::{Camera, CameraOptions},
        cull_face::CullFace,
        model::{
            model_animation::ModelAnimationOptions,
            model_animation_channel::ModelAnimationChannel,
            model_animation_collection::ModelAnimationCollection,
            model_articulation::ModelArticulation,
            model_articulation_stage::ModelArticulationStage,
            model_matrix_update_stage::ModelMatrixUpdateStage,
            model_runtime_node::ModelRuntimeNode,
            model_runtime_primitive::{ModelRuntimePrimitive, PrimitivePipelineStage, RuntimeDrawCommand},
            model_scene_graph::{ModelPipelineProperties, ModelSceneGraph, ModelSceneGraphOptions},
            MappedPositions,
        },
        model_components as mc, FrameState, SceneMode,
    },
};
use common::*;
use serde_json::{json, Value};
use std::rc::Rc;

#[derive(Clone)]
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

#[derive(Clone, Debug)]
struct MockDrawCommand {
    model_matrix: Matrix4,
    primitive_type: Option<PrimitiveType>,
    cull_face: CullFace,
    back_face_culling: bool,
    shadows: bool,
    debug_show_bounding_volume: bool,
}

impl RuntimeDrawCommand for MockDrawCommand {
    fn model_matrix(&self) -> &Matrix4 {
        &self.model_matrix
    }
    fn set_model_matrix(&mut self, m: Matrix4) {
        self.model_matrix = m;
    }
    fn primitive_type(&self) -> Option<PrimitiveType> {
        self.primitive_type
    }
    fn set_cull_face(&mut self, c: CullFace) {
        self.cull_face = c;
    }
    fn set_back_face_culling(&mut self, b: bool) {
        self.back_face_culling = b;
    }
    fn set_shadows(&mut self, s: bool) {
        self.shadows = s;
    }
    fn set_debug_show_bounding_volume(&mut self, d: bool) {
        self.debug_show_bounding_volume = d;
    }
}

fn stage_name(stage: PrimitivePipelineStage) -> &'static str {
    match stage {
        PrimitivePipelineStage::SceneMode2D => "SceneMode2DPipelineStage",
        PrimitivePipelineStage::Geometry => "GeometryPipelineStage",
        PrimitivePipelineStage::Wireframe => "WireframePipelineStage",
        PrimitivePipelineStage::Classification => "ClassificationPipelineStage",
        PrimitivePipelineStage::MorphTargets => "MorphTargetsPipelineStage",
        PrimitivePipelineStage::Skinning => "SkinningPipelineStage",
        PrimitivePipelineStage::PointCloudStyling => "PointCloudStylingPipelineStage",
        PrimitivePipelineStage::Dequantization => "DequantizationPipelineStage",
        PrimitivePipelineStage::Imagery => "ImageryPipelineStage",
        PrimitivePipelineStage::Material => "MaterialPipelineStage",
        PrimitivePipelineStage::FeatureId => "FeatureIdPipelineStage",
        PrimitivePipelineStage::Metadata => "MetadataPipelineStage",
        PrimitivePipelineStage::MetadataPicking => "MetadataPickingPipelineStage",
        PrimitivePipelineStage::SelectedFeatureId => "SelectedFeatureIdPipelineStage",
        PrimitivePipelineStage::BatchTexture => "BatchTexturePipelineStage",
        PrimitivePipelineStage::CpuStyling => "CpuStylingPipelineStage",
        PrimitivePipelineStage::VerticalExaggeration => "VerticalExaggerationPipelineStage",
        PrimitivePipelineStage::CustomShader => "CustomShaderPipelineStage",
        PrimitivePipelineStage::Lighting => "LightingPipelineStage",
        PrimitivePipelineStage::Picking => "PickingPipelineStage",
        PrimitivePipelineStage::PrimitiveOutline => "PrimitiveOutlinePipelineStage",
        PrimitivePipelineStage::EdgeVisibility => "EdgeVisibilityPipelineStage",
        PrimitivePipelineStage::EdgeDetection => "EdgeDetectionPipelineStage",
        PrimitivePipelineStage::Alpha => "AlphaPipelineStage",
        PrimitivePipelineStage::PrimitiveStatistics => "PrimitiveStatisticsPipelineStage",
    }
}

fn parse_stage_type(name: &str) -> ArticulationStageType {
    match name {
        "xRotate" => ArticulationStageType::XRotate,
        "yRotate" => ArticulationStageType::YRotate,
        "zRotate" => ArticulationStageType::ZRotate,
        "xTranslate" => ArticulationStageType::XTranslate,
        "yTranslate" => ArticulationStageType::YTranslate,
        "zTranslate" => ArticulationStageType::ZTranslate,
        "xScale" => ArticulationStageType::XScale,
        "yScale" => ArticulationStageType::YScale,
        "zScale" => ArticulationStageType::ZScale,
        "uniformScale" => ArticulationStageType::UniformScale,
        other => panic!("unknown stage type {other}"),
    }
}

#[test]
fn model_articulation_stage_matches_cesium() {
    run(
        "scene",
        "ModelArticulationStage",
        |_| TIGHT,
        |f, a| match f {
            "apply" => {
                let stage_type = parse_stage_type(s(&a[0]));
                let val = n(&a[1]);
                let mat = m4(&a[2]);
                let stage = mc::ArticulationStage {
                    name: Some(s(&a[0]).to_string()),
                    stage_type: Some(stage_type),
                    minimum_value: Some(-100.0),
                    maximum_value: Some(100.0),
                    initial_value: Some(val),
                };
                let runtime_stage = ModelArticulationStage::new(&stage).unwrap();
                let result = runtime_stage.apply_stage_to_matrix(&mat);
                ok(result)
            }
            "chained" => {
                let stages_desc = a[0].as_array().unwrap();
                let mut m = Matrix4::IDENTITY;
                for st in stages_desc {
                    let stage_type = parse_stage_type(s(&st["type"]));
                    let val = n(&st["val"]);
                    let stage = mc::ArticulationStage {
                        name: Some(s(&st["type"]).to_string()),
                        stage_type: Some(stage_type),
                        minimum_value: Some(-1000.0),
                        maximum_value: Some(1000.0),
                        initial_value: Some(val),
                    };
                    let runtime_stage = ModelArticulationStage::new(&stage).unwrap();
                    m = runtime_stage.apply_stage_to_matrix(&m);
                }
                ok(m)
            }
            "clamping" => {
                let min = n(&a[0]);
                let max = n(&a[1]);
                let under = n(&a[2]);
                let inside = n(&a[3]);
                let over = n(&a[4]);
                let stage = mc::ArticulationStage {
                    name: Some("clampTest".into()),
                    stage_type: Some(ArticulationStageType::XTranslate),
                    minimum_value: Some(min),
                    maximum_value: Some(max),
                    initial_value: Some(0.0),
                };
                let mut runtime_stage = ModelArticulationStage::new(&stage).unwrap();
                runtime_stage.set_current_value(under);
                let v_under = runtime_stage.current_value();
                runtime_stage.set_current_value(inside);
                let v_inside = runtime_stage.current_value();
                runtime_stage.set_current_value(over);
                let v_over = runtime_stage.current_value();
                ok(vec![v_under, v_inside, v_over])
            }
            _ => None,
        },
    );
}

#[test]
fn model_articulation_matches_cesium() {
    run(
        "scene",
        "ModelArticulation",
        |_| TIGHT,
        |f, a| match f {
            "applyArticulation" => {
                let stages_desc = a[0].as_array().unwrap();
                let node_matrices = a[1].as_array().unwrap();

                let mut stages = Vec::new();
                for st in stages_desc {
                    stages.push(mc::ArticulationStage {
                        name: Some(s(&st["name"]).to_string()),
                        stage_type: Some(parse_stage_type(s(&st["type"]))),
                        minimum_value: Some(n(&st["min"])),
                        maximum_value: Some(n(&st["max"])),
                        initial_value: Some(0.0),
                    });
                }
                let articulation = mc::Articulation { name: Some("testArticulation".into()), stages };
                let mut runtime_art = ModelArticulation::new(&articulation).unwrap();

                let mut runtime_nodes = Vec::new();
                for (i, m) in node_matrices.iter().enumerate() {
                    let mat = m4(m);
                    let node_comp = mc::Node::<Host> { index: Some(i), matrix: Some(mat), ..Default::default() };
                    let runtime_node =
                        ModelRuntimeNode::<Host, ()>::new(&node_comp, mat, Matrix4::IDENTITY, Vec::new());
                    runtime_nodes.push(Some(runtime_node));
                    runtime_art.add_runtime_node(i);
                }

                for st in stages_desc {
                    runtime_art.set_articulation_stage(s(&st["name"]), n(&st["val"]));
                }

                runtime_art.apply(&mut runtime_nodes);

                let transforms: Vec<Matrix4> = runtime_nodes.iter().map(|n| *n.as_ref().unwrap().transform()).collect();
                ok(transforms)
            }
            _ => None,
        },
    );
}

#[test]
fn model_animation_channel_matches_cesium() {
    run(
        "scene",
        "ModelAnimationChannel",
        |_| TIGHT,
        |f, a| match f {
            "animateTranslation" => {
                let times = nums(&Value::Array(a.to_vec()));
                let sampler = mc::AnimationSampler {
                    input: vec![0.0, 1.0, 2.0],
                    interpolation: Some(InterpolationType::Linear),
                    output: vec![
                        mc::MathValue::Cartesian3(Cartesian3::new(0.0, 0.0, 0.0)),
                        mc::MathValue::Cartesian3(Cartesian3::new(10.0, 20.0, 30.0)),
                        mc::MathValue::Cartesian3(Cartesian3::new(20.0, 40.0, 60.0)),
                    ],
                };
                let channel = mc::AnimationChannel {
                    sampler: Some(0),
                    target: Some(mc::AnimationTarget {
                        node: Some(0),
                        path: Some(mc::AnimatedPropertyType::Translation),
                    }),
                };
                let anim_channel = ModelAnimationChannel::new(&channel, &sampler, 0, 1).unwrap();
                let node = ModelRuntimeNode::<Host, ()>::new(
                    &mc::Node::default(),
                    Matrix4::IDENTITY,
                    Matrix4::IDENTITY,
                    Vec::new(),
                );
                let mut nodes = vec![Some(node)];
                let mut results = Vec::new();
                for t in times {
                    anim_channel.animate(t, &mut nodes, false).unwrap();
                    results.push(nodes[0].as_ref().unwrap().translation().unwrap());
                }
                ok(results)
            }
            "animateRotation" => {
                let times = nums(&Value::Array(a.to_vec()));
                let q0 = Quaternion::IDENTITY;
                let q1 = Quaternion::from_axis_angle(Cartesian3::UNIT_Z, std::f64::consts::FRAC_PI_2);
                let sampler = mc::AnimationSampler {
                    input: vec![0.0, 1.0],
                    interpolation: Some(InterpolationType::Linear),
                    output: vec![mc::MathValue::Quaternion(q0), mc::MathValue::Quaternion(q1)],
                };
                let channel = mc::AnimationChannel {
                    sampler: Some(0),
                    target: Some(mc::AnimationTarget { node: Some(0), path: Some(mc::AnimatedPropertyType::Rotation) }),
                };
                let anim_channel = ModelAnimationChannel::new(&channel, &sampler, 0, 1).unwrap();
                let node = ModelRuntimeNode::<Host, ()>::new(
                    &mc::Node::default(),
                    Matrix4::IDENTITY,
                    Matrix4::IDENTITY,
                    Vec::new(),
                );
                let mut nodes = vec![Some(node)];
                let mut results = Vec::new();
                for t in times {
                    anim_channel.animate(t, &mut nodes, false).unwrap();
                    results.push(nodes[0].as_ref().unwrap().rotation().unwrap());
                }
                ok(results)
            }
            "animateScale" => {
                let times = nums(&Value::Array(a.to_vec()));
                let sampler = mc::AnimationSampler {
                    input: vec![0.0, 1.0, 2.0],
                    interpolation: Some(InterpolationType::Step),
                    output: vec![
                        mc::MathValue::Cartesian3(Cartesian3::new(1.0, 1.0, 1.0)),
                        mc::MathValue::Cartesian3(Cartesian3::new(2.0, 2.0, 2.0)),
                        mc::MathValue::Cartesian3(Cartesian3::new(4.0, 4.0, 4.0)),
                    ],
                };
                let channel = mc::AnimationChannel {
                    sampler: Some(0),
                    target: Some(mc::AnimationTarget { node: Some(0), path: Some(mc::AnimatedPropertyType::Scale) }),
                };
                let anim_channel = ModelAnimationChannel::new(&channel, &sampler, 0, 1).unwrap();
                let node = ModelRuntimeNode::<Host, ()>::new(
                    &mc::Node::default(),
                    Matrix4::IDENTITY,
                    Matrix4::IDENTITY,
                    Vec::new(),
                );
                let mut nodes = vec![Some(node)];
                let mut results = Vec::new();
                for t in times {
                    anim_channel.animate(t, &mut nodes, false).unwrap();
                    results.push(nodes[0].as_ref().unwrap().scale().unwrap());
                }
                ok(results)
            }
            "animateWeights" => {
                let times = nums(&Value::Array(a.to_vec()));
                let values = vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
                let sampler = mc::AnimationSampler {
                    input: vec![0.0, 1.0],
                    interpolation: Some(InterpolationType::Cubicspline),
                    output: values.into_iter().map(mc::MathValue::Number).collect(),
                };
                let channel = mc::AnimationChannel {
                    sampler: Some(0),
                    target: Some(mc::AnimationTarget { node: Some(0), path: Some(mc::AnimatedPropertyType::Weights) }),
                };
                let anim_channel = ModelAnimationChannel::new(&channel, &sampler, 0, 2).unwrap();
                let node_comp = mc::Node { morph_weights: vec![0.0, 0.0], ..Default::default() };
                let node =
                    ModelRuntimeNode::<Host, ()>::new(&node_comp, Matrix4::IDENTITY, Matrix4::IDENTITY, Vec::new());
                let mut nodes = vec![Some(node)];
                let mut results = Vec::new();
                for t in times {
                    anim_channel.animate(t, &mut nodes, false).unwrap();
                    results.push(nodes[0].as_ref().unwrap().morph_weights().to_vec());
                }
                ok(results)
            }
            _ => None,
        },
    );
}

#[test]
fn model_matrix_update_stage_matches_cesium() {
    run(
        "scene",
        "ModelMatrixUpdateStage",
        |_| TIGHT,
        |f, a| match f {
            "updateHierarchy" => {
                let root_t = m4(&a[0]);
                let child_t = m4(&a[1]);
                let grand_t = m4(&a[2]);
                let model_t = m4(&a[3]);

                let components = mc::Components::<Host> {
                    nodes: vec![
                        mc::Node { index: Some(0), matrix: Some(root_t), children: vec![1], ..Default::default() },
                        mc::Node { index: Some(1), matrix: Some(child_t), children: vec![2], ..Default::default() },
                        mc::Node {
                            index: Some(2),
                            matrix: Some(grand_t),
                            primitives: vec![mc::Primitive {
                                primitive_type: Some(PrimitiveType::Triangles),
                                ..Default::default()
                            }],
                            ..Default::default()
                        },
                    ],
                    scene: Some(mc::Scene { nodes: vec![0] }),
                    up_axis: Some(cesium_rs::scene::Axis::Z),
                    forward_axis: Some(cesium_rs::scene::Axis::X),
                    ..Default::default()
                };

                let mut graph = ModelSceneGraph::<Host, MockDrawCommand>::new(
                    components,
                    ModelSceneGraphOptions { model_matrix: model_t, ..Default::default() },
                )
                .unwrap();

                // Install a mock draw command on grandchild primitive
                if let Some(grand) = graph.runtime_node_mut(2) {
                    grand.runtime_primitives[0].draw_command = Some(MockDrawCommand {
                        model_matrix: Matrix4::IDENTITY,
                        primitive_type: Some(PrimitiveType::Triangles),
                        cull_face: CullFace::Back,
                        back_face_culling: true,
                        shadows: true,
                        debug_show_bounding_volume: false,
                    });
                }

                // Mark root dirty
                if let Some(root) = graph.runtime_node_mut(0) {
                    root.mark_transform_dirty();
                }

                let mut camera = Camera::new(CameraOptions::new(8.0, 8.0));
                let frame_state = FrameState::new(
                    UniformFrame::new(camera.uniform_camera(), JulianDate::new(2451545.0, 0.0, TimeStandard::Tai)),
                    CullingVolume::default(),
                    [8, 8],
                );

                ModelMatrixUpdateStage::update(&mut graph, &frame_state);

                let root = graph.runtime_node(0).unwrap();
                let child = graph.runtime_node(1).unwrap();
                let grand = graph.runtime_node(2).unwrap();
                let dc = grand.runtime_primitives[0].draw_command.as_ref().unwrap();

                ok(json!({
                    "rootComputed": root.computed_transform().enc(),
                    "childToRoot": child.transform_to_root().enc(),
                    "childComputed": child.computed_transform().enc(),
                    "grandToRoot": grand.transform_to_root().enc(),
                    "grandComputed": grand.computed_transform().enc(),
                    "drawCommandModelMatrix": dc.model_matrix.enc(),
                    "drawCommandCullFace": dc.cull_face as u32,
                }))
            }
            _ => None,
        },
    );
}

#[test]
fn mapped_positions_matches_cesium() {
    run(
        "scene",
        "MappedPositions",
        |_| TIGHT,
        |f, a| match f {
            "imageryCoordinates" => {
                use cesium_rs::scene::model::{ModelImageryMapping, ModelPrimitiveImagery};
                let mapped = MappedPositions::new(
                    Rc::new(vec![
                        Cartographic::new(0., 0., 0.),
                        Cartographic::new(1., 1., 0.),
                        Cartographic::new(0.5, 0.25, 0.),
                    ]),
                    a[0].as_u64().unwrap() as usize,
                    Rectangle::new(0., 0., 1., 1.),
                    Ellipsoid::UNIT_SPHERE,
                );
                let mut imagery = ModelPrimitiveImagery::<Host>::default();
                imagery.mapped_positions_per_ellipsoid.push(mapped);
                let mapped = imagery.mapped_positions_for_ellipsoid(&Ellipsoid::UNIT_SPHERE).unwrap();
                let projection = GeographicProjection::new(Ellipsoid::UNIT_SPHERE);
                ok(json!(ModelImageryMapping::create_texture_coordinates_for_mapped_positions(mapped, &projection)))
            }
            "properties" => {
                let coords = a[0].as_array().unwrap();
                let r_vals = nums(&a[1]);
                let positions = coords
                    .iter()
                    .map(|c| {
                        let xyz = nums(c);
                        Cartographic::new(xyz[0], xyz[1], xyz[2])
                    })
                    .collect::<Vec<_>>();
                let rectangle = Rectangle::new(r_vals[0], r_vals[1], r_vals[2], r_vals[3]);
                let mp = MappedPositions::new(Rc::new(positions.clone()), positions.len(), rectangle, Ellipsoid::WGS84);
                let radii = mp.ellipsoid().radii;
                ok(json!({
                    "numPositions": mp.num_positions(),
                    "rect": [rectangle.west, rectangle.south, rectangle.east, rectangle.north],
                    "positions": positions.iter().map(|p| [p.longitude, p.latitude, p.height]).collect::<Vec<_>>(),
                    "radii": [radii.x, radii.y, radii.z],
                }))
            }
            _ => None,
        },
    );
}

#[test]
fn model_runtime_primitive_matches_cesium() {
    run(
        "scene",
        "ModelRuntimePrimitive",
        |_| EXACT,
        |f, a| match f {
            "pipelineStages" => {
                let cfg = &a[0];
                let use_2d = cfg["use2D"].as_bool().unwrap();
                let vert_exag = cfg["vertExag"].as_f64().unwrap();
                let wireframe = cfg["wireframe"].as_bool().unwrap();

                let properties = ModelPipelineProperties {
                    use_webgl2: true,
                    project_to_2d: use_2d,
                    has_vertical_exaggeration: vert_exag != 1.0,
                    debug_wireframe: wireframe,
                    enable_debug_wireframe: wireframe,
                    allow_picking: true,
                    ..Default::default()
                };

                let node = mc::Node::<Host>::default();
                let primitive = mc::Primitive::<Host> {
                    primitive_type: Some(PrimitiveType::Triangles),
                    feature_ids: Vec::new(),
                    ..Default::default()
                };

                let mut rp = ModelRuntimePrimitive::<Host, ()>::new(0, 0);
                let mut edge_req = false;
                rp.configure_pipeline(
                    &properties,
                    &node,
                    &primitive,
                    if use_2d { SceneMode::Scene2D } else { SceneMode::Scene3D },
                    false,
                    vert_exag,
                    &mut edge_req,
                );

                let names: Vec<String> = rp.pipeline_stages.iter().map(|&s| stage_name(s).to_string()).collect();
                ok(names)
            }
            _ => None,
        },
    );
}

#[test]
fn model_node_user_animation_and_original_transform() {
    let components = mc::Components::<Host> {
        nodes: vec![mc::Node {
            index: Some(0),
            name: Some("Arm".into()),
            matrix: Some(Matrix4::from_translation(Cartesian3::new(1.0, 2.0, 3.0))),
            ..Default::default()
        }],
        scene: Some(mc::Scene { nodes: vec![0] }),
        ..Default::default()
    };
    let mut graph = ModelSceneGraph::<Host, ()>::new(components, ModelSceneGraphOptions::default()).unwrap();
    let node_handle = graph.nodes_by_name().get("Arm").expect("node exists").clone();

    assert_eq!(node_handle.name(), Some("Arm"));
    assert_eq!(node_handle.id(), Some(0));
    assert!(node_handle.show(&graph));

    let original = *node_handle.original_matrix(&graph).unwrap();
    assert_eq!(original, Matrix4::from_translation(Cartesian3::new(1.0, 2.0, 3.0)));

    // User assigns matrix
    let user_mat = Matrix4::from_scale(Cartesian3::new(2.0, 3.0, 4.0));
    node_handle.set_matrix(&mut graph, Some(&user_mat));
    assert!(graph.user_animation_dirty);
    assert!(graph.runtime_node(0).unwrap().user_animated);
    assert_eq!(node_handle.matrix(&graph), Some(&user_mat));

    // Restore original matrix
    node_handle.set_matrix(&mut graph, None);
    assert!(!graph.runtime_node(0).unwrap().user_animated);
    assert_eq!(node_handle.matrix(&graph), Some(&original));

    // Show toggling
    node_handle.set_show(&mut graph, false);
    assert!(!node_handle.show(&graph));
    node_handle.set_show(&mut graph, true);
    assert!(node_handle.show(&graph));
}

#[test]
fn model_skin_joint_matrices_and_update() {
    let components = mc::Components::<Host> {
        nodes: vec![
            mc::Node {
                index: Some(0),
                matrix: Some(Matrix4::from_translation(Cartesian3::new(10.0, 0.0, 0.0))),
                children: vec![1],
                ..Default::default()
            },
            mc::Node {
                index: Some(1),
                matrix: Some(Matrix4::from_translation(Cartesian3::new(0.0, 5.0, 0.0))),
                skin: Some(0),
                ..Default::default()
            },
        ],
        skins: vec![mc::Skin {
            index: Some(0),
            joints: vec![0],
            inverse_bind_matrices: vec![Matrix4::from_translation(Cartesian3::new(-10.0, 0.0, 0.0))],
        }],
        scene: Some(mc::Scene { nodes: vec![0] }),
        ..Default::default()
    };
    let mut graph = ModelSceneGraph::<Host, ()>::new(components, ModelSceneGraphOptions::default()).unwrap();
    assert_eq!(graph.runtime_skins().len(), 1);
    let skin = &graph.runtime_skins()[0];
    assert_eq!(skin.joints(), &[0]);
    // Joint world transform for joint 0 is Translation(10, 0, 0).
    // joint_matrix = joint_world * inv_bind = Translation(10, 0, 0) * Translation(-10, 0, 0) = IDENTITY.
    assert_eq!(skin.joint_matrices()[0], Matrix4::IDENTITY);

    // Update joint transform and recompute
    if let Some(joint_node) = graph.runtime_node_mut(0) {
        joint_node.set_transform(Matrix4::from_translation(Cartesian3::new(20.0, 0.0, 0.0)));
    }
    graph.update_joint_matrices();
    let updated_skin = &graph.runtime_skins()[0];
    assert_eq!(updated_skin.joint_matrices()[0], Matrix4::from_translation(Cartesian3::new(10.0, 0.0, 0.0)));
}

#[test]
fn model_animation_collection_and_events() {
    let anim_components = vec![mc::Animation {
        name: Some("Walk".into()),
        channels: vec![mc::AnimationChannel {
            sampler: Some(0),
            target: Some(mc::AnimationTarget { node: Some(0), path: Some(mc::AnimatedPropertyType::Translation) }),
        }],
        samplers: vec![mc::AnimationSampler {
            input: vec![0.0, 1.0, 2.0],
            interpolation: Some(InterpolationType::Linear),
            output: vec![
                mc::MathValue::Cartesian3(Cartesian3::ZERO),
                mc::MathValue::Cartesian3(Cartesian3::UNIT_X),
                mc::MathValue::Cartesian3(Cartesian3::new(2.0, 0.0, 0.0)),
            ],
        }],
    }];

    let node =
        ModelRuntimeNode::<Host, ()>::new(&mc::Node::default(), Matrix4::IDENTITY, Matrix4::IDENTITY, Vec::new());
    let mut nodes = vec![Some(node)];

    let mut collection = ModelAnimationCollection::new();
    let anim_idx = collection
        .add(&anim_components, &nodes, &ModelAnimationOptions { name: Some("Walk".into()), ..Default::default() })
        .unwrap();

    assert_eq!(collection.length(), 1);
    assert_eq!(anim_idx, 0);

    let start_date = JulianDate::new(2451545.0, 0.0, TimeStandard::Tai);

    collection.get_mut(0).unwrap().start.add_event_listener({
        let ev = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let ev_clone = ev.clone();
        move |arg| {
            ev_clone.lock().unwrap().push(arg.clone());
        }
    });

    collection.update(&start_date, false, &mut nodes).unwrap();
    collection.raise_pending_events();

    assert_eq!(nodes[0].as_ref().unwrap().translation().unwrap(), Cartesian3::ZERO);

    // Advance time by 0.5s
    let next_date = start_date.add_seconds(0.5);
    collection.update(&next_date, false, &mut nodes).unwrap();
    assert_eq!(nodes[0].as_ref().unwrap().translation().unwrap(), Cartesian3::new(0.5, 0.0, 0.0));

    // Removal
    collection.remove(0);
    assert_eq!(collection.length(), 0);
}

#[test]
fn model_scene_graph_full_lifecycle() {
    let components = mc::Components::<Host> {
        nodes: vec![
            mc::Node {
                index: Some(0),
                name: Some("Root".into()),
                matrix: Some(Matrix4::from_translation(Cartesian3::new(10.0, 0.0, 0.0))),
                children: vec![1],
                ..Default::default()
            },
            mc::Node {
                index: Some(1),
                name: Some("Turret".into()),
                matrix: Some(Matrix4::IDENTITY),
                children: vec![],
                primitives: vec![mc::Primitive {
                    primitive_type: Some(PrimitiveType::Triangles),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        scene: Some(mc::Scene { nodes: vec![0] }),
        articulations: vec![mc::Articulation {
            name: Some("TurretAim".into()),
            stages: vec![mc::ArticulationStage {
                name: Some("Yaw".into()),
                stage_type: Some(ArticulationStageType::ZRotate),
                minimum_value: Some(-180.0),
                maximum_value: Some(180.0),
                initial_value: Some(0.0),
            }],
        }],
        ..Default::default()
    };

    let mut graph = ModelSceneGraph::<Host, MockDrawCommand>::new(
        components,
        ModelSceneGraphOptions {
            model_matrix: Matrix4::from_translation(Cartesian3::new(100.0, 0.0, 0.0)),
            computed_scale: 1.0,
            clamped_scale: 1.0,
            pipeline: ModelPipelineProperties::default(),
        },
    )
    .unwrap();

    assert_eq!(graph.root_nodes(), &[0]);
    assert!(graph.nodes_by_name().get("Root").is_some());
    assert!(graph.nodes_by_name().get("Turret").is_some());

    // Articulation registration and application
    assert_eq!(graph.runtime_articulations().len(), 1);
    graph.set_articulation_stage("TurretAim.Yaw", 90.0);
    graph.apply_articulations();

    let mut camera = Camera::new(CameraOptions::new(8.0, 8.0));
    let frame_state = FrameState::new(
        UniformFrame::new(camera.uniform_camera(), JulianDate::new(2451545.0, 0.0, TimeStandard::Tai)),
        CullingVolume::default(),
        [8, 8],
    );

    graph.update(&frame_state, false);

    // Verify computed model matrix includes axis correction
    let computed_matrix = *graph.computed_model_matrix();
    assert_ne!(computed_matrix, Matrix4::IDENTITY);
}

#[test]
fn model_matrix_update_stage_updates_dirty_child_node_when_root_is_clean() {
    let components = mc::Components::<Host> {
        nodes: vec![
            mc::Node {
                index: Some(0),
                name: Some("Root".into()),
                matrix: Some(Matrix4::IDENTITY),
                children: vec![1],
                ..Default::default()
            },
            mc::Node {
                index: Some(1),
                name: Some("Child".into()),
                matrix: Some(Matrix4::from_translation(Cartesian3::new(10.0, 0.0, 0.0))),
                ..Default::default()
            },
        ],
        scene: Some(mc::Scene { nodes: vec![0] }),
        ..Default::default()
    };

    let mut graph = ModelSceneGraph::<Host, MockDrawCommand>::new(
        components,
        ModelSceneGraphOptions::default(),
    )
    .unwrap();

    let mut camera = Camera::new(CameraOptions::new(8.0, 8.0));
    let frame_state = FrameState::new(
        UniformFrame::new(camera.uniform_camera(), JulianDate::new(2451545.0, 0.0, TimeStandard::Tai)),
        CullingVolume::default(),
        [8, 8],
    );

    // Initially clean everything
    let _ = ModelMatrixUpdateStage::update_transforms(&mut graph, &frame_state);
    assert!(!graph.runtime_node(0).unwrap().is_transform_dirty());
    assert!(!graph.runtime_node(1).unwrap().is_transform_dirty());

    // Only mutate Child (node 1); Root (node 0) remains clean
    let child_handle = graph.nodes_by_name().get("Child").unwrap().clone();
    let new_mat = Matrix4::from_translation(Cartesian3::new(25.0, 5.0, 0.0));
    child_handle.set_matrix(&mut graph, Some(&new_mat));

    assert!(!graph.runtime_node(0).unwrap().is_transform_dirty());
    assert!(graph.runtime_node(1).unwrap().is_transform_dirty());

    let updated = ModelMatrixUpdateStage::update_transforms(&mut graph, &frame_state);
    assert!(updated.contains(&1), "dirty child node must be included in updated transforms");
    assert!(!graph.runtime_node(1).unwrap().is_transform_dirty());
    assert_eq!(
        *graph.runtime_node(1).unwrap().computed_transform(),
        new_mat
    );
}

#[test]
fn model_runtime_primitive_materials_enabled_respects_custom_shader_modes() {
    use cesium_rs::scene::model::model_scene_graph::CustomShaderPipelineProperties;
    let properties_replace = ModelPipelineProperties {
        custom_shader: Some(CustomShaderPipelineProperties {
            has_fragment_shader: true,
            replace_material: true,
        }),
        ..Default::default()
    };
    let mut prim = ModelRuntimePrimitive::<Host>::new(0, 0);
    let node = mc::Node::<Host>::default();
    let primitive = mc::Primitive::<Host>::default();
    let mut req_edge = false;

    prim.configure_pipeline(&properties_replace, &node, &primitive, SceneMode::Scene3D, true, 1.0, &mut req_edge);
    assert!(
        !prim.pipeline_stages.contains(&PrimitivePipelineStage::Material),
        "Material stage should be omitted when custom fragment shader replaces material"
    );

    let properties_vertex_only = ModelPipelineProperties {
        custom_shader: Some(CustomShaderPipelineProperties {
            has_fragment_shader: false,
            replace_material: true,
        }),
        ..Default::default()
    };
    prim.configure_pipeline(&properties_vertex_only, &node, &primitive, SceneMode::Scene3D, true, 1.0, &mut req_edge);
    assert!(
        prim.pipeline_stages.contains(&PrimitivePipelineStage::Material),
        "Material stage should remain enabled when custom shader is vertex-only"
    );

    let properties_modify = ModelPipelineProperties {
        custom_shader: Some(CustomShaderPipelineProperties {
            has_fragment_shader: true,
            replace_material: false,
        }),
        ..Default::default()
    };
    prim.configure_pipeline(&properties_modify, &node, &primitive, SceneMode::Scene3D, true, 1.0, &mut req_edge);
    assert!(
        prim.pipeline_stages.contains(&PrimitivePipelineStage::Material),
        "Material stage should remain enabled when custom shader modifies material without replacing"
    );
}

#[test]
fn model_animation_collection_fires_stop_event_with_remove_on_stop() {
    let components = mc::Components::<Host> {
        nodes: vec![mc::Node {
            index: Some(0),
            name: Some("Target".into()),
            translation: Some(Cartesian3::ZERO),
            ..Default::default()
        }],
        scene: Some(mc::Scene { nodes: vec![0] }),
        ..Default::default()
    };
    let mut graph = ModelSceneGraph::<Host, ()>::new(components, ModelSceneGraphOptions::default()).unwrap();
    let mut collection = ModelAnimationCollection::new();

    let animation = mc::Animation {
        name: Some("TestAnim".into()),
        samplers: vec![mc::AnimationSampler {
            input: vec![0.0, 1.0],
            output: vec![
                mc::MathValue::Cartesian3(Cartesian3::new(0.0, 0.0, 0.0)),
                mc::MathValue::Cartesian3(Cartesian3::new(1.0, 0.0, 0.0)),
            ],
            interpolation: Some(InterpolationType::Linear),
        }],
        channels: vec![mc::AnimationChannel {
            sampler: Some(0),
            target: Some(mc::AnimationTarget {
                node: Some(0),
                path: Some(mc::AnimatedPropertyType::Translation),
            }),
        }],
    };

    let start_date = JulianDate::new(2451545.0, 0.0, TimeStandard::Tai);
    let options = ModelAnimationOptions {
        name: Some("TestAnim".into()),
        start_time: Some(start_date),
        stop_time: Some(start_date.add_seconds(1.0)),
        remove_on_stop: true,
        ..Default::default()
    };

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let anim_id = collection.add(&[animation], graph.runtime_nodes(), &options).unwrap();
    let stopped_received = Arc::new(AtomicBool::new(false));
    let stopped_flag = stopped_received.clone();
    collection.get_mut(0).unwrap().stop.add_event_listener(move |_| {
        stopped_flag.store(true, Ordering::Relaxed);
    });

    // Start playing at t=0
    collection.update(&start_date, false, graph.runtime_nodes_mut()).unwrap();
    collection.raise_pending_events();
    assert!(!stopped_received.load(Ordering::Relaxed));

    // Advance past stop time at t=2.0s
    let stop_date = start_date.add_seconds(2.0);
    collection.update(&stop_date, false, graph.runtime_nodes_mut()).unwrap();
    assert!(!collection.contains(anim_id), "animation should be removed from active set");

    // Raise pending events: stop event must be dispatched
    collection.raise_pending_events();
    assert!(stopped_received.load(Ordering::Relaxed), "stop event listener must be invoked even when remove_on_stop is true");
}

#[test]
fn model_articulation_stage_unconstrained_range() {
    let stage_components = mc::ArticulationStage {
        name: Some("UnconstrainedRoll".into()),
        stage_type: Some(ArticulationStageType::XRotate),
        minimum_value: None,
        maximum_value: None,
        initial_value: None,
    };
    let mut stage = ModelArticulationStage::new(&stage_components).unwrap();
    assert!(stage.set_current_value(45.0));
    assert_eq!(stage.current_value(), 45.0);
    assert!(stage.set_current_value(-90.0));
    assert_eq!(stage.current_value(), -90.0);
}

#[test]
fn model_imagery_mapping_clamps_uvs() {
    use cesium_rs::scene::model::model_imagery_mapping::ModelImageryMapping;
    let rect = Rectangle::from_degrees(-10.0, -10.0, 10.0, 10.0);
    let projection = GeographicProjection::default();
    // Position slightly outside west border:
    let positions = vec![
        Cartographic::from_degrees(-10.0001, 0.0, 0.0),
        Cartographic::from_degrees(10.0001, 0.0, 0.0),
    ];
    let uvs = ModelImageryMapping::create_texture_coordinates(&positions, &rect, &projection);
    assert_eq!(uvs[0], 0.0, "u should be clamped to 0.0");
    assert_eq!(uvs[2], 1.0, "u should be clamped to 1.0");
}

