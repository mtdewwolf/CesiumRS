//! CesiumJS `BatchTableHierarchy`: the `3DTILES_batch_table_hierarchy`
//! extension, where each feature is an instance of a class and inherits the
//! properties of its parent instances.
//!
//! Instance ids are numbers or `undefined`: an id array element that is not
//! a JSON number reads as `undefined`, and an id array that is neither a JSON
//! array nor a binary reference has no elements. Cesium validates the
//! hierarchy in debug builds; here that always happens, without recursion.

use super::json_metadata_table::JsonColumn;
use super::metadata_class_property::{component_count, from_components, pack};
use super::metadata_table_property::{element_index, store_number, to_index, typed_array_view, ByteView};
use super::metadata_value::{developer, json_entries, JsMap, MetadataError, MetadataValue};
use super::AttributeType;
use crate::core::math::js_number_to_string;
use crate::core::{ComponentDatatype, TypedArray};
use crate::scene::gltf_pipeline::js;
use serde_json::Value;
use std::collections::HashSet;

type Result<T> = std::result::Result<T, MetadataError>;

/// An instance id: a number, or `undefined`.
type Id = Option<f64>;

/// `a === b` of two ids.
fn same(a: Id, b: Id) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn type_error(reading: &str) -> MetadataError {
    MetadataError::Type(format!("Cannot read properties of undefined (reading '{reading}')"))
}

#[derive(Clone, Debug, PartialEq)]
enum Ids {
    Json(Vec<Id>),
    Typed(TypedArray),
}

impl Ids {
    fn len(&self) -> usize {
        match self {
            Self::Json(v) => v.len(),
            Self::Typed(a) => a.len(),
        }
    }

    /// `ids[id]`.
    fn get(&self, id: Id) -> Id {
        let i = element_index(id?)?;
        match self {
            Self::Json(v) => v.get(i).copied().flatten(),
            Self::Typed(a) => a.get(i),
        }
    }

    fn byte_length(&self) -> usize {
        match self {
            Self::Json(_) => 0,
            Self::Typed(a) => a.len() * a.component_datatype().get_size_in_bytes(),
        }
    }
}

/// `values[id]` of a `Uint16Array`.
fn lookup16(values: &[u16], id: Id) -> Id {
    element_index(id?).and_then(|i| values.get(i)).map(|&x| f64::from(x))
}

/// The datatype `getBinaryAccessor` resolves: a name, or a GL value.
fn component_datatype(value: Option<&Value>) -> Result<ComponentDatatype> {
    match value {
        Some(Value::String(name)) => {
            ComponentDatatype::from_name(name).ok_or_else(|| developer("name is not a valid value."))
        }
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .and_then(ComponentDatatype::from_gl)
            .ok_or_else(|| developer("componentDatatype is not a valid value.")),
        _ => Err(developer("componentDatatype is required.")),
    }
}

/// `binaryBody.byteOffset + reference.byteOffset`.
fn body_offset<'a>(body: Option<&ByteView<'a>>, reference: &Value) -> Result<(ByteView<'a>, f64)> {
    let body = *body.ok_or_else(|| type_error("buffer"))?;
    Ok((body, body.byte_offset as f64 + js::to_number(reference.get("byteOffset"))))
}

/// A `classIds`, `parentCounts` or `parentIds` array: JSON, or a scalar
/// binary reference read with `length` elements.
fn read_ids(value: &Value, body: Option<&ByteView<'_>>, length: f64) -> Result<Ids> {
    if js::defined(value.get("byteOffset")) {
        let datatype = match value.get("componentType").filter(|v| !v.is_null()) {
            Some(t) => component_datatype(Some(t))?,
            None => ComponentDatatype::UnsignedShort,
        };
        let (body, offset) = body_offset(body, value)?;
        return Ok(Ids::Typed(typed_array_view(datatype, body.buffer, offset, length)?));
    }
    Ok(Ids::Json(match value {
        Value::Array(items) => items.iter().map(Value::as_f64).collect(),
        _ => Vec::new(),
    }))
}

#[derive(Clone, Debug, PartialEq)]
struct BinaryColumn {
    typed_array: TypedArray,
    /// `undefined` for an unknown `type`, which reads no elements.
    attribute_type: Option<AttributeType>,
}

#[derive(Clone, Debug, PartialEq)]
enum InstanceColumn {
    Binary(BinaryColumn),
    Json(JsonColumn),
}

impl InstanceColumn {
    fn is_defined(&self) -> bool {
        match self {
            Self::Binary(_) => true,
            Self::Json(column) => column.is_defined(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct HierarchyClass {
    name: Option<Value>,
    /// The binary properties, then the JSON ones.
    instances: JsMap<InstanceColumn>,
}

/// `Cesium3DTileBatchTable.getBinaryProperties` and `combine` of one class.
fn read_class(class: &Value, body: Option<&ByteView<'_>>) -> Result<HierarchyClass> {
    if !(class.is_object() || class.is_array()) {
        return Err(MetadataError::Type("Cannot create property 'instances' on a primitive".into()));
    }
    let length = js::to_number(class.get("length"));
    let mut binary = JsMap::new();
    let mut json = JsMap::new();
    for (name, property) in json_entries(class.get("instances")) {
        if property.is_null() {
            return Err(MetadataError::Type("Cannot read properties of null (reading 'byteOffset')".into()));
        }
        if !js::defined(property.get("byteOffset")) {
            json.insert(name, InstanceColumn::Json(JsonColumn::from_json(property)));
            continue;
        }
        if !js::defined(property.get("componentType")) {
            return Err(MetadataError::Runtime("componentType is required.".into()));
        }
        if !js::defined(property.get("type")) {
            return Err(MetadataError::Runtime("type is required.".into()));
        }
        if body.is_none() {
            return Err(MetadataError::Runtime(format!("Property {name} requires a batch table binary.")));
        }
        let datatype = component_datatype(property.get("componentType"))?;
        let attribute_type = property.get("type").and_then(Value::as_str).and_then(AttributeType::from_str_value);
        let components = attribute_type.map_or(f64::NAN, |t| t.get_number_of_components() as f64);
        let (body, offset) = body_offset(body, property)?;
        let typed_array = typed_array_view(datatype, body.buffer, offset, components * length)?;
        binary.insert(name, InstanceColumn::Binary(BinaryColumn { typed_array, attribute_type }));
    }
    for (name, column) in json.iter() {
        if !binary.contains_key(name) {
            binary.insert(name, column.clone());
        }
    }
    Ok(HierarchyClass { name: class.get("name").cloned(), instances: binary })
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchTableHierarchy {
    classes: Vec<HierarchyClass>,
    class_ids: Ids,
    class_indexes: Vec<u16>,
    parent_counts: Option<Ids>,
    parent_indexes: Option<Vec<u16>>,
    parent_ids: Option<Ids>,
    byte_length: usize,
}

impl BatchTableHierarchy {
    /// The extension JSON, with the batch table binary its references read.
    pub fn new(extension: &Value, binary_body: Option<ByteView<'_>>) -> Result<Self> {
        if !(extension.is_object() || extension.is_array()) {
            return Err(developer("Expected options.extension to be typeof object"));
        }
        let body = binary_body.as_ref();
        let instances_length = js::to_number(extension.get("instancesLength"));
        let instance_count = to_index(instances_length, "length")?;
        let loop_count = if instances_length > 0.0 { instances_length.ceil() as usize } else { 0 };
        let mut byte_length = 0;

        let class_ids = extension.get("classIds").filter(|v| !v.is_null()).ok_or_else(|| type_error("byteOffset"))?;
        let class_ids = read_ids(class_ids, body, instances_length)?;
        byte_length += class_ids.byte_length();

        let mut parent_ids_length = instances_length;
        let mut parent_counts = None;
        let mut parent_indexes = None;
        if let Some(counts) = extension.get("parentCounts").filter(|v| !v.is_null()) {
            let counts = read_ids(counts, body, instances_length)?;
            byte_length += counts.byte_length();
            let mut indexes = vec![0u16; instance_count];
            parent_ids_length = 0.0;
            for i in 0..loop_count {
                if let Some(x) = indexes.get_mut(i) {
                    *x = crate::core::math::to_uint32(parent_ids_length) as u16;
                }
                parent_ids_length += counts.get(Some(i as f64)).unwrap_or(f64::NAN);
            }
            byte_length += indexes.len() * 2;
            parent_counts = Some(counts);
            parent_indexes = Some(indexes);
        }

        let parent_ids = match extension.get("parentIds").filter(|v| !v.is_null()) {
            Some(ids) => {
                let ids = read_ids(ids, body, parent_ids_length)?;
                byte_length += ids.byte_length();
                Some(ids)
            }
            None => None,
        };

        let classes_json = extension.get("classes").filter(|v| !v.is_null()).ok_or_else(|| type_error("length"))?;
        let mut classes = Vec::new();
        if let Value::Array(items) = classes_json {
            for class in items {
                let class = read_class(class, body)?;
                byte_length += class
                    .instances
                    .values()
                    .map(|c| match c {
                        InstanceColumn::Binary(b) => {
                            b.typed_array.len() * b.typed_array.component_datatype().get_size_in_bytes()
                        }
                        InstanceColumn::Json(_) => 0,
                    })
                    .sum::<usize>();
                classes.push(class);
            }
        }

        // An id past the classes counts as 0: `undefined`, then NaN, stored
        // in a Uint16Array.
        let mut class_counts = vec![0u16; classes.len()];
        let mut class_indexes = vec![0u16; instance_count];
        for i in 0..loop_count {
            let class_id = class_ids.get(Some(i as f64)).and_then(element_index);
            let index = match class_id.and_then(|c| class_counts.get_mut(c)) {
                Some(count) => {
                    let index = *count;
                    *count = count.wrapping_add(1);
                    index
                }
                None => 0,
            };
            if let Some(x) = class_indexes.get_mut(i) {
                *x = index;
            }
        }
        byte_length += class_indexes.len() * 2;

        let hierarchy =
            Self { classes, class_ids, class_indexes, parent_counts, parent_indexes, parent_ids, byte_length };
        hierarchy.validate()?;
        Ok(hierarchy)
    }

    /// The bytes of the typed arrays read and made at construction.
    pub fn byte_length(&self) -> usize {
        self.byte_length
    }

    /// `parentCounts[id]` and `parentIndexes[id]`, or one parent at `id`.
    fn parents_of(&self, id: Id) -> (f64, Id) {
        match (&self.parent_counts, &self.parent_indexes) {
            (Some(counts), Some(indexes)) => (counts.get(id).unwrap_or(f64::NAN), lookup16(indexes, id)),
            _ => (1.0, id),
        }
    }

    /// The parents of `id` in order, with the run of `undefined`s past the
    /// end of the parent ids collapsed to one, which is all either walk sees.
    fn parent_list(&self, parent_ids: &Ids, id: Id) -> Vec<Id> {
        let (count, index) = self.parents_of(id);
        let mut parents = Vec::new();
        let mut i = 0usize;
        while (i as f64) < count {
            let at = index.map(|x| x + i as f64);
            parents.push(parent_ids.get(at));
            if at.is_none_or(|x| x >= parent_ids.len() as f64) {
                break;
            }
            i += 1;
        }
        parents
    }

    fn validate(&self) -> Result<()> {
        let Some(parent_ids) = &self.parent_ids else { return Ok(()) };
        let instances_length = self.class_ids.len() as f64;
        let mut path: Vec<Id> = Vec::new();
        let enter = |id: Id, path: &mut Vec<Id>| -> Result<(Id, Vec<Id>, usize)> {
            if let Some(x) = id.filter(|&x| x >= instances_length) {
                return Err(developer(format!(
                    "Parent index {} exceeds the total number of instances: {}",
                    js_number_to_string(x),
                    js_number_to_string(instances_length)
                )));
            }
            if path.iter().any(|&p| same(p, id)) {
                return Err(developer("Circular dependency detected in the batch table hierarchy."));
            }
            path.push(id);
            Ok((id, self.parent_list(parent_ids, id), 0))
        };
        for i in 0..self.class_ids.len() {
            let mut frames = vec![enter(Some(i as f64), &mut path)?];
            while let Some((id, parents, next)) = frames.last_mut() {
                let Some(&parent) = parents.get(*next) else {
                    frames.pop();
                    path.pop();
                    continue;
                };
                *next += 1;
                if !same(parent, *id) {
                    let frame = enter(parent, &mut path)?;
                    frames.push(frame);
                }
            }
        }
        Ok(())
    }

    /// Visits the instance and its ancestors until `visit` returns a value.
    fn traverse<T>(&self, start: Id, mut visit: impl FnMut(Id) -> Result<Option<T>>) -> Result<Option<T>> {
        let Some(parent_ids) = &self.parent_ids else { return visit(start) };
        if self.parent_counts.is_some() {
            let key = |id: Id| id.map(|x| if x == 0.0 { 0 } else { x.to_bits() });
            let mut visited = HashSet::new();
            let mut stack = vec![start];
            while let Some(id) = stack.pop() {
                if !visited.insert(key(id)) {
                    continue;
                }
                if let Some(result) = visit(id)? {
                    return Ok(Some(result));
                }
                stack.extend(self.parent_list(parent_ids, id).into_iter().filter(|&p| !same(p, id)));
            }
            return Ok(None);
        }
        let mut id = start;
        // Validation leaves no cycles; the bound only guards that.
        for _ in 0..=self.class_ids.len() + 1 {
            if let Some(result) = visit(id)? {
                return Ok(Some(result));
            }
            let parent = parent_ids.get(id);
            if same(parent, id) {
                break;
            }
            id = parent;
        }
        Ok(None)
    }

    fn class_index(&self, id: Id) -> Result<usize> {
        self.class_ids
            .get(id)
            .and_then(element_index)
            .filter(|&c| c < self.classes.len())
            .ok_or_else(|| type_error("instances"))
    }

    fn class_of(&self, id: Id) -> Result<&HierarchyClass> {
        Ok(&self.classes[self.class_index(id)?])
    }

    /// Whether the instance or an ancestor has the property.
    pub fn has_property(&self, batch_id: usize, property_id: &str) -> Result<bool> {
        let found = self.traverse(Some(batch_id as f64), |id| {
            Ok(self.class_of(id)?.instances.get(property_id).is_some_and(InstanceColumn::is_defined).then_some(()))
        })?;
        Ok(found.is_some())
    }

    /// Whether any class has the property.
    pub fn property_exists(&self, property_id: &str) -> bool {
        self.classes.iter().any(|c| c.instances.get(property_id).is_some_and(InstanceColumn::is_defined))
    }

    /// The properties of the instance and its ancestors, nearest first.
    pub fn get_property_ids(&self, batch_id: usize) -> Result<Vec<String>> {
        let mut ids: Vec<String> = Vec::new();
        self.traverse::<()>(Some(batch_id as f64), |id| {
            for name in self.class_of(id)?.instances.keys() {
                if !ids.iter().any(|x| x == name) {
                    ids.push(name.to_string());
                }
            }
            Ok(None)
        })?;
        Ok(ids)
    }

    /// The nearest defined value of the property.
    pub fn get_property(&self, batch_id: usize, property_id: &str) -> Result<Option<MetadataValue>> {
        self.traverse(Some(batch_id as f64), |id| {
            let class = self.class_of(id)?;
            let index = lookup16(&self.class_indexes, id);
            let value = match class.instances.get(property_id) {
                Some(InstanceColumn::Binary(column)) => get_binary(column, index)?,
                Some(InstanceColumn::Json(column)) if column.is_defined() => {
                    index.and_then(element_index).and_then(|i| column.get(i))
                }
                _ => None,
            };
            Ok(value.filter(|v| !matches!(v, MetadataValue::Null)))
        })
    }

    /// Stores the value in the instance's own property. `false` when no
    /// instance on the way up has it; an inherited one is read-only.
    pub fn set_property(&mut self, batch_id: usize, property_id: &str, value: MetadataValue) -> Result<bool> {
        let batch = Some(batch_id as f64);
        let found = self.traverse(batch, |id| {
            Ok(self.class_of(id)?.instances.get(property_id).is_some_and(InstanceColumn::is_defined).then_some(id))
        })?;
        let Some(instance) = found else { return Ok(false) };
        if !same(instance, batch) {
            return Err(developer(format!("Inherited property \"{property_id}\" is read-only.")));
        }
        let class = self.class_index(instance)?;
        let index = lookup16(&self.class_indexes, instance);
        match self.classes[class].instances.get_mut(property_id) {
            Some(InstanceColumn::Binary(column)) => set_binary(column, index, &value)?,
            Some(InstanceColumn::Json(column)) => {
                if let Some(i) = index.and_then(element_index) {
                    column.set(i, value)?;
                }
            }
            None => {}
        }
        Ok(true)
    }

    /// Whether the instance or an ancestor is of the class.
    pub fn is_class(&self, batch_id: usize, class_name: &str) -> Result<bool> {
        let found = self.traverse(Some(batch_id as f64), |id| {
            Ok(self.class_of(id)?.name.as_ref().and_then(Value::as_str).filter(|&n| n == class_name).map(|_| ()))
        })?;
        Ok(found.is_some())
    }

    /// The `name` of the instance's class.
    pub fn get_class_name(&self, batch_id: usize) -> Result<Option<&Value>> {
        Ok(self.class_of(Some(batch_id as f64))?.name.as_ref())
    }
}

fn get_binary(column: &BinaryColumn, index: Id) -> Result<Option<MetadataValue>> {
    let attribute_type = column.attribute_type.ok_or_else(|| type_error("unpack"))?;
    let math = attribute_type.get_math_type();
    let n = component_count(math);
    let at =
        |k: usize| index.and_then(|i| element_index(i * n as f64 + k as f64)).and_then(|i| column.typed_array.get(i));
    if n == 1 {
        return Ok(at(0).map(MetadataValue::Number));
    }
    let components: Vec<f64> = (0..n).map(|k| at(k).unwrap_or(f64::NAN)).collect();
    Ok(Some(from_components(math, &components)))
}

fn set_binary(column: &mut BinaryColumn, index: Id, value: &MetadataValue) -> Result<()> {
    let attribute_type = column.attribute_type.ok_or_else(|| type_error("pack"))?;
    let math = attribute_type.get_math_type();
    let n = component_count(math);
    let mut write = |k: usize, x: f64| {
        if let Some(i) = index.and_then(|i| element_index(i * n as f64 + k as f64)) {
            column.typed_array.set(i, x);
        }
    };
    if n == 1 {
        let x = store_number(Some(value))?;
        write(0, x);
        return Ok(());
    }
    let type_name = match value {
        MetadataValue::Number(_) => Some("number"),
        MetadataValue::String(_) => Some("string"),
        MetadataValue::Boolean(_) => Some("boolean"),
        MetadataValue::BigInt(_) => Some("bigint"),
        _ => None,
    };
    if let Some(t) = type_name {
        return Err(developer(format!("Expected value to be typeof object, actual typeof was {t}")));
    }
    let MetadataValue::Array(components) = pack(math, value)? else { unreachable!("pack makes an array") };
    for (k, component) in components.iter().enumerate() {
        let x = store_number(Some(component))?;
        write(k, x);
    }
    Ok(())
}
