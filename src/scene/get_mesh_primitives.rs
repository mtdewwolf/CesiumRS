//! CesiumJS `getMeshPrimitives`: a mesh's primitives, with each
//! `EXT_mesh_primitive_restart` group combined into one primitive.
use super::gltf_pipeline::{js, PipelineError};
use serde_json::{Map, Value};

const TRIANGLE_FAN: f64 = 6.0;
const TRIANGLE_STRIP: f64 = 5.0;
const LINE_STRIP: f64 = 3.0;
const LINE_LOOP: f64 = 2.0;

fn read_error(property: &str) -> PipelineError {
    PipelineError::Type(format!("Cannot read properties of undefined (reading '{property}')"))
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `getMeshPrimitives(mesh)`: `mesh.primitives` (`None` when undefined), or
/// the combined list when every primitive restart group is valid. A group that
/// violates the extension falls back to `mesh.primitives`, as the extension
/// requires. A combined primitive is a copy of its group's first primitive
/// with the group's `indices`; `null` entries are dropped from a combined list.
pub fn get_mesh_primitives(mesh: &Value) -> Result<Option<Value>, PipelineError> {
    let mesh_primitives = js::get(Some(mesh), "primitives").cloned();
    let extensions = js::get(Some(mesh), "extensions");
    let Some(extension) = js::get(extensions, "EXT_mesh_primitive_restart").filter(|v| !v.is_null()) else {
        return Ok(mesh_primitives);
    };
    let fallback = Ok(mesh_primitives.clone());
    let source = mesh_primitives.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
    // A copy of mesh.primitives; members of a group become undefined.
    let mut primitives = source.iter().cloned().map(Some).collect::<Vec<_>>();
    let groups = match js::get(Some(extension), "primitiveGroups") {
        Some(Value::Array(groups)) => groups,
        _ => return Err(PipelineError::Type("primitiveRestartExtension.primitiveGroups is not iterable".into())),
    };
    for group in groups {
        if group.is_null() {
            return Err(read_error("primitives"));
        }
        let members = js::get(Some(group), "primitives").filter(|v| !v.is_null()).ok_or_else(|| read_error("0"))?;
        let first = js::get(Some(members), "0");
        if !js::defined(first) {
            return fallback;
        }
        if mesh_primitives.is_none() {
            return Err(read_error(&js::key(first.expect("defined"))));
        }
        let first_key = js::key(first.expect("defined"));
        let first_primitive = js::get(mesh_primitives.as_ref(), &first_key);
        if !truthy(first_primitive) {
            return fallback;
        }
        let mut primitive = first_primitive.and_then(Value::as_object).cloned().unwrap_or_else(Map::new);
        // `indices: group.indices`; an undefined value is an unset property.
        match js::get(Some(group), "indices") {
            Some(indices) => {
                primitive.insert("indices".into(), indices.clone());
            }
            None => {
                primitive.remove("indices");
            }
        }
        let mode = primitive.get("mode").and_then(Value::as_f64);
        if !matches!(mode, Some(m) if [TRIANGLE_FAN, TRIANGLE_STRIP, LINE_STRIP, LINE_LOOP].contains(&m)) {
            return fallback;
        }
        let Value::Array(indices) = members else {
            return Err(PipelineError::Type("group.primitives is not iterable".into()));
        };
        for index in indices {
            let slot = js::array_index(&js::key(index)).filter(|&i| i < primitives.len());
            let member = slot.and_then(|i| primitives[i].as_ref());
            let indexed = js::defined(member.and_then(|m| js::get(Some(m), "indices")));
            if !indexed || !js::strict_eq(member.and_then(|m| js::get(Some(m), "mode")), primitive.get("mode")) {
                return fallback;
            }
            primitives[slot.expect("member exists")] = None;
        }
        // The first index was a valid property of mesh.primitives, so it is in range.
        let first_index = js::array_index(&first_key).filter(|&i| i < primitives.len());
        match first_index {
            Some(i) => primitives[i] = Some(Value::Object(primitive)),
            None => return fallback,
        }
    }
    Ok(Some(Value::Array(primitives.into_iter().flatten().filter(|p| !p.is_null()).collect())))
}
