mod common;
use cesium_rs::{
    core::*,
    renderer::*,
    scene::{metadata_value::MetadataValue, model::*, *},
};
use common::*;
use serde_json::{json, Value};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Default)]
struct Host {
    next: Cell<u32>,
    uploads: RefCell<Vec<Vec<u8>>>,
    fail_upload: Cell<bool>,
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        if name == "texImage2D" || name == "texSubImage2D" {
            if self.fail_upload.get() {
                return Err(RuntimeError::new("injected texture upload failure"));
            }
            for a in args {
                if let GlValue::Pixels { bytes, .. } = a {
                    self.uploads.borrow_mut().push(bytes.clone());
                }
            }
        }
        Ok(if name == "getParameter" { GlValue::Number(16384.) } else { GlValue::Null })
    }
}
fn value(v: Option<MetadataValue>) -> Value {
    match v {
        None | Some(MetadataValue::Null) => Value::Null,
        Some(MetadataValue::Number(v)) => num(v),
        Some(MetadataValue::String(v)) => json!(v),
        Some(MetadataValue::Boolean(v)) => json!(v),
        Some(MetadataValue::Object(v)) => v,
        Some(MetadataValue::Array(v)) => v.into_iter().map(|v| value(Some(v))).collect(),
        other => panic!("unexpected value {other:?}"),
    }
}
fn table() -> PropertyTable {
    PropertyTable::new(PropertyTableOptions {
        count: 3,
        json_metadata_table: Some(
            JsonMetadataTable::new(3, &json!({"height":[10,20,30],"name":["a","b","c"]})).unwrap(),
        ),
        ..Default::default()
    })
}
fn snapshot(table: &ModelFeatureTable<Host>, owner: &Rc<dyn Any>) -> Value {
    let features:Vec<_>=(0..3).map(|i|{let f=table.get_feature(i).unwrap();json!({"id":f.feature_id(),"show":f.show().unwrap(),"color":f.color().unwrap().enc(),"ids":f.get_property_ids().unwrap(),"height":value(f.get_property_inherited("height").unwrap()),"name":value(f.get_property("name").unwrap()),"added":value(f.get_property("added").unwrap()),"same":Rc::ptr_eq(&f,&table.get_feature(i).unwrap()),"owner":Rc::ptr_eq(&f.primitive().unwrap(),owner)})}).collect();
    let batch = table.batch_texture();
    json!({"features":features,"translucent":batch.translucent_features_length(),"commands":table.style_commands_needed() as i32,"dirty":table.style_commands_needed_dirty(),"values":batch.batch_values(),"dimensions":batch.texture_dimensions().map(|v|v.enc()),"step":batch.texture_step().map(|v|v.enc())})
}
#[test]
fn model_features_match_cesium() {
    run(
        "scene",
        "ModelFeatures",
        |_| TIGHT,
        |_, a| {
            let owner: Rc<dyn Any> = Rc::new("model".to_string());
            let table = ModelFeatureTable::<Host>::new(table(), Some(Rc::downgrade(&owner)), 2).unwrap();
            let context = Context::new_webgl2(Host::default());
            let mut out = vec![snapshot(&table, &owner)];
            for op in a[0]["ops"].as_array().unwrap() {
                let id = op["id"].as_u64().unwrap_or(0) as usize;
                let feature = table.get_feature(id).unwrap();
                match s(&op["kind"]) {
                    "show" => feature.set_show(op["value"].as_bool().unwrap()).unwrap(),
                    "color" => feature.set_color(color(&op["value"])).unwrap(),
                    "allShow" => table.set_all_show(op["value"].as_bool().unwrap()).unwrap(),
                    "allColor" => table.set_all_color(color(&op["value"])).unwrap(),
                    "property" => feature.set_property(s(&op["name"]), MetadataValue::from_json(&op["value"])).unwrap(),
                    "update" => table.update(&context, false).unwrap(),
                    "reset" => table.apply_style(None).unwrap(),
                    "style" => table
                        .apply_style(Some(&ModelFeatureStyle {
                            color: Some(&|f| {
                                Ok(Some(if f.get_property("height")?.unwrap().to_number() >= 20. {
                                    Color::new(0.2, 0.4, 0.6, 0.5)
                                } else {
                                    Color::WHITE
                                }))
                            }),
                            show: Some(&|f| Ok(Some(f.get_property("height")?.unwrap().to_number() != 20.))),
                        }))
                        .unwrap(),
                    other => panic!("{other}"),
                }
                out.push(snapshot(&table, &owner));
            }
            Some(Out::Ok(json!(out)))
        },
    );
}
#[test]
fn feature_texture_uploads_pick_identity_and_lifetimes() {
    let context = Context::new_webgl2(Host::default());
    let table = ModelFeatureTable::<Host>::new(table(), None, 2).unwrap();
    let feature = table.get_feature(1).unwrap();
    table.update(&context, false).unwrap();
    assert_eq!(table.batch_texture_byte_length(), 0);
    assert!(table.batch_texture().batch_values().is_none());
    feature.set_color(Color::new(1., 0., 0., 0.5)).unwrap();
    feature.set_show(false).unwrap();
    table.update(&context, true).unwrap();
    let pick = table.batch_texture().get_pick_color(1).unwrap().unwrap().color.to_rgba();
    assert!(Rc::ptr_eq(&context.pick_object::<ModelFeature<Host>>(pick).unwrap(), &feature));
    assert_eq!(table.batch_texture_byte_length(), 32);
    assert_eq!(context.host().uploads.borrow().last().unwrap()[7], 0);
    assert_eq!(table.style_commands_needed(), StyleCommandsNeeded::OpaqueAndTranslucent);
    assert!(table.style_commands_needed_dirty());
    feature.set_show(true).unwrap();
    table.update(&context, false).unwrap();
    assert!(!table.style_commands_needed_dirty());
    assert_eq!(context.host().uploads.borrow().last().unwrap()[7], 128);
    table.destroy().unwrap();
    assert!(context.pick_object::<ModelFeature<Host>>(pick).is_none());
    assert!(feature.show().is_err());
    assert!(table.update(&context, true).is_err());
    drop(table);
    assert!(feature.feature_table().is_none());
}
#[test]
fn empty_tables_and_upload_failures_are_recoverable() {
    let context = Context::new_webgl2(Host::default());
    let table = ModelFeatureTable::<Host>::new(PropertyTable::new(PropertyTableOptions::default()), None, 2).unwrap();
    table.update(&context, true).unwrap();
    assert_eq!(table.features_length(), 0);
    assert_eq!(table.batch_texture_byte_length(), 0);
    assert!(table.get_feature(0).is_err());
    table.apply_style(None).unwrap();
    table.destroy().unwrap();
    let table = ModelFeatureTable::<Host>::new(self::table(), None, 2).unwrap();
    table.update(&context, false).unwrap();
    table.set_color(0, Color::RED).unwrap();
    context.host().fail_upload.set(true);
    assert!(table.update(&context, true).is_err());
    assert_eq!(table.batch_texture_byte_length(), 0);
    for id in 1..=3 {
        assert!(context.pick_object::<ModelFeature<Host>>(id).is_none());
    }
    context.host().fail_upload.set(false);
    table.update(&context, true).unwrap();
    assert_eq!(table.batch_texture_byte_length(), 32);
}

fn stage_uniform(v: UniformValue) -> Value {
    match v {
        UniformValue::Float(v) => {
            if v.len() == 1 {
                json!(v[0])
            } else {
                json!(v)
            }
        }
        UniformValue::Int(v) => {
            if v.len() == 1 {
                json!(v[0])
            } else {
                json!(v)
            }
        }
        UniformValue::UIntArray { values, .. } => {
            if values.len() == 1 {
                json!(values[0])
            } else {
                json!(values)
            }
        }
        UniformValue::Matrix { values, .. } => json!(values),
        other => panic!("unexpected uniform {other:?}"),
    }
}
fn metadata_resources(c: &Value) -> PrimitiveRenderResources<Host> {
    use cesium_rs::scene::{
        metadata_table_property::ByteView,
        metadata_value::JsMap,
        parse_structural_metadata::{parse_structural_metadata, ParseStructuralMetadataOptions},
        structural_metadata::StructuralMetadataOptions,
    };
    use std::{collections::HashMap, sync::Arc};
    let context = Context::new_webgl2(Host::default());
    let schema = Arc::new(MetadataSchema::from_json(&c["schema"]).unwrap());
    let bytes = TypedArray::Float32(vec![10., 20.]).to_le_bytes();
    let mut views = JsMap::new();
    views.insert("0", ByteView { buffer: &bytes, byte_offset: 0 });
    let texture = Rc::new(Texture::new(&context, TextureOptions::rgba(1, 1), None).unwrap());
    let textures = HashMap::from([("0".into(), texture.clone()), ("1".into(), texture.clone())]);
    let metadata = if c["kind"] == "table" {
        let class = schema.classes.get("sensor").unwrap().clone();
        let table =
            MetadataTable::new(2, Some(&c["extension"]["propertyTables"][0]["properties"]), class, &views).unwrap();
        StructuralMetadata::<Host>::new(StructuralMetadataOptions {
            schema: schema.clone(),
            property_tables: Some(vec![PropertyTable::new(PropertyTableOptions {
                count: 2,
                id: Some(0usize.into()),
                metadata_table: Some(table),
                texture: if c["noTexture"] == true { None } else { Some(FrameResource::new(texture.clone())) },
                ..Default::default()
            })]),
            ..Default::default()
        })
    } else {
        parse_structural_metadata(
            &ParseStructuralMetadataOptions {
                extension: &c["extension"],
                schema: schema.clone(),
                buffer_views: &views,
                textures: &textures,
                context: None,
            },
            &mut vec![],
        )
        .unwrap()
    };
    let default_statistics = json!({"classes":{"sensor":{"properties":{"temperature":{"min":-10,"max":50,"mean":20,"median":25,"standardDeviation":2,"variance":4,"sum":100},"tint":{"min":[0,0,0],"max":[1,1,1],"mean":[0.2,0.3,0.4]}}}}});
    let statistics = c.get("statistics").unwrap_or(&default_statistics);
    let model = ModelRenderResources::<Host>::new(Rc::new(RefCell::new(ModelUniformState::default())));
    let node_resources = NodeRenderResources::new(&model, 0);
    let mut primitive = cesium_rs::scene::model_components::Primitive {
        attributes: vec![cesium_rs::scene::model_components::Attribute {
            semantic: Some(cesium_rs::scene::model_components::AttributeSemantic::Vertex(
                VertexAttributeSemantic::Position,
            )),
            attribute_type: Some(AttributeType::Vec3),
            count: Some(4),
            min: Some(cesium_rs::scene::model_components::MathValue::Cartesian3(Cartesian3::ZERO)),
            max: Some(cesium_rs::scene::model_components::MathValue::Cartesian3(Cartesian3::ONE)),
            ..Default::default()
        }],
        ..Default::default()
    };
    for a in c["attrs"].as_array().unwrap() {
        primitive.attributes.push(cesium_rs::scene::model_components::Attribute {
            name: Some(s(&a["name"]).into()),
            attribute_type: AttributeType::from_str_value(s(&a["type"])),
            component_datatype: Some(ComponentDatatype::Float),
            count: Some(4),
            ..Default::default()
        });
    }
    use cesium_rs::scene::model_components as mc;
    primitive.feature_ids = vec![if c["textureFeature"] == true {
        mc::FeatureId::Texture(mc::FeatureIdTexture {
            property_table_id: Some(0),
            positional_label: Some("featureId_0".into()),
            texture_reader: Some(mc::TextureReader::default()),
            ..Default::default()
        })
    } else {
        mc::FeatureId::Attribute(mc::FeatureIdAttribute {
            property_table_id: Some(0),
            positional_label: Some("featureId_0".into()),
            ..Default::default()
        })
    }];
    primitive.property_attribute_ids = c
        .get("propertyAttributeIds")
        .map(|v| v.as_array().unwrap().iter().map(|i| i.as_u64().unwrap() as usize).collect())
        .unwrap_or_else(|| if c["kind"] == "attribute" { vec![0] } else { vec![] });
    primitive.property_texture_ids = c
        .get("propertyTextureIds")
        .map(|v| v.as_array().unwrap().iter().map(|i| i.as_u64().unwrap() as usize).collect())
        .unwrap_or_else(|| if c["kind"] == "texture" { vec![0] } else { vec![] });
    let mut r = PrimitiveRenderResources::new(&node_resources, 0, &primitive, None, None).unwrap();
    let usage = metadata_pipeline_stage::MetadataUsage {
        vertex: c["usageVertex"].as_array().unwrap().iter().map(|v| s(v).to_string()).collect(),
        fragment: c["usageFragment"].as_array().unwrap().iter().map(|v| s(v).to_string()).collect(),
    };
    metadata_pipeline_stage::process(
        &mut r,
        &primitive,
        &mc::Node::default(),
        Some(&metadata),
        metadata_pipeline_stage::MetadataStageOptions {
            webgl2: c["webgl2"] == true,
            statistics: if c["stats"] == true { Some(statistics) } else { None },
            usage: &usage,
        },
    )
    .unwrap();
    r
}
fn metadata_snapshot(r: &PrimitiveRenderResources<Host>) -> Value {
    let (vs, fs) = r.shader_builder.sources();
    let mut uniforms: serde_json::Map<String, Value> =
        r.uniform_map.iter().map(|(k, f)| (k.clone(), stage_uniform(f()))).collect();
    for k in r.texture_uniform_map.keys() {
        uniforms.insert(k.clone(), Value::Null);
    }
    json!({"vertex":{"defines":vs.defines,"source":vs.sources[0]},"fragment":{"defines":fs.defines,"source":fs.sources[0]},"attributeLocations":r.shader_builder.attribute_locations(),"uniforms":uniforms})
}
#[test]
fn metadata_stage_matches_cesium() {
    run(
        "scene",
        "ModelMetadataStage",
        |_| Tol { abs: 1e-6, rel: 1e-6, vector: false },
        |_, args| Some(Out::Ok(metadata_snapshot(&metadata_resources(&args[0])))),
    );
}

#[test]
fn feature_pipeline_sources_track_uploads_statistics_and_destruction() {
    let context = Context::new_webgl2(Host::default());
    let table = ModelFeatureTable::<Host>::new(table(), None, 2).unwrap();
    let batch_source = table.batch_pipeline_source().unwrap();
    let pick_source = table.picking_pipeline_source().unwrap();
    let mut statistics = ModelStatistics::new();
    statistics.add_batch_texture(&table.batch_statistics());
    table.update(&context, false).unwrap();
    assert!(batch_source.borrow().default_texture.is_some());
    assert!(batch_source.borrow().batch_texture.is_none());
    table.set_color(0, Color::RED).unwrap();
    table.update(&context, true).unwrap();
    assert!(batch_source.borrow().batch_texture.is_some());
    assert!(pick_source.borrow().pick_texture.is_some());
    assert_eq!(statistics.textures_byte_length(), 32);
    table.destroy().unwrap();
    assert!(batch_source.borrow().batch_texture.is_none());
    assert!(pick_source.borrow().pick_texture.is_none());
    assert_eq!(statistics.textures_byte_length(), 0);
}

#[test]
fn feature_inheritance_prioritizes_semantics_and_values_are_copied() {
    use cesium_rs::scene::{metadata_table_property::ByteView, metadata_value::JsMap};
    use std::sync::Arc;
    let schema=MetadataSchema::from_json(&json!({"classes":{"building":{"properties":{"height":{"type":"SCALAR","componentType":"FLOAT32","semantic":"ALTITUDE"},"ALTITUDE":{"type":"SCALAR","componentType":"FLOAT32"}}}}})).unwrap();
    let bytes = TypedArray::Float32(vec![10., 99.]).to_le_bytes();
    let mut views = JsMap::new();
    views.insert("0", ByteView { buffer: &bytes, byte_offset: 0 });
    views.insert("1", ByteView { buffer: &bytes, byte_offset: 4 });
    let metadata = MetadataTable::new(
        1,
        Some(&json!({"height":{"values":0},"ALTITUDE":{"values":1}})),
        schema.classes.get("building").unwrap().clone(),
        &views,
    )
    .unwrap();
    let table = ModelFeatureTable::<Host>::new(
        PropertyTable::new(PropertyTableOptions { count: 1, metadata_table: Some(metadata), ..Default::default() }),
        None,
        2,
    )
    .unwrap();
    let feature = table.get_feature(0).unwrap();
    assert_eq!(feature.get_property_inherited("ALTITUDE").unwrap(), Some(MetadataValue::Number(10.)));
    assert_eq!(feature.get_property("ALTITUDE").unwrap(), Some(MetadataValue::Number(99.)));
    feature.set_property("extra", MetadataValue::Array(vec![MetadataValue::Number(1.)])).unwrap();
    let mut copy = feature.get_property("extra").unwrap().unwrap();
    if let MetadataValue::Array(v) = &mut copy {
        v.push(MetadataValue::Number(2.));
    }
    assert_eq!(feature.get_property("extra").unwrap(), Some(MetadataValue::Array(vec![MetadataValue::Number(1.)])));
    assert!(table.has_property_by_semantic(0, "ALTITUDE").unwrap());
    assert_eq!(table.get_property_by_semantic(0, "ALTITUDE").unwrap(), Some(MetadataValue::Number(10.)));
    assert!(!table.is_class(0, "building").unwrap());
    assert!(!table.is_exact_class(0, "building").unwrap());
    assert!(table.get_exact_class_name(0).unwrap().is_none());
    let weak = Arc::downgrade(schema.classes.get("building").unwrap());
    drop(table);
    drop(schema);
    assert!(feature.get_property("height").is_err());
    assert!(weak.upgrade().is_none());
}

#[test]
fn dropping_a_feature_table_breaks_pick_registry_ownership_cycles() {
    let context = Context::new_webgl2(Host::default());
    let table = ModelFeatureTable::<Host>::new(table(), None, 2).unwrap();
    let feature = table.get_feature(0).unwrap();
    table.update(&context, true).unwrap();
    let pick = table.get_pick_color(0).unwrap().unwrap().color.to_rgba();
    let source = table.picking_pipeline_source().unwrap();
    let statistics = table.batch_statistics();
    drop(table);
    assert!(source.borrow().pick_texture.is_none());
    assert_eq!(statistics.byte_length.get(), 0);
    assert!(context.pick_object::<ModelFeature<Host>>(pick).is_none());
    assert!(feature.feature_table().is_none());
    assert!(feature.color().is_err());
}

fn texture_descriptor() -> Value {
    load("scene", "ModelMetadataStage")
        .into_iter()
        .find(|c| c.a[0]["kind"] == "texture" && c.a[0]["webgl2"] == true && c.a[0]["stats"] == false)
        .unwrap()
        .a
        .remove(0)
}

#[test]
fn metadata_uses_only_the_primitive_referenced_attributes_and_textures() {
    let mut c =
        load("scene", "ModelMetadataStage").into_iter().find(|c| c.a[0]["kind"] == "attribute").unwrap().a.remove(0);
    c["extension"]["propertyAttributes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"class":"sensor","properties":{"code":{"attribute":"_OTHER"}}}));
    // The other primitive's attribute must never be looked up on this primitive.
    let first = metadata_snapshot(&metadata_resources(&c));
    assert!(s(&first["vertex"]["source"]).contains("metadata.temperature"));
    assert!(!s(&first["vertex"]["source"]).contains("metadata.code"));
    c["propertyAttributeIds"] = json!([1]);
    c["attrs"] = json!([{"name":"_OTHER","type":"SCALAR"}]);
    let second = metadata_snapshot(&metadata_resources(&c));
    assert!(s(&second["vertex"]["source"]).contains("metadata.code"));
    assert!(!s(&second["vertex"]["source"]).contains("metadata.temperature"));
    let mut c = texture_descriptor();
    c["extension"]["propertyTextures"]
        .as_array_mut()
        .unwrap()
        .push(json!({"class":"sensor","properties":{"code":{"index":1,"channels":[0]}}}));
    c["propertyTextureIds"] = json!([1]);
    let selected = metadata_snapshot(&metadata_resources(&c));
    assert!(s(&selected["fragment"]["source"]).contains("metadata.code"));
    assert!(!s(&selected["fragment"]["source"]).contains("metadata.temperature"));
    assert!(selected["uniforms"].get("u_propertyTexture_0").is_none());
    c["propertyTextureIds"] = json!([]);
    let empty = metadata_snapshot(&metadata_resources(&c));
    assert!(empty["uniforms"].as_object().unwrap().is_empty());
}

#[test]
fn metadata_shared_samplers_preserve_distinct_transforms_and_deduplicate_equal_ones() {
    let mut c = texture_descriptor();
    c["extension"]["propertyTextures"][0]["properties"]["code"]["index"] = json!(0);
    c["extension"]["propertyTextures"][0]["properties"]["code"]["extensions"] =
        json!({"KHR_texture_transform":{"offset":[0.7,0.8],"scale":[4,5]}});
    let r = metadata_resources(&c);
    let (_, fs) = r.shader_builder.sources();
    let source = &fs.sources[0];
    assert_eq!(source.matches("uniform sampler2D u_propertyTexture_0;").count(), 1);
    assert_eq!(source.matches("uniform mat3 u_propertyTexture_0Transform;").count(), 1);
    assert_eq!(source.matches("uniform mat3 u_propertyTexture_0Transform_code;").count(), 1);
    assert!(source.contains("texture(u_propertyTexture_0, vec2(u_propertyTexture_0Transform *"));
    assert!(source.contains("texture(u_propertyTexture_0, vec2(u_propertyTexture_0Transform_code *"));
    assert_ne!(r.uniform_map["u_propertyTexture_0Transform"](), r.uniform_map["u_propertyTexture_0Transform_code"]());
    c["extension"]["propertyTextures"][0]["properties"]["code"]["extensions"] =
        c["extension"]["propertyTextures"][0]["properties"]["temperature"]["extensions"].clone();
    let r = metadata_resources(&c);
    let (_, fs) = r.shader_builder.sources();
    assert_eq!(fs.sources[0].matches("uniform mat3 u_propertyTexture_0Transform;").count(), 1);
    assert!(!r.uniform_map.contains_key("u_propertyTexture_0Transform_code"));
}

fn integer_texture_descriptor(component: &str, vector: bool) -> Value {
    let mut c = texture_descriptor();
    c["schema"]["classes"]["sensor"]["properties"]["code"] =
        json!({"type":if vector {"VEC2"}else{"SCALAR"},"componentType":component});
    c["extension"]["propertyTextures"][0]["properties"] =
        json!({"code":{"index":0,"channels":if vector {vec![0,1]}else{vec![0]}}});
    c["stats"] = json!(true);
    let fraction = if vector { json!([1.5, 2.5]) } else { json!(1.5) };
    c["statistics"] = json!({"classes":{"sensor":{"properties":{"code":{"mean":fraction,"standardDeviation":fraction,"variance":fraction}}}}});
    c
}

#[test]
fn metadata_fractional_statistics_use_float_scalar_and_vector_constructors() {
    for (component, vector, glsl, float) in [
        ("INT8", false, "int", "float"),
        ("UINT8", false, "uint", "float"),
        ("INT8", true, "ivec2", "vec2"),
        ("UINT8", true, "uvec2", "vec2"),
    ] {
        let c = integer_texture_descriptor(component, vector);
        let r = metadata_resources(&c);
        let (_, fs) = r.shader_builder.sources();
        let source = &fs.sources[0];
        for field in ["mean", "standardDeviation", "variance"] {
            assert!(source.contains(&format!("{float} {field};")), "{source}");
            let value = if vector { "1.5,2.5" } else { "1.5" };
            assert!(source.contains(&format!("metadataStatistics.code.{field} = {float}({value});")), "{source}");
            assert!(!source.contains(&format!("metadataStatistics.code.{field} = {glsl}(")));
        }
    }
}

#[test]
fn metadata_shader_types_and_integer_transforms_match_the_backend() {
    let mut shaders = vec![];
    for webgl2 in [false, true] {
        for vector in [false, true] {
            let mut c = integer_texture_descriptor("UINT8", vector);
            c["webgl2"] = json!(webgl2);
            c["schema"]["classes"]["sensor"]["properties"]["code"]["offset"] =
                if vector { json!([3, 4]) } else { json!(3) };
            c["schema"]["classes"]["sensor"]["properties"]["code"]["scale"] =
                if vector { json!([2, 3]) } else { json!(2) };
            let r = metadata_resources(&c);
            let (_, fs) = r.shader_builder.sources();
            let source = &fs.sources[0];
            if !webgl2 {
                assert!(!source.contains("uint"));
                assert!(!source.contains("uvec"));
                assert!(source.contains(if vector { "ivec2 code;" } else { "int code;" }));
                assert!(matches!(r.uniform_map["u_code_offset"](), UniformValue::Int(_)));
            }
            assert!(source.contains("metadata.code = (u_code_offset + u_code_scale * ("));
            let prefix="struct FeatureIds { int featureId_0; };\nstruct ProcessedAttributes { vec2 texCoord_0; vec2 texCoord_1; };\n";
            let main="void main() { FeatureIds ids; ProcessedAttributes attributes; Metadata metadata; MetadataClass metadataClass; MetadataStatistics metadataStatistics; metadataStage(ids, metadata, metadataClass, metadataStatistics, attributes); out_FragColor = vec4(1.0); }";
            let fragment = ShaderSource { sources: vec![format!("{prefix}{source}\n{main}")], ..Default::default() }
                .combine(true, ShaderCapabilities::default(), webgl2)
                .unwrap();
            shaders.push(
                json!({"name":format!("uint8-vector{vector}-webgl2{webgl2}"),"webgl2":webgl2,"fragment":fragment}),
            );
        }
    }
    let mut c = integer_texture_descriptor("UINT32", false);
    c["extension"]["propertyTextures"][0]["properties"]["code"]["channels"] = json!([0, 1, 2, 3]);
    c["schema"]["classes"]["sensor"]["properties"]["code"]["offset"] = json!(16777217);
    let r = metadata_resources(&c);
    assert_eq!(r.uniform_map["u_code_offset"](), UniformValue::UIntArray { components: 1, values: vec![16777217] });
    // Optional artifact for real WebGL compiler verification; normal CI tests
    // remain independent of a locally installed browser.
    if let Ok(path) = std::env::var("CESIUM_METADATA_SHADER_OUTPUT") {
        std::fs::write(path, serde_json::to_vec(&shaders).unwrap()).unwrap();
    }
}
