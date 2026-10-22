//! CesiumJS `removeExtensionsUsed`: drops an extension from
//! `extensionsUsed` and `extensionsRequired`, and empty lists.

use super::remove_extensions_required;
use super::remove_extensions_required::{delete_if_empty, remove_from_list};
use serde_json::Value;

pub fn remove_extensions_used(gltf: &mut Value, extension: &str) {
    if remove_from_list(gltf, "extensionsUsed", extension) {
        remove_extensions_required(gltf, extension);
        delete_if_empty(gltf, "extensionsUsed");
    }
}
