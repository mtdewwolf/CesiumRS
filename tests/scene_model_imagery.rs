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
    let cases = load("scene", "ModelImagery");
    let geo_scheme = GeographicTilingScheme::default();
    let merc_scheme = WebMercatorTilingScheme::default();

    for case in cases {
        match case.f.as_str() {
            "coverage_clamp_rectangle" => {
                let input = rect_from_val(&case.a[0]);
                let clamp = rect_from_val(&case.a[1]);
                let res = ImageryCoverage::clamp_rectangle(&input, &clamp);
                let actual = json!([num(res.west), num(res.south), num(res.east), num(res.north)]);
                assert!(
                    close(&actual, &case.r, TIGHT),
                    "coverage_clamp_rectangle mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "coverage_localize_rectangle" => {
                let ra = rect_from_val(&case.a[0]);
                let rb = rect_from_val(&case.a[1]);
                let res = ImageryCoverage::localize_to_cartesian_rectangle(&ra, &rb);
                let actual = json!([num(res.min_x), num(res.min_y), num(res.max_x), num(res.max_y)]);
                assert!(
                    close(&actual, &case.r, TIGHT),
                    "coverage_localize_rectangle mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "coverage_compute_overlapped_rectangle" => {
                let input = rect_from_val(&case.a[0]);
                let bounds = rect_from_val(&case.a[1]);
                let res = ImageryCoverage::compute_overlapped_rectangle(&input, &bounds);
                let actual = json!([num(res.west), num(res.south), num(res.east), num(res.north)]);
                assert!(
                    close(&actual, &case.r, TIGHT),
                    "coverage_compute_overlapped_rectangle mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "coverage_compute_imagery_range" => {
                let input = rect_from_val(&case.a[0]);
                let bounds = rect_from_val(&case.a[1]);
                let scheme_str = case.a[2].as_str().unwrap();
                let level = case.a[3].as_i64().unwrap() as i32;
                let ts: &dyn TilingScheme = if scheme_str == "geographic" { &geo_scheme } else { &merc_scheme };
                let res = ImageryCoverage::compute_imagery_range(&input, &bounds, ts, level);
                let actual = json!([num(res.min_x), num(res.min_y), num(res.max_x), num(res.max_y)]);
                assert!(
                    close(&actual, &case.r, TIGHT),
                    "coverage_compute_imagery_range mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "mapping_bounding_rectangle" => {
                let pts: Vec<Cartographic> = case.a[0].as_array().unwrap().iter().map(carto_from_val).collect();
                let res = ModelImageryMapping::compute_cartographic_bounding_rectangle(&pts);
                let actual = json!([num(res.west), num(res.south), num(res.east), num(res.north)]);
                assert!(
                    close(&actual, &case.r, TIGHT),
                    "mapping_bounding_rectangle mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "mapping_compute_tex_coords" => {
                let positions: Vec<Cartesian3> = case.a[0].as_array().unwrap().iter().map(c3_from_val).collect();
                let br = brect_from_val(&case.a[1]);
                let res = ModelImageryMapping::compute_tex_coords(&positions, &br);
                let actual: Vec<Value> = res.iter().map(|uv| json!([num(uv.x), num(uv.y)])).collect();
                assert!(
                    close(&json!(actual), &case.r, TIGHT),
                    "mapping_compute_tex_coords mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "mapping_create_typed_array" => {
                let elements: Vec<Cartesian2> = case.a[0].as_array().unwrap().iter().map(c2_from_val).collect();
                let res = ModelImageryMapping::create_typed_array_from_cartesians2(&elements);
                let actual: Vec<Value> = res.iter().map(|v| num(*v as f64)).collect();
                assert!(
                    close(&json!(actual), &case.r, TIGHT),
                    "mapping_create_typed_array mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "pipeline_translation_and_scale" => {
                let nb = rect_from_val(&case.a[0]);
                let ni = rect_from_val(&case.a[1]);
                let res = ImageryPipelineStage::compute_texture_translation_and_scale_from_native(&nb, &ni);
                let actual = json!([num(res.x), num(res.y), num(res.z), num(res.w)]);
                assert!(
                    close(&actual, &case.r, TIGHT),
                    "pipeline_translation_and_scale mismatch: {actual:?} vs {:?}",
                    case.r
                );
            }
            "pipeline_imagery_flags" => {
                let flags = flags_from_cfg(&case.a[0]);
                let actual = json!({
                    "alpha": flags.alpha,
                    "brightness": flags.brightness,
                    "contrast": flags.contrast,
                    "hue": flags.hue,
                    "saturation": flags.saturation,
                    "gamma": flags.gamma,
                    "colorToAlpha": flags.color_to_alpha,
                });
                assert_eq!(actual, case.r, "pipeline_imagery_flags mismatch");
            }
            "pipeline_shader_generation" => {
                let flags = flags_from_cfg(&case.a[0]);
                let mut sb = ShaderBuilder::default();
                sb.add_define("IMAGERY_TEXTURE_UNITS 2", None, ShaderDestination::BOTH);
                ImageryPipelineStage::define_uniforms(&mut sb, &flags);
                ImageryPipelineStage::build_sample_and_blend_function(&mut sb, &flags).unwrap();
                ImageryPipelineStage::create_main_imagery_shader_from_indices(&mut sb, &[0, 1], &flags).unwrap();

                let (_, fs_parts) = sb.sources();
                let all_lines: Vec<&str> = fs_parts.sources[0].lines().collect();

                let actual_uniforms: Vec<&str> =
                    all_lines.iter().copied().filter(|l| l.starts_with("uniform ")).collect();

                let expected_uniforms = case.r["uniforms"].as_array().unwrap();
                let expected_sample_and_blend = case.r["sampleAndBlend"].as_array().unwrap();
                let expected_main = case.r["blendBaseColorWithImagery"].as_array().unwrap();

                assert_eq!(json!(actual_uniforms), json!(expected_uniforms), "uniforms mismatch");

                let sab_glsl = extract_function(&all_lines, "sampleAndBlend");
                assert_eq!(json!(sab_glsl), json!(expected_sample_and_blend), "sampleAndBlend mismatch");

                let main_glsl = extract_function(&all_lines, "blendBaseColorWithImagery");
                assert_eq!(json!(main_glsl), json!(expected_main), "blendBaseColorWithImagery mismatch");
            }
            other => panic!("Unknown case function: {other}"),
        }
    }
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
