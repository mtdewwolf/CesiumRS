//! CesiumJS `moveTechniqueRenderStates`: turns glTF 1.0 technique render
//! states into material `alphaMode`, `doubleSided` and `KHR_blend`.

use super::for_each::{self, Collection};
use super::{add_extensions_used, js, Result};
use crate::core::webgl_constants as gl;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const SUPPORTED_BLEND_FACTORS: [u32; 10] = [
    gl::ZERO,
    gl::ONE,
    gl::SRC_COLOR,
    gl::ONE_MINUS_SRC_COLOR,
    gl::SRC_ALPHA,
    gl::ONE_MINUS_SRC_ALPHA,
    gl::DST_ALPHA,
    gl::ONE_MINUS_DST_ALPHA,
    gl::DST_COLOR,
    gl::ONE_MINUS_DST_COLOR,
];

/// `renderStates.enable.indexOf(state) > -1`.
pub(crate) fn is_state_enabled(render_states: &Value, state: u32) -> bool {
    match js::get(Some(render_states), "enable") {
        Some(Value::Array(enabled)) => js::index_of(enabled, Some(&json!(state))).is_some(),
        // A string's `indexOf` looks for the number's text.
        Some(Value::String(enabled)) => enabled.contains(&state.to_string()),
        _ => false,
    }
}

/// The factors as given when all four are supported, else the default.
fn supported_blend_factors(value: Option<&Value>) -> Value {
    let default = json!([gl::ONE, gl::ZERO, gl::ONE, gl::ZERO]);
    let Some(value) = value.filter(|v| !v.is_null()) else { return default };
    let supported = (0..4).all(|i| {
        let factor = js::get(Some(value), &i.to_string());
        SUPPORTED_BLEND_FACTORS.iter().any(|f| js::strict_eq(factor, Some(&json!(*f))))
    });
    if supported {
        value.clone()
    } else {
        default
    }
}

pub fn move_technique_render_states(gltf: &mut Value) -> Result<()> {
    if !js::defined(gltf.get("techniques")) {
        return Ok(());
    }
    // Keyed by the technique's property key, as Cesium's objects are.
    let mut blending: HashMap<String, Value> = HashMap::new();
    let mut properties: HashMap<String, Map<String, Value>> = HashMap::new();
    let techniques = Collection::Techniques("techniques");
    for_each::each_mut(gltf, techniques, |technique, id| {
        let Some(render_states) = js::get(Some(&*technique), "states").filter(|s| !s.is_null()).cloned() else {
            return Ok(());
        };
        let material_properties = properties.entry(id.key()).or_default();
        if is_state_enabled(&render_states, gl::BLEND) {
            material_properties.insert("alphaMode".into(), json!("BLEND"));
            let functions = js::get(Some(&render_states), "functions");
            let equation = js::get(functions, "blendEquationSeparate");
            let factors = js::get(functions, "blendFuncSeparate");
            if js::defined(functions) && (js::defined(equation) || js::defined(factors)) {
                let blend_equation =
                    equation.filter(|e| !e.is_null()).cloned().unwrap_or(json!([gl::FUNC_ADD, gl::FUNC_ADD]));
                blending.insert(
                    id.key(),
                    json!({ "blendEquation": blend_equation, "blendFactors": supported_blend_factors(factors) }),
                );
            }
        }
        if !is_state_enabled(&render_states, gl::CULL_FACE) {
            material_properties.insert("doubleSided".into(), json!(true));
        }
        js::delete(technique, "states");
        Ok(())
    })?;
    if !blending.is_empty() {
        if !js::defined(gltf.get("extensions")) {
            js::set(gltf, "extensions", Value::Object(Map::new()))?;
        }
        add_extensions_used(gltf, "KHR_blend")?;
    }
    for_each::each_mut(gltf, Collection::TopLevel("materials"), |material, _| {
        let Some(technique) = js::get(Some(&*material), "technique").filter(|t| !t.is_null()) else { return Ok(()) };
        let technique = js::key(technique);
        if let Some(material_properties) = properties.get(&technique) {
            for (property, value) in material_properties {
                js::set(material, property, value.clone())?;
            }
        }
        if let Some(blend) = blending.get(&technique) {
            if !js::defined(material.get("extensions")) {
                js::set(material, "extensions", Value::Object(Map::new()))?;
            }
            let extensions = js::object_mut(material.get_mut("extensions"), "KHR_blend")?;
            js::set(extensions, "KHR_blend", blend.clone())?;
        }
        Ok(())
    })
}
