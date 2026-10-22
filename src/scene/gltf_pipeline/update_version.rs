//! CesiumJS `updateVersion`: upgrades glTF 0.8 and 1.0 to 2.0, then turns
//! technique and `KHR_materials_common` materials into PBR materials.
//!
//! Cesium assigns `undefined` where a glTF 1.0 id has no mapping. JSON has
//! no `undefined`, so the port removes that property instead, and writes
//! `null` for such elements of arrays, which is what `JSON.stringify`
//! produces. Later steps that would read the `undefined` and throw a
//! `TypeError` therefore skip it.

use super::for_each::{
    self, collect_values, each_array_mut, each_mut, each_primitive_mut, each_property_mut, Collection,
};
use super::get_accessor_byte_stride::datatype;
use super::read_accessor_packed::js_max;
use super::{
    add_extensions_used, add_to_array, find_accessor_min_max, get_accessor_byte_stride, js,
    move_technique_render_states, move_techniques_to_extension, number_of_components_for_type, remove_extension,
    remove_unused_elements, update_accessor_component_types, ElementType, Gltf, Id, PipelineError, Result,
};
use crate::core::webgl_constants as gl;
use crate::core::{Cartesian3, ComponentDatatype, Quaternion, TypedArray};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// `updateVersion` options. Unset names use Cesium's defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateVersionOptions {
    /// Stop upgrading once the glTF reaches this version.
    pub target_version: Option<String>,
    /// Keep `KHR_techniques_webgl`, `KHR_blend` and `KHR_materials_common`
    /// instead of converting them to PBR.
    pub keep_legacy_extensions: bool,
    /// Material values used as the base color texture; default `u_tex`,
    /// `u_diffuse`, `u_emission`, `u_diffuse_tex`.
    pub base_color_texture_names: Option<Vec<String>>,
    /// Material values used as the base color factor; default `u_diffuse`,
    /// `u_diffuse_mat`.
    pub base_color_factor_names: Option<Vec<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Update {
    From08,
    From10,
}

fn update_function(version: &str) -> Option<Option<Update>> {
    match version {
        "0.8" => Some(Some(Update::From08)),
        "1.0" => Some(Some(Update::From10)),
        "2.0" => Some(None),
        _ => None,
    }
}

pub fn update_version(gltf: &mut Gltf, options: &UpdateVersionOptions) -> Result<()> {
    let version = gltf.json.get("version").filter(|v| !v.is_null()).cloned();
    js::assign_default(&mut gltf.json, "asset", json!({ "version": "1.0" }))?;
    let asset = gltf.json.get_mut("asset").expect("assigned");
    js::assign_default(asset, "version", json!("1.0"))?;
    let asset_version = js::get(Some(&*asset), "version").cloned();
    let mut version = version.or(asset_version).map_or_else(|| "1.0".to_string(), |v| js::key(&v));
    if update_function(&version).is_none() {
        version = String::from_utf16_lossy(&version.encode_utf16().take(3).collect::<Vec<_>>());
        if update_function(&version).is_none() {
            version = "1.0".into();
        }
    }
    let mut update = update_function(&version).flatten();
    while let Some(step) = update {
        if options.target_version.as_deref() == Some(version.as_str()) {
            break;
        }
        match step {
            Update::From08 => gltf_08_to_10(gltf)?,
            Update::From10 => gltf_10_to_20(gltf)?,
        }
        version = gltf.json.pointer("/asset/version").map_or_else(|| "undefined".into(), js::key);
        update = update_function(&version).flatten();
    }
    if !options.keep_legacy_extensions {
        convert_techniques_to_pbr(&mut gltf.json, options)?;
        convert_materials_common_to_pbr(&mut gltf.json)?;
    }
    Ok(())
}

// ---- small helpers ----------------------------------------------------------

/// `object[key] = value`, where `None` is JavaScript `undefined`.
fn set_or_delete(object: &mut Value, key: &str, value: Option<Value>) -> Result<()> {
    match value {
        Some(value) => js::set(object, key, value),
        None => {
            if !matches!(object, Value::Object(_) | Value::Array(_)) {
                return js::set(object, key, Value::Null);
            }
            js::delete(object, key);
            Ok(())
        }
    }
}

/// `mapping[id]` for a glTF 1.0 id.
fn mapped(mapping: &HashMap<String, usize>, id: Option<&Value>) -> Option<Value> {
    let key = id.map_or_else(|| "undefined".to_string(), js::key);
    mapping.get(&key).map(|&i| Value::from(i))
}

/// JavaScript truthiness.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn type_error(reading: &str) -> PipelineError {
    PipelineError::Type(format!("Cannot read properties of undefined (reading '{reading}')"))
}

/// The `for...in` ids of `gltf[name]`, walked whatever its type, as the
/// upgrade steps walk glTF 1.0 objects.
fn legacy_ids(gltf: &Value, name: &str) -> Vec<String> {
    js::for_in(gltf.get(name))
}

fn legacy_mut<'a>(gltf: &'a mut Value, name: &str, id: &str) -> Option<&'a mut Value> {
    js::get_mut(gltf.get_mut(name), id)
}

/// Element `i` of an array-like value: `array[i]` read as a number.
fn element(array: Option<&Value>, i: usize) -> f64 {
    js::to_number(js::get(array, &i.to_string()))
}

/// `Cartesian3.fromArray(array, offset)` read through `ToNumber`.
fn cartesian3(array: Option<&Value>, offset: usize) -> Cartesian3 {
    Cartesian3::new(element(array, offset), element(array, offset + 1), element(array, offset + 2))
}

// ---- 0.8 to 1.0 -------------------------------------------------------------

fn gltf_08_to_10(gltf: &mut Gltf) -> Result<()> {
    let json = &mut gltf.json;
    if !js::defined(json.get("asset")) {
        js::set(json, "asset", Value::Object(Map::new()))?;
    }
    let asset = json.get_mut("asset").expect("defined");
    js::set(asset, "version", json!("1.0"))?;
    let profile = match js::get(Some(&*asset), "profile") {
        Some(Value::String(profile)) => {
            let mut split = profile.split(' ');
            let mut object = Map::new();
            object.insert("api".into(), json!(split.next().unwrap_or("")));
            if let Some(version) = split.next() {
                object.insert("version".into(), json!(version));
            }
            Value::Object(object)
        }
        _ => Value::Object(Map::new()),
    };
    js::set(asset, "profile", profile)?;
    if js::defined(json.get("version")) {
        js::delete(json, "version");
    }
    update_instance_techniques(json)?;
    set_primitive_modes(json)?;
    update_nodes(json)?;
    update_animations(gltf)?;
    let json = &mut gltf.json;
    remove_technique_passes(json)?;
    if js::defined(json.get("allExtensions")) {
        let all = json.get("allExtensions").cloned().expect("defined");
        js::set(json, "extensionsUsed", all)?;
        js::delete(json, "allExtensions");
    }
    if js::defined(json.get("lights")) {
        js::assign_default(json, "extensions", Value::Object(Map::new()))?;
        let lights = json.get("lights").cloned().expect("defined");
        let extensions = json.get_mut("extensions").expect("assigned");
        js::assign_default(extensions, "KHR_materials_common", Value::Object(Map::new()))?;
        let common = js::object_mut(extensions.get_mut("KHR_materials_common"), "lights")?;
        js::set(common, "lights", lights)?;
        js::delete(json, "lights");
        add_extensions_used(json, "KHR_materials_common")?;
    }
    Ok(())
}

fn update_instance_techniques(gltf: &mut Value) -> Result<()> {
    for id in legacy_ids(gltf, "materials") {
        let Some(material) = legacy_mut(gltf, "materials", &id) else { continue };
        let material = js::object_mut(Some(material), "instanceTechnique")?;
        let Some(instance) = js::get(Some(&*material), "instanceTechnique").filter(|i| !i.is_null()).cloned() else {
            continue;
        };
        set_or_delete(material, "technique", js::get(Some(&instance), "technique").cloned())?;
        set_or_delete(material, "values", js::get(Some(&instance), "values").cloned())?;
        js::delete(material, "instanceTechnique");
    }
    Ok(())
}

fn set_primitive_modes(gltf: &mut Value) -> Result<()> {
    for id in legacy_ids(gltf, "meshes") {
        let Some(mesh) = legacy_mut(gltf, "meshes", &id) else { continue };
        let mesh = js::object_mut(Some(mesh), "primitives")?;
        if !js::defined(js::get(Some(&*mesh), "primitives")) {
            continue;
        }
        each_array_mut(mesh.get_mut("primitives"), |primitive| {
            let primitive = js::object_mut(Some(primitive), "primitive")?;
            let default_mode = js::get(Some(&*primitive), "primitive")
                .filter(|p| !p.is_null())
                .cloned()
                .unwrap_or(json!(gl::TRIANGLES));
            js::assign_default(primitive, "mode", default_mode)?;
            js::delete(primitive, "primitive");
            Ok(())
        })?;
    }
    Ok(())
}

/// The quaternion of an axis and an angle, as glTF 0.8 stored rotations.
fn axis_angle(axis: Cartesian3, angle: f64) -> Result<Quaternion> {
    let normalized = axis.normalize();
    if normalized.x.is_nan() || normalized.y.is_nan() || normalized.z.is_nan() {
        // Cartesian3.normalize's debug-build check.
        return Err(PipelineError::Developer("normalized result is not a number".into()));
    }
    Ok(Quaternion::from_axis_angle(axis, angle))
}

fn update_nodes(gltf: &mut Value) -> Result<()> {
    for id in legacy_ids(gltf, "nodes") {
        let Some(node) = legacy_mut(gltf, "nodes", &id) else { continue };
        let node = js::object_mut(Some(node), "rotation")?;
        if let Some(rotation) = js::get(Some(&*node), "rotation").filter(|r| !r.is_null()).cloned() {
            let q = axis_angle(cartesian3(Some(&rotation), 0), element(Some(&rotation), 3))?;
            js::set(node, "rotation", json!([js::number(q.x), js::number(q.y), js::number(q.z), js::number(q.w)]))?;
        }
        if let Some(skin) = js::get(Some(&*node), "instanceSkin").filter(|s| !s.is_null()).cloned() {
            for key in ["skeletons", "skin", "meshes"] {
                set_or_delete(node, key, js::get(Some(&skin), key).cloned())?;
            }
            js::delete(node, "instanceSkin");
        }
    }
    Ok(())
}

/// Rewrites axis-angle rotation outputs to quaternions in place, in the
/// typed array over the source buffer.
fn update_animations(gltf: &mut Gltf) -> Result<()> {
    let mut updated = HashSet::new();
    for animation_id in legacy_ids(&gltf.json, "animations") {
        let Some(animation) = js::get(gltf.json.get("animations"), &animation_id).cloned() else { continue };
        let animation = js::object(Some(&animation), "channels")?;
        let Some(Value::Array(channels)) = animation.get("channels").filter(|c| !c.is_null()) else { continue };
        for channel in channels {
            let target = js::object(js::get(Some(channel), "target"), "path")?;
            if target.get("path").and_then(Value::as_str) != Some("rotation") {
                continue;
            }
            let samplers = js::object(animation.get("samplers"), "sampler")?;
            let sampler =
                js::object(js::index(Some(samplers), channel.get("sampler").unwrap_or(&Value::Null)), "output")?;
            let parameters = js::object(animation.get("parameters"), "output")?;
            let accessor_id = js::index(Some(parameters), sampler.get("output").unwrap_or(&Value::Null)).cloned();
            let key = accessor_id.as_ref().map_or_else(|| "undefined".into(), js::key);
            if !updated.insert(key.clone()) {
                continue;
            }
            rotate_accessor(gltf, &key)?;
        }
    }
    Ok(())
}

fn rotate_accessor(gltf: &mut Gltf, accessor_key: &str) -> Result<()> {
    let accessor = js::object(js::get(gltf.json.get("accessors"), accessor_key), "bufferView")?.clone();
    let buffer_view = js::object(
        js::index(gltf.json.get("bufferViews"), accessor.get("bufferView").unwrap_or(&Value::Null)),
        "buffer",
    )?
    .clone();
    let buffer =
        js::object(js::index(gltf.json.get("buffers"), buffer_view.get("buffer").unwrap_or(&Value::Null)), "extras")?;
    let marker = js::object(js::get(js::get(buffer.get("extras"), "_pipeline"), "source"), "byteOffset")?.clone();
    let datatype = datatype(accessor.get("componentType"))?;
    let components = accessor.get("type").and_then(Value::as_str).and_then(number_of_components_for_type);
    let count = js::to_number(accessor.get("count"));
    let source = gltf.source_mut(&marker).ok_or_else(|| type_error("buffer"))?;
    let byte_offset = source.byte_offset as f64
        + js::to_number(buffer_view.get("byteOffset"))
        + js::to_number(accessor.get("byteOffset"));
    let length = components.map_or(f64::NAN, |c| c as f64 * count);
    let view = TypedView::new(datatype, source.buffer.len(), byte_offset, length)?;
    let data = std::sync::Arc::make_mut(&mut source.buffer);
    let components = components.unwrap_or(0);
    let mut j = 0.0;
    while j < count {
        let offset = j as usize * components;
        let axis = Cartesian3::new(view.get(data, offset), view.get(data, offset + 1), view.get(data, offset + 2));
        let q = axis_angle(axis, view.get(data, offset + 3))?;
        for (k, value) in [q.x, q.y, q.z, q.w].into_iter().enumerate() {
            view.set(data, offset + k, value);
        }
        j += 1.0;
    }
    Ok(())
}

/// `ComponentDatatype.createArrayBufferView(type, buffer, byteOffset,
/// length)`: element reads and writes through a typed array.
struct TypedView {
    datatype: ComponentDatatype,
    byte_offset: usize,
    length: usize,
}

impl TypedView {
    fn new(datatype: ComponentDatatype, buffer_length: usize, byte_offset: f64, length: f64) -> Result<Self> {
        let size = datatype.get_size_in_bytes();
        let byte_offset = js::to_index(byte_offset)?;
        if byte_offset % size != 0 {
            return Err(PipelineError::Range(format!("start offset should be a multiple of {size}")));
        }
        let length = js::to_index(length)?;
        if byte_offset + length * size > buffer_length {
            return Err(PipelineError::Range("Invalid typed array length".into()));
        }
        Ok(Self { datatype, byte_offset, length })
    }

    /// `array[i]`: `undefined` (NaN in arithmetic) past the end.
    fn get(&self, data: &[u8], i: usize) -> f64 {
        if i >= self.length {
            return f64::NAN;
        }
        let size = self.datatype.get_size_in_bytes();
        let at = self.byte_offset + i * size;
        let view = self.datatype.create_array_buffer_view(data, at, Some(1)).expect("in bounds");
        view.to_f64_vec()[0]
    }

    /// `array[i] = value`, ignored past the end.
    fn set(&self, data: &mut [u8], i: usize, value: f64) {
        if i >= self.length {
            return;
        }
        let size = self.datatype.get_size_in_bytes();
        let at = self.byte_offset + i * size;
        let bytes = super::update_accessor_component_types::typed_array_bytes(&TypedArray::from_values(
            self.datatype,
            &[value],
        ));
        data[at..at + size].copy_from_slice(&bytes);
    }
}

fn remove_technique_passes(gltf: &mut Value) -> Result<()> {
    for id in legacy_ids(gltf, "techniques") {
        let Some(technique) = legacy_mut(gltf, "techniques", &id) else { continue };
        let technique = js::object_mut(Some(technique), "passes")?;
        let Some(passes) = js::get(Some(&*technique), "passes").filter(|p| !p.is_null()).cloned() else { continue };
        let pass_name =
            js::get(Some(&*technique), "pass").filter(|p| !p.is_null()).map_or_else(|| "defaultPass".into(), js::key);
        if let Some(pass) = js::get(Some(&passes), &pass_name) {
            let pass = js::object(Some(pass), "instanceProgram")?;
            let program = js::object(pass.get("instanceProgram"), "attributes")?;
            for key in ["attributes", "program", "uniforms"] {
                if !js::defined(js::get(Some(&*technique), key)) {
                    set_or_delete(technique, key, js::get(Some(program), key).cloned())?;
                }
            }
            if !js::defined(js::get(Some(&*technique), "states")) {
                set_or_delete(technique, "states", js::get(Some(pass), "states").cloned())?;
            }
        }
        js::delete(technique, "passes");
        js::delete(technique, "pass");
    }
    Ok(())
}

// ---- 1.0 to 2.0 -------------------------------------------------------------

fn gltf_10_to_20(gltf: &mut Gltf) -> Result<()> {
    let json = &mut gltf.json;
    js::assign_default(json, "asset", Value::Object(Map::new()))?;
    js::set(json.get_mut("asset").expect("assigned"), "version", json!("2.0"))?;
    update_instance_techniques(json)?;
    remove_animation_samplers_indirection(json)?;
    remove_empty_nodes(json)?;
    objects_to_arrays(json)?;
    remove_animation_sampler_names(json)?;
    strip_asset(json)?;
    require_known_extensions(json)?;
    require_byte_length(gltf)?;
    move_byte_stride_to_buffer_view(gltf)?;
    require_position_accessor_min_max(gltf)?;
    require_animation_accessor_min_max(gltf)?;
    validate_present_accessor_min_max(gltf)?;
    let json = &mut gltf.json;
    each_mut(json, Collection::TopLevel("buffers"), |buffer, _| {
        js::delete(buffer, "type");
        Ok(())
    })?;
    each_mut(json, Collection::TopLevel("textures"), |texture, _| {
        for key in ["format", "internalFormat", "target", "type"] {
            js::delete(texture, key);
        }
        Ok(())
    })?;
    require_attribute_set_index(json)?;
    underscore_application_specific_semantics(json)?;
    update_accessor_component_types(gltf)?;
    let json = &mut gltf.json;
    clamp_camera_parameters(json)?;
    move_technique_render_states(json)?;
    move_techniques_to_extension(json)?;
    remove_empty_arrays(json)
}

fn remove_animation_samplers_indirection(gltf: &mut Value) -> Result<()> {
    for id in legacy_ids(gltf, "animations") {
        let Some(animation) = legacy_mut(gltf, "animations", &id) else { continue };
        let animation = js::object_mut(Some(animation), "parameters")?;
        let Some(parameters) = js::get(Some(&*animation), "parameters").filter(|p| !p.is_null()).cloned() else {
            continue;
        };
        each_property_mut(animation.get_mut("samplers"), |sampler, _| {
            let sampler = js::object_mut(Some(sampler), "input")?;
            for key in ["input", "output"] {
                let value = js::get(Some(&*sampler), key).cloned();
                let resolved = value
                    .as_ref()
                    .map_or_else(|| js::get(Some(&parameters), "undefined"), |v| js::index(Some(&parameters), v));
                set_or_delete(sampler, key, resolved.cloned())?;
            }
            Ok(())
        })?;
        js::delete(animation, "parameters");
    }
    Ok(())
}

fn number_at(array: Option<&Value>, i: usize) -> Option<f64> {
    js::get(array, &i.to_string()).and_then(Value::as_f64)
}

/// `Cartesian*.fromArray(values).equals(expected)`: every component `===`.
fn equals(values: Option<&Value>, expected: &[f64]) -> bool {
    expected.iter().enumerate().all(|(i, e)| number_at(values, i) == Some(*e))
}

fn length_is_zero(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

fn is_node_empty(node: &Value) -> Result<bool> {
    let node = js::object(Some(node), "children")?;
    let get = |k: &str| js::get(Some(node), k);
    let undefined_or = |k: &str, test: &dyn Fn(Option<&Value>) -> bool| !js::defined(get(k)) || test(get(k));
    let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    Ok(undefined_or("children", &length_is_zero)
        && undefined_or("meshes", &length_is_zero)
        && !js::defined(get("camera"))
        && !js::defined(get("skin"))
        && !js::defined(get("skeletons"))
        && !js::defined(get("jointName"))
        && undefined_or("translation", &|v| equals(v, &[0.0, 0.0, 0.0]))
        && undefined_or("scale", &|v| equals(v, &[1.0, 1.0, 1.0]))
        && undefined_or("rotation", &|v| equals(v, &[0.0, 0.0, 0.0, 1.0]))
        && undefined_or("matrix", &|v| equals(v, &identity))
        && !js::defined(get("extensions"))
        && !js::defined(get("extras")))
}

const MAXIMUM_NODE_DEPTH: usize = 4096;

/// Removes the node from scenes and parents, removing parents it empties.
fn delete_node(gltf: &mut Value, node_id: &Id, depth: usize) -> Result<()> {
    if depth > MAXIMUM_NODE_DEPTH {
        return Err(PipelineError::Range("Maximum call stack size exceeded".into()));
    }
    let id_value = node_id.value();
    each_mut(gltf, Collection::TopLevel("scenes"), |scene, _| {
        if let Some(Value::Array(nodes)) = scene.get_mut("nodes") {
            if let Some(i) = nodes.iter().rposition(|n| js::strict_eq(Some(n), Some(&id_value))) {
                nodes.remove(i);
            }
        }
        Ok(())
    })?;
    let nodes = Collection::TopLevel("nodes");
    for parent_id in for_each::ids(gltf, nodes) {
        let Some(parent) = for_each::get_mut(gltf, nodes, &parent_id) else { continue };
        let parent = js::object_mut(Some(parent), "children")?;
        let Some(Value::Array(children)) = parent.get_mut("children") else { continue };
        let Some(index) = js::index_of(children, Some(&id_value)) else { continue };
        children.remove(index);
        if is_node_empty(parent)? {
            delete_node(gltf, &parent_id, depth + 1)?;
        }
    }
    match gltf.get_mut("nodes") {
        Some(Value::Array(items)) => {
            if let Id::Index(i) = node_id {
                if let Some(item) = items.get_mut(*i) {
                    // `delete array[i]` leaves a hole, which JSON writes as null.
                    *item = Value::Null;
                }
            }
        }
        Some(nodes) => js::delete(nodes, &node_id.key()),
        None => {}
    }
    Ok(())
}

fn remove_empty_nodes(gltf: &mut Value) -> Result<()> {
    let nodes = Collection::TopLevel("nodes");
    for id in for_each::ids(gltf, nodes) {
        let Some(node) = for_each::get_mut(gltf, nodes, &id) else { continue };
        if is_node_empty(node)? {
            delete_node(gltf, &id, 0)?;
        }
    }
    Ok(())
}

const GLOBAL_MAPPING_NAMES: [&str; 16] = [
    "accessors",
    "animations",
    "buffers",
    "bufferViews",
    "cameras",
    "images",
    "materials",
    "meshes",
    "nodes",
    "programs",
    "samplers",
    "scenes",
    "shaders",
    "skins",
    "textures",
    "techniques",
];

/// `objectToArray`: the object's values in `for...in` order, each named by
/// its id unless it has a name.
fn object_to_array(object: Option<&Value>, mapping: &mut HashMap<String, usize>) -> Result<Vec<Value>> {
    let mut array = Vec::new();
    for id in js::for_in(object) {
        let mut value = js::get(object, &id).cloned().expect("listed");
        mapping.insert(id.clone(), array.len());
        let value_ref = js::object_mut(Some(&mut value), "name")?;
        if !js::defined(js::get(Some(&*value_ref), "name")) {
            js::set(value_ref, "name", Value::from(id))?;
        }
        array.push(value);
    }
    Ok(array)
}

fn map_property(object: &mut Value, key: &str, mapping: &HashMap<String, usize>) -> Result<()> {
    if js::defined(js::get(Some(&*object), key)) {
        let value = mapped(mapping, js::get(Some(&*object), key));
        set_or_delete(object, key, value)?;
    }
    Ok(())
}

/// Replaces each element with its mapping, `null` where there is none.
fn map_array(array: Option<&mut Value>, mapping: &HashMap<String, usize>) {
    if let Some(Value::Array(items)) = array {
        for item in items {
            *item = mapped(mapping, Some(item)).unwrap_or(Value::Null);
        }
    }
}

/// Moves `extensions.KHR_binary_glTF` onto the object, dropping empty
/// `extensions`.
fn move_binary_gltf(object: &mut Value, mapping: &HashMap<String, usize>, mime_type: bool) -> Result<()> {
    let Some(extensions) = object.get("extensions").filter(|e| !e.is_null()).cloned() else { return Ok(()) };
    let mut extensions = extensions;
    if let Some(binary) = js::get(Some(&extensions), "KHR_binary_glTF").filter(|b| !b.is_null()).cloned() {
        set_or_delete(object, "bufferView", mapped(mapping, js::get(Some(&binary), "bufferView")))?;
        if mime_type {
            set_or_delete(object, "mimeType", js::get(Some(&binary), "mimeType").cloned())?;
        }
        js::delete(&mut extensions, "KHR_binary_glTF");
        js::set(object, "extensions", extensions.clone())?;
    }
    if js::key_count(Some(&extensions)) == 0 && !matches!(&extensions, Value::String(s) if !s.is_empty()) {
        js::delete(object, "extensions");
    }
    Ok(())
}

fn objects_to_arrays(gltf: &mut Value) -> Result<()> {
    let mut global: HashMap<&str, HashMap<String, usize>> =
        GLOBAL_MAPPING_NAMES.iter().map(|n| (*n, HashMap::new())).collect();
    let mut joint_name_to_id: Vec<(String, Value)> = Vec::new();
    for id in legacy_ids(gltf, "nodes") {
        let node = js::object(js::get(gltf.get("nodes"), &id), "jointName")?;
        if let Some(joint_name) = js::get(Some(node), "jointName").filter(|j| !j.is_null()) {
            let key = js::key(joint_name);
            match joint_name_to_id.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = Value::from(id.clone()),
                None => joint_name_to_id.push((key, Value::from(id.clone()))),
            }
        }
    }
    let top_level = match gltf {
        Value::Object(map) => js::object_keys(map),
        _ => Vec::new(),
    };
    for name in top_level {
        let Some(&known) = GLOBAL_MAPPING_NAMES.iter().find(|n| **n == name) else { continue };
        let mut mapping = HashMap::new();
        let array = object_to_array(gltf.get(known), &mut mapping)?;
        js::set(gltf, known, Value::Array(array))?;
        global.insert(known, mapping);
    }
    let nodes_mapping = global["nodes"].clone();
    let joint_name_to_id: HashMap<String, Option<Value>> =
        joint_name_to_id.into_iter().map(|(name, id)| (name, mapped(&nodes_mapping, Some(&id)))).collect();
    if js::defined(gltf.get("scene")) {
        let scene = mapped(&global["scenes"], gltf.get("scene"));
        set_or_delete(gltf, "scene", scene)?;
    }
    each_mut(gltf, Collection::TopLevel("bufferViews"), |view, _| map_property(view, "buffer", &global["buffers"]))?;
    each_mut(gltf, Collection::TopLevel("accessors"), |accessor, _| {
        map_property(accessor, "bufferView", &global["bufferViews"])
    })?;
    each_mut(gltf, Collection::Techniques("shaders"), |shader, _| {
        move_binary_gltf(shader, &global["bufferViews"], false)
    })?;
    each_mut(gltf, Collection::Techniques("programs"), |program, _| {
        map_property(program, "vertexShader", &global["shaders"])?;
        map_property(program, "fragmentShader", &global["shaders"])
    })?;
    each_mut(gltf, Collection::Techniques("techniques"), |technique, _| {
        map_property(technique, "program", &global["programs"])?;
        each_property_mut(technique.get_mut("parameters"), |parameter, _| {
            let parameter = js::object_mut(Some(parameter), "node")?;
            map_property(parameter, "node", &global["nodes"])?;
            if let Some(Value::String(texture)) = parameter.get("value").cloned() {
                let mut value = Map::new();
                if let Some(index) = global["textures"].get(&texture) {
                    value.insert("index".into(), json!(index));
                }
                js::set(parameter, "value", Value::Object(value))?;
            }
            Ok(())
        })
    })?;
    each_primitive_mut(gltf, |primitive| {
        let primitive = js::object_mut(Some(primitive), "indices")?;
        map_property(primitive, "indices", &global["accessors"])?;
        each_property_mut(primitive.get_mut("attributes"), |accessor, _| {
            *accessor = mapped(&global["accessors"], Some(accessor)).unwrap_or(Value::Null);
            Ok(())
        })?;
        // `attributes[semantic] = undefined` drops the attribute from JSON.
        if let Some(Value::Object(attributes)) = primitive.get_mut("attributes") {
            attributes.retain(|_, v| !v.is_null());
        }
        map_property(primitive, "material", &global["materials"])
    })?;
    objects_to_arrays_nodes(gltf, &global)?;
    each_mut(gltf, Collection::TopLevel("skins"), |skin, _| {
        map_property(skin, "inverseBindMatrices", &global["accessors"])?;
        if let Some(names) = js::get(Some(&*skin), "jointNames").filter(|n| !n.is_null()).cloned() {
            let joints: Vec<Value> = match &names {
                Value::Array(names) => names
                    .iter()
                    .map(|name| joint_name_to_id.get(&js::key(name)).cloned().flatten().unwrap_or(Value::Null))
                    .collect(),
                _ => Vec::new(),
            };
            js::set(skin, "joints", Value::Array(joints))?;
            js::delete(skin, "jointNames");
        }
        Ok(())
    })?;
    each_mut(gltf, Collection::TopLevel("scenes"), |scene, _| {
        map_array(scene.get_mut("nodes"), &global["nodes"]);
        Ok(())
    })?;
    each_mut(gltf, Collection::TopLevel("animations"), |animation, _| {
        let animation = js::object_mut(Some(animation), "samplers")?;
        let mut sampler_mapping = HashMap::new();
        let samplers = object_to_array(animation.get("samplers"), &mut sampler_mapping)?;
        js::set(animation, "samplers", Value::Array(samplers))?;
        each_array_mut(animation.get_mut("samplers"), |sampler| {
            for key in ["input", "output"] {
                let value = mapped(&global["accessors"], js::get(Some(&*sampler), key));
                set_or_delete(sampler, key, value)?;
            }
            Ok(())
        })?;
        each_array_mut(animation.get_mut("channels"), |channel| {
            let channel = js::object_mut(Some(channel), "sampler")?;
            let sampler = mapped(&sampler_mapping, js::get(Some(&*channel), "sampler"));
            set_or_delete(channel, "sampler", sampler)?;
            if let Some(target) = channel.get_mut("target").filter(|t| !t.is_null()) {
                let node = mapped(&global["nodes"], js::get(Some(&*target), "id"));
                set_or_delete(target, "node", node)?;
                js::delete(target, "id");
            }
            Ok(())
        })
    })?;
    each_mut(gltf, Collection::TopLevel("materials"), |material, _| {
        map_property(material, "technique", &global["techniques"])?;
        let names = js::for_in(for_each::material_values(material));
        let values = for_each::material_values(material).cloned();
        for name in names {
            if let Some(Value::String(texture)) = js::get(values.as_ref(), &name) {
                let target = js::object_mut(material.get_mut("values"), &name)?;
                js::set(target, &name, texture_index(&global["textures"], texture))?;
            }
        }
        let common = material.pointer("/extensions/KHR_materials_common").filter(|c| !c.is_null()).cloned();
        let common =
            common.filter(|c| js::defined(material.get("extensions")) && js::defined(js::get(Some(c), "values")));
        if let Some(common_value) = common {
            let values = for_each::material_values(&common_value).cloned();
            for name in js::for_in(values.as_ref()) {
                if let Some(Value::String(texture)) = js::get(values.as_ref(), &name) {
                    let target =
                        js::object_mut(material.pointer_mut("/extensions/KHR_materials_common/values"), &name)?;
                    js::set(target, &name, texture_index(&global["textures"], texture))?;
                }
            }
        }
        Ok(())
    })?;
    each_mut(gltf, Collection::TopLevel("images"), |image, _| move_binary_gltf(image, &global["bufferViews"], true))?;
    each_mut(gltf, Collection::TopLevel("textures"), |texture, _| {
        map_property(texture, "sampler", &global["samplers"])?;
        map_property(texture, "source", &global["images"])
    })
}

/// `{ index: mapping.textures[value] }`.
fn texture_index(textures: &HashMap<String, usize>, texture: &str) -> Value {
    match textures.get(texture) {
        Some(index) => json!({ "index": index }),
        None => json!({}),
    }
}

fn objects_to_arrays_nodes(gltf: &mut Value, global: &HashMap<&str, HashMap<String, usize>>) -> Result<()> {
    let count = match gltf.get("nodes") {
        Some(Value::Array(nodes)) => nodes.len(),
        _ => 0,
    };
    for i in 0..count {
        let node = js::object_mut(js::get_mut(gltf.get_mut("nodes"), &i.to_string()), "children")?;
        map_array(node.get_mut("children"), &global["nodes"]);
        let mut new_children = Vec::new();
        if let Some(meshes) = js::get(Some(&*node), "meshes").filter(|m| !m.is_null()).cloned() {
            if let Value::Array(meshes) = &meshes {
                if let Some(first) = meshes.first() {
                    set_or_delete(node, "mesh", mapped(&global["meshes"], Some(first)))?;
                    new_children = meshes[1..].iter().map(|m| mapped(&global["meshes"], Some(m))).collect();
                }
            }
            js::delete(node, "meshes");
        }
        map_property(node, "camera", &global["cameras"])?;
        map_property(node, "skin", &global["skins"])?;
        let skeletons = js::get(Some(&*node), "skeletons").filter(|s| !s.is_null()).cloned();
        let skin = js::get(Some(&*node), "skin").filter(|s| !s.is_null()).cloned();
        if skeletons.is_some() {
            js::delete(node, "skeletons");
        }
        if js::defined(js::get(Some(&*node), "jointName")) {
            js::delete(node, "jointName");
        }
        for mesh in new_children {
            let mut mesh_node = Map::new();
            if let Some(mesh) = mesh {
                mesh_node.insert("mesh".into(), mesh);
            }
            let Some(Value::Array(nodes)) = gltf.get_mut("nodes") else { unreachable!("nodes is an array") };
            let mesh_node_id = add_to_array(nodes, Value::Object(mesh_node), false);
            let node = gltf.pointer_mut(&format!("/nodes/{i}")).expect("node exists");
            if !js::defined(node.get("children")) {
                js::set(node, "children", Value::Array(Vec::new()))?;
            }
            match node.get_mut("children") {
                Some(Value::Array(children)) => children.push(json!(mesh_node_id)),
                _ => return Err(PipelineError::Type("children.push is not a function".into())),
            }
        }
        if let (Some(Value::Array(skeletons)), Some(skin)) = (&skeletons, &skin) {
            if let Some(first) = skeletons.first() {
                let skins = js::object_mut(gltf.get_mut("skins"), "skeleton")?;
                let skin = js::object_mut(js::index_mut(Some(skins), skin), "skeleton")?;
                set_or_delete(skin, "skeleton", mapped(&global["nodes"], Some(first)))?;
            }
        }
    }
    Ok(())
}

fn remove_animation_sampler_names(gltf: &mut Value) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("animations"), |animation, _| {
        each_array_mut(animation.get_mut("samplers"), |sampler| {
            js::delete(sampler, "name");
            Ok(())
        })
    })
}

fn strip_asset(gltf: &mut Value) -> Result<()> {
    let asset = js::object_mut(gltf.get_mut("asset"), "profile")?;
    js::delete(asset, "profile");
    js::delete(asset, "premultipliedAlpha");
    Ok(())
}

fn require_known_extensions(gltf: &mut Value) -> Result<()> {
    js::assign_default(gltf, "extensionsRequired", Value::Array(Vec::new()))?;
    let used = match gltf.get("extensionsUsed") {
        Some(Value::Array(used)) => used.clone(),
        _ => Vec::new(),
    };
    for extension in used {
        if matches!(extension.as_str(), Some("CESIUM_RTC" | "KHR_materials_common" | "WEB3D_quantized_attributes")) {
            match gltf.get_mut("extensionsRequired") {
                Some(Value::Array(required)) => required.push(extension),
                _ => return Err(PipelineError::Type("extensionsRequired.push is not a function".into())),
            }
        }
    }
    Ok(())
}

/// `accessor.byteStride` when nonzero, else the stride the buffer view or
/// element size gives.
fn compute_accessor_byte_stride(gltf: &Value, accessor: &Value) -> Result<f64> {
    let stride = accessor.get("byteStride").filter(|s| !s.is_null());
    if stride.is_some() && !js::strict_eq(stride, Some(&json!(0))) {
        return Ok(js::to_number(stride));
    }
    get_accessor_byte_stride(gltf, accessor)
}

fn require_byte_length(gltf: &mut Gltf) -> Result<()> {
    let buffers = Collection::TopLevel("buffers");
    for id in for_each::ids(&gltf.json, buffers) {
        let Some(buffer) = for_each::get_mut(&mut gltf.json, buffers, &id) else { continue };
        if js::defined(js::get(Some(&*buffer), "byteLength")) {
            continue;
        }
        let marker = js::object(js::get(js::get(buffer.get("extras"), "_pipeline"), "source"), "length")?.clone();
        let length = gltf.source(&marker).ok_or_else(|| type_error("length"))?.byte_length;
        let buffer = for_each::get_mut(&mut gltf.json, buffers, &id).expect("present");
        js::set(buffer, "byteLength", json!(length))?;
    }
    let accessors = Collection::TopLevel("accessors");
    for id in for_each::ids(&gltf.json, accessors) {
        let Some(accessor) = for_each::get_mut(&mut gltf.json, accessors, &id).cloned() else { continue };
        let Some(buffer_view_id) = accessor.get("bufferView").filter(|v| !v.is_null()) else { continue };
        let stride = compute_accessor_byte_stride(&gltf.json, &accessor)?;
        let end = js::to_number(accessor.get("byteOffset")) + js::to_number(accessor.get("count")) * stride;
        let buffer_view =
            js::object_mut(js::index_mut(gltf.json.get_mut("bufferViews"), buffer_view_id), "byteLength")?;
        let current = js::get(Some(&*buffer_view), "byteLength").filter(|l| !l.is_null());
        let current = current.map_or(0.0, |l| js::to_number(Some(l)));
        js::set(buffer_view, "byteLength", js::number(js_max(current, end)))?;
    }
    Ok(())
}

fn move_byte_stride_to_buffer_view(gltf: &mut Gltf) -> Result<()> {
    let json = &mut gltf.json;
    let mut has_vertex_attributes = HashSet::new();
    for accessor_id in collect_values(|visit| {
        for_each::accessor_containing_vertex_attribute_data(json, visit);
    }) {
        let accessor = js::object(js::index(json.get("accessors"), &accessor_id), "bufferView")?;
        if let Some(view) = accessor.get("bufferView").filter(|v| !v.is_null()) {
            has_vertex_attributes.insert(js::key(view));
        }
    }
    // Accessor ids per buffer view, in `for...in` key order.
    let mut by_view: Vec<(String, Vec<Id>)> = Vec::new();
    let accessors = Collection::TopLevel("accessors");
    for id in for_each::ids(json, accessors) {
        let accessor = for_each::get_mut(json, accessors, &id).expect("listed");
        let Some(view) = accessor.get("bufferView").filter(|v| !v.is_null()) else { continue };
        let key = js::key(view);
        match by_view.iter_mut().find(|(k, _)| *k == key) {
            Some((_, ids)) => ids.push(id),
            None => by_view.push((key, vec![id])),
        }
    }
    let order: Vec<String> = {
        let map: Map<String, Value> = by_view.iter().map(|(k, _)| (k.clone(), Value::Null)).collect();
        js::object_keys(&map)
    };
    for view_key in order {
        let ids = by_view.iter().find(|(k, _)| *k == view_key).map(|(_, ids)| ids.clone()).expect("listed");
        let buffer_view = js::get(json.get("bufferViews"), &view_key).cloned();
        let accessor = |json: &Value, id: &Id| for_each::get(json, accessors, id).cloned();
        let mut sorted: Vec<(Id, f64)> = ids
            .iter()
            .map(|id| (id.clone(), js::to_number(accessor(json, id).as_ref().and_then(|a| a.get("byteOffset")))))
            .collect();
        // V8's sort is stable; a NaN difference compares as equal.
        sorted.sort_by(|a, b| (a.1 - b.1).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
        let sorted: Vec<Id> = sorted.into_iter().map(|(id, _)| id).collect();
        let mut current_byte_offset = Some(0.0);
        let mut current_index = 0;
        for i in 0..sorted.len() {
            let this = accessor(json, &sorted[i]).expect("listed");
            let stride = compute_accessor_byte_stride(json, &this)?;
            let byte_offset = js::to_number(this.get("byteOffset"));
            let byte_length = js::to_number(this.get("count")) * stride;
            js::delete(for_each::get_mut(json, accessors, &sorted[i]).expect("listed"), "byteStride");
            let has_next = i + 1 < sorted.len();
            let next_stride = if has_next {
                let next = accessor(json, &sorted[i + 1]).expect("listed");
                Some(compute_accessor_byte_stride(json, &next)?)
            } else {
                None
            };
            // `!==`: no next accessor, a different stride, or NaN.
            if next_stride != Some(stride) {
                let mut new_view = js::object(buffer_view.as_ref(), "byteOffset")?.clone();
                if has_vertex_attributes.contains(&view_key) {
                    js::set(&mut new_view, "byteStride", js::number(stride))?;
                }
                let offset = current_byte_offset.unwrap_or(f64::NAN);
                let shifted = js::to_number(js::get(Some(&new_view), "byteOffset")) + offset;
                js::set(&mut new_view, "byteOffset", js::number(shifted))?;
                js::set(&mut new_view, "byteLength", js::number(byte_offset + byte_length - offset))?;
                let Some(Value::Array(views)) = json.get_mut("bufferViews") else {
                    return Err(PipelineError::Type("bufferViews.push is not a function".into()));
                };
                let new_id = add_to_array(views, new_view, false);
                for id in &sorted[current_index..=i] {
                    let a = for_each::get_mut(json, accessors, id).expect("listed");
                    js::set(a, "bufferView", json!(new_id))?;
                    let shifted = js::to_number(a.get("byteOffset")) - offset;
                    js::set(a, "byteOffset", js::number(shifted))?;
                }
                current_byte_offset = if has_next {
                    Some(js::to_number(accessor(json, &sorted[i + 1]).expect("listed").get("byteOffset")))
                } else {
                    None
                };
                current_index = i + 1;
            }
        }
    }
    remove_unused_elements(json, Some(&[ElementType::Accessor, ElementType::BufferView, ElementType::Buffer]))
}

fn set_min_max(gltf: &mut Gltf, accessor_id: &Value, min: bool, max: bool) -> Result<()> {
    let accessor = js::object(js::index(gltf.json.get("accessors"), accessor_id), "min")?.clone();
    let bounds = find_accessor_min_max(gltf, &accessor)?;
    let accessor = js::object_mut(js::index_mut(gltf.json.get_mut("accessors"), accessor_id), "min")?;
    let array = |values: Vec<f64>| Value::Array(values.into_iter().map(js::number).collect());
    if min {
        js::set(accessor, "min", array(bounds.min))?;
    }
    if max {
        js::set(accessor, "max", array(bounds.max))?;
    }
    Ok(())
}

fn missing_min_max(gltf: &Gltf, accessor_id: &Value) -> Result<bool> {
    let accessor = js::object(js::index(gltf.json.get("accessors"), accessor_id), "min")?;
    Ok(!js::defined(accessor.get("min")) || !js::defined(accessor.get("max")))
}

fn require_position_accessor_min_max(gltf: &mut Gltf) -> Result<()> {
    let ids = collect_values(|visit| {
        for_each::accessor_with_semantic(&gltf.json, "POSITION", visit);
    });
    for id in ids {
        if missing_min_max(gltf, &id)? {
            set_min_max(gltf, &id, true, true)?;
        }
    }
    Ok(())
}

fn require_animation_accessor_min_max(gltf: &mut Gltf) -> Result<()> {
    let mut inputs = Vec::new();
    for_each::animation::<()>(&gltf.json, |animation, _| {
        for_each::animation_sampler::<()>(animation, |sampler, _| {
            inputs.push(sampler.get("input").cloned().unwrap_or(Value::Null));
            None
        });
        None
    });
    for input in inputs {
        if missing_min_max(gltf, &input)? {
            set_min_max(gltf, &input, true, true)?;
        }
    }
    Ok(())
}

fn validate_present_accessor_min_max(gltf: &mut Gltf) -> Result<()> {
    for id in for_each::ids(&gltf.json, Collection::TopLevel("accessors")) {
        let Some(accessor) = for_each::get(&gltf.json, Collection::TopLevel("accessors"), &id).cloned() else {
            continue;
        };
        let (min, max) = (js::defined(accessor.get("min")), js::defined(accessor.get("max")));
        if min || max {
            set_min_max(gltf, &id.value(), min, max)?;
        }
    }
    Ok(())
}

fn require_attribute_set_index(gltf: &mut Value) -> Result<()> {
    each_primitive_mut(gltf, |primitive| {
        let attributes = js::object_mut(primitive.get_mut("attributes"), "TEXCOORD")?;
        for semantic in js::for_in(Some(&*attributes)) {
            let accessor = js::get(Some(&*attributes), &semantic).cloned().expect("listed");
            if semantic == "TEXCOORD" {
                js::set(attributes, "TEXCOORD_0", accessor)?;
            } else if semantic == "COLOR" {
                js::set(attributes, "COLOR_0", accessor)?;
            }
        }
        js::delete(attributes, "TEXCOORD");
        js::delete(attributes, "COLOR");
        Ok(())
    })?;
    each_mut(gltf, Collection::Techniques("techniques"), |technique, _| {
        each_property_mut(technique.get_mut("parameters"), |parameter, _| {
            let parameter = js::object_mut(Some(parameter), "semantic")?;
            match parameter.get("semantic").and_then(Value::as_str) {
                Some("TEXCOORD") => js::set(parameter, "semantic", json!("TEXCOORD_0")),
                Some("COLOR") => js::set(parameter, "semantic", json!("COLOR_0")),
                _ => Ok(()),
            }
        })
    })
}

/// `semantic.search(/_[0-9]+/)`: the byte index of the first `_` followed by
/// a digit.
fn set_index_position(semantic: &str) -> Option<usize> {
    let bytes = semantic.as_bytes();
    (0..bytes.len().saturating_sub(1)).find(|&i| bytes[i] == b'_' && bytes[i + 1].is_ascii_digit())
}

fn underscore_application_specific_semantics(gltf: &mut Value) -> Result<()> {
    // Shared by every primitive, as in Cesium: later primitives also apply
    // the renames found in earlier ones.
    let mut mapped_semantics = Map::new();
    each_primitive_mut(gltf, |primitive| {
        for semantic in js::for_in(primitive.get("attributes")) {
            if semantic.starts_with('_') {
                continue;
            }
            let (stripped, suffix) = match set_index_position(&semantic) {
                Some(i) => (&semantic[..i], &semantic[i..]),
                None => (semantic.as_str(), "_0"),
            };
            let indexed = match stripped {
                "COLOR" => Some("COLOR"),
                "JOINT" | "JOINTS" => Some("JOINTS"),
                "TEXCOORD" => Some("TEXCOORD"),
                "WEIGHT" | "WEIGHTS" => Some("WEIGHTS"),
                _ => None,
            };
            if let Some(indexed) = indexed {
                mapped_semantics.insert(semantic.clone(), json!(format!("{indexed}{suffix}")));
            } else if !matches!(stripped, "POSITION" | "NORMAL" | "TANGENT") {
                mapped_semantics.insert(semantic.clone(), json!(format!("_{semantic}")));
            }
        }
        for semantic in js::object_keys(&mapped_semantics) {
            let new_semantic = mapped_semantics[&semantic].as_str().expect("string").to_string();
            let attributes = js::object_mut(primitive.get_mut("attributes"), &semantic)?;
            if let Some(accessor) = js::get(Some(&*attributes), &semantic).filter(|a| !a.is_null()).cloned() {
                js::delete(attributes, &semantic);
                js::set(attributes, &new_semantic, accessor)?;
            }
        }
        Ok(())
    })?;
    each_mut(gltf, Collection::Techniques("techniques"), |technique, _| {
        each_property_mut(technique.get_mut("parameters"), |parameter, _| {
            let parameter = js::object_mut(Some(parameter), "semantic")?;
            let key = js::get(Some(&*parameter), "semantic").map_or_else(|| "undefined".into(), js::key);
            if let Some(mapped) = mapped_semantics.get(&key).cloned() {
                js::set(parameter, "semantic", mapped)?;
            }
            Ok(())
        })
    })
}

fn clamp_camera_parameters(gltf: &mut Value) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("cameras"), |camera, _| {
        let Some(perspective) = camera.get_mut("perspective").filter(|p| !p.is_null()) else { return Ok(()) };
        if js::strict_eq(js::get(Some(&*perspective), "aspectRatio"), Some(&json!(0))) {
            js::delete(perspective, "aspectRatio");
        }
        if js::strict_eq(js::get(Some(&*perspective), "yfov"), Some(&json!(0))) {
            js::set(perspective, "yfov", json!(1.0))?;
        }
        Ok(())
    })
}

fn remove_empty_arrays(gltf: &mut Value) -> Result<()> {
    if let Value::Object(map) = gltf {
        map.retain(|_, v| !matches!(v, Value::Array(items) if items.is_empty()));
    }
    each_mut(gltf, Collection::TopLevel("nodes"), |node, _| {
        let Some(node) = Some(node).filter(|n| !n.is_null()) else { return Err(type_error("children")) };
        if length_is_zero(node.get("children")) {
            js::delete(node, "children");
        }
        Ok(())
    })
}

// ---- legacy materials to PBR -------------------------------------------------

fn initialize_pbr_material(material: &mut Value) -> Result<()> {
    if !js::defined(js::get(Some(&*material), "pbrMetallicRoughness")) {
        js::set(material, "pbrMetallicRoughness", Value::Object(Map::new()))?;
    }
    let pbr = material.get_mut("pbrMetallicRoughness").expect("assigned");
    js::set(pbr, "roughnessFactor", json!(1.0))?;
    js::set(pbr, "metallicFactor", json!(0.0))
}

/// `defined(value.index)`; reading a property of `null` throws.
fn is_texture(value: &Value) -> Result<bool> {
    let value = js::object(Some(value), "index")?;
    Ok(js::defined(js::get(Some(value), "index")))
}

fn is_vec4(value: &Value) -> bool {
    matches!(value, Value::Array(items) if items.len() == 4)
}

/// Cesium's sRGB to linear conversion. `Math.pow` is V8's own `pow`, which
/// neither the platform `pow` nor `libm` reproduces bit for bit, so the
/// result can differ from Cesium's in the last bit.
// The constants are Cesium's, digit for digit.
#[allow(clippy::excessive_precision)]
fn srgb_to_linear(srgb: &Value) -> Value {
    let mut linear: Vec<Value> = (0..3)
        .map(|i| {
            let c = element(Some(srgb), i);
            js::number(if c <= 0.04045 {
                c * 0.07739938080495356037151702786378
            } else {
                ((c + 0.055) * 0.94786729857819905213270142180095).powf(2.4)
            })
        })
        .collect();
    linear.push(js::get(Some(srgb), "3").cloned().unwrap_or(Value::Null));
    Value::Array(linear)
}

fn convert_techniques_to_pbr(gltf: &mut Value, options: &UpdateVersionOptions) -> Result<()> {
    let default_textures = ["u_tex", "u_diffuse", "u_emission", "u_diffuse_tex"].map(String::from).to_vec();
    let default_factors = ["u_diffuse", "u_diffuse_mat"].map(String::from).to_vec();
    let texture_names = options.base_color_texture_names.clone().unwrap_or(default_textures);
    let factor_names = options.base_color_factor_names.clone().unwrap_or(default_factors);
    each_mut(gltf, Collection::TopLevel("materials"), |material, _| {
        let values = for_each::material_values(material).cloned();
        for name in js::for_in(values.as_ref()) {
            let value = js::get(values.as_ref(), &name).cloned().expect("listed");
            if texture_names.contains(&name) && is_texture(&value)? {
                initialize_pbr_material(material)?;
                js::set(material.get_mut("pbrMetallicRoughness").expect("initialized"), "baseColorTexture", value)?;
            } else if factor_names.contains(&name) && is_vec4(&value) {
                initialize_pbr_material(material)?;
                let pbr = material.get_mut("pbrMetallicRoughness").expect("initialized");
                js::set(pbr, "baseColorFactor", srgb_to_linear(&value))?;
            }
        }
        Ok(())
    })?;
    remove_extension(gltf, "KHR_techniques_webgl");
    remove_extension(gltf, "KHR_blend");
    Ok(())
}

fn assign_as_base_color(material: &mut Value, base_color: Option<&Value>) -> Result<()> {
    let Some(base_color) = base_color.filter(|b| !b.is_null()) else { return Ok(()) };
    let pbr = js::object_mut(material.get_mut("pbrMetallicRoughness"), "baseColorFactor")?;
    if is_vec4(base_color) {
        js::set(pbr, "baseColorFactor", srgb_to_linear(base_color))
    } else if is_texture(base_color)? {
        js::set(pbr, "baseColorTexture", base_color.clone())
    } else {
        Ok(())
    }
}

fn assign_as_emissive(material: &mut Value, emissive: Option<&Value>) -> Result<()> {
    let Some(emissive) = emissive.filter(|e| !e.is_null()) else { return Ok(()) };
    if let Value::Array(items) = emissive {
        if items.len() == 4 {
            return js::set(material, "emissiveFactor", Value::Array(items[..3].to_vec()));
        }
    }
    if is_texture(emissive)? {
        js::set(material, "emissiveTexture", emissive.clone())?;
    }
    Ok(())
}

fn convert_materials_common_to_pbr(gltf: &mut Value) -> Result<()> {
    let mut unlit = false;
    each_mut(gltf, Collection::TopLevel("materials"), |material, _| {
        let extensions = js::get(Some(&*material), "extensions").filter(|e| !e.is_null()).cloned();
        let Some(common) = js::get(extensions.as_ref(), "KHR_materials_common").filter(|c| !c.is_null()).cloned()
        else {
            return Ok(());
        };
        let values = js::get(Some(&common), "values").filter(|v| !v.is_null()).cloned().unwrap_or(json!({}));
        let value = |key: &str| js::get(Some(&values), key).cloned();
        let (ambient, diffuse, emission, transparency) =
            (value("ambient"), value("diffuse"), value("emission"), value("transparency"));
        let double_sided = js::get(Some(&common), "doubleSided").cloned();
        let transparent = js::get(Some(&common), "transparent").cloned();
        initialize_pbr_material(material)?;
        if js::get(Some(&common), "technique").and_then(Value::as_str) == Some("CONSTANT") {
            unlit = true;
            if !js::defined(material.get("extensions")) {
                js::set(material, "extensions", Value::Object(Map::new()))?;
            }
            let extensions = js::object_mut(material.get_mut("extensions"), "KHR_materials_unlit")?;
            js::set(extensions, "KHR_materials_unlit", Value::Object(Map::new()))?;
            assign_as_base_color(material, emission.as_ref())?;
            assign_as_base_color(material, ambient.as_ref())?;
        } else {
            assign_as_base_color(material, diffuse.as_ref())?;
            assign_as_emissive(material, ambient.as_ref())?;
            assign_as_emissive(material, emission.as_ref())?;
        }
        if let Some(double_sided) = double_sided.filter(|d| !d.is_null()) {
            js::set(material, "doubleSided", double_sided)?;
        }
        if let Some(transparency) = transparency.filter(|t| !t.is_null()) {
            let pbr = material.get_mut("pbrMetallicRoughness").expect("initialized");
            match js::get_mut(Some(pbr), "baseColorFactor").filter(|f| !f.is_null()) {
                Some(factor) => {
                    let alpha = js::to_number(js::get(Some(&*factor), "3")) * js::to_number(Some(&transparency));
                    js::set(factor, "3", js::number(alpha))?;
                }
                None => js::set(pbr, "baseColorFactor", json!([1, 1, 1, transparency]))?,
            }
        }
        if let Some(transparent) = transparent.filter(|t| !t.is_null()) {
            let mode = if truthy(Some(&transparent)) { "BLEND" } else { "OPAQUE" };
            js::set(material, "alphaMode", json!(mode))?;
        }
        Ok(())
    })?;
    // Cesium adds it at the first CONSTANT material; nothing reads the list
    // in between, so adding it once afterwards gives the same list.
    if unlit {
        add_extensions_used(gltf, "KHR_materials_unlit")?;
    }
    remove_extension(gltf, "KHR_materials_common");
    Ok(())
}
