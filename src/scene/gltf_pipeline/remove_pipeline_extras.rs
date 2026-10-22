//! CesiumJS `removePipelineExtras`: removes the `extras._pipeline` objects
//! `addPipelineExtras` added, and `extras` left empty.

use super::add_pipeline_extras::pipeline_collections;
use super::{for_each, js, Result};
use serde_json::Value;

/// A `null` element is a `TypeError`, as reading its `extras` is.
pub fn remove_pipeline_extras(gltf: &mut Value) -> Result<()> {
    for which in pipeline_collections() {
        for id in for_each::ids(gltf, which) {
            if let Some(object) = for_each::get_mut(gltf, which, &id) {
                remove_extras(object)?;
            }
        }
    }
    remove_extras(gltf)
}

fn remove_extras(object: &mut Value) -> Result<()> {
    let object = js::object_mut(Some(object), "extras")?;
    let Some(extras) = object.get_mut("extras").filter(|e| !e.is_null()) else { return Ok(()) };
    if js::defined(extras.get("_pipeline")) {
        js::delete(extras, "_pipeline");
    }
    if js::key_count(Some(extras)) == 0 && !matches!(extras, Value::String(s) if !s.is_empty()) {
        js::delete(object, "extras");
    }
    Ok(())
}
