//! CesiumJS `hasExtension`: whether a glTF object declares an extension.

use serde_json::Value;

/// Whether `json.extensions[extension_name]` is defined. A JSON `null` counts
/// as undefined, as Cesium's `defined` does.
pub fn has_extension(json: &Value, extension_name: &str) -> bool {
    json.get("extensions").and_then(|extensions| extensions.get(extension_name)).is_some_and(|v| !v.is_null())
}
