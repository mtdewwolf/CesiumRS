//! `Scene/Model/CustomShader`, `TextureUniform`, `TextureManager` and
//! `CustomShaderPipelineStage` parity: the pinned reference cases plus native
//! checks for the debug-only contracts that release Cesium strips, and for the
//! GPU half of the texture manager.

mod common;
use cesium_rs::scene::model::CustomShaderUniformValue as UniformValue;
use cesium_rs::{
    core::image_helpers::ImageFuture,
    core::*,
    renderer::*,
    scene::{model::*, model_components as mc, *},
};
use common::*;
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Default)]
struct Host {
    calls: Rc<RefCell<Vec<String>>>,
}

impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.calls.borrow_mut().push(name.into());
        if name.starts_with("create") {
            return Ok(GlValue::Object(1));
        }
        Ok(GlValue::Null)
    }
}

impl ImageHost for Host {
    type Image = RgbaImage;
    fn dimensions(&self, image: &RgbaImage) -> (u32, u32) {
        (image.width, image.height)
    }
    fn image_from_rgba(&self, image: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(image)
    }
    fn image_pixels(&self, image: &RgbaImage, width: u32, height: u32) -> Result<Vec<u8>, RuntimeError> {
        if (width, height) == (image.width, image.height) {
            return Ok(image.pixels.clone());
        }
        // Nearest-neighbour resampling keeps the test independent of any
        // host filtering convention; only the dimensions are asserted.
        let mut out = vec![0; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let sx = x * image.width / width;
                let sy = y * image.height / height;
                let source = ((sy * image.width + sx) * 4) as usize;
                let target = ((y * width + x) * 4) as usize;
                out[target..target + 4].copy_from_slice(&image.pixels[source..source + 4]);
            }
        }
        Ok(out)
    }
    fn resize_image(&self, image: &RgbaImage, width: u32, height: u32) -> Result<RgbaImage, RuntimeError> {
        Ok(RgbaImage { width, height, pixels: self.image_pixels(image, width, height)? })
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        None
    }
}

fn uniform_value(v: &Value) -> UniformValue {
    let kind = v["kind"].as_str().expect("uniform kind");
    let value = &v["value"];
    match kind {
        "bool" => UniformValue::Bool(value.as_bool().expect("bool")),
        "number" => UniformValue::Number(n(value)),
        "vec2" => UniformValue::Vec2(c2(value)),
        "vec3" => UniformValue::Vec3(c3(value)),
        "vec4" => UniformValue::Vec4(c4(value)),
        "mat2" => UniformValue::Mat2(m2(value)),
        "mat3" => UniformValue::Mat3(m3(value)),
        "mat4" => UniformValue::Mat4(m4(value)),
        "sampler" => {
            let pixels = value.as_array().expect("sampler pixels").iter().map(|v| n(v) as u8).collect();
            UniformValue::Sampler2D(
                TextureUniform::new(TextureUniformOptions {
                    typed_array: Some(pixels),
                    width: Some(v.get("width").and_then(Value::as_u64).unwrap_or(1) as u32),
                    height: Some(v.get("height").and_then(Value::as_u64).unwrap_or(1) as u32),
                    ..Default::default()
                })
                .unwrap(),
            )
        }
        other => panic!("unknown uniform kind {other}"),
    }
}

fn options(o: &Value) -> CustomShaderOptions {
    let mut options = CustomShaderOptions::default();
    if let Some(v) = o.get("mode").and_then(Value::as_str) {
        options.mode = CustomShaderMode::from_str_value(v).unwrap();
    }
    if let Some(v) = o.get("lightingModel").and_then(Value::as_i64) {
        options.lighting_model = Some(LightingModel::from_value(v as i32).unwrap());
    }
    if let Some(v) = o.get("translucencyMode").and_then(Value::as_i64) {
        options.translucency_mode = CustomShaderTranslucencyMode::from_value(v as i32).unwrap();
    }
    if let Some(u) = o.get("uniforms").and_then(Value::as_array) {
        for item in u {
            let name = item["name"].as_str().unwrap().to_string();
            let uniform_type = UniformType::from_str_value(item["type"].as_str().unwrap()).unwrap();
            options.uniforms.insert(name, (uniform_type, uniform_value(item)));
        }
    }
    if let Some(v) = o.get("varyings").and_then(Value::as_array) {
        for item in v {
            options.varyings.insert(
                item["name"].as_str().unwrap().to_string(),
                VaryingType::from_str_value(item["type"].as_str().unwrap()).unwrap(),
            );
        }
    }
    if let Some(v) = o.get("vertexShaderText").and_then(Value::as_str) {
        options.vertex_shader_text = Some(v.to_string());
    }
    if let Some(v) = o.get("fragmentShaderText").and_then(Value::as_str) {
        options.fragment_shader_text = Some(v.to_string());
    }
    options
}

/// The raw uniform value as JS exposes it through `uniformMap`.
fn enc_value(v: &UniformValue) -> Value {
    match v {
        UniformValue::Bool(v) => json!(v),
        UniformValue::Number(v) => json!(v),
        UniformValue::Vec2(v) => json!([v.x, v.y]),
        UniformValue::Vec3(v) => json!([v.x, v.y, v.z]),
        UniformValue::Vec4(v) => json!([v.x, v.y, v.z, v.w]),
        UniformValue::Mat2(v) => json!(v.0),
        UniformValue::Mat3(v) => json!(v.0),
        UniformValue::Mat4(v) => json!(v.0),
        UniformValue::Sampler2D(_) => json!("sampler"),
    }
}

fn shader_snapshot(cs: &CustomShader<Host>) -> Value {
    let names: Vec<String> = cs.uniforms.keys().cloned().collect();
    let uniforms = names
        .iter()
        .map(|name| {
            let spec = &cs.uniforms[name];
            json!({
                "name": name,
                "type": spec.uniform_type.as_str(),
                "value": enc_value(&spec.value.borrow()),
            })
        })
        .collect::<Vec<_>>();
    let values = names
        .iter()
        .map(|name| {
            if cs.uniforms[name].uniform_type == UniformType::Sampler2D {
                match cs.texture_manager.get_texture(name) {
                    Some(_) => json!("texture"),
                    None => Value::Null,
                }
            } else {
                enc_value(&cs.uniform_value(name).expect("declared uniform"))
            }
        })
        .collect::<Vec<_>>();
    let varyings = cs.varyings.iter().map(|(name, ty)| json!([name, ty.as_str()])).collect::<Vec<_>>();
    json!({
        "mode": cs.mode.as_str(),
        "lightingModel": cs.lighting_model.map(LightingModel::value),
        "translucencyMode": cs.translucency_mode.value(),
        "uniforms": uniforms,
        "uniformMapKeys": names,
        "uniformMapValues": values,
        "varyings": varyings,
        "vertex": {
            "attributeSet": cs.used_variables_vertex.attribute_set.sorted(),
            "featureIdSet": cs.used_variables_vertex.feature_id_set.sorted(),
            "metadataSet": cs.used_variables_vertex.metadata_set.sorted(),
        },
        "fragment": {
            "attributeSet": cs.used_variables_fragment.attribute_set.sorted(),
            "featureIdSet": cs.used_variables_fragment.feature_id_set.sorted(),
            "metadataSet": cs.used_variables_fragment.metadata_set.sorted(),
            "materialSet": cs.used_variables_fragment.material_set.sorted(),
        },
    })
}

fn texture_uniform(o: &Value) -> Value {
    let mut options = TextureUniformOptions::default();
    if let Some(v) = o.get("typedArray").and_then(Value::as_array) {
        options.typed_array = Some(v.iter().map(|v| n(v) as u8).collect());
    }
    options.width = o.get("width").and_then(Value::as_u64).map(|v| v as u32);
    options.height = o.get("height").and_then(Value::as_u64).map(|v| v as u32);
    options.url = o.get("url").and_then(Value::as_str).map(str::to_string);
    options.repeat = o.get("repeat").and_then(Value::as_bool);
    options.pixel_format = o.get("pixelFormat").and_then(Value::as_u64).and_then(|v| PixelFormat::from_value(v as u32));
    options.pixel_datatype =
        o.get("pixelDatatype").and_then(Value::as_u64).and_then(|v| PixelDatatype::from_value(v as u32));
    options.minification_filter =
        o.get("minificationFilter").and_then(Value::as_u64).and_then(|v| TextureMinificationFilter::from_gl(v as u32));
    options.magnification_filter = o
        .get("magnificationFilter")
        .and_then(Value::as_u64)
        .and_then(|v| TextureMagnificationFilter::from_gl(v as u32));
    options.maximum_anisotropy = o.get("maximumAnisotropy").and_then(Value::as_f64);
    let t = TextureUniform::new(options).unwrap();
    json!({
        "typedArray": t.typed_array,
        "width": t.width,
        "height": t.height,
        "pixelFormat": t.pixel_format as u32,
        "pixelDatatype": t.pixel_datatype as u32,
        "wrap": [
            t.sampler.wrap_s == TextureWrap::Repeat,
            t.sampler.wrap_t == TextureWrap::Repeat,
        ],
        "min": t.sampler.minification_filter as u32,
        "mag": t.sampler.magnification_filter as u32,
        "anisotropy": t.sampler.maximum_anisotropy,
        "hasResource": t.resource.is_some(),
    })
}

fn stage_attribute(v: &Value) -> mc::Attribute<Host> {
    let attribute_type = AttributeType::from_str_value(v["type"].as_str().unwrap()).unwrap();
    let position = v.get("semantic").and_then(Value::as_str) == Some("POSITION");
    mc::Attribute {
        name: v.get("name").and_then(Value::as_str).map(str::to_string),
        semantic: v
            .get("semantic")
            .and_then(Value::as_str)
            .and_then(VertexAttributeSemantic::from_gltf_semantic)
            .map(mc::AttributeSemantic::Vertex),
        set_index: v.get("setIndex").and_then(Value::as_u64).map(|v| v as u32),
        attribute_type: Some(attribute_type),
        component_datatype: Some(ComponentDatatype::Float),
        count: Some(if position { 3 } else { 1 }),
        min: position.then(|| mc::MathValue::Cartesian3(Cartesian3::new(-1., -1., -1.))),
        max: position.then(|| mc::MathValue::Cartesian3(Cartesian3::new(1., 1., 1.))),
        ..Default::default()
    }
}

fn stage_resources(shader: Option<Rc<CustomShader<Host>>>) -> PrimitiveRenderResources<Host> {
    let model = Rc::new(RefCell::new(ModelUniformState::default()));
    let model_resources = ModelRenderResources::<Host>::new(model);
    PrimitiveRenderResources {
        pipeline: model_resources.pipeline,
        texture_uniform_map: BTreeMap::new(),
        custom_shader: shader,
        runtime_node: 0,
        runtime_primitive: 0,
        attributes: vec![],
        attribute_index: 0,
        feature_id_vertex_attribute_set_index: 0,
        instance_count: 0,
        has_instances: false,
        count: None,
        has_property_table: false,
        indices: None,
        position_buffer_2d: None,
        wireframe_index_buffer: None,
        batch_lengths: None,
        batch_offsets: None,
        primitive_type: None,
        position_min: None,
        position_max: None,
        bounding_sphere: BoundingSphere::default(),
        lighting_options: ModelLightingOptions::default(),
        pick_id: None,
        pick_ids: Vec::new(),
    }
}

fn stage_snapshot(r: &PrimitiveRenderResources<Host>, shader: Option<&CustomShader<Host>>) -> Value {
    let (v, f) = r.pipeline.shader_builder.sources();
    let keys: Vec<String> = r.pipeline.uniform_map.keys().cloned().collect();
    let values = keys
        .iter()
        .map(|name| {
            if let Some(shader) = shader {
                if let Some(spec) = shader.uniforms.get(name) {
                    if spec.uniform_type == UniformType::Sampler2D {
                        return json!("sampler");
                    }
                    return enc_value(&shader.uniform_value(name).expect("declared uniform"));
                }
            }
            Value::Null
        })
        .collect::<Vec<_>>();
    json!({
        "vertexDefines": v.defines,
        "fragmentDefines": f.defines,
        "vertexSource": v.sources[0],
        "fragmentSource": f.sources[0],
        "pass": r.pipeline.alpha_options.borrow().pass.map(|p| p as u32),
        "lightingModel": r.lighting_options.lighting_model.value(),
        "uniformKeys": keys,
        "uniformValues": values,
    })
}

#[test]
fn custom_shader_reference() {
    run(
        "scene",
        "ModelCustomShader",
        |_| EXACT,
        |f, a| match f {
            "construct" | "constructAll" | "vertexOnly" | "fragmentOnly" | "usedVariables" => {
                let shader = CustomShader::<Host>::new(options(&a[0])).unwrap();
                Some(Out::Ok(shader_snapshot(&shader)))
            }
            "setUniform" => {
                let shader = CustomShader::<Host>::new(options(&a[0])).unwrap();
                let steps = a[0]["ops"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|op| {
                        let name = op["name"].as_str().unwrap();
                        match shader.set_uniform(name, uniform_value(op)) {
                            Ok(()) => json!({"value": shader_snapshot(&shader)}),
                            Err(_) => json!({"throws": true}),
                        }
                    })
                    .collect::<Vec<_>>();
                Some(Out::Ok(Value::Array(steps)))
            }
            "textureUniform" => Some(Out::Ok(texture_uniform(&a[0]))),
            "stage" => {
                let c = &a[0];
                let shader = c.get("options").and_then(|o| {
                    if o.is_null() {
                        None
                    } else {
                        Some(Rc::new(CustomShader::<Host>::new(options(o)).unwrap()))
                    }
                });
                let mut resources = stage_resources(shader.clone());
                if let Some(pass) = c.get("initialPass").and_then(Value::as_u64) {
                    resources.pipeline.alpha_options.borrow_mut().pass = Some(match pass {
                        9 => Pass::Translucent,
                        other => panic!("unexpected initial pass {other}"),
                    });
                }
                let primitive = mc::Primitive {
                    attributes: c["attributes"].as_array().unwrap().iter().map(stage_attribute).collect(),
                    ..Default::default()
                };
                custom_shader_pipeline_stage::process(
                    &mut resources,
                    &primitive,
                    None,
                    &mut CustomShaderWarnings::default(),
                )
                .unwrap();
                Some(Out::Ok(stage_snapshot(&resources, shader.as_deref())))
            }
            "stageMetadataIncompatible" => {
                let c = &a[0];
                let shader = Rc::new(CustomShader::<Host>::new(options(&c["options"])).unwrap());
                let mut resources = stage_resources(Some(shader.clone()));
                let primitive = mc::Primitive {
                    attributes: vec![stage_attribute(&json!({"semantic": "POSITION", "type": "VEC3"}))],
                    ..Default::default()
                };
                custom_shader_pipeline_stage::process(
                    &mut resources,
                    &primitive,
                    None,
                    &mut CustomShaderWarnings::default(),
                )
                .unwrap();
                let (v, f) = resources.pipeline.shader_builder.sources();
                Some(Out::Ok(json!({
                    "pass": resources.pipeline.alpha_options.borrow().pass.map(|p| p as u32),
                    "vertexDefines": v.defines,
                    "fragmentDefines": f.defines,
                    "vertexSource": v.sources[0],
                    "fragmentSource": f.sources[0],
                })))
            }
            _ => None,
        },
    );
}

/// `run` skips `DeveloperError` cases because release Cesium strips the debug
/// checks. The port keeps them as explicit preconditions, so compare every
/// message against the pinned release text.
#[test]
fn invalid_builtin_variables_match_cesium_messages() {
    let cases = load("scene", "ModelCustomShader");
    let mut checked = 0;
    for c in cases.iter().filter(|c| c.f == "invalidVariable" || c.f == "invalidVariableName") {
        let expected = c.r["throws"].as_str().expect("pinned message");
        let opts = if c.f == "invalidVariable" { &c.a[0] } else { &c.a[0]["options"] };
        let error = match CustomShader::<Host>::new(options(opts)) {
            Ok(_) => panic!("{} should be rejected", c.f),
            Err(error) => error.message,
        };
        assert_eq!(error, expected, "{} case {}", c.f, checked);
        checked += 1;
    }
    assert_eq!(checked, 24);
}

#[test]
fn custom_shader_debug_preconditions() {
    // samplerCube is a debug-only rejection upstream; the port always rejects.
    let cube = TextureUniform::new(TextureUniformOptions {
        typed_array: Some(vec![0, 0, 0, 0]),
        width: Some(1),
        height: Some(1),
        ..Default::default()
    })
    .unwrap();
    let mut options = CustomShaderOptions::default();
    options.uniforms.insert("u_cube".into(), (UniformType::SamplerCube, UniformValue::Sampler2D(cube)));
    assert_eq!(
        CustomShader::<Host>::new(options).unwrap_err().message,
        "CustomShader does not support samplerCube uniforms"
    );

    // A sampler2D declaration needs a TextureUniform value.
    let mut options = CustomShaderOptions::default();
    options.uniforms.insert("u_tex".into(), (UniformType::Sampler2D, UniformValue::Number(1.)));
    assert_eq!(
        CustomShader::<Host>::new(options).unwrap_err().message,
        "A sampler2D uniform must have a TextureUniform value"
    );

    // setUniform validates the declared type both ways.
    let mut options = CustomShaderOptions::default();
    options.uniforms.insert("u_float".into(), (UniformType::Float, UniformValue::Number(1.)));
    let shader = CustomShader::<Host>::new(options).unwrap();
    assert_eq!(
        shader.set_uniform("u_missing", UniformValue::Number(1.)).unwrap_err().message,
        "Uniform u_missing must be declared in the CustomShader constructor."
    );
    let texture = TextureUniform::new(TextureUniformOptions {
        typed_array: Some(vec![0, 0, 0, 0]),
        width: Some(1),
        height: Some(1),
        ..Default::default()
    })
    .unwrap();
    assert!(shader.set_uniform("u_float", UniformValue::Sampler2D(texture)).is_err());

    // TextureUniform requires exactly one source and a size for raw pixels.
    assert_eq!(
        TextureUniform::new(TextureUniformOptions::default()).unwrap_err().message,
        "exactly one of options.typedArray, options.url must be defined"
    );
    assert_eq!(
        TextureUniform::new(TextureUniformOptions { typed_array: Some(vec![1, 2, 3, 4]), ..Default::default() })
            .unwrap_err()
            .message,
        "options.width and options.height are required when options.typedArray is defined"
    );
}

#[test]
fn custom_shader_accepts_satisfied_metadata_requests() {
    use cesium_rs::scene::{
        structural_metadata::StructuralMetadataOptions, MetadataSchema, PropertyAttribute, StructuralMetadata,
    };
    use std::sync::Arc;

    let schema = Arc::new(
        MetadataSchema::from_json(&json!({
            "classes": {"thing": {"properties": {"height": {"type": "SCALAR", "componentType": "FLOAT32"}}}}
        }))
        .unwrap(),
    );
    let class = schema.classes.get("thing").unwrap().clone();
    let property_attribute =
        PropertyAttribute::new(&json!({"properties": {"height": {"attribute": "_HEIGHT"}}}), class, None, Some(0))
            .unwrap();
    let metadata = StructuralMetadata::<Host>::new(StructuralMetadataOptions {
        schema,
        property_attributes: vec![property_attribute],
        ..Default::default()
    });

    let options = CustomShaderOptions {
        fragment_shader_text: Some(
            "void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = vec3(fsInput.metadata.height);\n}"
                .into(),
        ),
        ..Default::default()
    };
    let shader = Rc::new(CustomShader::<Host>::new(options).unwrap());
    let attributes = json!([{"semantic": "POSITION", "type": "VEC3"}]);
    let mut resources = stage_resources(Some(shader.clone()));
    let primitive = mc::Primitive {
        attributes: vec![stage_attribute(&attributes[0])],
        property_attribute_ids: vec![0],
        ..Default::default()
    };
    let mut warnings = CustomShaderWarnings::default();
    let emitted =
        custom_shader_pipeline_stage::process(&mut resources, &primitive, Some(&metadata), &mut warnings).unwrap();
    assert!(emitted.is_empty());
    let (_, fragment) = resources.pipeline.shader_builder.sources();
    assert!(fragment.defines.contains(&"HAS_CUSTOM_FRAGMENT_SHADER".to_string()));
    assert!(fragment.sources[0].contains("void initializeInputStruct(out FragmentInput fsInput"));
}

#[test]
fn texture_manager_upload_refresh_and_destroy() {
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let manager = TextureManager::<Host>::new();
    let uniform = TextureUniform::new(TextureUniformOptions {
        typed_array: Some(vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255]),
        width: Some(2),
        height: Some(2),
        ..Default::default()
    })
    .unwrap();

    manager.load_texture_2d("u_tex", &uniform);
    assert_eq!(manager.pending_len(), 1);
    assert!(manager.get_texture("u_tex").is_none());
    let warnings = manager.update(&context, 1, None, &host).unwrap();
    assert!(warnings.is_empty());
    let texture = manager.get_texture("u_tex").expect("uploaded texture");
    assert_eq!(texture.dimensions(), [2, 2]);
    assert_eq!(manager.pending_len(), 0);

    // A second update in the same frame is a no-op, as upstream's
    // `lastUpdatedFrame` guard is.
    manager.load_texture_2d("u_tex_2", &uniform);
    assert!(manager.update(&context, 1, None, &host).unwrap().is_empty());
    assert_eq!(manager.pending_len(), 1);
    manager.update(&context, 2, None, &host).unwrap();
    assert_eq!(manager.pending_len(), 0);
    assert!(manager.get_texture("u_tex_2").is_some());

    let uploaded = manager.get_texture("u_tex").unwrap();
    manager.destroy().unwrap();
    assert!(uploaded.is_destroyed());
    assert!(manager.get_texture("u_tex").is_none());
}

#[test]
fn texture_manager_mipmaps_and_webgl1_resize() {
    let host = Host::default();
    let host2 = Host::default();
    let context = Context::new_webgl2(host.clone());
    let mipmap_sampler = TextureUniform::new(TextureUniformOptions {
        typed_array: Some(vec![0; 4 * 4 * 4]),
        width: Some(4),
        height: Some(4),
        minification_filter: Some(TextureMinificationFilter::LinearMipmapLinear),
        ..Default::default()
    })
    .unwrap();
    let manager = TextureManager::<Host>::new();
    manager.load_texture_2d("mipmapped", &mipmap_sampler);
    manager.update(&context, 1, None, &host).unwrap();
    assert!(host.calls.borrow().iter().any(|call| call == "generateMipmap"));

    // WebGL1 has no non-power-of-two repeat or mipmapped textures. The 3x3
    // source is resized through the host, matching the glTF texture loader.
    let webgl1 = Context::new_webgl1(host2.clone());
    let uniform = TextureUniform::new(TextureUniformOptions {
        typed_array: Some(vec![128; 3 * 3 * 4]),
        width: Some(3),
        height: Some(3),
        ..Default::default()
    })
    .unwrap();
    let manager = TextureManager::<Host>::new();
    manager.load_texture_2d("resized", &uniform);
    let warnings = manager.update(&webgl1, 1, None, &host2).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(manager.get_texture("resized").unwrap().dimensions(), [4, 4]);
}

#[test]
fn texture_manager_failure_falls_back_to_default_texture() {
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let default = Rc::new(Texture::new(&context, TextureOptions::rgba(1, 1), None).unwrap());
    let manager = TextureManager::<Host>::new();
    manager.update(&context, 1, Some(default.clone()), &host).unwrap();
    manager.load_texture_2d_failed("u_missing");
    assert!(manager.failed("u_missing"));
    assert!(manager.get_texture("u_missing").is_none());
    let fallback = manager.get_texture_or_default("u_missing").expect("default texture");
    assert!(Rc::ptr_eq(&fallback, &default));
}
