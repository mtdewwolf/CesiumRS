//! CesiumJS `removeExtensionsRequired`: drops an extension from
//! `extensionsRequired`, and the list once it is empty.

use super::js;
use serde_json::Value;

pub fn remove_extensions_required(gltf: &mut Value, extension: &str) {
    if remove_from_list(gltf, "extensionsRequired", extension) {
        delete_if_empty(gltf, "extensionsRequired");
    }
}

pub(crate) fn remove_from_list(gltf: &mut Value, list: &str, extension: &str) -> bool {
    if !js::defined(gltf.get(list)) {
        return false;
    }
    if let Some(Value::Array(items)) = gltf.get_mut(list) {
        if let Some(index) = js::index_of(items, Some(&Value::from(extension))) {
            items.remove(index);
        }
    }
    true
}

/// `delete gltf[list]` once its `length` is 0.
pub(crate) fn delete_if_empty(gltf: &mut Value, list: &str) {
    let empty = match gltf.get(list) {
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    };
    if empty {
        js::delete(gltf, list);
    }
}
