//! CesiumJS `moveTechniquesToExtension`: moves glTF 1.0 techniques, programs
//! and shaders into `KHR_techniques_webgl`.

use super::for_each::{self, Collection};
use super::{add_extensions_required, add_extensions_used, js, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// A shader as `addToArray`'s duplicate check sees it. Cesium compares the
/// shader objects by identity, which is the id they were looked up by; a
/// missing or primitive shader compares by value.
enum ShaderIdentity {
    Object(String),
    Primitive(Option<Value>),
}

impl ShaderIdentity {
    fn of(shaders: Option<&Value>, id: Option<&Value>) -> (Self, Option<Value>) {
        let key = id.map_or_else(|| "undefined".to_string(), js::key);
        let shader = js::get(shaders, &key).cloned();
        match &shader {
            Some(Value::Object(_) | Value::Array(_)) => (Self::Object(key), shader),
            _ => (Self::Primitive(shader.clone()), shader),
        }
    }

    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::Primitive(a), Self::Primitive(b)) => js::strict_eq(a.as_ref(), b.as_ref()),
            _ => false,
        }
    }
}

/// Inserts `value` under `key` unless it is `undefined`, which
/// `JSON.stringify` would drop.
fn insert_defined(map: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        map.insert(key.to_string(), value.clone());
    }
}

pub fn move_techniques_to_extension(gltf: &mut Value) -> Result<()> {
    let mut mapped_uniforms: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut updated_technique_indices: HashMap<String, usize> = HashMap::new();
    if js::defined(gltf.get("techniques")) {
        let mut programs: Vec<Value> = Vec::new();
        let mut shaders: Vec<(ShaderIdentity, Option<Value>)> = Vec::new();
        let mut techniques: Vec<Value> = Vec::new();
        let mut seen_programs: HashMap<String, usize> = HashMap::new();
        let gl_extensions = gltf.get("glExtensionsUsed").cloned();
        js::delete(gltf, "glExtensionsUsed");
        let snapshot = gltf.clone();
        let mut add_shader = |id: Option<&Value>| {
            let (identity, shader) = ShaderIdentity::of(snapshot.get("shaders"), id);
            match shaders.iter().position(|(seen, _)| seen.same(&identity)) {
                Some(index) => index,
                None => {
                    shaders.push((identity, shader));
                    shaders.len() - 1
                }
            }
        };
        let mut error = None;
        for_each::collection::<()>(
            &snapshot,
            Collection::Techniques("techniques"),
            |technique_legacy, technique_id| {
                let result = (|| -> Result<()> {
                    let parameters = js::get(Some(technique_legacy), "parameters");
                    let mut attributes = Map::new();
                    for name in js::for_in(js::get(Some(technique_legacy), "attributes")) {
                        let parameter_name = js::get(js::get(Some(technique_legacy), "attributes"), &name);
                        let parameter = js::object(
                            js::get(Some(js::object(parameters, "semantic")?), &parameter_key(parameter_name)),
                            "semantic",
                        )?;
                        let mut attribute = Map::new();
                        insert_defined(&mut attribute, "semantic", parameter.get("semantic"));
                        attributes.insert(name, Value::Object(attribute));
                    }
                    let mut uniforms = Map::new();
                    for name in js::for_in(js::get(Some(technique_legacy), "uniforms")) {
                        let parameter_name = js::get(js::get(Some(technique_legacy), "uniforms"), &name);
                        let key = parameter_key(parameter_name);
                        let parameter = js::object(js::get(Some(js::object(parameters, "count")?), &key), "count")?;
                        let mut uniform = Map::new();
                        for property in ["count", "node", "type", "semantic", "value"] {
                            insert_defined(&mut uniform, property, parameter.get(property));
                        }
                        uniforms.insert(name.clone(), Value::Object(uniform));
                        mapped_uniforms.entry(technique_id.key()).or_default().insert(key, name);
                    }
                    let program_key =
                        js::get(Some(technique_legacy), "program").map_or_else(|| "undefined".into(), js::key);
                    let program_index = match seen_programs.get(&program_key) {
                        Some(&index) => index,
                        None => {
                            let programs_legacy = js::object(snapshot.get("programs"), &program_key)?;
                            let program_legacy = js::object(js::get(Some(programs_legacy), &program_key), "name")?;
                            js::object(snapshot.get("shaders"), "fragmentShader")?;
                            let fragment_shader = add_shader(program_legacy.get("fragmentShader"));
                            let vertex_shader = add_shader(program_legacy.get("vertexShader"));
                            let mut program = Map::new();
                            insert_defined(&mut program, "name", program_legacy.get("name"));
                            program.insert("fragmentShader".into(), json!(fragment_shader));
                            program.insert("vertexShader".into(), json!(vertex_shader));
                            insert_defined(&mut program, "glExtensions", gl_extensions.as_ref());
                            programs.push(Value::Object(program));
                            seen_programs.insert(program_key, programs.len() - 1);
                            programs.len() - 1
                        }
                    };
                    let mut technique = Map::new();
                    insert_defined(&mut technique, "name", technique_legacy.get("name"));
                    technique.insert("program".into(), json!(program_index));
                    technique.insert("attributes".into(), Value::Object(attributes));
                    technique.insert("uniforms".into(), Value::Object(uniforms));
                    techniques.push(Value::Object(technique));
                    updated_technique_indices.insert(technique_id.key(), techniques.len() - 1);
                    Ok(())
                })();
                match result {
                    Ok(()) => None,
                    Err(e) => {
                        error = Some(e);
                        Some(())
                    }
                }
            },
        );
        if let Some(e) = error {
            return Err(e);
        }
        if !techniques.is_empty() {
            if !js::defined(gltf.get("extensions")) {
                js::set(gltf, "extensions", Value::Object(Map::new()))?;
            }
            // A missing shader is `undefined`, which JSON writes as null in an array.
            let shaders: Vec<Value> = shaders.into_iter().map(|(_, s)| s.unwrap_or(Value::Null)).collect();
            let extension = json!({ "programs": programs, "shaders": shaders, "techniques": techniques });
            let extensions = js::object_mut(gltf.get_mut("extensions"), "KHR_techniques_webgl")?;
            js::set(extensions, "KHR_techniques_webgl", extension)?;
            add_extensions_used(gltf, "KHR_techniques_webgl")?;
            add_extensions_required(gltf, "KHR_techniques_webgl")?;
        }
    }
    for_each::each_mut(gltf, Collection::TopLevel("materials"), |material, _| {
        if let Some(technique) = js::get(Some(&*material), "technique").filter(|t| !t.is_null()).cloned() {
            let technique = js::key(&technique);
            let mut extension = Map::new();
            if let Some(index) = updated_technique_indices.get(&technique) {
                extension.insert("technique".into(), json!(index));
            }
            let values = material.get("values").cloned();
            let mut mapped = Map::new();
            let mut any = false;
            for name in js::for_in(values.as_ref().filter(|v| !v.is_null())) {
                any = true;
                let uniforms = mapped_uniforms.get(&technique).ok_or_else(|| {
                    super::PipelineError::Type(format!("Cannot read properties of undefined (reading '{name}')"))
                })?;
                if let Some(uniform) = uniforms.get(&name) {
                    mapped.insert(uniform.clone(), js::get(values.as_ref(), &name).cloned().unwrap_or(Value::Null));
                }
            }
            if any {
                extension.insert("values".into(), Value::Object(mapped));
            }
            if !js::defined(material.get("extensions")) {
                js::set(material, "extensions", Value::Object(Map::new()))?;
            }
            let extensions = js::object_mut(material.get_mut("extensions"), "KHR_techniques_webgl")?;
            js::set(extensions, "KHR_techniques_webgl", Value::Object(extension))?;
        }
        js::delete(material, "technique");
        js::delete(material, "values");
        Ok(())
    })?;
    js::delete(gltf, "techniques");
    js::delete(gltf, "programs");
    js::delete(gltf, "shaders");
    Ok(())
}

/// The property key a parameter name looks up.
fn parameter_key(parameter_name: Option<&Value>) -> String {
    parameter_name.map_or_else(|| "undefined".to_string(), js::key)
}
