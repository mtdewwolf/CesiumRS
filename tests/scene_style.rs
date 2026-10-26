mod common;
use cesium_rs::{
    core::*,
    scene::{
        Cesium3DTileStyle, ConditionsExpression, Expression, ShaderState, StyleExpression, StyleFeature, StyleInput,
        StyleValue,
    },
};
use common::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

struct TestFeature<'a> {
    props: &'a Value,
    class_name: &'a str,
}

impl<'a> StyleFeature for TestFeature<'a> {
    fn property_inherited(&self, name: &str) -> Option<StyleValue> {
        let val = self.props.get(name)?;
        Some(StyleValue::from_json(val))
    }
    fn is_exact_class(&self, class_name: &str) -> bool {
        self.class_name == class_name
    }
    fn is_class(&self, class_name: &str) -> bool {
        self.class_name == class_name
    }
    fn exact_class_name(&self) -> Option<String> {
        Some(self.class_name.to_string())
    }
    fn tileset_time(&self) -> f64 {
        42.5
    }
}

fn enc_style_value(val: &StyleValue) -> Value {
    match val {
        StyleValue::Undefined | StyleValue::Null => Value::Null,
        StyleValue::Boolean(b) => json!(b),
        StyleValue::Number(n) => num(*n),
        StyleValue::String(s) => json!(s),
        StyleValue::Array(a) => Value::Array(a.iter().map(enc_style_value).collect()),
        StyleValue::Cartesian2(v) => json!([num(v.x), num(v.y)]),
        StyleValue::Cartesian3(v) => json!([num(v.x), num(v.y), num(v.z)]),
        StyleValue::Cartesian4(v) => json!([num(v.x), num(v.y), num(v.z), num(v.w)]),
        StyleValue::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), enc_style_value(v))).collect()),
        StyleValue::RegExp(r) => json!(r.borrow().to_string()),
    }
}

fn enc_color(c: &Color) -> Value {
    json!([num(c.red), num(c.green), num(c.blue), num(c.alpha)])
}

fn shader_map() -> BTreeMap<String, String> {
    [
        ("Height", "height"),
        ("id", "id"),
        ("Area", "area"),
        ("Temperature", "temperature"),
        ("red", "red"),
        ("green", "green"),
        ("blue", "blue"),
        ("name", "name"),
        ("a", "a"),
        ("b", "b"),
        ("x", "x"),
        ("y", "y"),
        ("z", "z"),
        ("County", "county"),
        ("YearBuilt", "yearBuilt"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[test]
fn reference_expression() {
    let sm = shader_map();
    run(
        "scene",
        "Expression",
        |_| TIGHT,
        |f, a| match f {
            "arrayCoercion" => Some(match Expression::new(s(&a[0]), None).and_then(|p| p.evaluate(None)) {
                Ok(value) => Out::Ok(enc_style_value(&value)),
                Err(error) => Out::Err(error.to_string()),
            }),
            "evaluate" => {
                let src = a[0].as_str().unwrap();
                let props_list = a[1].as_array().unwrap();
                let parsed = match Expression::new(src, None) {
                    Ok(p) => p,
                    Err(e) => return Some(Out::Err(e.to_string())),
                };
                let variables = parsed.get_variables();
                let mut evaluations = Vec::new();
                for props in props_list {
                    let feat = TestFeature { props, class_name: "door" };
                    let val = match parsed.evaluate(Some(&feat)) {
                        Ok(v) => enc_style_value(&v),
                        Err(e) => return Some(Out::Err(e.to_string())),
                    };
                    evaluations.push(val);
                }
                let mut state = ShaderState::default();
                let shader = match parsed.get_shader_expression(&sm, &mut state) {
                    Ok(expr) => json!({
                        "expression": expr,
                        "translucent": state.translucent,
                    }),
                    Err(e) => json!({
                        "throws": "RuntimeError",
                        "message": e.message,
                        "translucent": state.translucent,
                    }),
                };
                Some(Out::Ok(json!({
                    "variables": variables,
                    "evaluations": evaluations,
                    "shader": shader,
                })))
            }
            "evaluateColor" => {
                let src = a[0].as_str().unwrap();
                let props_list = a[1].as_array().unwrap();
                let parsed = match Expression::new(src, None) {
                    Ok(p) => p,
                    Err(e) => return Some(Out::Err(e.to_string())),
                };
                let mut evaluations = Vec::new();
                for props in props_list {
                    let feat = TestFeature { props, class_name: "door" };
                    let c = match parsed.evaluate_color(Some(&feat)) {
                        Ok(Some(c)) => enc_color(&c),
                        Ok(None) => Value::Null,
                        Err(e) => return Some(Out::Err(e.to_string())),
                    };
                    evaluations.push(c);
                }
                let mut state = ShaderState::default();
                let shader_function =
                    match parsed.get_shader_function("vec4 getColor(float x)", &sm, &mut state, "vec4") {
                        Ok(f) => f,
                        Err(e) => return Some(Out::Err(e.to_string())),
                    };
                Some(Out::Ok(json!({
                    "evaluations": evaluations,
                    "shaderFunction": shader_function,
                    "translucent": state.translucent,
                })))
            }
            "defines" => {
                let src = a[0].as_str().unwrap();
                let defines_obj = a[1].as_object().unwrap();
                let defines: BTreeMap<String, String> =
                    defines_obj.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
                let props_list = a[2].as_array().unwrap();
                let parsed = match Expression::new(src, Some(&defines)) {
                    Ok(p) => p,
                    Err(e) => return Some(Out::Err(e.to_string())),
                };
                let variables = parsed.get_variables();
                let mut evaluations = Vec::new();
                for props in props_list {
                    let feat = TestFeature { props, class_name: "door" };
                    let val = match parsed.evaluate(Some(&feat)) {
                        Ok(v) => enc_style_value(&v),
                        Err(e) => return Some(Out::Err(e.to_string())),
                    };
                    evaluations.push(val);
                }
                let mut state = ShaderState::default();
                let shader = parsed.get_shader_expression(&sm, &mut state).ok().flatten();
                Some(Out::Ok(json!({
                    "variables": variables,
                    "evaluations": evaluations,
                    "shader": shader,
                })))
            }
            "errors" => {
                let src = a[0].as_str().unwrap();
                res((|| -> Result<Value, RuntimeError> {
                    let parsed = Expression::new(src, None)?;
                    let feat = TestFeature {
                        props: &json!({"Height": 12, "id": "alpha", "Area": 80, "Temperature": 95, "name": "Chest", "a": 1.5, "b": 2.5}),
                        class_name: "door",
                    };
                    let val = parsed.evaluate(Some(&feat))?;
                    let mut state = ShaderState::default();
                    let shader = parsed.get_shader_expression(&sm, &mut state).ok().flatten();
                    Ok(json!({
                        "value": enc_style_value(&val),
                        "shader": shader,
                    }))
                })())
            }
            "variables" => {
                let src = a[0].as_str().unwrap();
                let parsed = match Expression::new(src, None) {
                    Ok(p) => p,
                    Err(e) => return Some(Out::Err(e.to_string())),
                };
                let variables = parsed.get_variables();
                let mut partial_map = BTreeMap::new();
                partial_map.insert("Height".into(), "height".into());
                partial_map.insert("a".into(), "a".into());
                partial_map.insert("red".into(), "r".into());
                partial_map.insert("green".into(), "g".into());
                partial_map.insert("blue".into(), "b".into());
                let mut state = ShaderState::default();
                let shader = parsed.get_shader_expression(&partial_map, &mut state).ok().flatten();
                Some(Out::Ok(json!({
                    "variables": variables,
                    "shader": shader,
                })))
            }
            _ => None,
        },
    );
}

#[test]
fn reference_conditions_expression() {
    let sm = shader_map();
    run(
        "scene",
        "ConditionsExpression",
        |_| TIGHT,
        |f, a| match f {
            "evaluate" => {
                let conditions_json = &a[0];
                let props_list = a[1].as_array().unwrap();
                let parsed = match ConditionsExpression::from_json(conditions_json, None) {
                    Ok(p) => p,
                    Err(e) => return Some(Out::Err(e.to_string())),
                };
                let variables = parsed.get_variables();
                let mut evaluations = Vec::new();
                for props in props_list {
                    let feat = TestFeature { props, class_name: "door" };
                    let val = match parsed.evaluate(Some(&feat)) {
                        Ok(v) => enc_style_value(&v),
                        Err(e) => return Some(Out::Err(e.to_string())),
                    };
                    evaluations.push(val);
                }
                let colors = {
                    let mut colors = Vec::new();
                    let mut err = None;
                    for props in props_list {
                        let feat = TestFeature { props, class_name: "door" };
                        match parsed.evaluate_color(Some(&feat)) {
                            Ok(Some(c)) => colors.push(enc_color(&c)),
                            Ok(None) => colors.push(Value::Null),
                            Err(e) => {
                                err = Some(e.to_string());
                                break;
                            }
                        }
                    }
                    match err {
                        Some(e) => return Some(Out::Err(e)),
                        None => json!(colors),
                    }
                };
                let mut state = ShaderState::default();
                let shader_function =
                    match parsed.get_shader_function("vec4 getColor(float x)", &sm, &mut state, "vec4") {
                        Ok(f) => f,
                        Err(e) => return Some(Out::Err(e.to_string())),
                    };
                Some(Out::Ok(json!({
                    "variables": variables,
                    "evaluations": evaluations,
                    "colors": colors,
                    "shaderFunction": shader_function,
                    "translucent": state.translucent,
                })))
            }
            "shader" => {
                let conditions_json = &a[0];
                let parsed = match ConditionsExpression::from_json(conditions_json, None) {
                    Ok(p) => p,
                    Err(e) => return Some(Out::Err(e.to_string())),
                };
                let mut state = ShaderState::default();
                let shader_function =
                    match parsed.get_shader_function("vec4 getColor(float x)", &sm, &mut state, "vec4") {
                        Ok(f) => f,
                        Err(e) => return Some(Out::Err(e.to_string())),
                    };
                Some(Out::Ok(json!({
                    "shaderFunction": shader_function,
                    "translucent": state.translucent,
                })))
            }
            "errors" => {
                let conditions_json = &a[0];
                let feat = TestFeature {
                    props: &json!({"Height": 12, "id": "alpha", "Area": 80, "Temperature": 95, "name": "Chest", "a": 1.5, "b": 2.5}),
                    class_name: "door",
                };
                res((|| -> Result<Value, RuntimeError> {
                    let parsed = ConditionsExpression::from_json(conditions_json, None)?;
                    let val = parsed.evaluate(Some(&feat))?;
                    Ok(json!({ "result": enc_style_value(&val) }))
                })())
            }
            _ => None,
        },
    );
}

#[test]
fn reference_tile_style() {
    let sm = shader_map();
    run(
        "scene",
        "Cesium3DTileStyle",
        |_| TIGHT,
        |f, a| match f {
            "style" => {
                let style_json = &a[0];
                let props_list = a[1].as_array().unwrap();
                res((|| -> Result<Value, RuntimeError> {
                    let mut style = Cesium3DTileStyle::new(Some(style_json))?;
                    let variables = style.get_variables();
                    let mut color_state = ShaderState::default();
                    let mut show_state = ShaderState::default();
                    let mut point_size_state = ShaderState::default();
                    let color_shader =
                        style.get_color_shader_function("vec4 getColor(float x)", &sm, &mut color_state)?;
                    let show_shader = style.get_show_shader_function("bool getShow(float x)", &sm, &mut show_state)?;
                    let point_size_shader = style.get_point_size_shader_function(
                        "float getPointSize(float x)",
                        &sm,
                        &mut point_size_state,
                    )?;

                    let feat0 = TestFeature { props: &props_list[0], class_name: "door" };
                    let fields = json!({
                        "pointOutlineColor": style.point_outline_color().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "pointOutlineWidth": style.point_outline_width().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "labelColor": style.label_color().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "labelOutlineColor": style.label_outline_color().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "labelOutlineWidth": style.label_outline_width().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "labelStyle": style.label_style().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "font": style.font().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "labelText": style.label_text().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "backgroundColor": style.background_color().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "backgroundPadding": style.background_padding().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "backgroundEnabled": style.background_enabled().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "scaleByDistance": style.scale_by_distance().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "translucencyByDistance": style.translucency_by_distance().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "distanceDisplayCondition": style.distance_display_condition().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "heightOffset": style.height_offset().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "anchorLineEnabled": style.anchor_line_enabled().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "anchorLineColor": style.anchor_line_color().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "image": style.image().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "disableDepthTestDistance": style.disable_depth_test_distance().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "horizontalOrigin": style.horizontal_origin().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "verticalOrigin": style.vertical_origin().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "labelHorizontalOrigin": style.label_horizontal_origin().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                        "labelVerticalOrigin": style.label_vertical_origin().and_then(|e| e.evaluate(Some(&feat0)).ok()).map(|v| enc_style_value(&v)),
                    });

                    let mut show_evals = Vec::new();
                    for props in props_list {
                        let feat = TestFeature { props, class_name: "door" };
                        let val = match style.show() {
                            Some(e) => enc_style_value(&e.evaluate(Some(&feat))?),
                            None => Value::Null,
                        };
                        show_evals.push(val);
                    }
                    let mut color_evals = Vec::new();
                    for props in props_list {
                        let feat = TestFeature { props, class_name: "door" };
                        let val = match style.color() {
                            Some(e) => match e.evaluate_color(Some(&feat))? {
                                Some(c) => enc_color(&c),
                                None => Value::Null,
                            },
                            None => Value::Null,
                        };
                        color_evals.push(val);
                    }
                    let mut point_size_evals = Vec::new();
                    for props in props_list {
                        let feat = TestFeature { props, class_name: "door" };
                        let val = match style.point_size() {
                            Some(e) => enc_style_value(&e.evaluate(Some(&feat))?),
                            None => Value::Null,
                        };
                        point_size_evals.push(val);
                    }
                    let mut meta_evals = serde_json::Map::new();
                    for (k, expr) in style.meta() {
                        let mut evals = Vec::new();
                        for props in props_list {
                            let feat = TestFeature { props, class_name: "door" };
                            evals.push(enc_style_value(&expr.evaluate(Some(&feat))?));
                        }
                        meta_evals.insert(k.clone(), Value::Array(evals));
                    }

                    Ok(json!({
                        "style": style.style(),
                        "variables": variables,
                        "colorShader": color_shader,
                        "showShader": show_shader,
                        "pointSizeShader": point_size_shader,
                        "translucent": [color_state.translucent, show_state.translucent, point_size_state.translucent],
                        "evaluations": {
                            "show": show_evals,
                            "color": color_evals,
                            "pointSize": point_size_evals,
                            "meta": meta_evals,
                        },
                        "fields": fields,
                    }))
                })())
            }
            "setters" => {
                let props_list = a[0].as_array().unwrap();
                let mut style = Cesium3DTileStyle::new(None).unwrap();
                style.set_show(Some(StyleInput::Boolean(true))).unwrap();
                style.set_color(Some(StyleInput::String("color('red')".into()))).unwrap();
                style.set_point_size(Some(StyleInput::Number(4.0))).unwrap();

                let feat0 = TestFeature { props: &props_list[0], class_name: "door" };
                let before = json!({
                    "style": style.style().clone(),
                    "show": enc_style_value(&style.show().unwrap().evaluate(Some(&feat0)).unwrap()),
                    "color": enc_color(&style.color().unwrap().evaluate_color(Some(&feat0)).unwrap().unwrap()),
                    "pointSize": enc_style_value(&style.point_size().unwrap().evaluate(Some(&feat0)).unwrap()),
                });

                let mut state = ShaderState::default();
                let shader = style.get_color_shader_function("vec4 f(float x)", &sm, &mut state).unwrap();
                style
                    .set_color(Some(StyleInput::Conditions(vec![("true".into(), "color('blue', 0.25)".into())])))
                    .unwrap();
                let mut state2 = ShaderState::default();
                let after = json!({
                    "style": style.style().clone(),
                    "shader": style.get_color_shader_function("vec4 f(float x)", &sm, &mut state2).unwrap(),
                    "color": enc_color(&style.color().unwrap().evaluate_color(Some(&feat0)).unwrap().unwrap()),
                });

                Some(Out::Ok(json!({
                    "before": before,
                    "shader": shader,
                    "after": after,
                })))
            }
            "errors" => {
                let style_json = &a[0];
                res((|| -> Result<Value, RuntimeError> {
                    Cesium3DTileStyle::new(Some(style_json))?;
                    Ok(json!({ "ok": true }))
                })())
            }
            _ => None,
        },
    );
}
