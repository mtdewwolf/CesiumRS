//! CesiumJS `usesExtension`: whether `extensionsUsed` lists an extension.

use super::js;
use serde_json::Value;

pub fn uses_extension(gltf: &Value, extension: &str) -> bool {
    match js::get(Some(gltf), "extensionsUsed") {
        Some(Value::Array(used)) => used.iter().any(|e| e.as_str() == Some(extension)),
        // `indexOf` on a string finds substrings.
        Some(Value::String(used)) => used.contains(extension),
        _ => false,
    }
}
