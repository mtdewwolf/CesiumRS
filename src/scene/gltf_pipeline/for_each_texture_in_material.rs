//! CesiumJS `forEachTextureInMaterial`: the texture infos a material uses.

use super::{for_each, js};
use serde_json::Value;

/// Calls `handler(textureInfo.index, textureInfo)` for each texture info, in
/// Cesium's order, and stops at the first `Some`:
/// - the `pbrMetallicRoughness` base color and metallic-roughness textures;
/// - `KHR_materials_pbrSpecularGlossiness` diffuse and specular-glossiness;
/// - `KHR_materials_specular` specular and specular color;
/// - `KHR_materials_common` diffuse, ambient, emission and specular values
///   that have an index;
/// - technique values (`ForEach.materialValue`) that have an index;
/// - the emissive, normal and occlusion textures.
pub fn for_each_texture_in_material<T>(
    material: &Value,
    mut handler: impl FnMut(Option<&Value>, &Value) -> Option<T>,
) -> Option<T> {
    texture_info_pointers(material).iter().find_map(|pointer| {
        let info = material.pointer(pointer)?;
        handler(info.get("index"), info)
    })
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// JSON pointers, relative to the material, of the texture infos in visit
/// order.
pub(crate) fn texture_info_pointers(material: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let defined_at = |base: &str, key: &str| {
        let pointer = format!("{base}/{}", escape(key));
        js::defined(material.pointer(&pointer)).then_some(pointer)
    };
    if js::defined(material.get("pbrMetallicRoughness")) {
        out.extend(defined_at("/pbrMetallicRoughness", "baseColorTexture"));
        out.extend(defined_at("/pbrMetallicRoughness", "metallicRoughnessTexture"));
    }
    let extensions = material.get("extensions");
    if js::defined(extensions) {
        if js::defined(js::get(extensions, "KHR_materials_pbrSpecularGlossiness")) {
            let base = "/extensions/KHR_materials_pbrSpecularGlossiness";
            out.extend(defined_at(base, "diffuseTexture"));
            out.extend(defined_at(base, "specularGlossinessTexture"));
        }
        if js::defined(js::get(extensions, "KHR_materials_specular")) {
            let base = "/extensions/KHR_materials_specular";
            out.extend(defined_at(base, "specularTexture"));
            out.extend(defined_at(base, "specularColorTexture"));
        }
        let common = js::get(extensions, "KHR_materials_common");
        if js::defined(common) && js::defined(js::get(common, "values")) {
            for key in ["diffuse", "ambient", "emission", "specular"] {
                let pointer = format!("/extensions/KHR_materials_common/values/{key}");
                if js::defined(material.pointer(&pointer)) && js::defined(js::get(material.pointer(&pointer), "index"))
                {
                    out.push(pointer);
                }
            }
        }
    }
    // Where `ForEach.materialValue` reads the values from.
    let techniques = material.pointer("/extensions/KHR_techniques_webgl");
    let values_base = if js::defined(extensions) && js::defined(techniques) {
        "/extensions/KHR_techniques_webgl/values"
    } else {
        "/values"
    };
    let values = for_each::material_values(material);
    for name in js::for_in(values) {
        let value = js::get(values, &name);
        if js::defined(js::get(value, "index")) {
            out.push(format!("{values_base}/{}", escape(&name)));
        }
    }
    for key in ["emissiveTexture", "normalTexture", "occlusionTexture"] {
        out.extend(defined_at("", key));
    }
    out
}
