mod common;

use cesium_rs::{
    core::*,
    renderer::*,
    scene::model::{
        imagery_pipeline_stage::ImageryPipelineStage, ImageryCoverage, ImageryFlags, ModelImagery, ModelImageryMapping,
        ModelPrimitiveImagery,
    },
};
use common::*;
use serde_json::{json, Value};

fn rect_from_val(v: &Value) -> Rectangle {
    let arr = v.as_array().expect("rectangle array");
    Rectangle::new(n(&arr[0]), n(&arr[1]), n(&arr[2]), n(&arr[3]))
}

fn carto_from_val(v: &Value) -> Cartographic {
    let arr = v.as_array().expect("cartographic array");
    Cartographic::new(n(&arr[0]), n(&arr[1]), n(&arr[2]))
}

fn c2_from_val(v: &Value) -> Cartesian2 {
    let arr = v.as_array().expect("cartesian2 array");
    Cartesian2::new(n(&arr[0]), n(&arr[1]))
}

fn c3_from_val(v: &Value) -> Cartesian3 {
    let arr = v.as_array().expect("cartesian3 array");
    Cartesian3::new(n(&arr[0]), n(&arr[1]), n(&arr[2]))
}

fn brect_from_val(v: &Value) -> BoundingRectangle {
    let arr = v.as_array().expect("bounding rectangle array");
    BoundingRectangle::new(n(&arr[0]), n(&arr[1]), n(&arr[2]), n(&arr[3]))
}

fn flags_from_cfg(cfg: &Value) -> ImageryFlags {
    let mut f = ImageryFlags::default();
    if let Some(alpha) = cfg.get("alpha").and_then(Value::as_f64) {
        if alpha != 1.0 {
            f.alpha = true;
        }
    }
    if let Some(brightness) = cfg.get("brightness").and_then(Value::as_f64) {
        if brightness != 1.0 {
            f.brightness = true;
        }
    }
    if let Some(contrast) = cfg.get("contrast").and_then(Value::as_f64) {
        if contrast != 1.0 {
            f.contrast = true;
        }
    }
    if let Some(hue) = cfg.get("hue").and_then(Value::as_f64) {
        if hue != 0.0 {
            f.hue = true;
        }
    }
    if let Some(sat) = cfg.get("saturation").and_then(Value::as_f64) {
        if sat != 1.0 {
            f.saturation = true;
        }
    }
    if let Some(gamma) = cfg.get("gamma").and_then(Value::as_f64) {
        if gamma != 1.0 {
            f.gamma = true;
        }
    }
    if cfg.get("colorToAlpha").is_some() {
        let thresh = cfg.get("threshold").and_then(Value::as_f64).unwrap_or(0.0);
        if thresh > 0.0 {
            f.color_to_alpha = true;
        }
    }
    f
}

fn extract_function<'a>(lines: &[&'a str], name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut capturing = false;
    let mut brace_depth = 0;
    for &line in lines {
        if !capturing {
            if line.contains(name) && line.contains('(') {
                capturing = true;
                out.push(line);
            }
        } else {
            out.push(line);
            for ch in line.chars() {
                if ch == '{' {
                    brace_depth += 1;
                } else if ch == '}' {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        return out;
                    }
                }
            }
        }
    }
    out
}

#[derive(Clone, Default)]
struct DummyGl;

impl Gl for DummyGl {
    type Object = u32;

    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }

    fn invoke(&self, _name: &str, _args: &[GlValue<Self::Object>]) -> Result<GlValue<Self::Object>, RuntimeError> {
        Ok(GlValue::Null)
    }
}

#[test]
fn parity_model_imagery() {
    let geo_scheme = GeographicTilingScheme::default();
    let merc_scheme = WebMercatorTilingScheme::default();
    run(
        "scene",
        "ModelImagery",
        |_| TIGHT,
        |f, a| {
            let actual = match f {
                "coverage_clamp_rectangle" => {
                    let res = ImageryCoverage::clamp_rectangle(&rect_from_val(&a[0]), &rect_from_val(&a[1]));
                    json!([num(res.west), num(res.south), num(res.east), num(res.north)])
                }
                "coverage_localize_rectangle" => {
                    let res =
                        ImageryCoverage::localize_to_cartesian_rectangle(&rect_from_val(&a[0]), &rect_from_val(&a[1]));
                    json!([num(res.min_x), num(res.min_y), num(res.max_x), num(res.max_y)])
                }
                "coverage_compute_overlapped_rectangle" => {
                    let res =
                        ImageryCoverage::compute_overlapped_rectangle(&rect_from_val(&a[0]), &rect_from_val(&a[1]));
                    json!([num(res.west), num(res.south), num(res.east), num(res.north)])
                }
                "coverage_compute_imagery_range" => {
                    let ts: &dyn TilingScheme = if s(&a[2]) == "geographic" { &geo_scheme } else { &merc_scheme };
                    let res = ImageryCoverage::compute_imagery_range(
                        &rect_from_val(&a[0]),
                        &rect_from_val(&a[1]),
                        ts,
                        a[3].as_i64().unwrap() as i32,
                    );
                    json!([num(res.min_x), num(res.min_y), num(res.max_x), num(res.max_y)])
                }
                "mapping_bounding_rectangle" => {
                    let pts: Vec<Cartographic> = a[0].as_array().unwrap().iter().map(carto_from_val).collect();
                    let res = ModelImageryMapping::compute_cartographic_bounding_rectangle(&pts);
                    json!([num(res.west), num(res.south), num(res.east), num(res.north)])
                }
                "mapping_compute_tex_coords" => {
                    let positions: Vec<Cartesian3> = a[0].as_array().unwrap().iter().map(c3_from_val).collect();
                    let res = ModelImageryMapping::compute_tex_coords(&positions, &brect_from_val(&a[1]));
                    json!(res.iter().map(|uv| json!([num(uv.x), num(uv.y)])).collect::<Vec<_>>())
                }
                "mapping_create_typed_array" => {
                    let elements: Vec<Cartesian2> = a[0].as_array().unwrap().iter().map(c2_from_val).collect();
                    let res = ModelImageryMapping::create_typed_array_from_cartesians2(&elements);
                    json!(res.iter().map(|v| num(*v as f64)).collect::<Vec<_>>())
                }
                "pipeline_translation_and_scale" => {
                    let res = ImageryPipelineStage::compute_texture_translation_and_scale_from_native(
                        &rect_from_val(&a[0]),
                        &rect_from_val(&a[1]),
                    );
                    json!([num(res.x), num(res.y), num(res.z), num(res.w)])
                }
                "pipeline_imagery_flags" => {
                    let flags = flags_from_cfg(&a[0]);
                    json!({
                        "alpha": flags.alpha, "brightness": flags.brightness,
                        "contrast": flags.contrast, "hue": flags.hue,
                        "saturation": flags.saturation, "gamma": flags.gamma,
                        "colorToAlpha": flags.color_to_alpha,
                    })
                }
                "pipeline_shader_generation" => {
                    let flags = flags_from_cfg(&a[0]);
                    let mut sb = ShaderBuilder::default();
                    sb.add_define("IMAGERY_TEXTURE_UNITS 2", None, ShaderDestination::BOTH);
                    ImageryPipelineStage::define_uniforms(&mut sb, &flags);
                    ImageryPipelineStage::build_sample_and_blend_function(&mut sb, &flags).unwrap();
                    ImageryPipelineStage::create_main_imagery_shader_from_indices(&mut sb, &[0, 1], &flags).unwrap();
                    let (_, fs_parts) = sb.sources();
                    let lines: Vec<&str> = fs_parts.sources[0].lines().collect();
                    json!({
                        "uniforms": lines.iter().copied().filter(|l| l.starts_with("uniform ")).collect::<Vec<_>>(),
                        "sampleAndBlend": extract_function(&lines, "sampleAndBlend"),
                        "blendBaseColorWithImagery": extract_function(&lines, "blendBaseColorWithImagery"),
                    })
                }
                _ => return None,
            };
            Some(Out::Ok(actual))
        },
    );
}

#[test]
fn native_imagery_lifecycle_and_components() {
    let mut model_imagery = ModelImagery::<DummyGl>::default();
    assert!(model_imagery.is_ready());
    assert_eq!(model_imagery.primitive_imageries().len(), 0);

    let prim_imagery = ModelPrimitiveImagery::<DummyGl>::default();
    assert!(prim_imagery.is_ready());
    assert!(prim_imagery.coverages_per_layer.is_empty());

    model_imagery.model_primitive_imageries.push(prim_imagery);
    assert_eq!(model_imagery.primitive_imageries().len(), 1);

    // If a primitive is not ready, model imagery is not ready
    model_imagery.primitive_imageries_mut()[0].ready = false;
    assert!(!model_imagery.is_ready());

    model_imagery.primitive_imageries_mut()[0].ready = true;
    assert!(model_imagery.is_ready());
}
