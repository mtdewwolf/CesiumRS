//! CesiumJS `addExtensionsRequired`: lists an extension in
//! `extensionsRequired` and `extensionsUsed`.

use super::add_extensions_used::add_to_list;
use super::{add_extensions_used, Result};
use serde_json::Value;

pub fn add_extensions_required(gltf: &mut Value, extension: &str) -> Result<()> {
    add_to_list(gltf, "extensionsRequired", extension)?;
    add_extensions_used(gltf, extension)
}
