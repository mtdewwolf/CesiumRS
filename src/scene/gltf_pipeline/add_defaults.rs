//! CesiumJS `addDefaults`: writes the glTF 2.0 defaults Cesium's loaders
//! expect to find.

use super::for_each::{self, collect_values, Collection};
use super::{add_to_array, get_accessor_byte_stride, js, PipelineError, Result};
use crate::core::webgl_constants as gl;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// Fills in accessor and buffer view offsets, primitive modes and a default
/// material, vertex and index buffer view targets and strides, material,
/// texture, animation sampler, node transform and sampler wrap defaults, and
/// `scene`.
pub fn add_defaults(gltf: &mut Value) -> Result<()> {
    let accessors = Collection::TopLevel("accessors");
    for id in for_each::ids(gltf, accessors) {
        let Some(accessor) = for_each::get_mut(gltf, accessors, &id) else { continue };
        if js::defined(js::get(Some(accessor), "bufferView")) {
            js::assign_default(accessor, "byteOffset", json!(0))?;
        }
    }
    let buffer_views = Collection::TopLevel("bufferViews");
    for id in for_each::ids(gltf, buffer_views) {
        let Some(buffer_view) = for_each::get_mut(gltf, buffer_views, &id) else { continue };
        if js::defined(js::get(Some(buffer_view), "buffer")) {
            js::assign_default(buffer_view, "byteOffset", json!(0))?;
        }
    }
    add_primitive_defaults(gltf)?;

    let vertex = collect_values(|visit| {
        for_each::accessor_containing_vertex_attribute_data(gltf, visit);
    });
    for accessor_id in vertex {
        let accessor = js::object_mut(js::index_mut(gltf.get_mut("accessors"), &accessor_id), "bufferView")?;
        js::assign_default(accessor, "normalized", json!(false))?;
        let accessor = accessor.clone();
        let Some(buffer_view_id) = accessor.get("bufferView").filter(|v| !v.is_null()) else { continue };
        let stride = get_accessor_byte_stride(gltf, &accessor)?;
        let buffer_view = js::object_mut(js::index_mut(gltf.get_mut("bufferViews"), buffer_view_id), "byteStride")?;
        js::set(buffer_view, "byteStride", js::number(stride))?;
        js::set(buffer_view, "target", json!(gl::ARRAY_BUFFER))?;
    }
    let indices = collect_values(|visit| {
        for_each::accessor_containing_index_data(gltf, visit);
    });
    for accessor_id in indices {
        let accessor = js::object(js::index(gltf.get("accessors"), &accessor_id), "bufferView")?;
        let Some(buffer_view_id) = accessor.get("bufferView").filter(|v| !v.is_null()).cloned() else { continue };
        let buffer_view = js::object_mut(js::index_mut(gltf.get_mut("bufferViews"), &buffer_view_id), "target")?;
        js::set(buffer_view, "target", json!(gl::ELEMENT_ARRAY_BUFFER))?;
    }

    let materials = Collection::TopLevel("materials");
    for id in for_each::ids(gltf, materials) {
        if let Some(material) = for_each::get_mut(gltf, materials, &id) {
            add_material_defaults(material)?;
        }
    }
    let animations = Collection::TopLevel("animations");
    for id in for_each::ids(gltf, animations) {
        let samplers = for_each::get_mut(gltf, animations, &id).and_then(|a| a.get_mut("samplers"));
        if let Some(Value::Array(samplers)) = samplers {
            for sampler in samplers {
                js::assign_default(sampler, "interpolation", json!("LINEAR"))?;
            }
        }
    }
    let animated = animated_nodes(gltf)?;
    let nodes = Collection::TopLevel("nodes");
    for id in for_each::ids(gltf, nodes) {
        let Some(node) = for_each::get_mut(gltf, nodes, &id) else { continue };
        let has = |k: &str| js::defined(js::get(Some(&*node), k));
        if animated.contains(&id.key()) || has("translation") || has("rotation") || has("scale") {
            js::assign_default(node, "translation", json!([0.0, 0.0, 0.0]))?;
            js::assign_default(node, "rotation", json!([0.0, 0.0, 0.0, 1.0]))?;
            js::assign_default(node, "scale", json!([1.0, 1.0, 1.0]))?;
        } else {
            let identity = json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
            js::assign_default(node, "matrix", identity)?;
        }
    }
    let samplers = Collection::TopLevel("samplers");
    for id in for_each::ids(gltf, samplers) {
        let Some(sampler) = for_each::get_mut(gltf, samplers, &id) else { continue };
        js::assign_default(sampler, "wrapS", json!(gl::REPEAT))?;
        js::assign_default(sampler, "wrapT", json!(gl::REPEAT))?;
    }
    if js::defined(gltf.get("scenes")) && !js::defined(gltf.get("scene")) {
        js::set(gltf, "scene", json!(0))?;
    }
    Ok(())
}

fn add_primitive_defaults(gltf: &mut Value) -> Result<()> {
    let meshes = Collection::TopLevel("meshes");
    for id in for_each::ids(gltf, meshes) {
        let count = match for_each::get_mut(gltf, meshes, &id).and_then(|m| m.get("primitives")) {
            Some(Value::Array(primitives)) => primitives.len(),
            _ => 0,
        };
        for i in 0..count {
            let primitive = primitive_mut(gltf, meshes, &id, i)?;
            js::assign_default(primitive, "mode", json!(gl::TRIANGLES))?;
            if js::defined(js::get(Some(&*primitive), "material")) {
                continue;
            }
            if !js::defined(gltf.get("materials")) {
                js::set(gltf, "materials", Value::Array(Vec::new()))?;
            }
            let Some(Value::Array(materials)) = gltf.get_mut("materials") else {
                return Err(PipelineError::Type("gltf.materials.push is not a function".into()));
            };
            let index = add_to_array(materials, json!({ "name": "default" }), false);
            js::set(primitive_mut(gltf, meshes, &id, i)?, "material", json!(index))?;
        }
    }
    Ok(())
}

fn primitive_mut<'a>(gltf: &'a mut Value, meshes: Collection, id: &super::Id, i: usize) -> Result<&'a mut Value> {
    let mesh = for_each::get_mut(gltf, meshes, id);
    js::object_mut(js::get_mut(js::get_mut(mesh, "primitives"), &i.to_string()), "mode")
}

fn add_material_defaults(material: &mut Value) -> Result<()> {
    let empty = Value::Object(Map::new());
    let extensions = js::get(Some(&*material), "extensions").filter(|e| !e.is_null()).unwrap_or(&empty).clone();
    if js::defined(js::get(Some(&extensions), "KHR_materials_common")) {
        let common = js::object_mut(material.pointer_mut("/extensions/KHR_materials_common"), "values")?;
        let technique = js::get(Some(&*common), "technique").cloned();
        if !js::defined(js::get(Some(&*common), "values")) {
            js::set(common, "values", Value::Object(Map::new()))?;
        }
        let values = js::object_mut(common.get_mut("values"), "ambient")?;
        js::assign_default(values, "ambient", json!([0.0, 0.0, 0.0, 1.0]))?;
        js::assign_default(values, "emission", json!([0.0, 0.0, 0.0, 1.0]))?;
        js::assign_default(values, "transparency", json!(1.0))?;
        let is = |name: &str| technique.as_ref().and_then(Value::as_str) == Some(name);
        if !is("CONSTANT") {
            js::assign_default(values, "diffuse", json!([0.0, 0.0, 0.0, 1.0]))?;
            if !is("LAMBERT") {
                js::assign_default(values, "specular", json!([0.0, 0.0, 0.0, 1.0]))?;
                js::assign_default(values, "shininess", json!(0.0))?;
            }
        }
        js::assign_default(common, "transparent", json!(false))?;
        js::assign_default(common, "doubleSided", json!(false))?;
        return Ok(());
    }
    js::assign_default(material, "emissiveFactor", json!([0.0, 0.0, 0.0]))?;
    js::assign_default(material, "alphaMode", json!("OPAQUE"))?;
    js::assign_default(material, "doubleSided", json!(false))?;
    if material.get("alphaMode").and_then(Value::as_str) == Some("MASK") {
        js::assign_default(material, "alphaCutoff", json!(0.5))?;
    }
    if js::defined(js::get(Some(&extensions), "KHR_techniques_webgl")) {
        let names = js::for_in(for_each::material_values(material));
        for name in names {
            let values = material_values_mut(material);
            let Some(value) = js::get_mut(values, &name) else { continue };
            if js::defined(js::get(Some(&*value), "index")) {
                add_texture_defaults(Some(value))?;
            }
        }
    }
    for key in ["emissiveTexture", "normalTexture", "occlusionTexture"] {
        add_texture_defaults(material.get_mut(key))?;
    }
    if let Some(pbr) = material.get_mut("pbrMetallicRoughness").filter(|p| !p.is_null()) {
        js::assign_default(pbr, "baseColorFactor", json!([1.0, 1.0, 1.0, 1.0]))?;
        js::assign_default(pbr, "metallicFactor", json!(1.0))?;
        js::assign_default(pbr, "roughnessFactor", json!(1.0))?;
        add_texture_defaults(pbr.get_mut("baseColorTexture"))?;
        add_texture_defaults(pbr.get_mut("metallicRoughnessTexture"))?;
    }
    if js::defined(js::get(Some(&extensions), "KHR_materials_pbrSpecularGlossiness")) {
        let gloss =
            js::object_mut(material.pointer_mut("/extensions/KHR_materials_pbrSpecularGlossiness"), "diffuseFactor")?;
        js::assign_default(gloss, "diffuseFactor", json!([1.0, 1.0, 1.0, 1.0]))?;
        js::assign_default(gloss, "specularFactor", json!([1.0, 1.0, 1.0]))?;
        js::assign_default(gloss, "glossinessFactor", json!(1.0))?;
        add_texture_defaults(gloss.get_mut("specularGlossinessTexture"))?;
    }
    Ok(())
}

/// The values `ForEach.materialValue` walks, mutably.
fn material_values_mut(material: &mut Value) -> Option<&mut Value> {
    let techniques =
        js::defined(material.get("extensions")) && js::defined(material.pointer("/extensions/KHR_techniques_webgl"));
    if techniques {
        js::get_mut(material.pointer_mut("/extensions/KHR_techniques_webgl"), "values")
    } else {
        js::get_mut(Some(material), "values")
    }
}

fn add_texture_defaults(texture: Option<&mut Value>) -> Result<()> {
    match texture.filter(|t| !t.is_null()) {
        Some(texture) => js::assign_default(texture, "texCoord", json!(0)),
        None => Ok(()),
    }
}

/// Property keys of the nodes animation channels translate, rotate or scale.
fn animated_nodes(gltf: &Value) -> Result<HashSet<String>> {
    let mut nodes = HashSet::new();
    let mut error = None;
    for_each::animation::<()>(gltf, |animation, _| {
        for_each::animation_channel::<()>(animation, |channel, _| {
            let Some(target) = channel.get("target").filter(|t| !t.is_null()) else {
                error = Some(PipelineError::Type("Cannot read properties of undefined (reading 'node')".into()));
                return Some(());
            };
            let path = target.get("path").and_then(Value::as_str);
            if matches!(path, Some("translation" | "rotation" | "scale")) {
                nodes.insert(target.get("node").map_or_else(|| "undefined".to_string(), js::key));
            }
            None
        });
        error.is_some().then_some(())
    });
    error.map_or(Ok(nodes), Err)
}
