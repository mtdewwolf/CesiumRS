//! CesiumJS `removeExtension`: removes an extension from the extension lists
//! and from every object that carries it.

use super::for_each::{self, Collection};
use super::{js, remove_extensions_used};
use serde_json::Value;

/// Returns the top-level `gltf.extensions[extension]` it removed, if any.
/// Removing `CESIUM_RTC` also turns `CESIUM_RTC_MODELVIEW` technique
/// uniforms back into `MODELVIEW`.
pub fn remove_extension(gltf: &mut Value, extension: &str) -> Option<Value> {
    remove_extensions_used(gltf, extension);
    if extension == "CESIUM_RTC" {
        remove_cesium_rtc(gltf);
    }
    remove_extension_and_traverse(gltf, extension)
}

fn remove_cesium_rtc(gltf: &mut Value) {
    let techniques = Collection::Techniques("techniques");
    for id in for_each::ids(gltf, techniques) {
        let Some(technique) = for_each::get_mut(gltf, techniques, &id) else { continue };
        let uniforms = js::get_mut(Some(technique), "uniforms");
        let Some(uniforms) = uniforms else { continue };
        for name in js::for_in(Some(uniforms)) {
            if let Some(uniform) = js::get_mut(Some(uniforms), &name) {
                if uniform.get("semantic").and_then(Value::as_str) == Some("CESIUM_RTC_MODELVIEW") {
                    let _ = js::set(uniform, "semantic", Value::from("MODELVIEW"));
                }
            }
        }
    }
}

/// Walks plain objects and arrays. Only the extension data of the object it
/// was called on is returned, as in Cesium.
fn remove_extension_and_traverse(object: &mut Value, extension: &str) -> Option<Value> {
    match object {
        Value::Array(items) => {
            for item in items {
                remove_extension_and_traverse(item, extension);
            }
            None
        }
        Value::Object(map) => {
            let mut extension_data = None;
            if let Some(extensions) = map.get_mut("extensions").filter(|e| !e.is_null()) {
                if js::defined(js::get(Some(extensions), extension)) {
                    extension_data = js::get(Some(extensions), extension).cloned();
                    js::delete(extensions, extension);
                    if js::key_count(Some(extensions)) == 0 && !matches!(extensions, Value::String(s) if !s.is_empty())
                    {
                        map.shift_remove("extensions");
                    }
                }
            }
            for (_, value) in map.iter_mut() {
                remove_extension_and_traverse(value, extension);
            }
            extension_data
        }
        _ => None,
    }
}
