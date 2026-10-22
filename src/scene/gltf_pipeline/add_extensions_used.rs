//! CesiumJS `addExtensionsUsed`: lists an extension in `extensionsUsed`.

use super::{add_to_array, js, Result};
use serde_json::Value;

/// Creates `extensionsUsed` if needed and adds `extension` once.
pub fn add_extensions_used(gltf: &mut Value, extension: &str) -> Result<()> {
    add_to_list(gltf, "extensionsUsed", extension)
}

pub(crate) fn add_to_list(gltf: &mut Value, list: &str, extension: &str) -> Result<()> {
    if !js::defined(gltf.get(list)) {
        js::set(gltf, list, Value::Array(Vec::new()))?;
    }
    match gltf.get_mut(list) {
        Some(Value::Array(items)) => {
            add_to_array(items, Value::from(extension), true);
            Ok(())
        }
        // A non-array has no `push`.
        _ => Err(super::PipelineError::Type(format!("{list}.push is not a function"))),
    }
}
