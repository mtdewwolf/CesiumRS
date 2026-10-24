use super::{
    create_property_descriptor::{create_property_descriptor, ComponentFactory},
    entity_component::component_equals,
    property::system_clock,
    BagValues, ChangeEvent, DefinitionChanged, DynamicValue, EntityComponent, PropertyChange, PropertyClock,
    PropertyDescriptor, PropertyInput, Subscription,
};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, cell::RefCell, rc::Rc};
pub struct PropertyBag {
    fields: RefCell<Vec<PropertyDescriptor>>,
    changed: DefinitionChanged,
    changes: ChangeEvent<PropertyChange>,
    _forward: Subscription,
    clock: PropertyClock,
}
impl Default for PropertyBag {
    fn default() -> Self {
        Self::new()
    }
}
impl PropertyBag {
    pub fn new() -> Self {
        Self::with_clock(system_clock())
    }
    pub fn with_clock(clock: PropertyClock) -> Self {
        let changes = ChangeEvent::new();
        let changed = DefinitionChanged::new();
        let weak = changed.downgrade();
        let forward = changes.add_listener(move |_| {
            if let Some(event) = weak.upgrade() {
                event.raise();
            }
        });
        Self { fields: RefCell::new(vec![]), changed, changes, _forward: forward, clock }
    }
    pub fn from_values(
        values: Vec<(String, PropertyInput)>,
        factory: Option<ComponentFactory>,
    ) -> Result<Rc<Self>, RuntimeError> {
        let bag = Rc::new(Self::new());
        bag.merge_values(&values, factory)?;
        Ok(bag)
    }
    pub fn from_json(value: serde_json::Value) -> Result<Rc<Self>, RuntimeError> {
        if value.is_null() {
            return Ok(Rc::new(Self::new()));
        }
        let serde_json::Value::Object(values) = value else {
            return Err(RuntimeError::new("properties requires an object"));
        };
        let entries = crate::core::query::js_object_order(
            values.into_iter().map(|(k, v)| (k, PropertyInput::Value(DynamicValue::from_json(v)))).collect(),
        );
        Self::from_values(entries, None)
    }
    pub fn property_names(&self) -> Vec<String> {
        self.fields.borrow().iter().map(|f| f.name().to_owned()).collect()
    }
    pub fn has_property(&self, name: &str) -> bool {
        self.fields.borrow().iter().any(|f| f.name() == name)
    }
    pub fn changes(&self) -> &ChangeEvent<PropertyChange> {
        &self.changes
    }
    pub fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    pub fn get(&self, name: &str) -> Option<PropertyInput> {
        self.fields.borrow().iter().find(|f| f.name() == name).and_then(PropertyDescriptor::get)
    }
    pub fn set(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        let field = self
            .fields
            .borrow()
            .iter()
            .find(|f| f.name() == name)
            .cloned()
            .ok_or_else(|| RuntimeError::new(format!("{name} is not a registered property.")))?;
        field.set(value)
    }
    pub fn add_property(
        &self,
        name: impl Into<String>,
        value: Option<PropertyInput>,
        factory: Option<ComponentFactory>,
    ) -> Result<(), RuntimeError> {
        let name = name.into();
        if self.has_property(&name) {
            return Err(RuntimeError::new(format!("{name} is already a registered property.")));
        }
        let field = create_property_descriptor(name, true, factory, &self.changes);
        self.fields.borrow_mut().push(field.clone());
        if value.as_ref().is_some_and(PropertyInput::is_defined) {
            field.set(value)?;
        }
        self.changed.raise();
        Ok(())
    }
    pub fn remove_property(&self, name: &str) -> Result<(), RuntimeError> {
        let removed = {
            let mut fields = self.fields.borrow_mut();
            let index = fields
                .iter()
                .position(|f| f.name() == name)
                .ok_or_else(|| RuntimeError::new(format!("{name} is not a registered property.")))?;
            fields.remove(index)
        };
        removed.deactivate();
        self.changed.raise();
        Ok(())
    }
    pub fn is_constant(&self) -> bool {
        let values = self.snapshot();
        values.iter().all(|(_, v)| match v {
            None => true,
            Some(PropertyInput::Component(c)) => c.is_constant(),
            Some(PropertyInput::Value(_)) => false,
        })
    }
    pub fn snapshot(&self) -> Vec<(String, Option<PropertyInput>)> {
        self.fields.borrow().iter().map(|f| (f.name().to_owned(), f.get())).collect()
    }
    pub fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut BagValues>,
    ) -> Result<BagValues, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        let mut out = result.as_deref().cloned().unwrap_or_default();
        for (name, value) in self.snapshot() {
            let index = out.iter().position(|(n, _)| n == &name);
            let mut scratch = index.and_then(|i| out[i].1.clone());
            let value = match value {
                None => None,
                Some(PropertyInput::Component(p)) => p.get_value(&time, scratch.as_mut())?,
                Some(PropertyInput::Value(_)) => return Err(RuntimeError::new("property.getValue is not a function")),
            };
            if let Some(i) = index {
                out[i].1 = value
            } else {
                out.push((name, value));
            }
        }
        let out = crate::core::query::js_object_order(out);
        if let Some(result) = result {
            result.clone_from(&out);
        }
        Ok(out)
    }
    pub fn merge(&self, source: &PropertyBag) -> Result<(), RuntimeError> {
        self.merge_entries(&source.snapshot(), None)
    }
    pub fn merge_values(
        &self,
        source: &[(String, PropertyInput)],
        factory: Option<ComponentFactory>,
    ) -> Result<(), RuntimeError> {
        self.merge_entries(&source.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect::<Vec<_>>(), factory)
    }
    pub fn merge_entries(
        &self,
        source: &[(String, Option<PropertyInput>)],
        factory: Option<ComponentFactory>,
    ) -> Result<(), RuntimeError> {
        for (name, source) in source {
            let target = self.get(name);
            if target.is_none() && !self.has_property(name) {
                self.add_property(name.clone(), None, factory.clone())?;
            }
            if let Some(source) = source {
                if let Some(target) = target {
                    if let PropertyInput::Component(target) = target {
                        if target.supports_merge() {
                            let PropertyInput::Component(source) = source else {
                                return Err(RuntimeError::new("merge source must be a component"));
                            };
                            target.merge_component(&**source)?;
                        }
                    }
                } else if let PropertyInput::Component(source) = source {
                    if source.supports_merge() && source.supports_clone() {
                        self.set(name, Some(PropertyInput::Component(source.clone_component(None)?)))?;
                    } else {
                        self.set(name, Some(PropertyInput::Component(source.clone())))?;
                    }
                } else {
                    self.set(name, Some(source.clone()))?;
                }
            }
        }
        Ok(())
    }
    pub fn equals(&self, other: &Self) -> Result<bool, RuntimeError> {
        if std::ptr::eq(self, other) {
            return Ok(true);
        }
        let a = self.snapshot();
        let b = other.snapshot();
        if a.len() != b.len() {
            return Ok(false);
        }
        for (name, value) in a {
            let Some((_, other)) = b.iter().find(|(n, _)| n == &name) else { return Ok(false) };
            if !component_equals(value.as_ref(), other.as_ref())? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
impl EntityComponent for PropertyBag {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn is_constant(&self) -> bool {
        self.is_constant()
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let mut scratch =
            result.as_ref().and_then(|v| if let DynamicValue::Record(v) = v { Some(v.clone()) } else { None });
        let values = self.get_value(Some(time), scratch.as_mut())?;
        let value = DynamicValue::Record(values);
        if let Some(result) = result {
            *result = value.clone();
        }
        Ok(Some(value))
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        other.as_any().downcast_ref::<Self>().map_or(Ok(false), |o| self.equals(o))
    }
    fn supports_merge(&self) -> bool {
        true
    }
    fn merge_component(&self, source: &dyn EntityComponent) -> Result<(), RuntimeError> {
        let Some(source) = source.as_any().downcast_ref::<Self>() else {
            return Err(RuntimeError::new("source must be a PropertyBag"));
        };
        self.merge(source)
    }
    fn get_property(&self, name: &str) -> Option<PropertyInput> {
        self.get(name)
    }
}

impl super::Property<DynamicValue> for PropertyBag {
    fn is_constant(&self) -> bool {
        self.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let mut scratch =
            result.as_ref().and_then(|v| if let DynamicValue::Record(v) = v { Some(v.clone()) } else { None });
        let value = DynamicValue::Record(self.get_value(time, scratch.as_mut())?);
        if let Some(result) = result {
            *result = value.clone();
        }
        Ok(Some(value))
    }
    fn equals(&self, other: &dyn super::Property<DynamicValue>) -> Result<bool, RuntimeError> {
        other.as_any().downcast_ref::<Self>().map_or(Ok(false), |o| self.equals(o))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
