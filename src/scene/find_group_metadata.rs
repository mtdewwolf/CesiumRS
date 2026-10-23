//! CesiumJS `findGroupMetadata`: the group a tile content belongs to.

use super::{has_extension, Cesium3DTilesetMetadata, GroupMetadata};
use serde_json::Value;

/// The group a content header names, by index or by dictionary id, from
/// either the 3D Tiles 1.1 `group` or the `3DTILES_metadata` extension.
pub fn find_group_metadata<'a>(
    metadata_extension: Option<&'a Cesium3DTilesetMetadata>,
    content_header: &Value,
) -> Option<&'a GroupMetadata> {
    let metadata = metadata_extension?;
    let group = if has_extension(content_header, "3DTILES_metadata") {
        content_header["extensions"]["3DTILES_metadata"].get("group")
    } else {
        content_header.get("group")
    };
    match group {
        Some(Value::Number(n)) => {
            let n = n.as_f64()?;
            (n >= 0.0 && n.fract() == 0.0).then(|| metadata.groups().get(n as usize)).flatten()
        }
        Some(Value::String(id)) => {
            let index = metadata.group_ids().iter().position(|g| g == id)?;
            metadata.groups().get(index)
        }
        _ => None,
    }
}
