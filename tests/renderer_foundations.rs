//! Portable renderer contracts replayed against the pinned CesiumJS fixtures.
mod common;
use cesium_rs::renderer::*;
use common::*;
use serde_json::{json, Value};
fn enum_case<T: Copy>(
    f: &str,
    a: &[Value],
    all: &[T],
    value: impl Fn(T) -> u32,
    name: impl Fn(T) -> &'static str,
) -> Option<Out> {
    match f {
        "constants" => ok(Value::Object(all.iter().map(|v| (name(*v).to_string(), json!(value(*v)))).collect())),
        "validate" => ok(all.iter().any(|v| value(*v) as f64 == n(&a[0]))),
        _ => None,
    }
}
#[test]
fn enums() {
    run(
        "renderer",
        "BufferUsage",
        |_| EXACT,
        |f, a| enum_case(f, a, BufferUsage::ALL, |v| v as u32, BufferUsage::name),
    );
    run(
        "renderer",
        "TextureWrap",
        |_| EXACT,
        |f, a| enum_case(f, a, TextureWrap::ALL, |v| v as u32, TextureWrap::name),
    );
    run(
        "renderer",
        "TextureMinificationFilter",
        |_| EXACT,
        |f, a| enum_case(f, a, TextureMinificationFilter::ALL, |v| v as u32, TextureMinificationFilter::name),
    );
    run(
        "renderer",
        "TextureMagnificationFilter",
        |_| EXACT,
        |f, a| enum_case(f, a, TextureMagnificationFilter::ALL, |v| v as u32, TextureMagnificationFilter::name),
    );
    run("renderer", "MipmapHint", |_| EXACT, |f, a| enum_case(f, a, MipmapHint::ALL, |v| v as u32, MipmapHint::name));
    run(
        "renderer",
        "Pass",
        |_| EXACT,
        |f, _| {
            if f == "constants" {
                ok(
                    json!({"ENVIRONMENT":Pass::Environment as u32,"COMPUTE":Pass::Compute as u32,"GLOBE":Pass::Globe as u32,"TERRAIN_CLASSIFICATION":Pass::TerrainClassification as u32,"CESIUM_3D_TILE_EDGES":Pass::Cesium3DTileEdges as u32,"CESIUM_3D_TILE":Pass::Cesium3DTile as u32,"CESIUM_3D_TILE_CLASSIFICATION":Pass::Cesium3DTileClassification as u32,"CESIUM_3D_TILE_CLASSIFICATION_IGNORE_SHOW":Pass::Cesium3DTileClassificationIgnoreShow as u32,"OPAQUE":Pass::Opaque as u32,"TRANSLUCENT":Pass::Translucent as u32,"VOXELS":Pass::Voxels as u32,"GAUSSIAN_SPLATS":Pass::GaussianSplats as u32,"OVERLAY":Pass::Overlay as u32,"NUMBER_OF_PASSES":Pass::NUMBER_OF_PASSES}),
                )
            } else {
                None
            }
        },
    );
}
#[test]
fn sampler() {
    run(
        "renderer",
        "Sampler",
        |_| EXACT,
        |f, a| {
            if f != "values" {
                return None;
            }
            let v = &a[0];
            let mut s = Sampler::default();
            if let Some(n) = v["wrapR"].as_u64() {
                s.wrap_r = TextureWrap::from_gl(n as u32).unwrap();
            }
            if let Some(n) = v["wrapS"].as_u64() {
                s.wrap_s = TextureWrap::from_gl(n as u32).unwrap();
            }
            if let Some(n) = v["wrapT"].as_u64() {
                s.wrap_t = TextureWrap::from_gl(n as u32).unwrap();
            }
            if let Some(n) = v["minificationFilter"].as_u64() {
                s.minification_filter = TextureMinificationFilter::from_gl(n as u32).unwrap();
            }
            if let Some(n) = v["magnificationFilter"].as_u64() {
                s.magnification_filter = TextureMagnificationFilter::from_gl(n as u32).unwrap();
            }
            if let Some(n) = v["maximumAnisotropy"].as_f64() {
                s.maximum_anisotropy = n;
            }
            ok(json!([
                s.wrap_r as u32,
                s.wrap_s as u32,
                s.wrap_t as u32,
                s.minification_filter as u32,
                s.magnification_filter as u32,
                s.maximum_anisotropy,
                s == s.clone(),
                s == Sampler::NEAREST
            ]))
        },
    );
}
#[test]
fn shader_helpers() {
    run(
        "renderer",
        "ShaderFunction",
        |_| EXACT,
        |f, a| {
            if f == "lines" {
                let mut v = ShaderFunction::new(s(&a[0]));
                v.add_lines(&list(&a[1], |v| s(v).to_string()));
                ok(v.generate_glsl_lines())
            } else {
                None
            }
        },
    );
    run(
        "renderer",
        "ShaderStruct",
        |_| EXACT,
        |f, a| {
            if f == "lines" {
                let mut v = ShaderStruct::new(s(&a[0]));
                for p in a[1].as_array().unwrap() {
                    v.add_field(s(&p[0]), s(&p[1]));
                }
                ok(v.generate_glsl_lines())
            } else {
                None
            }
        },
    );
    run(
        "renderer",
        "ShaderDestination",
        |_| EXACT,
        |f, a| {
            if f == "flags" {
                let x = ShaderDestination(u(&a[0]));
                let y = ShaderDestination(u(&a[1]));
                ok(json!([
                    x.includes_vertex_shader(),
                    x.includes_fragment_shader(),
                    ShaderDestination::union(&[x, y]).0,
                    ShaderDestination::intersection(&[x, y]).unwrap().0
                ]))
            } else {
                None
            }
        },
    );
    run(
        "renderer",
        "ShaderSource",
        |_| EXACT,
        |f, a| match f {
            "replaceMain" => ok(ShaderSource::replace_main(s(&a[0]), s(&a[1]))),
            "combine" => {
                let src = ShaderSource { sources: list(&a[0], |v| s(v).into()), defines: list(&a[1], |v| s(v).into()) };
                ok(src
                    .combine_webgl2(
                        a[2].as_bool().unwrap(),
                        ShaderCapabilities {
                            texture_float_linear: a[3].as_bool().unwrap(),
                            floating_point_texture: a[3].as_bool().unwrap(),
                        },
                    )
                    .unwrap())
            }
            _ => None,
        },
    );
}
#[test]
fn render_state_defaults() {
    run(
        "renderer",
        "RenderState",
        |_| EXACT,
        |f, a| {
            if f == "defaults" {
                let s: RenderState = serde_json::from_value(a[0].clone()).unwrap();
                s.validate().unwrap();
                let mut value = serde_json::to_value(&s).unwrap();
                let color = &s.blending.color;
                value["blending"]["color"] = json!([color.red, color.green, color.blue, color.alpha]);
                let rect = |v: Viewport| json!([v.x, v.y, v.width, v.height]);
                value["scissorTest"]["rectangle"] = rect(s.scissor_test.rectangle);
                value["viewport"] = s.viewport.map(rect).unwrap_or(Value::Null);
                ok(value)
            } else {
                None
            }
        },
    );
}
