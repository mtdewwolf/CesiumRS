//! CesiumJS `removeUnusedElements`: removes glTF 2.0 elements nothing
//! references and shifts the indices that point past them.

use super::for_each::{self, each_array_mut, each_mut, each_primitive_mut, each_property_mut, Collection};
use super::for_each_texture_in_material::texture_info_pointers;
use super::{for_each_texture_in_material, js, uses_extension, PipelineError, Result};
use serde_json::Value;
use std::collections::HashSet;

/// The element kinds, in the order Cesium removes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementType {
    Mesh,
    Node,
    Material,
    Accessor,
    BufferView,
    Buffer,
    Texture,
    Sampler,
    Image,
}

impl ElementType {
    pub const ALL: [Self; 9] = [
        Self::Mesh,
        Self::Node,
        Self::Material,
        Self::Accessor,
        Self::BufferView,
        Self::Buffer,
        Self::Texture,
        Self::Sampler,
        Self::Image,
    ];

    /// Cesium's name for the type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mesh => "mesh",
            Self::Node => "node",
            Self::Material => "material",
            Self::Accessor => "accessor",
            Self::BufferView => "bufferView",
            Self::Buffer => "buffer",
            Self::Texture => "texture",
            Self::Sampler => "sampler",
            Self::Image => "image",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    fn collection(self) -> &'static str {
        match self {
            Self::Mesh => "meshes",
            Self::Node => "nodes",
            Self::Material => "materials",
            Self::Accessor => "accessors",
            Self::BufferView => "bufferViews",
            Self::Buffer => "buffers",
            Self::Texture => "textures",
            Self::Sampler => "samplers",
            Self::Image => "images",
        }
    }
}

/// Removes unused elements of `element_types` (all of them by default),
/// always in [`ElementType::ALL`] order.
pub fn remove_unused_elements(gltf: &mut Value, element_types: Option<&[ElementType]>) -> Result<()> {
    let types = element_types.unwrap_or(&ElementType::ALL);
    for element_type in ElementType::ALL {
        if types.contains(&element_type) {
            remove_unused_elements_by_type(gltf, element_type)?;
        }
    }
    Ok(())
}

fn remove_unused_elements_by_type(gltf: &mut Value, element_type: ElementType) -> Result<()> {
    if !js::defined(gltf.get(element_type.collection())) {
        return Ok(());
    }
    let used = used_ids(gltf, element_type)?;
    // Only an array has a length; a glTF 1.0 object is left alone.
    let length = match gltf.get(element_type.collection()) {
        Some(Value::Array(items)) => items.len(),
        _ => 0,
    };
    let mut removed = 0;
    for i in 0..length {
        if !used.contains(&i.to_string()) {
            remove(gltf, element_type, i - removed)?;
            removed += 1;
        }
    }
    Ok(())
}

// ---- shifting ids ---------------------------------------------------------

/// Whether `value > id` in JavaScript.
fn greater(value: Option<&Value>, id: usize) -> bool {
    js::to_number(value) > id as f64
}

/// `value--`.
fn decrement(value: &mut Value) {
    *value = js::number(js::to_number(Some(value)) - 1.0);
}

/// `if (defined(object[key]) && object[key] > id) object[key]--`.
fn shift_defined(object: &mut Value, key: &str, id: usize) {
    if let Some(value) = js::get_mut(Some(object), key) {
        if !value.is_null() && greater(Some(value), id) {
            decrement(value);
        }
    }
}

/// `if (object[key] > id) object[key]--`, without the defined check.
fn shift(object: &mut Value, key: &str, id: usize) {
    if let Some(value) = js::get_mut(Some(object), key) {
        if greater(Some(value), id) {
            decrement(value);
        }
    }
}

fn type_error(reading: &str) -> PipelineError {
    PipelineError::Type(format!("Cannot read properties of undefined (reading '{reading}')"))
}

/// `extensions[name]` of an object when both are defined.
fn extension_mut<'a>(object: &'a mut Value, name: &str) -> Option<&'a mut Value> {
    let extensions = object.get_mut("extensions").filter(|e| !e.is_null())?;
    js::get_mut(Some(extensions), name).filter(|e| !e.is_null())
}

/// `gltf.extensions[name]`, which Cesium reads without checking.
fn root_extension_mut<'a>(gltf: &'a mut Value, name: &str) -> Result<&'a mut Value> {
    let extensions = js::object_mut(gltf.get_mut("extensions"), name)?;
    js::get_mut(Some(extensions), name).filter(|e| !e.is_null()).ok_or_else(|| type_error("featureTables"))
}

fn remove(gltf: &mut Value, element_type: ElementType, id: usize) -> Result<()> {
    if let Some(Value::Array(items)) = gltf.get_mut(element_type.collection()) {
        if id < items.len() {
            items.remove(id);
        }
    }
    match element_type {
        ElementType::Accessor => remove_accessor(gltf, id),
        ElementType::Buffer => remove_buffer(gltf, id),
        ElementType::BufferView => remove_buffer_view(gltf, id),
        ElementType::Image => remove_image(gltf, id),
        ElementType::Mesh => remove_mesh(gltf, id),
        ElementType::Node => remove_node(gltf, id),
        ElementType::Material => remove_material(gltf, id),
        ElementType::Sampler => remove_sampler(gltf, id),
        ElementType::Texture => remove_texture(gltf, id),
    }
}

fn remove_accessor(gltf: &mut Value, id: usize) -> Result<()> {
    each_primitive_mut(gltf, |primitive| {
        each_property_mut(primitive.get_mut("attributes"), |accessor, _| {
            if greater(Some(accessor), id) {
                decrement(accessor);
            }
            Ok(())
        })?;
        each_array_mut(primitive.get_mut("targets"), |target| {
            each_property_mut(Some(target), |accessor, _| {
                if greater(Some(accessor), id) {
                    decrement(accessor);
                }
                Ok(())
            })
        })?;
        shift_defined(primitive, "indices", id);
        if let Some(outline) = extension_mut(primitive, "CESIUM_primitive_outline") {
            shift(outline, "indices", id);
        }
        Ok(())
    })?;
    each_mut(gltf, Collection::TopLevel("skins"), |skin, _| {
        shift_defined(skin, "inverseBindMatrices", id);
        Ok(())
    })?;
    each_mut(gltf, Collection::TopLevel("animations"), |animation, _| {
        each_array_mut(animation.get_mut("samplers"), |sampler| {
            shift_defined(sampler, "input", id);
            shift_defined(sampler, "output", id);
            Ok(())
        })
    })
}

fn remove_buffer(gltf: &mut Value, id: usize) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("bufferViews"), |buffer_view, _| {
        shift_defined(buffer_view, "buffer", id);
        // Cesium decrements the meshopt buffer whatever it is.
        if let Some(meshopt) = extension_mut(buffer_view, "EXT_meshopt_compression") {
            match js::get_mut(Some(meshopt), "buffer") {
                Some(buffer) => decrement(buffer),
                None => js::set(meshopt, "buffer", Value::Null)?,
            }
        }
        Ok(())
    })
}

fn remove_buffer_view(gltf: &mut Value, id: usize) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("accessors"), |accessor, _| {
        shift_defined(accessor, "bufferView", id);
        Ok(())
    })?;
    each_mut(gltf, Collection::Techniques("shaders"), |shader, _| {
        shift_defined(shader, "bufferView", id);
        Ok(())
    })?;
    each_mut(gltf, Collection::TopLevel("images"), |image, _| {
        shift_defined(image, "bufferView", id);
        Ok(())
    })?;
    if uses_extension(gltf, "KHR_draco_mesh_compression") {
        each_primitive_mut(gltf, |primitive| {
            if let Some(draco) = extension_mut(primitive, "KHR_draco_mesh_compression") {
                shift(draco, "bufferView", id);
            }
            Ok(())
        })?;
    }
    if uses_extension(gltf, "EXT_feature_metadata") {
        let extension = root_extension_mut(gltf, "EXT_feature_metadata")?;
        each_property_mut(extension.get_mut("featureTables"), |table, _| {
            if let Some(properties) = table.get_mut("properties").filter(|p| !p.is_null()) {
                each_property_mut(Some(properties), |property, _| {
                    for key in ["bufferView", "arrayOffsetBufferView", "stringOffsetBufferView"] {
                        shift_defined(property, key, id);
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })?;
    }
    if uses_extension(gltf, "EXT_structural_metadata") {
        let extension = root_extension_mut(gltf, "EXT_structural_metadata")?;
        each_array_mut(extension.get_mut("propertyTables"), |table| {
            each_property_mut(table.get_mut("properties"), |property, _| {
                for key in ["values", "arrayOffsets", "stringOffsets"] {
                    shift_defined(property, key, id);
                }
                Ok(())
            })
        })?;
    }
    Ok(())
}

fn remove_image(gltf: &mut Value, id: usize) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("textures"), |texture, _| {
        shift_defined(texture, "source", id);
        let webp = texture.pointer("/extensions/EXT_texture_webp").filter(|e| !e.is_null());
        if js::defined(texture.get("extensions")) && webp.is_some() && greater(js::get(webp, "source"), id) {
            if let Some(webp) = texture.pointer_mut("/extensions/EXT_texture_webp/source") {
                decrement(webp);
            }
        } else {
            let basis = texture.pointer("/extensions/KHR_texture_basisu").filter(|e| !e.is_null());
            if js::defined(texture.get("extensions")) && basis.is_some() && greater(js::get(basis, "source"), id) {
                if let Some(basis) = texture.pointer_mut("/extensions/KHR_texture_basisu/source") {
                    decrement(basis);
                }
            }
        }
        Ok(())
    })
}

fn remove_mesh(gltf: &mut Value, id: usize) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("nodes"), |node, _| {
        let mesh = node.get("mesh").filter(|m| !m.is_null());
        if mesh.is_some() {
            if greater(mesh, id) {
                decrement(node.get_mut("mesh").expect("checked"));
            } else if js::strict_eq(mesh, Some(&Value::from(id))) {
                js::delete(node, "mesh");
            }
        }
        Ok(())
    })
}

/// `ids.filter(x => x !== id).map(x => x > id ? x - 1 : x)`.
fn remove_and_shift(ids: &[Value], id: usize, filter: bool) -> Vec<Value> {
    ids.iter()
        .filter(|x| !filter || !js::strict_eq(Some(x), Some(&Value::from(id))))
        .map(|x| if greater(Some(x), id) { js::number(js::to_number(Some(x)) - 1.0) } else { x.clone() })
        .collect()
}

fn remove_node(gltf: &mut Value, id: usize) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("skins"), |skin, _| {
        shift_defined(skin, "skeleton", id);
        let Some(Value::Array(joints)) = skin.get("joints") else { return Err(type_error("map")) };
        let joints = remove_and_shift(joints, id, false);
        js::set(skin, "joints", Value::Array(joints))
    })?;
    each_mut(gltf, Collection::TopLevel("animations"), |animation, _| {
        each_array_mut(animation.get_mut("channels"), |channel| {
            if let Some(target) = channel.get_mut("target").filter(|t| !t.is_null()) {
                shift_defined(target, "node", id);
            }
            Ok(())
        })
    })?;
    each_mut(gltf, Collection::Techniques("techniques"), |technique, _| {
        each_property_mut(technique.get_mut("uniforms"), |uniform, _| {
            shift_defined(uniform, "node", id);
            Ok(())
        })
    })?;
    each_mut(gltf, Collection::TopLevel("nodes"), |node, _| {
        let Some(children) = node.get("children").filter(|c| !c.is_null()) else { return Ok(()) };
        let Value::Array(children) = children else { return Err(type_error("filter")) };
        let children = remove_and_shift(children, id, true);
        js::set(node, "children", Value::Array(children))
    })?;
    each_mut(gltf, Collection::TopLevel("scenes"), |scene, _| {
        let Some(Value::Array(nodes)) = scene.get("nodes") else { return Err(type_error("filter")) };
        let nodes = remove_and_shift(nodes, id, true);
        js::set(scene, "nodes", Value::Array(nodes))
    })
}

fn remove_material(gltf: &mut Value, id: usize) -> Result<()> {
    each_primitive_mut(gltf, |primitive| {
        shift_defined(primitive, "material", id);
        Ok(())
    })
}

fn remove_sampler(gltf: &mut Value, id: usize) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("textures"), |texture, _| {
        shift_defined(texture, "sampler", id);
        Ok(())
    })
}

/// `textureInfo.index > id`, decrementing; the texture info must be an
/// object to read from.
fn shift_texture_info(info: Option<&mut Value>, id: usize) -> Result<()> {
    let info = js::object_mut(info, "index")?;
    shift(info, "index", id);
    Ok(())
}

fn remove_texture(gltf: &mut Value, id: usize) -> Result<()> {
    each_mut(gltf, Collection::TopLevel("materials"), |material, _| {
        for pointer in texture_info_pointers(material) {
            if let Some(info) = material.pointer_mut(&pointer) {
                shift(info, "index", id);
            }
        }
        Ok(())
    })?;
    if uses_extension(gltf, "EXT_feature_metadata") {
        each_primitive_mut(gltf, |primitive| {
            if let Some(extension) = extension_mut(primitive, "EXT_feature_metadata") {
                each_array_mut(extension.get_mut("featureIdTextures"), |texture| {
                    let ids = js::object_mut(texture.get_mut("featureIds"), "texture")?;
                    shift_texture_info(ids.get_mut("texture"), id)
                })?;
            }
            Ok(())
        })?;
        let extension = root_extension_mut(gltf, "EXT_feature_metadata")?;
        each_property_mut(extension.get_mut("featureTextures"), |texture, _| {
            if let Some(properties) = texture.get_mut("properties").filter(|p| !p.is_null()) {
                each_property_mut(Some(properties), |property, _| shift_texture_info(property.get_mut("texture"), id))?;
            }
            Ok(())
        })?;
    }
    if uses_extension(gltf, "EXT_mesh_features") {
        each_primitive_mut(gltf, |primitive| {
            if let Some(extension) = extension_mut(primitive, "EXT_mesh_features") {
                each_array_mut(extension.get_mut("featureIds"), |feature_id| {
                    if let Some(texture) = feature_id.get_mut("texture").filter(|t| !t.is_null()) {
                        shift(texture, "index", id);
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })?;
    }
    if uses_extension(gltf, "EXT_structural_metadata") {
        let extension = root_extension_mut(gltf, "EXT_structural_metadata")?;
        each_array_mut(extension.get_mut("propertyTextures"), |texture| {
            each_property_mut(texture.get_mut("properties"), |property, _| {
                shift(property, "index", id);
                Ok(())
            })
        })?;
    }
    Ok(())
}

// ---- ids in use -----------------------------------------------------------

/// Property keys of the elements of `element_type` something references.
/// `value` when it is defined.
fn defined(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null())
}

fn used_ids(gltf: &Value, element_type: ElementType) -> Result<HashSet<String>> {
    let mut used = HashSet::new();
    let mut mark = |value: Option<&Value>| {
        used.insert(value.map_or_else(|| "undefined".to_string(), js::key));
    };
    match element_type {
        ElementType::Accessor => {
            for_each::mesh::<()>(gltf, |mesh, _| {
                for_each::mesh_primitive(mesh, |primitive, _| {
                    for_each::mesh_primitive_attribute::<()>(primitive, |accessor, _| {
                        mark(Some(accessor));
                        None
                    });
                    for_each::mesh_primitive_target::<()>(primitive, |target, _| {
                        for_each::mesh_primitive_target_attribute(target, |accessor, _| {
                            mark(Some(accessor));
                            None
                        })
                    });
                    if let Some(indices) = defined(primitive.get("indices")) {
                        mark(Some(indices));
                    }
                    None
                })
            });
            for_each::skin::<()>(gltf, |skin, _| {
                if let Some(matrices) = defined(skin.get("inverseBindMatrices")) {
                    mark(Some(matrices));
                }
                None
            });
            for_each::animation::<()>(gltf, |animation, _| {
                for_each::animation_sampler(animation, |sampler, _| {
                    for key in ["input", "output"] {
                        if let Some(accessor) = defined(sampler.get(key)) {
                            mark(Some(accessor));
                        }
                    }
                    None
                })
            });
            if uses_extension(gltf, "EXT_mesh_gpu_instancing") {
                let mut error = None;
                for_each::node::<()>(gltf, |node, _| {
                    let instancing = js::get(js::get(Some(node), "extensions"), "EXT_mesh_gpu_instancing");
                    if js::defined(js::get(Some(node), "extensions")) && js::defined(instancing) {
                        match js::get(instancing, "attributes").filter(|a| !a.is_null()) {
                            Some(attributes) => {
                                for key in js::for_in(Some(attributes)) {
                                    mark(js::get(Some(attributes), &key));
                                }
                            }
                            None => {
                                error = Some(PipelineError::Type("Cannot convert undefined or null to object".into()));
                                return Some(());
                            }
                        }
                    }
                    None
                });
                if let Some(e) = error {
                    return Err(e);
                }
            }
            if uses_extension(gltf, "CESIUM_primitive_outline") {
                for_each::mesh::<()>(gltf, |mesh, _| {
                    for_each::mesh_primitive(mesh, |primitive, _| {
                        let outline = js::get(js::get(Some(primitive), "extensions"), "CESIUM_primitive_outline");
                        if js::defined(js::get(Some(primitive), "extensions")) && js::defined(outline) {
                            if let Some(indices) = defined(js::get(outline, "indices")) {
                                mark(Some(indices));
                            }
                        }
                        None
                    })
                });
            }
        }
        ElementType::Buffer => {
            for_each::buffer_view::<()>(gltf, |buffer_view, _| {
                if let Some(buffer) = defined(buffer_view.get("buffer")) {
                    mark(Some(buffer));
                }
                let meshopt = js::get(js::get(Some(buffer_view), "extensions"), "EXT_meshopt_compression");
                if js::defined(js::get(Some(buffer_view), "extensions")) && js::defined(meshopt) {
                    mark(js::get(meshopt, "buffer"));
                }
                None
            });
        }
        ElementType::BufferView => {
            for which in
                [Collection::TopLevel("accessors"), Collection::Techniques("shaders"), Collection::TopLevel("images")]
            {
                for_each::collection::<()>(gltf, which, |element, _| {
                    if let Some(buffer_view) = defined(element.get("bufferView")) {
                        mark(Some(buffer_view));
                    }
                    None
                });
            }
            if uses_extension(gltf, "KHR_draco_mesh_compression") {
                for_each::mesh::<()>(gltf, |mesh, _| {
                    for_each::mesh_primitive(mesh, |primitive, _| {
                        let draco = js::get(js::get(Some(primitive), "extensions"), "KHR_draco_mesh_compression");
                        if js::defined(js::get(Some(primitive), "extensions")) && js::defined(draco) {
                            mark(js::get(draco, "bufferView"));
                        }
                        None
                    })
                });
            }
            if uses_extension(gltf, "EXT_feature_metadata") {
                let extension = js::object(
                    js::get(Some(js::object(gltf.get("extensions"), "EXT_feature_metadata")?), "EXT_feature_metadata"),
                    "featureTables",
                )?;
                let tables = js::get(Some(extension), "featureTables");
                for table in js::for_in(tables) {
                    let properties = js::get(js::get(tables, &table), "properties");
                    if js::defined(properties) {
                        for property in js::for_in(properties) {
                            let property = js::get(properties, &property);
                            for key in ["bufferView", "arrayOffsetBufferView", "stringOffsetBufferView"] {
                                if let Some(view) = defined(js::get(property, key)) {
                                    mark(Some(view));
                                }
                            }
                        }
                    }
                }
            }
            if uses_extension(gltf, "EXT_structural_metadata") {
                let extension = js::object(
                    js::get(
                        Some(js::object(gltf.get("extensions"), "EXT_structural_metadata")?),
                        "EXT_structural_metadata",
                    ),
                    "propertyTables",
                )?;
                for_each::object::<()>(js::get(Some(extension), "propertyTables"), |table, _| {
                    let properties = js::get(Some(table), "properties");
                    for property in js::for_in(properties) {
                        let property = js::get(properties, &property);
                        for key in ["values", "arrayOffsets", "stringOffsets"] {
                            if let Some(view) = defined(js::get(property, key)) {
                                mark(Some(view));
                            }
                        }
                    }
                    None
                });
            }
        }
        ElementType::Image => {
            for_each::texture::<()>(gltf, |texture, _| {
                if let Some(source) = defined(texture.get("source")) {
                    mark(Some(source));
                }
                let extensions = js::get(Some(texture), "extensions");
                if js::defined(extensions) && js::defined(js::get(extensions, "EXT_texture_webp")) {
                    mark(js::get(js::get(extensions, "EXT_texture_webp"), "source"));
                } else if js::defined(extensions) && js::defined(js::get(extensions, "KHR_texture_basisu")) {
                    mark(js::get(js::get(extensions, "KHR_texture_basisu"), "source"));
                }
                None
            });
        }
        ElementType::Mesh => {
            let meshes = gltf.get("meshes");
            for_each::node::<()>(gltf, |node, _| {
                let mesh_id = defined(node.get("mesh"))?;
                let mesh = js::index(meshes, mesh_id).filter(|m| !m.is_null());
                let has_primitives = match mesh.and_then(|m| m.get("primitives")) {
                    Some(Value::Array(primitives)) => !primitives.is_empty(),
                    Some(Value::String(s)) => !s.is_empty(),
                    _ => false,
                };
                if has_primitives {
                    mark(Some(mesh_id));
                }
                None
            });
        }
        ElementType::Node => return used_node_ids(gltf),
        ElementType::Material => {
            for_each::mesh::<()>(gltf, |mesh, _| {
                for_each::mesh_primitive(mesh, |primitive, _| {
                    if let Some(material) = defined(primitive.get("material")) {
                        mark(Some(material));
                    }
                    None
                })
            });
        }
        ElementType::Texture => {
            for_each::material::<()>(gltf, |material, _| {
                for_each_texture_in_material::<()>(material, |index, _| {
                    mark(index);
                    None
                })
            });
            if uses_extension(gltf, "EXT_feature_metadata") {
                let mut error = None;
                for_each::mesh::<()>(gltf, |mesh, _| {
                    for_each::mesh_primitive(mesh, |primitive, _| {
                        let extension = js::get(js::get(Some(primitive), "extensions"), "EXT_feature_metadata");
                        if js::defined(js::get(Some(primitive), "extensions")) && js::defined(extension) {
                            let textures = js::get(extension, "featureIdTextures");
                            if js::defined(textures) {
                                for_each::object::<()>(textures, |texture, _| {
                                    let info = js::get(js::get(Some(texture), "featureIds"), "texture");
                                    match info.filter(|i| !i.is_null()) {
                                        Some(info) => mark(info.get("index")),
                                        None => error = Some(type_error("index")),
                                    }
                                    None
                                });
                            }
                        }
                        error.is_some().then_some(())
                    })
                });
                if let Some(e) = error {
                    return Err(e);
                }
                let extension = js::object(
                    js::get(Some(js::object(gltf.get("extensions"), "EXT_feature_metadata")?), "EXT_feature_metadata"),
                    "featureTextures",
                )?;
                let textures = js::get(Some(extension), "featureTextures");
                for texture in js::for_in(textures) {
                    let properties = js::get(js::get(textures, &texture), "properties");
                    if js::defined(properties) {
                        for property in js::for_in(properties) {
                            let info = js::object(js::get(js::get(properties, &property), "texture"), "index")?;
                            mark(info.get("index"));
                        }
                    }
                }
            }
            if uses_extension(gltf, "EXT_mesh_features") {
                for_each::mesh::<()>(gltf, |mesh, _| {
                    for_each::mesh_primitive(mesh, |primitive, _| {
                        let extension = js::get(js::get(Some(primitive), "extensions"), "EXT_mesh_features");
                        if js::defined(js::get(Some(primitive), "extensions")) && js::defined(extension) {
                            for_each::object::<()>(js::get(extension, "featureIds"), |feature_id, _| {
                                if let Some(texture) = defined(feature_id.get("texture")) {
                                    mark(texture.get("index"));
                                }
                                None
                            });
                        }
                        None
                    })
                });
            }
            if uses_extension(gltf, "EXT_structural_metadata") {
                let extension = js::object(
                    js::get(
                        Some(js::object(gltf.get("extensions"), "EXT_structural_metadata")?),
                        "EXT_structural_metadata",
                    ),
                    "propertyTextures",
                )?;
                for_each::object::<()>(js::get(Some(extension), "propertyTextures"), |texture, _| {
                    let properties = js::get(Some(texture), "properties");
                    for property in js::for_in(properties) {
                        mark(js::get(js::get(properties, &property), "index"));
                    }
                    None
                });
            }
        }
        ElementType::Sampler => {
            for_each::texture::<()>(gltf, |texture, _| {
                if let Some(sampler) = defined(texture.get("sampler")) {
                    mark(Some(sampler));
                }
                None
            });
        }
    }
    Ok(used)
}

fn used_node_ids(gltf: &Value) -> Result<HashSet<String>> {
    let mut used = HashSet::new();
    for_each::skin::<()>(gltf, |skin, _| {
        if let Some(skeleton) = skin.get("skeleton").filter(|s| !s.is_null()) {
            used.insert(js::key(skeleton));
        }
        for_each::skin_joint::<()>(skin, |joint| {
            used.insert(js::key(joint));
            None
        });
        None
    });
    for_each::animation::<()>(gltf, |animation, _| {
        for_each::animation_channel::<()>(animation, |channel, _| {
            let node = js::get(js::get(Some(channel), "target"), "node");
            if js::defined(channel.get("target")) && js::defined(node) {
                used.insert(js::key(node.expect("defined")));
            }
            None
        });
        None
    });
    for_each::technique::<()>(gltf, |technique, _| {
        for_each::technique_uniform::<()>(technique, |uniform, _| {
            if let Some(node) = js::get(Some(uniform), "node").filter(|n| !n.is_null()) {
                used.insert(js::key(node));
            }
            None
        });
        None
    });
    for id in for_each::ids(gltf, Collection::TopLevel("nodes")) {
        let node_id = id.value();
        if !node_is_empty(gltf, &node_id, &used, 0)? {
            used.insert(id.key());
        }
    }
    Ok(used)
}

/// Deep enough for any real node hierarchy; a cycle overflows the stack in
/// JavaScript.
const MAXIMUM_NODE_DEPTH: usize = 4096;

fn node_is_empty(gltf: &Value, node_id: &Value, used: &HashSet<String>, depth: usize) -> Result<bool> {
    if depth > MAXIMUM_NODE_DEPTH {
        return Err(PipelineError::Range("Maximum call stack size exceeded".into()));
    }
    let node = js::object(js::index(gltf.get("nodes"), node_id), "mesh")?;
    let has = |key: &str| js::defined(js::get(Some(node), key));
    let extensions = js::get(Some(node), "extensions");
    let non_empty_extensions = js::defined(extensions)
        && (js::key_count(extensions) != 0 || matches!(extensions, Some(Value::String(s)) if !s.is_empty()));
    if has("mesh")
        || has("camera")
        || has("skin")
        || has("weights")
        || has("extras")
        || non_empty_extensions
        || used.contains(&js::key(node_id))
    {
        return Ok(false);
    }
    let Some(children) = node.get("children").filter(|c| !c.is_null()) else { return Ok(true) };
    let Value::Array(children) = children else { return Err(type_error("filter")) };
    // `filter` visits every child, so a bad later child still throws.
    let mut empty = true;
    for child in children {
        empty &= node_is_empty(gltf, child, used, depth + 1)?;
    }
    Ok(empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn element_type_names_round_trip() {
        for t in ElementType::ALL {
            assert_eq!(ElementType::from_name(t.name()), Some(t));
        }
    }

    #[test]
    fn node_cycle_is_a_range_error() {
        let gltf = json!({ "nodes": [{ "children": [1] }, { "children": [0] }] });
        assert!(matches!(used_node_ids(&gltf), Err(PipelineError::Range(_))));
    }
}
