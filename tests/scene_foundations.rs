//! Scene enums and stateless helpers replayed against the pinned CesiumJS fixtures.
mod common;
use cesium_rs::core::{Cartesian3, Color, ComponentDatatype, Ellipsoid, TypedArray};
use cesium_rs::renderer::{UniformMap, UniformValue};
use cesium_rs::scene::model::extensions::gpm::StorageType;
use cesium_rs::scene::model::*;
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Map, Value};
use std::rc::Rc;

macro_rules! numeric_constants {
    ($t:ty $(, $extra:literal => $value:expr)*) => {{
        #[allow(unused_mut)]
        let mut map: Map<String, Value> = <$t>::ALL.iter().map(|v| (v.name().to_string(), json!(v.value()))).collect();
        $(map.insert($extra.to_string(), json!($value));)*
        Value::Object(map)
    }};
}

macro_rules! string_constants {
    ($t:ty) => {
        Value::Object(<$t>::ALL.iter().map(|v| (v.name().to_string(), json!(v.as_str()))).collect())
    };
}

/// The port of a module whose only case is its constants.
fn constants(value: Value) -> impl Fn(&str, &[Value]) -> Option<Out> {
    move |f, _| (f == "constants").then(|| Out::Ok(value.clone()))
}

#[test]
fn plain_enums() {
    run("scene", "ModelAnimationState", |_| EXACT, constants(numeric_constants!(ModelAnimationState)));
    run(
        "scene",
        "JobType",
        |_| EXACT,
        constants(numeric_constants!(JobType, "NUMBER_OF_JOB_TYPES" => JobType::NUMBER_OF_JOB_TYPES)),
    );
    run("scene", "TileState", |_| EXACT, constants(numeric_constants!(TileState)));
    run("scene", "ArcGisBaseMapType", |_| EXACT, constants(numeric_constants!(ArcGisBaseMapType)));
    run("scene", "Cesium3DTileContentState", |_| EXACT, constants(numeric_constants!(Cesium3DTileContentState)));
    run(
        "scene",
        "Cesium3DTileOptimizationHint",
        |_| EXACT,
        constants(numeric_constants!(Cesium3DTileOptimizationHint)),
    );
    run("scene", "TerrainState", |_| EXACT, constants(numeric_constants!(TerrainState)));
    run("scene", "ImageryState", |_| EXACT, constants(numeric_constants!(ImageryState)));
    run("scene", "PostProcessStageSampleMode", |_| EXACT, constants(numeric_constants!(PostProcessStageSampleMode)));
    run("scene", "MapMode2D", |_| EXACT, constants(numeric_constants!(MapMode2D)));
    run("scene", "VoxelMetadataOrder", |_| EXACT, constants(numeric_constants!(VoxelMetadataOrder)));
    run("scene", "ExpressionNodeType", |_| EXACT, constants(numeric_constants!(ExpressionNodeType)));
    run("scene", "BlendOption", |_| EXACT, constants(numeric_constants!(BlendOption)));
    run("scene", "Cesium3DTileRefine", |_| EXACT, constants(numeric_constants!(Cesium3DTileRefine)));
    run("scene", "CullFace", |_| EXACT, constants(numeric_constants!(CullFace)));
    run("scene", "IonWorldImageryStyle", |_| EXACT, constants(numeric_constants!(IonWorldImageryStyle)));
    run("scene", "LabelStyle", |_| EXACT, constants(numeric_constants!(LabelStyle)));
    run("scene", "ModelAnimationLoop", |_| EXACT, constants(numeric_constants!(ModelAnimationLoop)));
    run("scene", "SplitDirection", |_| EXACT, constants(numeric_constants!(SplitDirection)));
    run("scene", "QuadtreeTileLoadState", |_| EXACT, constants(numeric_constants!(QuadtreeTileLoadState)));
    run("scene", "HorizontalOrigin", |_| EXACT, constants(numeric_constants!(HorizontalOrigin)));
    run("scene", "BillboardLoadState", |_| EXACT, constants(numeric_constants!(BillboardLoadState)));
    run("scene", "CameraEventType", |_| EXACT, constants(numeric_constants!(CameraEventType)));
    run("scene", "VerticalOrigin", |_| EXACT, constants(numeric_constants!(VerticalOrigin)));
    run("scene", "Cesium3DTileColorBlendMode", |_| EXACT, constants(numeric_constants!(Cesium3DTileColorBlendMode)));
    run("scene", "BlendEquation", |_| EXACT, constants(numeric_constants!(BlendEquation)));
    run("scene", "ResourceLoaderState", |_| EXACT, constants(numeric_constants!(ResourceLoaderState)));
    run("scene", "DepthFunction", |_| EXACT, constants(numeric_constants!(DepthFunction)));
    run("scene", "StencilFunction", |_| EXACT, constants(numeric_constants!(StencilFunction)));
    run("scene", "StencilOperation", |_| EXACT, constants(numeric_constants!(StencilOperation)));
    run("scene", "BlendFunction", |_| EXACT, constants(numeric_constants!(BlendFunction)));
    run(
        "scene",
        "ClassificationType",
        |_| EXACT,
        constants(
            numeric_constants!(ClassificationType, "NUMBER_OF_CLASSIFICATION_TYPES" => ClassificationType::NUMBER_OF_CLASSIFICATION_TYPES),
        ),
    );
    run("scene", "AlphaMode", |_| EXACT, constants(string_constants!(AlphaMode)));
    run("scene", "BingMapsStyle", |_| EXACT, constants(string_constants!(BingMapsStyle)));
    run("scene", "MetadataSemantic", |_| EXACT, constants(string_constants!(MetadataSemantic)));
    use cesium_rs::scene::sdf_settings::*;
    run(
        "scene",
        "SDFSettings",
        |_| EXACT,
        constants(json!({"FONT_SIZE": FONT_SIZE, "PADDING": PADDING, "RADIUS": RADIUS, "CUTOFF": CUTOFF})),
    );
    run("scene/model", "LightingModel", |_| EXACT, constants(numeric_constants!(LightingModel)));
    run(
        "scene/model",
        "CustomShaderTranslucencyMode",
        |_| EXACT,
        constants(numeric_constants!(CustomShaderTranslucencyMode)),
    );
    run("scene/model", "UniformType", |_| EXACT, constants(string_constants!(UniformType)));
    run("scene/model", "VaryingType", |_| EXACT, constants(string_constants!(VaryingType)));
    run("scene/model/extensions/gpm", "StorageType", |_| EXACT, constants(string_constants!(StorageType)));
}

#[test]
fn enum_helpers() {
    run(
        "scene",
        "CloudType",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(CloudType)),
            "validate" => ok(CloudType::validate(n(&a[0]))),
            _ => None,
        },
    );
    run(
        "scene",
        "ColorBlendMode",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(ColorBlendMode)),
            "getColorBlend" => ok(ColorBlendMode::from_value(n(&a[0]) as i32).unwrap().get_color_blend(n(&a[1]))),
            _ => None,
        },
    );
    use cesium_rs::scene::stencil_constants::*;
    let stencil = || serde_json::to_value(set_cesium_3d_tile_bit()).unwrap();
    run(
        "scene",
        "StencilConstants",
        |_| EXACT,
        |f, _| match f {
            "constants" => ok(json!({
                "CESIUM_3D_TILE_MASK": CESIUM_3D_TILE_MASK,
                "SKIP_LOD_MASK": SKIP_LOD_MASK,
                "SKIP_LOD_BIT_SHIFT": SKIP_LOD_BIT_SHIFT,
                "CLASSIFICATION_MASK": CLASSIFICATION_MASK,
            })),
            // Cesium's object lists every field the render state reads.
            "setCesium3DTileBit" | "resolved" => ok(stencil()),
            _ => None,
        },
    );
    run(
        "scene",
        "ImplicitSubdivisionScheme",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(string_constants!(ImplicitSubdivisionScheme)),
            "getBranchingFactor" => {
                ImplicitSubdivisionScheme::from_str_value(s(&a[0])).map(|v| Out::Ok(json!(v.get_branching_factor())))
            }
            _ => None,
        },
    );
    run(
        "scene",
        "Cesium3DTilePass",
        |_| EXACT,
        |f, a| {
            match f {
            "constants" => ok(numeric_constants!(Cesium3DTilePass, "NUMBER_OF_PASSES" => Cesium3DTilePass::NUMBER_OF_PASSES)),
            "getPassOptions" => ok(Cesium3DTilePass::from_value(n(&a[0]) as i32)
                .map(|pass| {
                    let o = pass.get_pass_options();
                    json!({"pass": o.pass.value(), "isRender": o.is_render, "requestTiles": o.request_tiles, "ignoreCommands": o.ignore_commands})
                })
                .unwrap_or(Value::Null)),
            _ => None,
        }
        },
    );
    run(
        "scene",
        "DynamicAtmosphereLightingType",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(DynamicAtmosphereLightingType)),
            "fromGlobeFlags" => {
                let flag = |k: &str| a[0][k].as_bool().unwrap();
                let globe = GlobeLightingFlags {
                    enable_lighting: flag("enableLighting"),
                    dynamic_atmosphere_lighting: flag("dynamicAtmosphereLighting"),
                    dynamic_atmosphere_lighting_from_sun: flag("dynamicAtmosphereLightingFromSun"),
                };
                ok(DynamicAtmosphereLightingType::from_globe_flags(globe).value())
            }
            _ => None,
        },
    );
    run(
        "scene",
        "SceneMode",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(SceneMode)),
            "getMorphTime" => ok(SceneMode::from_value(n(&a[0]) as i32).unwrap().get_morph_time()),
            _ => None,
        },
    );
    run(
        "scene",
        "SensorVolumePortionToDisplay",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(SensorVolumePortionToDisplay)),
            "validate" => ok(SensorVolumePortionToDisplay::validate(n(&a[0]))),
            "toString" => SensorVolumePortionToDisplay::from_value(n(&a[0]) as i32).map(|v| Out::Ok(json!(v.to_str()))),
            _ => None,
        },
    );
    run(
        "scene",
        "ShadowMode",
        |_| EXACT,
        |f, a| match f {
            "constants" => {
                ok(numeric_constants!(ShadowMode, "NUMBER_OF_SHADOW_MODES" => ShadowMode::NUMBER_OF_SHADOW_MODES))
            }
            "castReceive" => {
                let mode = ShadowMode::from_value(n(&a[0]) as i32).unwrap();
                ok(json!([mode.cast_shadows(), mode.receive_shadows()]))
            }
            "fromCastReceive" => {
                ok(ShadowMode::from_cast_receive(a[0].as_bool().unwrap(), a[1].as_bool().unwrap()).value())
            }
            _ => None,
        },
    );
    run(
        "scene",
        "TileSelectionResult",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(TileSelectionResult)),
            "bits" => {
                let v = n(&a[0]) as i32;
                ok(json!([
                    TileSelectionResult::was_kicked(v),
                    TileSelectionResult::original_result(v),
                    TileSelectionResult::kick(v)
                ]))
            }
            _ => None,
        },
    );
    run(
        "scene",
        "HeightReference",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(HeightReference)),
            "clampRelative" => {
                let h = HeightReference::from_value(n(&a[0]) as i32).unwrap();
                ok(json!([is_height_reference_clamp(h), is_height_reference_relative(h)]))
            }
            _ => None,
        },
    );
    run(
        "scene",
        "Tonemapper",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(string_constants!(Tonemapper)),
            "validateTonemapper" => ok(validate_tonemapper(s(&a[0]))),
            _ => None,
        },
    );
    run(
        "scene",
        "InstanceAttributeSemantic",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(string_constants!(InstanceAttributeSemantic)),
            "fromGltfSemantic" => {
                ok(InstanceAttributeSemantic::from_gltf_semantic(s(&a[0])).map(|v| v.as_str().to_string()))
            }
            _ => None,
        },
    );
    run(
        "scene",
        "Axis",
        |_| EXACT,
        |f, a| match f {
            "constants" => {
                let mut constants = numeric_constants!(Axis);
                for (name, m) in [
                    ("Y_UP_TO_Z_UP", Axis::Y_UP_TO_Z_UP),
                    ("Z_UP_TO_Y_UP", Axis::Z_UP_TO_Y_UP),
                    ("X_UP_TO_Z_UP", Axis::X_UP_TO_Z_UP),
                    ("Z_UP_TO_X_UP", Axis::Z_UP_TO_X_UP),
                    ("X_UP_TO_Y_UP", Axis::X_UP_TO_Y_UP),
                    ("Y_UP_TO_X_UP", Axis::Y_UP_TO_X_UP),
                ] {
                    constants[name] = m.enc();
                }
                ok(constants)
            }
            "fromName" => ok(Axis::from_name(s(&a[0])).map(|v| v.value())),
            _ => None,
        },
    );
}

#[test]
fn attribute_and_metadata_types() {
    run(
        "scene",
        "AttributeType",
        |_| EXACT,
        |f, a| {
            if f == "constants" {
                return ok(string_constants!(AttributeType));
            }
            let t = AttributeType::from_str_value(s(&a[0]))?;
            match f {
                "getMathType" => ok(t.get_math_type().name().to_string()),
                "getNumberOfComponents" => ok(t.get_number_of_components() as u32),
                "getAttributeLocationCount" => ok(t.get_attribute_location_count() as u32),
                "getGlslType" => ok(t.get_glsl_type().to_string()),
                _ => None,
            }
        },
    );
    run(
        "scene",
        "MetadataType",
        |_| EXACT,
        |f, a| {
            if f == "constants" {
                return ok(string_constants!(MetadataType));
            }
            let t = MetadataType::from_str_value(s(&a[0]));
            match f {
                "isVectorType" => ok(t.is_some_and(MetadataType::is_vector_type)),
                "isMatrixType" => ok(t.is_some_and(MetadataType::is_matrix_type)),
                "getComponentCount" => t.map(|t| Out::Ok(json!(t.get_component_count()))),
                "getMathType" => ok(t.and_then(MetadataType::get_math_type).map(|m| m.name().to_string())),
                _ => None,
            }
        },
    );
    run(
        "scene",
        "VertexAttributeSemantic",
        |_| EXACT,
        |f, a| {
            let semantic = || VertexAttributeSemantic::from_str_value(s(&a[0]));
            match f {
                "constants" => ok(string_constants!(VertexAttributeSemantic)),
                "hasSetIndex" => semantic().map(|v| Out::Ok(json!(v.has_set_index()))),
                "getGlslType" => semantic().map(|v| Out::Ok(json!(v.get_glsl_type()))),
                "getVariableName" => {
                    semantic().map(|v| Out::Ok(json!(v.get_variable_name(opt(&a[1]).map(|i| i as u32)))))
                }
                "fromGltfSemantic" => {
                    ok(VertexAttributeSemantic::from_gltf_semantic(s(&a[0])).map(|v| v.as_str().to_string()))
                }
                "fromPntsSemantic" => {
                    VertexAttributeSemantic::from_pnts_semantic(s(&a[0])).map(|v| Out::Ok(json!(v.as_str())))
                }
                _ => None,
            }
        },
    );
    run(
        "scene",
        "Cesium3DTileContentType",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(string_constants!(Cesium3DTileContentType)),
            "isBinaryFormat" => ok(Cesium3DTileContentType::from_str_value(s(&a[0]))
                .is_some_and(Cesium3DTileContentType::is_binary_format)),
            _ => None,
        },
    );
    run(
        "scene/model",
        "ModelType",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(string_constants!(ModelType)),
            "is3DTiles" => ModelType::from_str_value(s(&a[0])).map(|v| Out::Ok(json!(v.is_3d_tiles()))),
            _ => None,
        },
    );
    run(
        "scene/model",
        "CustomShaderMode",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(string_constants!(CustomShaderMode)),
            "getDefineName" => ok(CustomShaderMode::from_str_value(s(&a[0])).unwrap().get_define_name()),
            _ => None,
        },
    );
    run(
        "scene/model",
        "StyleCommandsNeeded",
        |_| EXACT,
        |f, a| match f {
            "constants" => ok(numeric_constants!(StyleCommandsNeeded)),
            "getStyleCommandsNeeded" => ok(StyleCommandsNeeded::get_style_commands_needed(n(&a[0]), n(&a[1])).value()),
            _ => None,
        },
    );
}

/// A render state blending object the way the fixtures encode it: the
/// color as `[red, green, blue, alpha]`.
fn blending_json(blending: cesium_rs::renderer::render_state::Blending) -> Value {
    let color = blending.color.clone();
    let mut value = serde_json::to_value(blending).unwrap();
    value["color"] = json!([color.red, color.green, color.blue, color.alpha]);
    value
}

#[test]
fn blending_state() {
    use cesium_rs::scene::blending_state::*;
    let states = [
        ("DISABLED", disabled()),
        ("ALPHA_BLEND", alpha_blend()),
        ("PRE_MULTIPLIED_ALPHA_BLEND", pre_multiplied_alpha_blend()),
        ("ADDITIVE_BLEND", additive_blend()),
    ];
    run(
        "scene",
        "BlendingState",
        |_| EXACT,
        |f, a| match f {
            // Cesium's frozen objects list only the fields they set; every
            // field they do set must match.
            "constants" => {
                let fixture = load("scene", "BlendingState");
                let expected = &fixture[0].r;
                let mut out = Map::new();
                for (name, state) in &states {
                    let full = blending_json(state.clone());
                    let keys = expected[*name].as_object().unwrap().keys();
                    out.insert(name.to_string(), Value::Object(keys.map(|k| (k.clone(), full[k].clone())).collect()));
                }
                ok(Value::Object(out))
            }
            "resolved" => states
                .iter()
                .find(|(name, _)| *name == s(&a[0]))
                .map(|(_, state)| Out::Ok(blending_json(state.clone()))),
            _ => None,
        },
    );
}

fn metadata_number_json(value: MetadataNumber) -> Value {
    match value {
        MetadataNumber::Number(v) => num(v),
        MetadataNumber::BigInt(v) => json!({"bigint": v.to_string()}),
    }
}

#[test]
fn metadata_component_type() {
    run(
        "scene",
        "MetadataComponentType",
        |_| EXACT,
        |f, a| {
            let component = |v: &Value| MetadataComponentType::from_str_value(s(v));
            match f {
                "constants" => {
                    let mut constants = string_constants!(MetadataComponentType);
                    let info = MetadataComponentType::ALL
                        .iter()
                        .map(|&t| {
                            (
                                t.as_str().to_string(),
                                json!({
                                    "vectorCompatible": t.is_vector_compatible(),
                                    "size": t.get_size_in_bytes(),
                                    "maximumValue": metadata_number_json(t.get_maximum()),
                                    "minimumValue": metadata_number_json(t.get_minimum()),
                                    "category": t.category().as_str(),
                                }),
                            )
                        })
                        .collect::<Map<_, _>>();
                    constants["typeInfo"] = Value::Object(info);
                    ok(constants)
                }
                "info" => {
                    let t = component(&a[0])?;
                    ok(json!([
                        metadata_number_json(t.get_minimum()),
                        metadata_number_json(t.get_maximum()),
                        t.is_integer_type(),
                        t.is_unsigned_integer_type(),
                        t.is_vector_compatible(),
                        t.category().as_str(),
                        t.get_size_in_bytes(),
                        t.to_component_datatype().map(|d| d as u32),
                    ]))
                }
                "unnormalize" => {
                    let t = component(&a[1])?;
                    match MetadataComponentType::unnormalize(n(&a[0]), t) {
                        Ok(v) => v.map(|v| Out::Ok(metadata_number_json(v))),
                        Err(e) => Some(Out::Err(e.to_string())),
                    }
                }
                "normalize" => MetadataComponentType::normalize(MetadataNumber::Number(n(&a[0])), component(&a[1])?)
                    .map(|v| Out::Ok(num(v))),
                "normalizeBigInt" => {
                    let value = MetadataNumber::BigInt(s(&a[0]).parse().unwrap());
                    MetadataComponentType::normalize(value, component(&a[1])?).map(|v| Out::Ok(num(v)))
                }
                "applyValueTransform" => ok(MetadataComponentType::apply_value_transform(n(&a[0]), n(&a[1]), n(&a[2]))),
                "unapplyValueTransform" => {
                    ok(MetadataComponentType::unapply_value_transform(n(&a[0]), n(&a[1]), n(&a[2])))
                }
                "fromComponentDatatype" => ok(ComponentDatatype::from_gl(n(&a[0]) as u32)
                    .map(|d| MetadataComponentType::from_component_datatype(d).as_str().to_string())),
                "setter" | "setterBigInt" => {
                    let t = component(&a[0])?;
                    let value = if f == "setter" {
                        MetadataNumber::Number(n(&a[2]))
                    } else {
                        MetadataNumber::BigInt(s(&a[2]).parse().unwrap())
                    };
                    let mut bytes = [0u8; 12];
                    Some(match t.set_data_view_value(&mut bytes, n(&a[1]) as usize, value) {
                        Some(()) => Out::Ok(json!(bytes.to_vec())),
                        None => Out::Err("setter threw".into()),
                    })
                }
                _ => None,
            }
        },
    );
}

#[test]
fn helpers() {
    run(
        "scene",
        "hasExtension",
        |_| EXACT,
        |f, a| (f == "hasExtension").then(|| Out::Ok(json!(has_extension(&a[0], s(&a[1]))))),
    );
    run(
        "scene",
        "getClipAndStyleCode",
        |_| EXACT,
        |f, a| (f == "code").then(|| Out::Ok(json!(get_clip_and_style_code(s(&a[0]), s(&a[1]), s(&a[2]))))),
    );
    run(
        "scene",
        "NeverTileDiscardPolicy",
        |_| EXACT,
        |f, _| {
            let policy = NeverTileDiscardPolicy;
            (f == "policy").then(|| {
                Out::Ok(json!([
                    TileDiscardPolicy::<()>::is_ready(&policy),
                    policy.should_discard_image(&()),
                    policy.should_discard_image(&())
                ]))
            })
        },
    );
    run(
        "scene",
        "Splitter",
        |_| EXACT,
        |f, a| match f {
            "modifyFragmentShader" => ok(splitter::modify_fragment_shader(s(&a[0]))),
            "addUniforms" => {
                let direction = SplitDirection::from_value(n(&a[0]) as i32).unwrap();
                let base: UniformMap = Rc::new(|| Ok(vec![("u_other".to_string(), UniformValue::Float(vec![7.0]))]));
                let uniforms = splitter::add_uniforms(move || direction, Some(base))().unwrap();
                let names: Vec<_> = uniforms.iter().map(|(name, _)| name.clone()).collect();
                let UniformValue::Float(value) = &uniforms[1].1 else { panic!("czm_splitDirection is a float") };
                ok(json!([names, value[0] as f64]))
            }
            _ => None,
        },
    );
    run(
        "scene",
        "SupportedImageFormats",
        |_| EXACT,
        |f, a| {
            let flag = |k: &str| a[0].get(k).and_then(Value::as_bool).unwrap_or(false);
            (f == "new").then(|| {
                let formats = SupportedImageFormats { webp: flag("webp"), basis: flag("basis") };
                Out::Ok(json!([formats.webp, formats.basis]))
            })
        },
    );
    run(
        "scene",
        "SunLight",
        |_| EXACT,
        |f, a| {
            (f == "new").then(|| {
                let light = SunLight::new(SunLightOptions {
                    color: a[0].get("color").map(color),
                    intensity: a[0].get("intensity").map(n),
                });
                Out::Ok(json!([light.color.enc(), light.intensity]))
            })
        },
    );
    run(
        "scene",
        "DirectionalLight",
        |_| EXACT,
        |f, a| {
            (f == "new").then(|| {
                let light = DirectionalLight::new(DirectionalLightOptions {
                    direction: c3(&a[0]["direction"]),
                    color: a[0].get("color").map(color),
                    intensity: a[0].get("intensity").map(n),
                });
                match light {
                    Some(light) => Out::Ok(json!([light.direction.enc(), light.color.enc(), light.intensity])),
                    None => Out::Err("options.direction cannot be zero-length".into()),
                }
            })
        },
    );
    run(
        "scene",
        "ParticleBurst",
        |_| EXACT,
        |f, a| {
            (f == "new").then(|| {
                let burst = ParticleBurst::new(ParticleBurstOptions {
                    time: a[0].get("time").map(n),
                    minimum: a[0].get("minimum").map(n),
                    maximum: a[0].get("maximum").map(n),
                });
                Out::Ok(json!([burst.time, burst.minimum, burst.maximum, burst.complete()]))
            })
        },
    );
    run(
        "scene",
        "QuadtreeOccluders",
        |_| EXACT,
        |f, a| {
            let ellipsoid = match s(&a[0]) {
                "WGS84" => Ellipsoid::WGS84,
                "UNIT_SPHERE" => Ellipsoid::UNIT_SPHERE,
                _ => Ellipsoid::MOON,
            };
            (f == "new").then(|| {
                let occluders = QuadtreeOccluders::new(&ellipsoid);
                Out::Ok(json!([
                    occluders.ellipsoid().ellipsoid().radii.enc(),
                    occluders.ellipsoid().camera_position().enc()
                ]))
            })
        },
    );
    run(
        "scene",
        "getBinaryAccessor",
        |_| EXACT,
        |f, a| {
            if f != "accessor" {
                return None;
            }
            let component_type = match &a[0] {
                Value::String(name) => get_binary_accessor::ComponentType::Name(name),
                v => get_binary_accessor::ComponentType::Value(ComponentDatatype::from_gl(n(v) as u32).unwrap()),
            };
            let accessor = get_binary_accessor(component_type, s(&a[1])).unwrap();
            let bytes: Vec<u8> = nums(&a[4]).into_iter().map(|b| b as u8).collect();
            let view = accessor.create_array_buffer_view(&bytes, n(&a[2]) as usize, n(&a[3]) as usize);
            let class_type = match accessor.attribute_type.get_math_type() {
                MathType::Number => Value::Null,
                m => json!(m.name()),
            };
            Some(match view {
                Some(view) => Out::Ok(json!([accessor.components_per_attribute, class_type, typed_values(&view)])),
                None => Out::Err("RangeError".into()),
            })
        },
    );
}

fn typed_values(view: &TypedArray) -> Vec<Value> {
    view.to_f64_vec().into_iter().map(num).collect()
}

#[test]
fn model_options() {
    run(
        "scene/model",
        "ModelAlphaOptions",
        |_| EXACT,
        |f, _| {
            (f == "new").then(|| {
                let options = ModelAlphaOptions::default();
                Out::Ok(json!([options.pass.map(|p| p as u32), options.alpha_cutoff]))
            })
        },
    );
    run(
        "scene/model",
        "ModelLightingOptions",
        |_| EXACT,
        |f, a| {
            (f == "new").then(|| {
                let lighting_model = a[0]
                    .get("lightingModel")
                    .map(|v| LightingModel::from_value(n(v) as i32).unwrap())
                    .unwrap_or(ModelLightingOptions::default().lighting_model);
                Out::Ok(json!(ModelLightingOptions { lighting_model }.lighting_model.value()))
            })
        },
    );
    run(
        "scene/model",
        "ImageryFlags",
        |_| EXACT,
        |f, _| {
            (f == "new").then(|| {
                let flags = ImageryFlags::default();
                Out::Ok(json!({
                    "alpha": flags.alpha,
                    "brightness": flags.brightness,
                    "contrast": flags.contrast,
                    "hue": flags.hue,
                    "saturation": flags.saturation,
                    "gamma": flags.gamma,
                    "colorToAlpha": flags.color_to_alpha,
                }))
            })
        },
    );
    run(
        "scene/model",
        "CartesianRectangle",
        |_| EXACT,
        |f, a| match f {
            "defaults" => {
                let r = CartesianRectangle::default();
                ok(json!([r.min_x, r.min_y, r.max_x, r.max_y]))
            }
            "contains" => {
                let r = CartesianRectangle::new(n(&a[0]), n(&a[1]), n(&a[2]), n(&a[3]));
                let (x, y) = (n(&a[4]), n(&a[5]));
                ok(json!([r.contains(x, y), r.contains_exclusive(x, y), r.contains_inclusive(x, y)]))
            }
            _ => None,
        },
    );
}

#[test]
fn light_defaults() {
    let light = SunLight::default();
    assert_eq!((light.color, light.intensity), (Color::WHITE, 2.0));
    let direction = Cartesian3::new(0.0, 0.0, 1.0);
    let light = DirectionalLight::new(DirectionalLightOptions { direction, color: None, intensity: None }).unwrap();
    assert_eq!((light.direction, light.color, light.intensity), (direction, Color::WHITE, 1.0));
    assert!(DirectionalLight::new(DirectionalLightOptions {
        direction: Cartesian3::ZERO,
        color: None,
        intensity: None
    })
    .is_none());
}
