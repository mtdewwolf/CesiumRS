//! ResourceCacheKey, ResourceCacheStatistics, GltfLoaderUtil and ModelComponents
//! against CesiumJS (`tools/cesium/fixtures/scene-loader-foundations.mjs`).
mod common;
use cesium_rs::core::*;
use cesium_rs::renderer::{Gl, GlValue};
use cesium_rs::scene::gltf_loader_util::*;
use cesium_rs::scene::gltf_pipeline::PipelineError;
use cesium_rs::scene::model_components::*;
use cesium_rs::scene::resource_cache_key::*;
use cesium_rs::scene::{ResourceCacheStatistics, SupportedImageFormats};
use common::*;
use serde_json::{json, Value};

/// Model components are generic over the GL host; these fixtures never touch GPU objects.
struct NoGl;
impl Gl for NoGl {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([1, 1])
    }
    fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        Err(RuntimeError::new(format!("no GL: {method}")))
    }
}

fn resource(v: &Value) -> Resource {
    Resource::new(s(v)).expect("fixture URL")
}

fn formats(v: &Value) -> SupportedImageFormats {
    SupportedImageFormats { webp: v["webp"] == true, basis: v["basis"] == true }
}

fn key(result: Result<String, PipelineError>) -> Option<Out> {
    res(result)
}

/// Fixture arguments back to JSON numbers: the encoder writes -0 and non-finite numbers as strings.
fn numbers(v: &Value) -> Value {
    match v {
        Value::String(_) if matches!(v.as_str(), Some("-0" | "NaN" | "Infinity" | "-Infinity")) => {
            serde_json::Number::from_f64(n(v)).map_or(Value::Null, Value::Number)
        }
        Value::Array(items) => Value::Array(items.iter().map(numbers).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, x)| (k.clone(), numbers(x))).collect()),
        _ => v.clone(),
    }
}

fn flag(v: &Value) -> bool {
    v.as_bool().unwrap_or(false)
}

#[test]
fn resource_cache_key() {
    run(
        "scene/foundations",
        "ResourceCacheKey",
        |_| EXACT,
        |f, a| {
            Some(match f {
                "externalBuffer" => Out::Ok(get_external_buffer_cache_key(&resource(&a[0])).enc()),
                "gltf" => Out::Ok(get_gltf_cache_key(&resource(&a[0])).enc()),
                "embeddedBuffer" => Out::Ok(get_embedded_buffer_cache_key(&resource(&a[0]), u(&a[1]) as usize).enc()),
                "schema" => {
                    let schema = numbers(&a[0]);
                    let external = (!a[1].is_null()).then(|| resource(&a[1]));
                    let source = match &external {
                        Some(r) => SchemaSource::External(r),
                        None => SchemaSource::Embedded(&schema),
                    };
                    Out::Ok(get_schema_cache_key(source).enc())
                }
                "bufferView" => {
                    key(get_buffer_view_cache_key(&a[0], u(&a[1]) as usize, &resource(&a[2]), &resource(&a[3])))?
                }
                "image" => key(get_image_cache_key(&a[0], u(&a[1]) as usize, &resource(&a[2]), &resource(&a[3])))?,
                "draco" => key(get_draco_cache_key(&a[0], &a[1], &resource(&a[2]), &resource(&a[3])))?,
                "spz" => key(get_spz_cache_key(&a[0], &resource(&a[1]), &resource(&a[2])))?,
                "vertexBuffer" => {
                    let o = &a[4];
                    let (gltf_resource, base_resource) = (resource(&a[1]), resource(&a[2]));
                    key(get_vertex_buffer_cache_key(VertexBufferKeyOptions {
                        gltf: &a[0],
                        gltf_resource: &gltf_resource,
                        base_resource: &base_resource,
                        context_id: s(&a[3]),
                        buffer_view_id: o["bufferViewId"].as_u64().map(|v| v as usize),
                        draco: o.get("draco"),
                        spz: o.get("spz").is_some_and(|v| !v.is_null()),
                        attribute_semantic: o["attributeSemantic"].as_str(),
                        dequantize: flag(&o["dequantize"]),
                        load_buffer: flag(&o["loadBuffer"]),
                        load_typed_array: flag(&o["loadTypedArray"]),
                    }))?
                }
                "indexBuffer" => {
                    let (gltf_resource, base_resource) = (resource(&a[2]), resource(&a[3]));
                    key(get_index_buffer_cache_key(IndexBufferKeyOptions {
                        gltf: &a[0],
                        accessor_id: u(&a[1]) as usize,
                        gltf_resource: &gltf_resource,
                        base_resource: &base_resource,
                        context_id: s(&a[4]),
                        draco: (!a[5].is_null()).then_some(&a[5]),
                        load_buffer: flag(&a[6]),
                        load_typed_array: flag(&a[7]),
                    }))?
                }
                "texture" => key(get_texture_cache_key(
                    &a[0],
                    &a[1],
                    &resource(&a[2]),
                    &resource(&a[3]),
                    formats(&a[4]),
                    s(&a[5]),
                ))?,
                _ => return None,
            })
        },
    );
}

#[test]
fn resource_cache_statistics() {
    run(
        "scene/foundations",
        "ResourceCacheStatistics",
        |_| EXACT,
        |f, a| {
            (f == "sequence").then_some(())?;
            let mut stats = ResourceCacheStatistics::new();
            let size = |v: &Value| v.as_u64();
            let out: Vec<Value> = a[0]
                .as_array()?
                .iter()
                .map(|op| {
                    match s(&op[0]) {
                        "geometry" => stats.add_geometry_loader(s(&op[1]), size(&op[2]), size(&op[3])),
                        "texture" => stats.add_texture_loader(s(&op[1]), size(&op[2]).unwrap()),
                        "remove" => stats.remove_loader(s(&op[1])),
                        _ => stats.clear(),
                    }
                    json!([stats.geometry_byte_length, stats.textures_byte_length])
                })
                .collect();
            ok(Value::Array(out))
        },
    );
}

fn opt_matrix3(m: Option<Matrix3>) -> Value {
    m.map_or(Value::Null, |m| m.enc())
}

fn reader(r: &TextureReader<NoGl>) -> Value {
    json!({
        "texture": Value::Null, "index": r.index.enc(), "texCoord": r.tex_coord.enc(),
        "transform": opt_matrix3(r.transform), "scale": r.scale.enc(), "channels": r.channels,
    })
}

#[test]
fn gltf_loader_util() {
    run(
        "scene/foundations",
        "GltfLoaderUtil",
        |_| TIGHT,
        |f, a| {
            Some(match f {
                "getImageIdFromTexture" => match get_image_id_from_texture(&a[0], &a[1], formats(&a[2])) {
                    Ok(id) => Out::Ok(id.cloned().unwrap_or(Value::Null)),
                    Err(e) => Out::Err(e.to_string()),
                },
                "createSampler" => match create_sampler(&a[0], &a[1], flag(&a[2])) {
                    Ok(sampler) => Out::Ok(json!([
                        sampler.wrap_s as u32,
                        sampler.wrap_t as u32,
                        sampler.wrap_r as u32,
                        sampler.minification_filter as u32,
                        sampler.magnification_filter as u32,
                        sampler.maximum_anisotropy
                    ])),
                    Err(e) => Out::Err(e.to_string()),
                },
                "createModelTextureReader" => {
                    let channels = a[1].as_str().map(str::to_string);
                    // No texture can be created without a context; the fixture checks that the
                    // reader keeps the texture it was given, which here is none.
                    let r = create_model_texture_reader::<NoGl>(&a[0], channels, None);
                    let mut value = reader(&r);
                    value["texture"] = json!(r.texture.is_none());
                    Out::Ok(value)
                }
                _ => return None,
            })
        },
    );
}

fn math(v: &Option<MathValue>) -> Value {
    match v {
        None => Value::Null,
        Some(MathValue::Number(x)) => x.enc(),
        Some(MathValue::Cartesian2(c)) => c.enc(),
        Some(MathValue::Cartesian3(c)) => c.enc(),
        Some(MathValue::Cartesian4(c)) => c.enc(),
        Some(MathValue::Matrix2(m)) => m.enc(),
        Some(MathValue::Matrix3(m)) => m.enc(),
        Some(MathValue::Matrix4(m)) => m.enc(),
        Some(MathValue::Quaternion(q)) => q.enc(),
    }
}

fn opt_enc<T: Enc>(v: Option<T>) -> Value {
    v.map_or(Value::Null, |v| v.enc())
}

fn null_unless_empty<T>(v: &Option<T>) -> Value {
    assert!(v.is_none(), "defaults only");
    Value::Null
}

fn quantization(q: &Quantization) -> Value {
    json!({
        "octEncoded": q.oct_encoded, "octEncodedZXY": q.oct_encoded_zxy,
        "normalizationRange": math(&q.normalization_range), "quantizedVolumeOffset": math(&q.quantized_volume_offset),
        "quantizedVolumeDimensions": math(&q.quantized_volume_dimensions), "quantizedVolumeStepSize": math(&q.quantized_volume_step_size),
        "componentDatatype": q.component_datatype.map(|c| c as u32), "type": q.attribute_type.map(|t| t.as_str()),
    })
}

fn attribute(x: &Attribute<NoGl>) -> Value {
    json!({
        "name": x.name, "semantic": x.semantic.map(|s| s.as_str()), "setIndex": x.set_index,
        "componentDatatype": x.component_datatype.map(|c| c as u32), "type": x.attribute_type.map(|t| t.as_str()),
        "normalized": x.normalized, "count": x.count, "min": math(&x.min), "max": math(&x.max), "constant": math(&x.constant),
        "quantization": x.quantization.as_ref().map(quantization), "typedArray": null_unless_empty(&x.typed_array),
        "buffer": null_unless_empty(&x.buffer), "byteOffset": x.byte_offset, "byteStride": x.byte_stride,
    })
}

fn feature_fields(
    count: Option<u32>,
    null_id: Option<u32>,
    table: Option<usize>,
    label: &Option<String>,
    positional: &Option<String>,
) -> Value {
    json!({ "featureCount": count, "nullFeatureId": null_id, "propertyTableId": table, "label": label, "positionalLabel": positional })
}

fn with(mut base: Value, extra: Value) -> Value {
    base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    base
}

fn metallic_roughness(m: &MetallicRoughness<NoGl>) -> Value {
    json!({
        "baseColorTexture": m.base_color_texture.as_ref().map(reader), "metallicRoughnessTexture": m.metallic_roughness_texture.as_ref().map(reader),
        "baseColorFactor": m.base_color_factor.enc(), "metallicFactor": m.metallic_factor, "roughnessFactor": m.roughness_factor,
    })
}

fn material(m: &Material<NoGl>) -> Value {
    json!({
        "metallicRoughness": metallic_roughness(&m.metallic_roughness),
        "specularGlossiness": null_unless_empty(&m.specular_glossiness), "specular": null_unless_empty(&m.specular),
        "anisotropy": null_unless_empty(&m.anisotropy), "clearcoat": null_unless_empty(&m.clearcoat),
        "emissiveTexture": m.emissive_texture.as_ref().map(reader), "normalTexture": m.normal_texture.as_ref().map(reader),
        "occlusionTexture": m.occlusion_texture.as_ref().map(reader), "emissiveFactor": m.emissive_factor.enc(),
        "alphaMode": m.alpha_mode.as_str(), "alphaCutoff": m.alpha_cutoff, "doubleSided": m.double_sided, "unlit": m.unlit,
        "pointDiameter": m.point_diameter,
    })
}

fn defaults(name: &str) -> Option<Value> {
    Some(match name {
        "Quantization" => quantization(&Quantization::default()),
        "Attribute" => attribute(&Attribute::default()),
        "Indices" => {
            let x = Indices::<NoGl>::default();
            json!({"indexDatatype": x.index_datatype.map(|d| d as u32), "count": x.count, "buffer": null_unless_empty(&x.buffer), "typedArray": null_unless_empty(&x.typed_array)})
        }
        "FeatureIdAttribute" => {
            let x = FeatureIdAttribute::default();
            with(
                feature_fields(x.feature_count, x.null_feature_id, x.property_table_id, &x.label, &x.positional_label),
                json!({"setIndex": x.set_index}),
            )
        }
        "FeatureIdImplicitRange" => {
            let x = FeatureIdImplicitRange::default();
            with(
                feature_fields(x.feature_count, x.null_feature_id, x.property_table_id, &x.label, &x.positional_label),
                json!({"offset": x.offset.enc(), "repeat": opt_enc(x.repeat)}),
            )
        }
        "FeatureIdTexture" => {
            let x = FeatureIdTexture::<NoGl>::default();
            with(
                feature_fields(x.feature_count, x.null_feature_id, x.property_table_id, &x.label, &x.positional_label),
                json!({"textureReader": x.texture_reader.as_ref().map(reader)}),
            )
        }
        "MorphTarget" => {
            json!({"attributes": MorphTarget::<NoGl>::default().attributes.iter().map(attribute).collect::<Vec<_>>()})
        }
        "Primitive" => {
            let x = Primitive::<NoGl>::default();
            assert!(x.morph_targets.is_empty() && x.feature_ids.is_empty());
            json!({
                "attributes": x.attributes.iter().map(attribute).collect::<Vec<_>>(), "morphTargets": [],
                "indices": null_unless_empty(&x.indices), "material": x.material.as_ref().map(material), "primitiveType": x.primitive_type.map(|p| p as u32),
                "featureIds": [], "propertyTextureIds": x.property_texture_ids, "propertyAttributeIds": x.property_attribute_ids,
                "outlineCoordinates": x.outline_coordinates.as_ref().map(attribute),
                "modelPrimitiveImagery": null_unless_empty(&x.model_primitive_imagery),
            })
        }
        "Instances" => {
            let x = Instances::<NoGl>::default();
            assert!(x.feature_ids.is_empty());
            json!({"attributes": x.attributes.iter().map(attribute).collect::<Vec<_>>(), "featureIds": [], "transformInWorldSpace": x.transform_in_world_space})
        }
        "Skin" => {
            let x = Skin::default();
            json!({"index": x.index, "joints": x.joints, "inverseBindMatrices": x.inverse_bind_matrices.iter().map(Enc::enc).collect::<Vec<_>>()})
        }
        "Node" => {
            let x = Node::<NoGl>::default();
            assert!(x.primitives.is_empty() && x.instances.is_none());
            json!({
                "name": x.name, "index": x.index, "children": x.children, "primitives": [], "instances": Value::Null, "skin": x.skin,
                "matrix": opt_enc(x.matrix), "translation": opt_enc(x.translation), "rotation": opt_enc(x.rotation), "scale": opt_enc(x.scale),
                "morphWeights": x.morph_weights, "articulationName": x.articulation_name,
            })
        }
        "Scene" => json!({"nodes": Scene::default().nodes}),
        "AnimationSampler" => {
            let x = AnimationSampler::default();
            json!({"input": x.input, "interpolation": x.interpolation.map(|i| format!("{i:?}")), "output": x.output.iter().map(|v| math(&Some(*v))).collect::<Vec<_>>()})
        }
        "AnimationTarget" => {
            let x = AnimationTarget::default();
            json!({"node": x.node, "path": x.path.map(|p| p.as_str())})
        }
        "AnimationChannel" => {
            let x = AnimationChannel::default();
            json!({"sampler": x.sampler, "target": null_unless_empty(&x.target)})
        }
        "Animation" => {
            let x = Animation::default();
            assert!(x.samplers.is_empty() && x.channels.is_empty());
            json!({"name": x.name, "samplers": [], "channels": []})
        }
        "ArticulationStage" => {
            let x = ArticulationStage::default();
            json!({"name": x.name, "type": null_unless_empty(&x.stage_type), "minimumValue": x.minimum_value, "maximumValue": x.maximum_value, "initialValue": x.initial_value})
        }
        "Articulation" => {
            let x = Articulation::default();
            assert!(x.stages.is_empty());
            json!({"name": x.name, "stages": []})
        }
        "Asset" => {
            assert!(Asset::default().credits.is_empty());
            json!({"credits": []})
        }
        "Components" => {
            let x = Components::<NoGl>::default();
            assert!(
                x.asset.credits.is_empty()
                    && x.nodes.is_empty()
                    && x.skins.is_empty()
                    && x.animations.is_empty()
                    && x.articulations.is_empty()
            );
            json!({
                "asset": {"credits": []}, "scene": x.scene.map(|s| s.nodes), "nodes": [], "skins": [], "animations": [], "articulations": [],
                "structuralMetadata": null_unless_empty(&x.structural_metadata),
                "upAxis": x.up_axis.map(|a| a as i32), "forwardAxis": x.forward_axis.map(|a| a as i32),
                "transform": x.transform.enc(), "extensions": Value::Object(x.extensions),
            })
        }
        "TextureReader" => reader(&TextureReader::default()),
        "MetallicRoughness" => metallic_roughness(&MetallicRoughness::default()),
        "SpecularGlossiness" => {
            let x = SpecularGlossiness::<NoGl>::default();
            json!({
                "diffuseTexture": x.diffuse_texture.as_ref().map(reader), "specularGlossinessTexture": x.specular_glossiness_texture.as_ref().map(reader),
                "diffuseFactor": x.diffuse_factor.enc(), "specularFactor": x.specular_factor.enc(), "glossinessFactor": x.glossiness_factor,
            })
        }
        "Specular" => {
            let x = Specular::<NoGl>::default();
            json!({
                "specularFactor": x.specular_factor, "specularTexture": x.specular_texture.as_ref().map(reader),
                "specularColorFactor": x.specular_color_factor.enc(), "specularColorTexture": x.specular_color_texture.as_ref().map(reader),
            })
        }
        "Anisotropy" => {
            let x = Anisotropy::<NoGl>::default();
            json!({"anisotropyStrength": x.anisotropy_strength, "anisotropyRotation": x.anisotropy_rotation, "anisotropyTexture": x.anisotropy_texture.as_ref().map(reader)})
        }
        "Clearcoat" => {
            let x = Clearcoat::<NoGl>::default();
            json!({
                "clearcoatFactor": x.clearcoat_factor, "clearcoatTexture": x.clearcoat_texture.as_ref().map(reader),
                "clearcoatRoughnessFactor": x.clearcoat_roughness_factor, "clearcoatRoughnessTexture": x.clearcoat_roughness_texture.as_ref().map(reader),
                "clearcoatNormalTexture": x.clearcoat_normal_texture.as_ref().map(reader),
            })
        }
        "Material" => material(&Material::default()),
        _ => return None,
    })
}

#[test]
fn model_components() {
    run(
        "scene",
        "ModelComponents",
        |_| EXACT,
        |f, a| match f {
            "defaults" => defaults(s(&a[0])).map(Out::Ok),
            "animatedPropertyType" => ok(Value::Object(
                AnimatedPropertyType::ALL.iter().map(|t| (t.name().to_string(), json!(t.as_str()))).collect(),
            )),
            "constants" => ok(json!({
                "metallicRoughness": [MetallicRoughness::<NoGl>::DEFAULT_BASE_COLOR_FACTOR.enc(), MetallicRoughness::<NoGl>::DEFAULT_METALLIC_FACTOR, MetallicRoughness::<NoGl>::DEFAULT_ROUGHNESS_FACTOR],
                "specularGlossiness": [SpecularGlossiness::<NoGl>::DEFAULT_DIFFUSE_FACTOR.enc(), SpecularGlossiness::<NoGl>::DEFAULT_SPECULAR_FACTOR.enc(), SpecularGlossiness::<NoGl>::DEFAULT_GLOSSINESS_FACTOR],
                "specular": [Specular::<NoGl>::DEFAULT_SPECULAR_FACTOR, Specular::<NoGl>::DEFAULT_SPECULAR_COLOR_FACTOR.enc()],
                "anisotropy": [Anisotropy::<NoGl>::DEFAULT_ANISOTROPY_STRENGTH, Anisotropy::<NoGl>::DEFAULT_ANISOTROPY_ROTATION],
                "clearcoat": [Clearcoat::<NoGl>::DEFAULT_CLEARCOAT_FACTOR, Clearcoat::<NoGl>::DEFAULT_CLEARCOAT_ROUGHNESS_FACTOR],
                "material": [Material::<NoGl>::DEFAULT_EMISSIVE_FACTOR.enc()],
            })),
            _ => None,
        },
    );
}
