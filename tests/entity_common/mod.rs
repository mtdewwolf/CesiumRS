#![allow(dead_code)]
use crate::common::*;
use cesium_rs::{core::*, data_sources::*};
use serde_json::{json, Value};
use std::{any::Any, rc::Rc};
pub fn date(t: f64) -> JulianDate {
    JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap().add_seconds(t)
}
pub fn record(values: &BagValues) -> Value {
    Value::Object(values.iter().map(|(k, v)| (k.clone(), v.as_ref().map_or(Value::Null, encode))).collect())
}
pub fn encode(v: &DynamicValue) -> Value {
    if let DynamicValue::Record(v) = v {
        record(v)
    } else if let DynamicValue::Number(v) = v {
        num(*v)
    } else {
        v.to_json()
    }
}
pub fn describe(v: Option<&PropertyInput>) -> Value {
    match v {
        None => json!(["undefined"]),
        Some(PropertyInput::Value(DynamicValue::Entity(v))) => {
            json!(["entity", v.upgrade().map(|v| v.id().to_owned())])
        }
        Some(PropertyInput::Value(DynamicValue::Availability(v))) => {
            let v = v.upgrade().unwrap();
            let r = v.read();
            json!(["availability", r.start().unwrap().to_iso8601(None), r.stop().unwrap().to_iso8601(None)])
        }
        Some(PropertyInput::Value(v)) => json!(["raw", encode(v)]),
        Some(PropertyInput::Component(p)) => {
            json!(["property", p.get_value(&date(0.), None).unwrap().as_ref().map_or(Value::Null, encode)])
        }
    }
}
pub fn trace(v: &PropertyChange) -> Value {
    let same = match (&v.new_value, &v.old_value) {
        (None, None) => true,
        (Some(a), Some(b)) => a.strict_equals(b),
        _ => false,
    };
    json!([v.name, describe(v.new_value.as_ref()), describe(v.old_value.as_ref()), same])
}
pub struct Thing {
    pub bag: Rc<PropertyBag>,
}
impl Thing {
    pub fn empty() -> Rc<Self> {
        let bag = Rc::new(PropertyBag::new());
        bag.add_property("a", None, None).unwrap();
        bag.add_property("b", None, None).unwrap();
        Rc::new(Self { bag })
    }
    pub fn from_dynamic(value: DynamicValue) -> Result<Rc<Self>, RuntimeError> {
        let value = value.to_json();
        let values = value.as_object().ok_or_else(|| RuntimeError::new("component requires an object"))?;
        let thing = Self::empty();
        for name in ["a", "b"] {
            if let Some(value) = values.get(name) {
                thing.bag.set(name, Some(PropertyInput::Value(DynamicValue::from_json(value.clone()))))?;
            }
        }
        Ok(thing)
    }
}
impl EntityComponent for Thing {
    fn get_type(&self, _: &JulianDate) -> Result<Option<String>, RuntimeError> {
        Ok(Some("Fixture".into()))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.bag.definition_changed()
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        EntityComponent::get_value(&*self.bag, time, result)
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        other.as_any().downcast_ref::<Self>().map_or(Ok(false), |o| self.bag.equals(&o.bag))
    }
    fn supports_merge(&self) -> bool {
        true
    }
    fn supports_clone(&self) -> bool {
        true
    }
    fn merge_component(&self, other: &dyn EntityComponent) -> Result<(), RuntimeError> {
        let other =
            other.as_any().downcast_ref::<Self>().ok_or_else(|| RuntimeError::new("component type mismatch"))?;
        self.bag.merge(&other.bag)
    }
    fn clone_component(&self, result: Option<ComponentRef>) -> Result<ComponentRef, RuntimeError> {
        let target = result.filter(|p| p.as_any().is::<Self>()).unwrap_or_else(|| Self::empty());
        let thing = target.as_any().downcast_ref::<Self>().unwrap();
        for (n, v) in self.bag.snapshot() {
            thing.bag.set(&n, v)?;
        }
        Ok(target)
    }
    fn get_property(&self, name: &str) -> Option<PropertyInput> {
        self.bag.get(name)
    }
}
pub fn registry() -> EntityTypeRegistry {
    let r = EntityTypeRegistry::default();
    r.register("fixtureData", Rc::new(|v| Ok(Thing::from_dynamic(v)? as ComponentRef))).unwrap();
    r
}
pub fn entity(id: &str) -> EntityRef {
    Entity::new(EntityOptions { id: Some(id.into()), registry: registry(), ..Default::default() }).unwrap()
}
pub fn capture_entity(e: &Entity) -> Value {
    let names = e.property_names();
    json!([
        e.id(),
        e.name(),
        e.show(),
        e.is_showing(),
        e.parent().map(|e| e.id().to_owned()),
        e.tracking_reference_frame() as u8,
        names,
        names.iter().map(|n| json!([n, describe(e.get_property(n).as_ref())])).collect::<Vec<_>>(),
        e.availability().map(|a| {
            let a = a.read();
            json!([a.start().unwrap().to_iso8601(None), a.stop().unwrap().to_iso8601(None)])
        })
    ])
}
#[derive(Default)]
pub struct Pool {
    values: Vec<(String, PropertyInput)>,
    constants: Vec<(String, Rc<ConstantProperty<DynamicValue>>)>,
}
impl Pool {
    pub fn input(&mut self, spec: &Value) -> Option<PropertyInput> {
        if spec.is_null() {
            return None;
        }
        let kind = s(&spec[0]);
        Some(match kind {
            "raw" => PropertyInput::Value(DynamicValue::from_json(spec[1].clone())),
            "vec" => PropertyInput::Value(DynamicValue::Cartesian3(c3(&spec[1]))),
            "quat" => PropertyInput::Value(DynamicValue::Quaternion(q(&spec[1]))),
            _ => {
                let id = s(&spec[1]);
                if let Some((_, v)) = self.values.iter().find(|(n, _)| n == id) {
                    return Some(v.clone());
                }
                let value = match kind {
                    "property" => {
                        let p = Rc::new(ConstantProperty::new(Some(DynamicValue::from_json(spec[2].clone()))));
                        self.constants.push((id.into(), p.clone()));
                        PropertyInput::Component(PropertyComponent::wrap(p))
                    }
                    "callback" => {
                        let base = n(&spec[2]);
                        let p = Rc::new(CallbackProperty::new(
                            Rc::new(move |time, _| {
                                Ok(Some(DynamicValue::Number(base + JulianDate::seconds_difference(time, &date(0.)))))
                            }),
                            false,
                        ));
                        PropertyInput::Component(PropertyComponent::wrap(p))
                    }
                    "object" => PropertyInput::Value(DynamicValue::from_json(spec[2].clone())),
                    "component" => {
                        PropertyInput::Component(Thing::from_dynamic(DynamicValue::from_json(spec[2].clone())).unwrap())
                    }
                    _ => panic!("unknown input {kind}"),
                };
                self.values.push((id.into(), value.clone()));
                value
            }
        })
    }
    pub fn mutate(&self, id: &str, value: &Value) {
        self.constants.iter().find(|(n, _)| n == id).unwrap().1.set_value(Some(DynamicValue::from_json(value.clone())));
    }
    pub fn listeners(&self) -> Vec<usize> {
        self.values
            .iter()
            .filter_map(|(_, v)| v.component().map(|v| v.definition_changed().number_of_listeners()))
            .collect()
    }
}
pub fn availability(start: f64, stop: f64) -> Availability {
    let a = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
    a.add_interval(TimeInterval::new(date(start), date(stop), true, true, None));
    a
}
pub fn ids(values: &[EntityRef]) -> Value {
    json!(values.iter().map(|e| e.id()).collect::<Vec<_>>())
}
pub fn collection_trace(c: &CollectionChange) -> Value {
    json!([ids(&c.added), ids(&c.removed), ids(&c.changed)])
}
