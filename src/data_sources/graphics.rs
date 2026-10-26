//! Shared storage and contracts for the concrete DataSources graphics classes.
//!
//! Cesium's graphics objects are plain JavaScript objects with accessor
//! descriptors rather than `Property` implementations. This module keeps that
//! distinction: each graphics class owns one [`PropertyDescriptor`] slot per
//! documented field and exposes `definitionChanged` through the local
//! payload-bearing event adapter. The `Graphics` trait is the native analogue
//! of Cesium's duck-typed graphics object, used by tests and later visualizers.
use super::{
    create_property_descriptor, entity_component::component_equals, ChangeEvent, ComponentRef, ConstantProperty,
    DefinitionChanged, DynamicValue, EntityComponent, JsonReference, PropertyBag, PropertyChange, PropertyComponent,
    PropertyDescriptor, PropertyInput,
};
use crate::core::RuntimeError;
use std::{cell::RefCell, rc::Rc};
/// The common read/write contract of a concrete graphics definition.
pub trait Graphics: EntityComponent {
    fn empty() -> Self
    where
        Self: Sized;
    fn from_options_values(options: Vec<(String, PropertyInput)>) -> Result<Self, RuntimeError>
    where
        Self: Sized;
    fn property_names(&self) -> &'static [&'static str];
    fn definition_changes(&self) -> &ChangeEvent<PropertyChange>;
    fn get(&self, name: &str) -> Option<PropertyInput>;
    fn set(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError>;
    fn merge_graphics(&self, source: &dyn EntityComponent) -> Result<(), RuntimeError>;
    fn clone_graphics(&self) -> Result<ComponentRef, RuntimeError>;
}
/// Ordered property slots plus the graphics object's definition event.
pub(crate) struct GraphicsProperties {
    fields: RefCell<Vec<PropertyDescriptor>>,
    changed: ChangeEvent<PropertyChange>,
}
impl GraphicsProperties {
    pub(crate) fn new(names: &'static [&'static str]) -> Self {
        let changed = ChangeEvent::new();
        let fields = names
            .iter()
            .map(|name| {
                // Cesium's createMaterialPropertyDescriptor coerces a raw Color
                // into ColorMaterialProperty. Other material inputs stay
                // unsupported until ImageMaterialProperty is ported.
                let factory: Option<super::ComponentFactory> = match *name {
                    "material" | "depthFailMaterial" => {
                        Some(super::create_material_property_descriptor::material_factory())
                    }
                    // PolygonGraphics accepts a position array where a
                    // PolygonHierarchy is expected.
                    "hierarchy" => Some(Rc::new(create_polygon_hierarchy_component)),
                    // ModelGraphics articulations and environment map options
                    // are PropertyBags; nested bag merging is handled below.
                    "articulations" | "environmentMapOptions" => Some(Rc::new(create_property_bag_component)),
                    _ => None,
                };
                create_property_descriptor(*name, false, factory, &changed)
            })
            .collect();
        Self { fields: RefCell::new(fields), changed }
    }
    pub(crate) fn definition_changed(&self) -> &DefinitionChanged {
        self.changed.definition_changed()
    }
    pub(crate) fn changes(&self) -> &ChangeEvent<PropertyChange> {
        &self.changed
    }
    pub(crate) fn get(&self, name: &str) -> Option<PropertyInput> {
        self.fields.borrow().iter().find(|f| f.name() == name).and_then(PropertyDescriptor::get)
    }
    pub(crate) fn set(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        let field = self
            .fields
            .borrow()
            .iter()
            .find(|f| f.name() == name)
            .cloned()
            .ok_or_else(|| RuntimeError::new(format!("{name} is not a graphics property.")))?;
        field.set(value)
    }
    pub(crate) fn is_constant(&self) -> bool {
        self.fields.borrow().iter().all(|field| {
            field.get().is_none_or(|value| match value {
                PropertyInput::Value(_) => true,
                PropertyInput::Component(component) => component.is_constant(),
            })
        })
    }
    pub(crate) fn equals(&self, other: &Self) -> Result<bool, RuntimeError> {
        for field in self.fields.borrow().iter() {
            if !component_equals(field.get().as_ref(), other.get(field.name()).as_ref())? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Cesium's `merge` uses `this.field = this.field ?? source.field`, so an
    /// existing slot wins and ordinary absent slots share the source property object.
    /// Model nested bags are copied or merged separately.
    pub(crate) fn merge(&self, source: &Self, merge_names: &[&str], nested_names: &[&str]) -> Result<(), RuntimeError> {
        for name in merge_names {
            let source_value = source.get(name);
            if nested_names.contains(name) && self.merge_nested(name, source_value.as_ref())? {
                continue;
            }
            if self.get(name).is_none() {
                if let Some(value) = source_value {
                    self.set(name, Some(value))?;
                }
            }
        }
        Ok(())
    }
    /// Applies constructor options, which upstream reads in `merge` order.
    pub(crate) fn merge_values(
        &self,
        values: &[(String, PropertyInput)],
        merge_names: &[&str],
        nested_names: &[&str],
    ) -> Result<(), RuntimeError> {
        for name in merge_names {
            let source_value = values.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value);
            if nested_names.contains(name) && self.merge_nested(name, source_value)? {
                continue;
            }
            if self.get(name).is_none() {
                if let Some(value) = source_value {
                    self.set(name, Some(value.clone()))?;
                }
            }
        }
        Ok(())
    }
    /// `PropertyBag`-valued fields (ModelGraphics node transformations and
    /// articulations) merge hierarchically when both sides expose a bag.
    /// Articulations and environment map options also coerce raw objects in
    /// `GraphicsProperties::new`; raw node transformations still need the
    /// unported NodeTransformationProperty factory and are documented.
    fn merge_nested(&self, name: &str, source: Option<&PropertyInput>) -> Result<bool, RuntimeError> {
        let source_component = match source {
            Some(PropertyInput::Component(component)) => component.clone(),
            Some(PropertyInput::Value(DynamicValue::Null)) => return Ok(true),
            Some(PropertyInput::Value(value)) if name == "articulations" => {
                create_property_bag_component(value.clone())?
            }
            _ => return Ok(false),
        };
        let Some(source_bag) = source_component.as_any().downcast_ref::<PropertyBag>() else { return Ok(false) };
        match self.get(name) {
            None => {
                let bag = Rc::new(PropertyBag::new());
                bag.merge(source_bag)?;
                self.set(name, Some(PropertyInput::Component(bag)))?;
                Ok(true)
            }
            Some(PropertyInput::Component(target)) => {
                let Some(target_bag) = target.as_any().downcast_ref::<PropertyBag>() else { return Ok(false) };
                target_bag.merge(source_bag)?;
                Ok(true)
            }
            Some(_) => Ok(false),
        }
    }
    /// Cesium's `clone(result)` assigns every field, including undefined.
    pub(crate) fn clone_into(&self, target: &Self, clone_names: &[&str]) {
        for name in clone_names {
            let value = self.get(name);
            // The target has the same slots and names, so this cannot fail.
            let _ = target.set(name, value);
        }
    }
}
/// Cesium's `createPolygonHierarchyProperty`: a raw position array becomes a
/// `PolygonHierarchy`-shaped object with an empty `holes` array.
fn create_polygon_hierarchy_component(value: DynamicValue) -> Result<ComponentRef, RuntimeError> {
    let value = match value {
        DynamicValue::Json(json) => {
            let positions = match &*json.value() {
                serde_json::Value::Array(positions) => Some(positions.clone()),
                _ => None,
            };
            match positions {
                Some(positions) => {
                    let mut object = serde_json::Map::new();
                    object.insert("positions".into(), serde_json::Value::Array(positions));
                    object.insert("holes".into(), serde_json::Value::Array(vec![]));
                    DynamicValue::Json(JsonReference::object(object))
                }
                None => DynamicValue::Json(json),
            }
        }
        other => other,
    };
    Ok(PropertyComponent::wrap(Rc::new(ConstantProperty::new(Some(value)))))
}

/// Cesium's model PropertyBag factories for raw objects and records.
fn create_property_bag_component(value: DynamicValue) -> Result<ComponentRef, RuntimeError> {
    let bag = match value {
        DynamicValue::Json(json) => {
            let value = json.value().clone();
            if value.is_object() {
                PropertyBag::from_json(value)?
            } else if let serde_json::Value::Array(values) = value {
                let entries = values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| (index.to_string(), PropertyInput::Value(DynamicValue::from_json(value))))
                    .collect();
                PropertyBag::from_values(entries, None)?
            } else {
                Rc::new(PropertyBag::new())
            }
        }
        DynamicValue::Record(values) => {
            let entries = values
                .into_iter()
                .filter_map(|(name, value)| value.map(|value| (name, PropertyInput::Value(value))))
                .collect();
            PropertyBag::from_values(entries, None)?
        }
        _ => Rc::new(PropertyBag::new()),
    };
    Ok(bag as ComponentRef)
}
/// Builds the native graphics component for a Cesium graphics class.
///
/// Each concrete module invokes this with its documented property order, plus
/// the distinct order used by upstream `merge` and `clone` implementations.
/// Keeping those lists separate preserves pinned quirks such as fields that are
/// accepted by a setter but omitted from constructor-option merging.
macro_rules! graphics_definition {
    (
        $(#[$meta:meta])*
        $ty:ident;
        properties: [ $($name:literal => $getter:ident, $setter:ident),* $(,)? ];
        merge: [ $($merge_name:literal),* $(,)? ];
        clone: [ $($clone_name:literal),* $(,)? ];
        nested: [ $($nested_name:literal),* $(,)? ];
    ) => {
        $(#[$meta])*
        pub struct $ty {
            properties: $crate::data_sources::graphics::GraphicsProperties,
        }
        impl $ty {
            pub const PROPERTY_NAMES: &'static [&'static str] = &[$($name),*];
            const MERGE_PROPERTY_NAMES: &'static [&'static str] = &[$($merge_name),*];
            const CLONE_PROPERTY_NAMES: &'static [&'static str] = &[$($clone_name),*];
            const NESTED_PROPERTY_NAMES: &'static [&'static str] = &[$($nested_name),*];

            pub fn new() -> Self {
                Self { properties: $crate::data_sources::graphics::GraphicsProperties::new(Self::PROPERTY_NAMES) }
            }
            /// Applies constructor options in upstream `merge` order. Options
            /// for fields that upstream does not merge are ignored.
            pub fn with_options<I>(options: I) -> Result<Self, RuntimeError>
            where
                I: IntoIterator<Item = (String, PropertyInput)>,
            {
                let value = Self::new();
                value.merge_options(options)?;
                Ok(value)
            }
            pub fn merge_options<I>(&self, options: I) -> Result<(), RuntimeError>
            where
                I: IntoIterator<Item = (String, PropertyInput)>,
            {
                let options = options.into_iter().collect::<Vec<_>>();
                let selected = Self::MERGE_PROPERTY_NAMES
                    .iter()
                    .filter_map(|name| {
                        options.iter().rev().find(|(key, _)| key == name).map(|(_, value)| ((*name).to_owned(), value.clone()))
                    })
                    .collect::<Vec<_>>();
                self.properties.merge_values(&selected, Self::MERGE_PROPERTY_NAMES, Self::NESTED_PROPERTY_NAMES)
            }
            pub fn from_dynamic(options: DynamicValue) -> Result<Self, RuntimeError> {
                let value = Self::new();
                value.merge_dynamic_options_value(options)?;
                Ok(value)
            }
            fn merge_dynamic_options_value(&self, value: DynamicValue) -> Result<(), RuntimeError> {
                let options = match value {
                    DynamicValue::Null => vec![],
                    DynamicValue::Record(values) => values
                        .into_iter()
                        .filter_map(|(name, value)| value.map(|value| (name, PropertyInput::Value(value))))
                        .collect(),
                    DynamicValue::Json(value) => {
                        let value = value.value();
                        let object = value
                            .as_object()
                            .ok_or_else(|| RuntimeError::new("graphics options require an object"))?;
                        object
                            .iter()
                            .map(|(name, value)| {
                                (name.clone(), PropertyInput::Value(DynamicValue::from_json(value.clone())))
                            })
                            .collect()
                    }
                    _ => return Err(RuntimeError::new("graphics options require an object")),
                };
                self.merge_options(options)
            }
            pub fn merge_definition(&self, source: &Self) -> Result<(), RuntimeError> {
                self.properties.merge(&source.properties, Self::MERGE_PROPERTY_NAMES, Self::NESTED_PROPERTY_NAMES)
            }
            /// `clone()` without a result constructs a new graphics object from
            /// `this`, so upstream routes through the constructor's merge path.
            pub fn clone_definition(&self) -> Result<Self, RuntimeError> {
                let target = Self::new();
                // The target is empty, so merge fills every upstream merge field.
                target.merge_definition(self)?;
                Ok(target)
            }
            pub fn clone_definition_into(&self, target: &Self) {
                self.properties.clone_into(&target.properties, Self::CLONE_PROPERTY_NAMES);
            }
            /// Payload-bearing definition event (native counterpart of the
            /// arguments Cesium passes to `definitionChanged` listeners).
            pub fn definition_changes(
                &self,
            ) -> &$crate::data_sources::ChangeEvent<$crate::data_sources::PropertyChange> {
                self.properties.changes()
            }
            $(
                pub fn $getter(&self) -> Option<PropertyInput> {
                    self.properties.get($name)
                }
                pub fn $setter(&self, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
                    self.properties.set($name, value)
                }
            )*
        }
        impl Default for $ty {
            fn default() -> Self {
                Self::new()
            }
        }
        impl $crate::data_sources::EntityComponent for $ty {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn definition_changed(&self) -> &DefinitionChanged {
                self.properties.definition_changed()
            }
            fn is_constant(&self) -> bool {
                self.properties.is_constant()
            }
            fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
                other.as_any().downcast_ref::<Self>().map_or(Ok(false), |other| self.properties.equals(&other.properties))
            }
            fn supports_merge(&self) -> bool {
                true
            }
            fn supports_clone(&self) -> bool {
                true
            }
            fn merge_component(&self, source: &dyn EntityComponent) -> Result<(), RuntimeError> {
                let Some(source) = source.as_any().downcast_ref::<Self>() else {
                    return Err(RuntimeError::new("graphics component type mismatch"));
                };
                self.merge_definition(source)
            }
            fn merge_dynamic_options(&self, source: DynamicValue) -> Result<(), RuntimeError> {
                self.merge_dynamic_options_value(source)
            }
            fn clone_component(&self, result: Option<ComponentRef>) -> Result<ComponentRef, RuntimeError> {
                if let Some(result) = result {
                    if let Some(target) = result.as_any().downcast_ref::<Self>() {
                        self.clone_definition_into(target);
                        return Ok(result);
                    }
                }
                Ok(std::rc::Rc::new(self.clone_definition()?))
            }
            fn get_property(&self, name: &str) -> Option<PropertyInput> {
                self.properties.get(name)
            }
        }
        impl $crate::data_sources::graphics::Graphics for $ty {
            fn empty() -> Self {
                Self::new()
            }
            fn from_options_values(options: Vec<(String, PropertyInput)>) -> Result<Self, RuntimeError> {
                Self::with_options(options)
            }
            fn property_names(&self) -> &'static [&'static str] {
                Self::PROPERTY_NAMES
            }
            fn definition_changes(&self) -> &$crate::data_sources::ChangeEvent<$crate::data_sources::PropertyChange> {
                self.properties.changes()
            }
            fn get(&self, name: &str) -> Option<PropertyInput> {
                self.properties.get(name)
            }
            fn set(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
                self.properties.set(name, value)
            }
            fn merge_graphics(&self, source: &dyn EntityComponent) -> Result<(), RuntimeError> {
                self.merge_component(source)
            }
            fn clone_graphics(&self) -> Result<ComponentRef, RuntimeError> {
                self.clone_component(None)
            }
        }
    };
}
pub(crate) use graphics_definition;
