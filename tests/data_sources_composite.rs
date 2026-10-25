mod common;
mod entity_common;
use cesium_rs::data_sources::*;
use common::*;
use entity_common::*;
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
fn make_entity(v: &Value) -> EntityRef {
    let e = entity(s(&v["id"]));
    if let Some(v) = v.get("name") {
        e.set_name(Some(s(v).into()));
    }
    for (name, value) in v.as_object().unwrap() {
        if name == "id" || name == "name" {
            continue;
        }
        set(&e, name, value);
    }
    e
}
fn set(e: &Entity, name: &str, value: &Value) {
    if name == "name" {
        e.set_name(value.as_str().map(str::to_owned));
        return;
    }
    if name == "availability" {
        e.set_availability((!value.is_null()).then(|| availability(n(&value[0]), n(&value[1]))));
        return;
    }
    if !e.property_names().iter().any(|n| n == name) {
        e.add_property(name).unwrap();
    }
    let input = if value.is_null() {
        None
    } else if name == "position" {
        Some(PropertyInput::value(c3(value)))
    } else {
        Some(PropertyInput::Value(DynamicValue::from_json(value.clone())))
    };
    e.set_property(name, input).unwrap();
}
fn value(e: &Entity, name: &str) -> Value {
    match e.get_property(name) {
        None => Value::Null,
        Some(PropertyInput::Value(v)) => encode(&v),
        Some(PropertyInput::Component(p)) => p.get_value(&date(0.), None).unwrap().as_ref().map_or(Value::Null, encode),
    }
}
fn capture(
    c: &CompositeEntityCollection,
    layers: &[Rc<EntityCollection>],
    trace: &RefCell<Vec<Value>>,
    identities: &mut HashMap<String, EntityRef>,
) -> Value {
    let span = c.compute_availability();
    let sources = layers.iter().cloned().map(|s| s as EntitySourceRef).collect::<Vec<_>>();
    let order =
        c.collections().iter().map(|c| sources.iter().position(|s| Rc::ptr_eq(s, c)).unwrap()).collect::<Vec<_>>();
    let entities = c
        .values()
        .iter()
        .map(|e| {
            let same = identities.get(e.id()).is_none_or(|old| Rc::ptr_eq(old, e));
            identities.insert(e.id().into(), e.clone());
            let custom = e
                .property_names()
                .into_iter()
                .filter(|n| n != "fixtureData" && !data_sources_names().contains(&n.as_str()))
                .collect::<Vec<_>>();
            json!([
                e.id(),
                e.name(),
                custom,
                value(e, "description"),
                value(e, "fixtureData"),
                value(e, "properties"),
                value(e, "custom"),
                value(e, "position"),
                e.availability().map(|a| {
                    let a = a.read();
                    json!([a.start().unwrap().to_iso8601(None), a.stop().unwrap().to_iso8601(None)])
                }),
                same,
                c.contains(e)
            ])
        })
        .collect::<Vec<_>>();
    let listeners = layers
        .iter()
        .map(|l| {
            json!([
                l.collection_changed().number_of_listeners(),
                l.values()
                    .iter()
                    .map(|e| json!([e.id(), e.definition_changed().number_of_listeners()]))
                    .collect::<Vec<_>>()
            ])
        })
        .collect::<Vec<_>>();
    json!([
        order,
        entities,
        std::mem::take(&mut *trace.borrow_mut()),
        [span.start.to_iso8601(None), span.stop.to_iso8601(None)],
        listeners
    ])
}
fn data_sources_names() -> &'static [&'static str] {
    cesium_rs::data_sources::entity::ENTITY_PROPERTY_NAMES
}
fn port(f: &str, args: &[Value]) -> Option<Out> {
    if f != "operations" {
        return None;
    }
    let spec = &args[0];
    let layers = spec["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let c = EntityCollection::new();
            for v in v.as_array().unwrap() {
                c.add(make_entity(v)).unwrap();
            }
            c
        })
        .collect::<Vec<_>>();
    let sources = layers.iter().cloned().map(|s| s as EntitySourceRef).collect::<Vec<_>>();
    let order = spec["order"].as_array().unwrap().iter().map(|i| sources[u(i) as usize].clone()).collect();
    let c = CompositeEntityCollection::with_options(
        order,
        EntityCollectionOptions { registry: registry(), ..Default::default() },
    )
    .unwrap();
    let trace = Rc::new(RefCell::new(vec![]));
    let t = trace.clone();
    let _guard = c.collection_changed().add_listener(move |v| t.borrow_mut().push(collection_trace(v)));
    let mut identities = HashMap::new();
    let mut out = vec![capture(&c, &layers, &trace, &mut identities)];
    for op in spec["ops"].as_array().unwrap() {
        let a = op.as_array().unwrap();
        let kind = s(&a[0]);
        let index = a.get(1).map(|v| u(v) as usize).unwrap_or(0);
        match kind {
            "addCollection" => {
                c.add_collection(sources[index].clone(), a.get(2).and_then(|v| v.as_u64()).map(|v| v as usize)).unwrap()
            }
            "removeCollection" => {
                c.remove_collection(&sources[index]).unwrap();
            }
            "allCollections" => c.remove_all_collections().unwrap(),
            "raise" => c.raise_collection(&sources[index]).unwrap(),
            "lower" => c.lower_collection(&sources[index]).unwrap(),
            "top" => c.raise_collection_to_top(&sources[index]).unwrap(),
            "bottom" => c.lower_collection_to_bottom(&sources[index]).unwrap(),
            "suspend" => c.suspend_events(),
            "resume" => c.resume_events().unwrap(),
            "add" => {
                layers[index].add(make_entity(&a[2])).unwrap();
            }
            "remove" => {
                layers[index].remove_by_id(s(&a[2]));
            }
            "clear" => layers[index].remove_all(),
            "set" => set(&layers[index].get_by_id(s(&a[2])).unwrap(), s(&a[3]), &a[4]),
            "child" => {
                let e = layers[index].get_by_id(s(&a[2])).unwrap();
                let parts = s(&a[3]).split('.').collect::<Vec<_>>();
                let p = e.get_property(parts[0]).unwrap().component().unwrap();
                let bag = if let Some(t) = p.as_any().downcast_ref::<Thing>() {
                    &t.bag
                } else {
                    p.as_any().downcast_ref::<PropertyBag>().unwrap()
                };
                bag.set(
                    parts[1],
                    (!a[4].is_null()).then(|| PropertyInput::Value(DynamicValue::from_json(a[4].clone()))),
                )
                .unwrap();
            }
            "mutate" => {
                let e = layers[index].get_by_id(s(&a[2])).unwrap();
                let p = e.get_property(s(&a[3])).unwrap().component().unwrap();
                p.as_any()
                    .downcast_ref::<PropertyComponent<DynamicValue>>()
                    .unwrap()
                    .property()
                    .as_any()
                    .downcast_ref::<ConstantProperty<DynamicValue>>()
                    .unwrap()
                    .set_value(Some(DynamicValue::from_json(a[4].clone())));
            }
            "custom" => set(&layers[index].get_by_id(s(&a[2])).unwrap(), s(&a[3]), &a[4]),
            _ => panic!("unknown operation {kind}"),
        }
        assert!(c.take_error().is_none(), "automatic composite update failed");
        out.push(capture(&c, &layers, &trace, &mut identities));
    }
    ok(out)
}
#[test]
fn composite_entity_collection() {
    run("data_sources", "CompositeEntityCollection", |_| EXACT, port);
}
