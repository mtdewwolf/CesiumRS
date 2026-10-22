//! CesiumJS `ForEach`: iteration over glTF 1.0 objects and glTF 2.0 arrays.
//!
//! Each function calls `handler` in Cesium's order and stops at the first
//! handler that returns `Some`, returning that value, as Cesium stops at the
//! first defined return value. Top-level collections that are objects (glTF
//! 1.0) are walked with `for...in` and pass [`Id::Key`]; arrays pass
//! [`Id::Index`].
//!
//! Handlers see the JSON immutably. Passes that edit elements collect ids
//! with [`ids`] and look elements up with [`get_mut`].

use super::{js, uses_extension, Id};
use serde_json::Value;
use std::collections::HashSet;

/// `ForEach.objectLegacy`: `for...in` over an object's elements.
pub fn object_legacy<T>(objects: Option<&Value>, mut handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    if !js::defined(objects) {
        return None;
    }
    for key in js::for_in(objects) {
        if let Some(found) = js::get(objects, &key).and_then(|object| handler(object, &key)) {
            return Some(found);
        }
    }
    None
}

/// `ForEach.object`: the elements of an array. Anything else has no
/// `length`, so nothing is visited.
pub fn object<T>(array: Option<&Value>, mut handler: impl FnMut(&Value, usize) -> Option<T>) -> Option<T> {
    let Some(Value::Array(items)) = array else { return None };
    items.iter().enumerate().find_map(|(i, item)| handler(item, i))
}

/// Where a collection's elements live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Collection {
    /// `gltf[name]`: an array, or a glTF 1.0 object.
    TopLevel(&'static str),
    /// `gltf.extensions.KHR_techniques_webgl[name]` when the extension is used,
    /// else `gltf[name]`: programs, shaders and techniques.
    Techniques(&'static str),
}

impl Collection {
    fn resolve(self, gltf: &Value) -> (Option<&Value>, bool) {
        match self {
            Self::TopLevel(name) => top_level(gltf, name),
            Self::Techniques(name) => {
                if uses_extension(gltf, "KHR_techniques_webgl") {
                    let extension = js::get(js::get(Some(gltf), "extensions"), "KHR_techniques_webgl");
                    (js::get(extension, name), false)
                } else {
                    top_level(gltf, name)
                }
            }
        }
    }
}

/// `ForEach.topLevel` picks `objectLegacy` for a defined non-array.
fn top_level<'a>(gltf: &'a Value, name: &str) -> (Option<&'a Value>, bool) {
    let property = js::get(Some(gltf), name);
    let legacy = js::defined(property) && !property.is_some_and(Value::is_array);
    (property, legacy)
}

/// Visits a collection, as `ForEach.topLevel` (or the technique variants) do.
pub fn collection<T>(gltf: &Value, which: Collection, mut handler: impl FnMut(&Value, Id) -> Option<T>) -> Option<T> {
    match which.resolve(gltf) {
        (objects, true) => object_legacy(objects, |object, key| handler(object, Id::Key(key.to_string()))),
        (array, false) => object(array, |object, i| handler(object, Id::Index(i))),
    }
}

/// The ids a collection visits, in order.
pub fn ids(gltf: &Value, which: Collection) -> Vec<Id> {
    let mut out = Vec::new();
    collection::<()>(gltf, which, |_, id| {
        out.push(id);
        None
    });
    out
}

/// The element `id` of a collection, if it is still there.
pub fn get<'a>(gltf: &'a Value, which: Collection, id: &Id) -> Option<&'a Value> {
    js::get(which.resolve(gltf).0, &id.key())
}

/// The element `id` of a collection, mutably.
pub fn get_mut<'a>(gltf: &'a mut Value, which: Collection, id: &Id) -> Option<&'a mut Value> {
    let container = match which {
        Collection::TopLevel(name) => js::get_mut(Some(gltf), name),
        Collection::Techniques(name) => {
            if uses_extension(gltf, "KHR_techniques_webgl") {
                let extension = js::get_mut(js::get_mut(Some(gltf), "extensions"), "KHR_techniques_webgl");
                js::get_mut(extension, name)
            } else {
                js::get_mut(Some(gltf), name)
            }
        }
    };
    js::get_mut(container, &id.key())
}

macro_rules! top_level_fns {
    ($($(#[$m:meta])* $fn:ident => $collection:expr),+ $(,)?) => {
        $(
            $(#[$m])*
            pub fn $fn<T>(gltf: &Value, handler: impl FnMut(&Value, Id) -> Option<T>) -> Option<T> {
                collection(gltf, $collection, handler)
            }
        )+
    };
}

top_level_fns! {
    accessor => Collection::TopLevel("accessors"),
    animation => Collection::TopLevel("animations"),
    buffer => Collection::TopLevel("buffers"),
    buffer_view => Collection::TopLevel("bufferViews"),
    camera => Collection::TopLevel("cameras"),
    image => Collection::TopLevel("images"),
    material => Collection::TopLevel("materials"),
    mesh => Collection::TopLevel("meshes"),
    node => Collection::TopLevel("nodes"),
    sampler => Collection::TopLevel("samplers"),
    scene => Collection::TopLevel("scenes"),
    skin => Collection::TopLevel("skins"),
    texture => Collection::TopLevel("textures"),
    /// The `KHR_techniques_webgl` programs when the extension is used.
    program => Collection::Techniques("programs"),
    /// The `KHR_techniques_webgl` shaders when the extension is used.
    shader => Collection::Techniques("shaders"),
    /// The `KHR_techniques_webgl` techniques when the extension is used.
    technique => Collection::Techniques("techniques"),
}

pub fn animation_channel<T>(animation: &Value, handler: impl FnMut(&Value, usize) -> Option<T>) -> Option<T> {
    object(animation.get("channels"), handler)
}

/// Only an array of samplers is visited, as in Cesium.
pub fn animation_sampler<T>(animation: &Value, handler: impl FnMut(&Value, usize) -> Option<T>) -> Option<T> {
    object(animation.get("samplers"), handler)
}

/// `for...in` over `material.values`, or over
/// `material.extensions.KHR_techniques_webgl.values` when that is defined.
pub fn material_value<T>(material: &Value, mut handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    let values = material_values(material);
    for name in js::for_in(values) {
        if let Some(found) = js::get(values, &name).and_then(|value| handler(value, &name)) {
            return Some(found);
        }
    }
    None
}

/// The values `material_value` walks.
pub(crate) fn material_values(material: &Value) -> Option<&Value> {
    let extensions = js::get(Some(material), "extensions");
    let techniques = js::get(extensions, "KHR_techniques_webgl");
    if js::defined(extensions) && js::defined(techniques) {
        js::get(techniques, "values")
    } else {
        js::get(Some(material), "values")
    }
}

pub fn mesh_primitive<T>(mesh: &Value, handler: impl FnMut(&Value, usize) -> Option<T>) -> Option<T> {
    object(mesh.get("primitives"), handler)
}

/// `for...in` over `primitive.attributes`: the accessor id and the semantic.
pub fn mesh_primitive_attribute<T>(primitive: &Value, handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    for_in_values(primitive.get("attributes"), handler)
}

pub fn mesh_primitive_target<T>(primitive: &Value, handler: impl FnMut(&Value, usize) -> Option<T>) -> Option<T> {
    object(primitive.get("targets"), handler)
}

/// `for...in` over a morph target: the accessor id and the semantic.
pub fn mesh_primitive_target_attribute<T>(target: &Value, handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    for_in_values(Some(target), handler)
}

fn for_in_values<T>(object: Option<&Value>, mut handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    for name in js::for_in(object) {
        if let Some(found) = js::get(object, &name).and_then(|value| handler(value, &name)) {
            return Some(found);
        }
    }
    None
}

/// `ForEach.nodeInTree`: each listed node that exists, then its children,
/// depth first. The handler gets the node and the id as listed.
pub fn node_in_tree<T>(
    gltf: &Value,
    node_ids: &Value,
    handler: &mut impl FnMut(&Value, &Value) -> Option<T>,
) -> Option<T> {
    let nodes = js::get(Some(gltf), "nodes");
    if !js::defined(nodes) {
        return None;
    }
    let Value::Array(ids) = node_ids else { return None };
    for node_id in ids {
        let Some(node) = js::index(nodes, node_id).filter(|n| !n.is_null()) else { continue };
        if let Some(found) = handler(node, node_id) {
            return Some(found);
        }
        if let Some(children) = node.get("children").filter(|c| !c.is_null()) {
            if let Some(found) = node_in_tree(gltf, children, handler) {
                return Some(found);
            }
        }
    }
    None
}

pub fn node_in_scene<T>(
    gltf: &Value,
    scene: &Value,
    mut handler: impl FnMut(&Value, &Value) -> Option<T>,
) -> Option<T> {
    let nodes = scene.get("nodes").filter(|n| !n.is_null())?;
    node_in_tree(gltf, nodes, &mut handler)
}

pub fn skin_joint<T>(skin: &Value, mut handler: impl FnMut(&Value) -> Option<T>) -> Option<T> {
    object(skin.get("joints"), |joint, _| handler(joint))
}

/// `for...in` over `technique.attributes`: the parameter name and the
/// attribute name.
pub fn technique_attribute<T>(technique: &Value, handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    for_in_values(technique.get("attributes"), handler)
}

pub fn technique_uniform<T>(technique: &Value, handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    for_in_values(technique.get("uniforms"), handler)
}

pub fn technique_parameter<T>(technique: &Value, handler: impl FnMut(&Value, &str) -> Option<T>) -> Option<T> {
    for_in_values(technique.get("parameters"), handler)
}

/// Each accessor a primitive attribute or morph target attribute names with
/// a semantic starting with `semantic`, once.
pub fn accessor_with_semantic<T>(gltf: &Value, semantic: &str, handler: impl FnMut(&Value) -> Option<T>) -> Option<T> {
    vertex_accessors(gltf, Some(semantic), handler)
}

/// Each accessor any primitive attribute or morph target names, once.
pub fn accessor_containing_vertex_attribute_data<T>(
    gltf: &Value,
    handler: impl FnMut(&Value) -> Option<T>,
) -> Option<T> {
    vertex_accessors(gltf, None, handler)
}

fn vertex_accessors<T>(
    gltf: &Value,
    semantic: Option<&str>,
    mut handler: impl FnMut(&Value) -> Option<T>,
) -> Option<T> {
    let mut visited = HashSet::new();
    let mut visit = |accessor_id: &Value, attribute_semantic: &str| {
        if semantic.is_some_and(|s| !attribute_semantic.starts_with(s)) || !visited.insert(js::key(accessor_id)) {
            return None;
        }
        handler(accessor_id)
    };
    mesh(gltf, |mesh, _| {
        mesh_primitive(mesh, |primitive, _| {
            mesh_primitive_attribute(primitive, &mut visit).or_else(|| {
                mesh_primitive_target(primitive, |target, _| mesh_primitive_target_attribute(target, &mut visit))
            })
        })
    })
}

/// Each accessor a primitive uses for indices, once.
pub fn accessor_containing_index_data<T>(gltf: &Value, mut handler: impl FnMut(&Value) -> Option<T>) -> Option<T> {
    let mut visited = HashSet::new();
    mesh(gltf, |mesh, _| {
        mesh_primitive(mesh, |primitive, _| {
            let indices = primitive.get("indices").filter(|i| !i.is_null())?;
            if !visited.insert(js::key(indices)) {
                return None;
            }
            handler(indices)
        })
    })
}

/// The values a visitor passes, for passes that edit while iterating.
pub(crate) fn collect_values(visit: impl FnOnce(&mut dyn FnMut(&Value) -> Option<()>)) -> Vec<Value> {
    let mut out = Vec::new();
    visit(&mut |value| {
        out.push(value.clone());
        None
    });
    out
}

/// Calls `f` on each element of a collection that is still present when its
/// turn comes, as the live JavaScript loops read them.
pub(crate) fn each_mut(
    gltf: &mut Value,
    which: Collection,
    mut f: impl FnMut(&mut Value, &Id) -> super::Result<()>,
) -> super::Result<()> {
    for id in ids(gltf, which) {
        if let Some(element) = get_mut(gltf, which, &id) {
            f(element, &id)?;
        }
    }
    Ok(())
}

/// Calls `f` on each primitive of each mesh.
pub(crate) fn each_primitive_mut(
    gltf: &mut Value,
    mut f: impl FnMut(&mut Value) -> super::Result<()>,
) -> super::Result<()> {
    each_mut(gltf, Collection::TopLevel("meshes"), |mesh, _| each_array_mut(mesh.get_mut("primitives"), &mut f))
}

/// Calls `f` on each element of an array property, if it is an array.
pub(crate) fn each_array_mut(
    array: Option<&mut Value>,
    mut f: impl FnMut(&mut Value) -> super::Result<()>,
) -> super::Result<()> {
    if let Some(Value::Array(items)) = array {
        for item in items {
            f(item)?;
        }
    }
    Ok(())
}

/// Calls `f(value, key)` on each `for...in` property of an object.
pub(crate) fn each_property_mut(
    object: Option<&mut Value>,
    mut f: impl FnMut(&mut Value, &str) -> super::Result<()>,
) -> super::Result<()> {
    let Some(object) = object else { return Ok(()) };
    for key in js::for_in(Some(object)) {
        if let Some(value) = js::get_mut(Some(&mut *object), &key) {
            f(value, &key)?;
        }
    }
    Ok(())
}
