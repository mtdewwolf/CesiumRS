mod common;
use cesium_rs::{
    core::*,
    renderer::*,
    scene::{model::*, model_components as mc, *},
};
use common::*;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Host;
impl Gl for Host {
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

fn frame_state(
    camera_position: Cartesian3,
    fog_density: f64,
    vertical_exaggeration: f64,
    relative_height: f64,
) -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(8., 8.));
    let mut uniform = camera.uniform_camera();
    uniform.position = camera_position;
    let culling = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    let mut frame =
        FrameState::new(UniformFrame::new(uniform, JulianDate::new(2451545., 0., TimeStandard::Tai)), culling, [8, 8]);
    frame.fog.density = fog_density;
    frame.vertical_exaggeration = vertical_exaggeration;
    frame.vertical_exaggeration_relative_height = relative_height;
    frame
}

fn model_state(c: &Value) -> Rc<RefCell<ModelUniformState>> {
    let mut state = ModelUniformState::default();
    if let Some(t) = c.get("modelType").and_then(Value::as_str) {
        state.model_type = ModelType::from_str_value(t).unwrap();
    }
    state.project_to_2d = c.get("projectTo2D") == Some(&json!(true));
    if c.get("classification") == Some(&json!(true)) {
        state.classification_type = Some(ClassificationType::Cesium3DTile);
    }
    if let Some(v) = c.get("lightColor").and_then(Value::as_array) {
        state.light_color = Some(c3(&Value::Array(v.clone())));
    }
    if let Some(center) = c.get("center").and_then(Value::as_array) {
        let radius = c.get("radius").and_then(Value::as_f64).unwrap_or(10.);
        state.bounding_sphere = BoundingSphere::new(c3(&Value::Array(center.clone())), radius);
    }
    if let Some(v) = c.get("color").and_then(Value::as_array) {
        state.silhouette_color = color(&Value::Array(v.clone()));
    }
    if let Some(v) = c.get("size").and_then(Value::as_f64) {
        state.silhouette_size = v;
    }
    if let Some(v) = c.get("id").and_then(Value::as_u64) {
        state.silhouette_id = Some(v as usize);
    }
    Rc::new(RefCell::new(state))
}

fn math_value(attribute_type: AttributeType, v: &Value) -> mc::MathValue {
    match attribute_type {
        AttributeType::Scalar => mc::MathValue::Number(if let Some(a) = v.as_array() { n(&a[0]) } else { n(v) }),
        AttributeType::Vec2 => mc::MathValue::Cartesian2(c2(v)),
        AttributeType::Vec3 => mc::MathValue::Cartesian3(c3(v)),
        AttributeType::Vec4 => mc::MathValue::Cartesian4(c4(v)),
        AttributeType::Mat2 => mc::MathValue::Matrix2(m2(v)),
        AttributeType::Mat3 => mc::MathValue::Matrix3(m3(v)),
        AttributeType::Mat4 => mc::MathValue::Matrix4(m4(v)),
    }
}

fn attribute(v: &Value, buffer: Option<&Buffer<Host>>) -> mc::Attribute<Host> {
    let attribute_type = AttributeType::from_str_value(s(&v["type"])).unwrap();
    let quantization = v.get("quantization").map(|q| {
        let qtype = AttributeType::from_str_value(s(&q["type"])).unwrap();
        mc::Quantization {
            oct_encoded: q.get("octEncoded") == Some(&json!(true)),
            oct_encoded_zxy: q.get("octEncodedZXY") == Some(&json!(true)),
            normalization_range: q.get("normalizationRange").map(|v| math_value(AttributeType::Scalar, v)),
            quantized_volume_offset: q.get("quantizedVolumeOffset").map(|v| math_value(attribute_type, v)),
            quantized_volume_dimensions: None,
            quantized_volume_step_size: q.get("quantizedVolumeStepSize").map(|v| math_value(attribute_type, v)),
            component_datatype: q
                .get("componentDatatype")
                .and_then(Value::as_u64)
                .and_then(|v| ComponentDatatype::from_gl(v as u32)),
            attribute_type: Some(qtype),
        }
    });
    let constant_type = attribute_type;
    let mut a = mc::Attribute::<Host> {
        name: v.get("name").and_then(Value::as_str).map(str::to_string),
        semantic: v
            .get("semantic")
            .and_then(Value::as_str)
            .and_then(VertexAttributeSemantic::from_gltf_semantic)
            .map(mc::AttributeSemantic::Vertex),
        set_index: v.get("setIndex").and_then(Value::as_u64).map(|x| x as u32),
        attribute_type: Some(attribute_type),
        component_datatype: Some(
            v.get("componentDatatype")
                .and_then(Value::as_u64)
                .and_then(|v| ComponentDatatype::from_gl(v as u32))
                .unwrap_or(ComponentDatatype::Float),
        ),
        count: v.get("count").and_then(Value::as_u64).map(|x| x as usize),
        normalized: v.get("normalized") == Some(&json!(true)),
        byte_offset: Some(v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize),
        byte_stride: Some(v.get("byteStride").and_then(Value::as_u64).unwrap_or(0) as usize),
        ..Default::default()
    };
    if let Some(constant) = v.get("constant") {
        a.constant = Some(math_value(constant_type, constant));
    }
    if let Some(min) = v.get("min") {
        a.min = Some(math_value(attribute_type, min));
    }
    if let Some(max) = v.get("max") {
        a.max = Some(math_value(attribute_type, max));
    }
    if v.get("buffer") == Some(&json!(true)) {
        a.buffer = Some(buffer.expect("fixture buffer").clone());
    }
    a.quantization = quantization;
    a
}

fn position_attribute() -> mc::Attribute<Host> {
    mc::Attribute {
        semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
        attribute_type: Some(AttributeType::Vec3),
        component_datatype: Some(ComponentDatatype::Float),
        count: Some(4),
        min: Some(mc::MathValue::Cartesian3(Cartesian3::new(-1., -1., -1.))),
        max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1., 1., 1.))),
        constant: Some(mc::MathValue::Cartesian3(Cartesian3::new(1., 2., 3.))),
        byte_offset: Some(0),
        byte_stride: Some(12),
        ..Default::default()
    }
}

fn base_primitive(extra: Vec<mc::Attribute<Host>>) -> mc::Primitive<Host> {
    let mut attributes = vec![position_attribute()];
    attributes.extend(extra);
    mc::Primitive { attributes, primitive_type: Some(PrimitiveType::Triangles), ..Default::default() }
}

fn primitive_from_descriptor(c: &Value, buffer: Option<&Buffer<Host>>) -> mc::Primitive<Host> {
    let attributes = c["attrs"].as_array().unwrap().iter().map(|a| attribute(a, buffer)).collect();
    mc::Primitive {
        attributes,
        primitive_type: Some(if c.get("primitiveType").and_then(Value::as_str) == Some("POINTS") {
            PrimitiveType::Points
        } else {
            PrimitiveType::Triangles
        }),
        indices: c
            .get("indicesCount")
            .and_then(Value::as_u64)
            .map(|count| mc::Indices { count: Some(count as usize), ..Default::default() }),
        ..Default::default()
    }
}

fn enc_components(v: &[f32]) -> Value {
    if v.len() == 1 {
        json!(v[0])
    } else {
        json!(v)
    }
}

fn enc_uniform(v: UniformValue) -> Value {
    match v {
        UniformValue::Float(v) => enc_components(&v),
        UniformValue::Int(v) if v.len() == 1 => json!(v[0]),
        UniformValue::Int(v) => json!(v),
        UniformValue::FloatArray { values, .. } => json!(values),
        UniformValue::IntArray { values, .. } => json!(values),
        UniformValue::UIntArray { values, .. } => json!(values),
        UniformValue::MatrixArray { values, .. } => json!(values),
        UniformValue::Matrix { values, .. } => json!(values),
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
        "vertex": {"defines": v.defines, "source": v.sources[0]},
        "fragment": {"defines": f.defines, "source": f.sources[0]},
        "attributeLocations": locations,
    })
}

fn uniforms(r: &PipelineResources) -> Value {
    let values = r
        .uniform_map
        .iter()
        .map(|(name, provider)| (name.clone(), enc_uniform(provider())))
        .collect::<serde_json::Map<_, _>>();
    Value::Object(values)
}

fn attribute_snapshot(a: &VertexAttribute<Host>) -> Value {
    let (value, buffer) = match &a.source {
        AttributeSource::Constant(v) => (enc_components(v), false),
        AttributeSource::Buffer(_) => (Value::Null, true),
    };
    json!({
        "index": a.index,
        "value": value,
        "buffer": buffer,
        "components": a.components,
        "datatype": a.datatype as u32,
        "offset": a.offset,
        "stride": a.stride,
        "normalize": a.normalize,
    })
}

fn snapshot(p: &PrimitiveRenderResources<Host>) -> Value {
    let mut out = shader_snapshot(p);
    out["uniforms"] = uniforms(p);
    out["attributes"] = Value::Array(p.attributes.iter().map(|a| attribute_snapshot(a)).collect());
    out["attributeIndex"] = json!(p.attribute_index);
    out["featureIdSetIndex"] = json!(p.feature_id_vertex_attribute_set_index);
    out["hasSilhouette"] = json!(p.has_silhouette);
    out
}

fn model_snapshot(r: &ModelRenderResources<Host>) -> Value {
    let mut out = shader_snapshot(r);
    out["uniforms"] = uniforms(r);
    out
}

#[test]
fn model_stages_match_cesium() {
    let context = Context::new_webgl2(Host);
    let buffer = Buffer::new(&context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(256)).unwrap();
    run(
        "scene",
        "ModelStages",
        |_| Tol { abs: 1e-6, rel: 1e-6, vector: false },
        |f, a| {
            let c = &a[0];
            match f {
                "geometry" => {
                    let state = model_state(c);
                    let model = ModelRenderResources::<Host>::new(state);
                    let mut node = NodeRenderResources::new(&model, 0);
                    node.has_instances = c.get("instanced") == Some(&json!(true));
                    let primitive = primitive_from_descriptor(c, Some(&buffer));
                    let mut p = PrimitiveRenderResources::new(&node, 0, &primitive, None, None).unwrap();
                    if c.get("projectTo2D") == Some(&json!(true)) && !node.has_instances {
                        p.position_buffer_2d = Some(buffer.clone());
                    }
                    let mode =
                        SceneMode::from_value(c.get("mode").and_then(Value::as_i64).unwrap_or(3) as i32).unwrap();
                    geometry_pipeline_stage::process(
                        &mut p,
                        &primitive,
                        mode,
                        c.get("scene3DOnly") == Some(&json!(true)),
                        c.get("webgl2") != Some(&json!(false)),
                    )
                    .unwrap();
                    Some(Out::Ok(snapshot(&p)))
                }
                "dequantization" => {
                    let state = model_state(c);
                    let model = ModelRenderResources::<Host>::new(state);
                    let node = NodeRenderResources::new(&model, 0);
                    let attributes = c["attrs"].as_array().unwrap().iter().map(|a| attribute(a, None)).collect();
                    let primitive = base_primitive(attributes);
                    let mut p = PrimitiveRenderResources::new(&node, 0, &primitive, None, None).unwrap();
                    dequantization_pipeline_stage::process(&mut p, &primitive).unwrap();
                    Some(Out::Ok(snapshot(&p)))
                }
                "lighting" => {
                    let state = model_state(c);
                    let model = ModelRenderResources::<Host>::new(state);
                    let node = NodeRenderResources::new(&model, 0);
                    let primitive = base_primitive(vec![]);
                    let mut p = PrimitiveRenderResources::new(&node, 0, &primitive, None, None).unwrap();
                    p.lighting_options.lighting_model = if c.get("lightingModel").and_then(Value::as_str) == Some("PBR")
                    {
                        LightingModel::Pbr
                    } else {
                        LightingModel::Unlit
                    };
                    lighting_pipeline_stage::process(&mut p, &primitive);
                    Some(Out::Ok(snapshot(&p)))
                }
                "vertical" => {
                    let state = model_state(c);
                    let model = ModelRenderResources::<Host>::new(state);
                    let node = NodeRenderResources::new(&model, 0);
                    let primitive = base_primitive(vec![]);
                    let mut p = PrimitiveRenderResources::new(&node, 0, &primitive, None, None).unwrap();
                    let frame = frame_state(
                        Cartesian3::ZERO,
                        0.,
                        c.get("verticalExaggeration").and_then(Value::as_f64).unwrap_or(1.),
                        c.get("verticalExaggerationRelativeHeight").and_then(Value::as_f64).unwrap_or(0.),
                    );
                    vertical_exaggeration_pipeline_stage::process(&mut p, &primitive, &frame);
                    Some(Out::Ok(snapshot(&p)))
                }
                "edge" => {
                    let state = model_state(c);
                    let model = ModelRenderResources::<Host>::new(state);
                    let node = NodeRenderResources::new(&model, 0);
                    let primitive = base_primitive(vec![]);
                    let mut p = PrimitiveRenderResources::new(&node, 0, &primitive, None, None).unwrap();
                    edge_detection_pipeline_stage::process(&mut p);
                    Some(Out::Ok(snapshot(&p)))
                }
                "atmosphere" => {
                    let state = model_state(c);
                    let mut model = ModelRenderResources::<Host>::new(state);
                    let camera_position = c
                        .get("cameraPosition")
                        .and_then(Value::as_array)
                        .map(|v| c3(&Value::Array(v.clone())))
                        .unwrap_or(Cartesian3::ZERO);
                    let frame =
                        frame_state(camera_position, c.get("fogDensity").and_then(Value::as_f64).unwrap_or(0.), 1., 0.);
                    atmosphere_pipeline_stage::process(&mut model, &frame);
                    Some(Out::Ok(model_snapshot(&model)))
                }
                "silhouette" => {
                    let state = model_state(c);
                    let model = ModelRenderResources::<Host>::new(state.clone());
                    let node = NodeRenderResources::new(&model, 0);
                    let primitive = base_primitive(vec![]);
                    let mut p = PrimitiveRenderResources::new(&node, 0, &primitive, None, None).unwrap();
                    model_silhouette_pipeline_stage::process(&mut p);
                    state.borrow_mut().silhouette_color =
                        color(&Value::Array(c["afterColor"].as_array().unwrap().clone()));
                    state.borrow_mut().silhouette_size = c["afterSize"].as_f64().unwrap();
                    let mut out = snapshot(&p);
                    out["silhouetteId"] = json!(state.borrow().silhouette_id);
                    Some(Out::Ok(out))
                }
                "classification" => {
                    let state = model_state(c);
                    let model = ModelRenderResources::<Host>::new(state);
                    let node = NodeRenderResources::new(&model, 0);
                    let count = u(&c["count"]) as usize;
                    let indices = c.get("indices").and_then(Value::as_array).map(|values| mc::Indices {
                        count: Some(values.len()),
                        typed_array: Some(TypedArray::Uint16(values.iter().map(|v| n(v) as u16).collect())),
                        ..Default::default()
                    });
                    let constructor = mc::Primitive::<Host> {
                        attributes: vec![position_attribute()],
                        primitive_type: Some(PrimitiveType::Triangles),
                        indices: indices.clone(),
                        ..Default::default()
                    };
                    let mut p = PrimitiveRenderResources::new(&node, 0, &constructor, None, None).unwrap();
                    let mut process_primitive = constructor.clone();
                    if c.get("noPosition") == Some(&json!(true)) {
                        process_primitive.attributes.clear();
                    } else {
                        process_primitive.attributes[0].count = Some(count);
                        if let Some(ids) = c.get("featureIds").and_then(Value::as_array) {
                            process_primitive.attributes.push(mc::Attribute {
                                semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId)),
                                set_index: Some(c.get("featureSetIndex").and_then(Value::as_u64).unwrap_or(0) as u32),
                                attribute_type: Some(AttributeType::Scalar),
                                component_datatype: Some(ComponentDatatype::UnsignedShort),
                                count: Some(count),
                                typed_array: Some(TypedArray::Uint16(ids.iter().map(|v| n(v) as u16).collect())),
                                ..Default::default()
                            });
                        }
                    }
                    let first = classification_pipeline_stage::process(&mut p, &process_primitive);
                    if let Err(e) = first {
                        return Some(Out::Err(e.to_string()));
                    }
                    if c.get("repeat") == Some(&json!(true)) {
                        let other = mc::Primitive::<Host> {
                            attributes: vec![position_attribute()],
                            primitive_type: Some(PrimitiveType::Triangles),
                            indices: Some(mc::Indices {
                                count: Some(2),
                                typed_array: Some(TypedArray::Uint16(vec![0, 1])),
                                ..Default::default()
                            }),
                            ..Default::default()
                        };
                        classification_pipeline_stage::process(&mut p, &other).unwrap();
                    }
                    Some(Out::Ok(json!({
                        "batchLengths": p.batch_lengths,
                        "batchOffsets": p.batch_offsets,
                        "defines": shader_snapshot(&p)["vertex"]["defines"],
                    })))
                }
                _ => None,
            }
        },
    );
}

fn next_stage_snapshot(r: &PipelineResources, textures: &ModelTextureUniformProviders<Host>) -> Value {
    let mut out = shader_snapshot(r);
    let mut values = uniforms(r);
    for name in textures.keys() {
        values[name] = Value::Null;
    }
    out["uniforms"] = values;
    out
}

#[test]
fn next_model_stages_match_cesium() {
    let context = Context::new_webgl2(Host);
    run(
        "scene",
        "ModelNextStages",
        |_| Tol { abs: 1e-6, rel: 1e-6, vector: false },
        |_, a| {
            let c = &a[0];
            let yes = |key: &str| c[key] == true;
            let state = Rc::new(RefCell::new(ModelUniformState {
                feature_id_label: "buildings".into(),
                instance_feature_id_label: "instances".into(),
                has_color: yes("color"),
                back_face_culling: !yes("noCull"),
                point_cloud_shading_normal_shading: c["normalShading"].as_bool(),
                ..Default::default()
            }));
            let mut m = ModelRenderResources::<Host>::new(state);
            let n = NodeRenderResources::new(&m, 0);
            let mut primitive = base_primitive(if yes("normals") {
                vec![mc::Attribute {
                    semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Normal)),
                    attribute_type: Some(AttributeType::Vec3),
                    constant: Some(mc::MathValue::Cartesian3(Cartesian3::UNIT_Z)),
                    ..Default::default()
                }]
            } else {
                vec![]
            });
            primitive.material = Some(mc::Material::default());
            let node = mc::Node::default();
            let mut p = PrimitiveRenderResources::new(&n, 0, &primitive, None, None).unwrap();
            match s(&c["stage"]) {
                "metadata" => metadata_picking_pipeline_stage::process(&mut p).unwrap(),
                "cpu" => cpu_styling_pipeline_stage::process(&mut p),
                "tileset" => tileset_pipeline_stage::process(&mut p, yes("skip")),
                "batch" => batch_texture_pipeline_stage::process(
                    &mut p,
                    Rc::new(RefCell::new(batch_texture_pipeline_stage::BatchTextureState {
                        features_length: 4,
                        batch_texture: None,
                        default_texture: None,
                        texture_step: Cartesian4::new(0.1, 0.2, 0.3, 0.4),
                        texture_dimensions: Cartesian2::new(4., common::n(&c["rows"])),
                    })),
                ),
                "selected" | "feature" => {
                    geometry_pipeline_stage::process(&mut p, &primitive, SceneMode::Scene3D, false, true).unwrap();
                    let null_feature_id = c["nullId"].as_u64().map(|v| v as u32);
                    primitive.feature_ids = vec![if yes("texture") {
                        mc::FeatureId::Texture(mc::FeatureIdTexture {
                            feature_count: Some(4),
                            property_table_id: Some(0),
                            label: Some("buildings".into()),
                            positional_label: Some("featureId_0".into()),
                            null_feature_id,
                            texture_reader: Some(mc::TextureReader {
                                tex_coord: 0.,
                                channels: Some("rg".into()),
                                ..Default::default()
                            }),
                        })
                    } else {
                        mc::FeatureId::Attribute(mc::FeatureIdAttribute {
                            feature_count: Some(4),
                            property_table_id: Some(0),
                            set_index: Some(0),
                            label: Some("buildings".into()),
                            positional_label: Some("featureId_0".into()),
                            null_feature_id,
                        })
                    }];
                    if s(&c["stage"]) == "selected" {
                        selected_feature_id_pipeline_stage::process(&mut p, &node, &primitive).unwrap();
                    } else {
                        feature_id_pipeline_stage::process(
                            &mut p,
                            &node,
                            &primitive,
                            &context,
                            &mut ModelStatistics::default(),
                            &mut vec![],
                        )
                        .unwrap();
                    }
                }
                "material" => {
                    let mat = primitive.material.as_mut().unwrap();
                    mat.unlit = yes("unlit");
                    mat.double_sided = yes("doubleSided");
                    mat.alpha_mode = match c["alphaMode"].as_str() {
                        Some("BLEND") => AlphaMode::Blend,
                        Some("MASK") => AlphaMode::Mask,
                        _ => AlphaMode::Opaque,
                    };
                    mat.alpha_cutoff = Some(0.3);
                    mat.point_diameter = c["diameter"].as_f64();
                    mat.metallic_roughness.metallic_factor = 0.4;
                    mat.metallic_roughness.roughness_factor = 0.7;
                    mat.metallic_roughness.base_color_factor = Cartesian4::new(0.2, 0.3, 0.4, 0.5);
                    if yes("specular") {
                        mat.specular = Some(mc::Specular { specular_factor: 0.5, ..Default::default() });
                    }
                    if yes("anisotropy") {
                        mat.anisotropy = Some(mc::Anisotropy {
                            anisotropy_strength: 0.6,
                            anisotropy_rotation: 0.7,
                            ..Default::default()
                        });
                    }
                    if yes("clearcoat") {
                        mat.clearcoat = Some(mc::Clearcoat {
                            clearcoat_factor: 0.8,
                            clearcoat_roughness_factor: 0.9,
                            ..Default::default()
                        });
                    }
                    material_pipeline_stage::process(&mut p, &primitive, &context, 2.);
                }
                "planes" => {
                    let mut planes = ClippingPlaneCollection::default();
                    planes.add(ClippingPlane::handle(Cartesian3::UNIT_X, 0.));
                    planes.edge_color = Color::new(0.2, 0.3, 0.4, 0.5);
                    planes.edge_width = 3.;
                    planes.union_clipping_regions = yes("union");
                    model_clipping_planes_pipeline_stage::process(
                        &mut m,
                        &model_clipping_planes_pipeline_stage::ModelClippingPlanesState {
                            clipping_planes: Rc::new(RefCell::new(planes)),
                            clipping_planes_matrix: Rc::new(RefCell::new(Matrix4::IDENTITY)),
                            use_float_texture: yes("float"),
                            max_texture_size: 4096,
                            default_texture: None,
                        },
                    );
                    return Some(Out::Ok(next_stage_snapshot(&m, &m.texture_uniform_map)));
                }
                "polygons" => {
                    model_clipping_polygons_pipeline_stage::process(
                        &mut m,
                        &model_clipping_polygons_pipeline_stage::ModelClippingPolygonsState {
                            inverse: yes("inverse"),
                            compute_polygon_bounds_length: 2,
                            clipping_texture: Rc::new(RefCell::new(None)),
                            extents_texture: Rc::new(RefCell::new(None)),
                            default_texture: None,
                        },
                    );
                    return Some(Out::Ok(next_stage_snapshot(&m, &m.texture_uniform_map)));
                }
                other => panic!("unknown stage {other}"),
            }
            Some(Out::Ok(next_stage_snapshot(&p, &p.texture_uniform_map)))
        },
    );
}
