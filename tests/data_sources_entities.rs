mod common;
mod entity_common;
use cesium_rs::scene::HeightReference;
use cesium_rs::{core::*, data_sources::*};
use common::*;
use entity_common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn slot_port(f: &str, a: &[Value]) -> Option<Out> {
    if f != "slot" {
        return None;
    }
    let raw = a[0].as_bool().unwrap();
    let configurable = a[1].as_bool().unwrap();
    let custom = a[2].as_bool().unwrap();
    let events = ChangeEvent::<PropertyChange>::new();
    let traces = Rc::new(RefCell::new(vec![]));
    let t = traces.clone();
    let _guard = events.add_listener(move |v| t.borrow_mut().push(trace(v)));
    let slot = if raw {
        create_raw_property_descriptor("field", configurable, &events)
    } else {
        create_property_descriptor(
            "field",
            configurable,
            custom.then(|| {
                Rc::new(|v: DynamicValue| {
                    let n = match v {
                        DynamicValue::Null => 99.,
                        DynamicValue::Number(v) => v * 2.,
                        _ => panic!("numeric fixture"),
                    };
                    Ok(PropertyComponent::wrap(Rc::new(ConstantProperty::new(Some(DynamicValue::Number(n))))))
                }) as ComponentFactory
            }),
            &events,
        )
    };
    let mut pool = Pool::default();
    let mut out = vec![];
    for op in a[3].as_array().unwrap() {
        if op[0] == "set" {
            slot.set(pool.input(&op[1])).unwrap();
        } else {
            pool.mutate(s(&op[1]), &op[2]);
        }
        out.push(json!([
            describe(slot.get().as_ref()),
            std::mem::take(&mut *traces.borrow_mut()),
            slot.configurable(),
            pool.listeners()
        ]));
    }
    ok(out)
}
#[test]
fn property_descriptor() {
    run("data_sources", "createPropertyDescriptor", |_| EXACT, slot_port);
}
#[test]
fn raw_property_descriptor() {
    run("data_sources", "createRawPropertyDescriptor", |_| EXACT, slot_port);
}
#[test]
fn property_bag() {
    run(
        "data_sources",
        "PropertyBag",
        |_| EXACT,
        |f, a| match f {
            "operations" => {
                let p = PropertyBag::new();
                let changes = Rc::new(Cell::new(0));
                let c = changes.clone();
                let _guard = p.definition_changed().add_listener(move |_| c.set(c.get() + 1));
                let mut pool = Pool::default();
                let mut result = vec![("kept".into(), Some(DynamicValue::Number(42.)))];
                let mut out = vec![];
                for op in a[0].as_array().unwrap() {
                    match s(&op[0]) {
                        "add" => p.add_property(s(&op[1]), pool.input(&op[2]), None).unwrap(),
                        "set" => p.set(s(&op[1]), pool.input(&op[2])).unwrap(),
                        "mutate" => pool.mutate(s(&op[1]), &op[2]),
                        "remove" => p.remove_property(s(&op[1])).unwrap(),
                        "merge" => {
                            let source = op[1]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| (s(&v[0]).to_owned(), pool.input(&v[1])))
                                .collect::<Vec<_>>();
                            p.merge_entries(&source, None).unwrap();
                        }
                        _ => panic!("op"),
                    };
                    let value = p.get_value(Some(&date(2.)), Some(&mut result)).unwrap();
                    out.push(json!([
                        p.property_names(),
                        p.is_constant(),
                        record(&value),
                        p.property_names().iter().map(|n| json!([n, describe(p.get(n).as_ref())])).collect::<Vec<_>>(),
                        changes.get(),
                        pool.listeners()
                    ]));
                }
                ok(out)
            }
            "equals" => {
                let order = a[0].as_bool().unwrap();
                let equal = a[1].as_bool().unwrap();
                let p = PropertyBag::from_values(
                    vec![("a".into(), PropertyInput::value(1.)), ("b".into(), PropertyInput::value(2.))],
                    None,
                )
                .unwrap();
                let mut values = vec![
                    ("a".into(), PropertyInput::value(if equal { 1. } else { 3. })),
                    ("b".into(), PropertyInput::value(2.)),
                ];
                if order {
                    values.reverse();
                }
                let other = PropertyBag::from_values(values, None).unwrap();
                ok(json!([
                    p.equals(&other).unwrap(),
                    p.equals(&p).unwrap(),
                    EntityComponent::equals(&*p, &*PropertyComponent::wrap(Rc::new(ConstantProperty::new(Some(1.)))))
                        .unwrap()
                ]))
            }
            "numericKeys" => {
                let p = PropertyBag::from_json(json!({"10":10,"2":2,"01":1,"4294967295":4,"0":0})).unwrap();
                let values = p.get_value(Some(&date(0.)), None).unwrap();
                let q = PropertyBag::new();
                q.add_property("10", Some(PropertyInput::value(10.)), None).unwrap();
                q.add_property("2", Some(PropertyInput::value(2.)), None).unwrap();
                ok(json!([
                    p.property_names(),
                    values.iter().map(|(k, _)| k).collect::<Vec<_>>(),
                    record(&values),
                    q.property_names(),
                    q.get_value(Some(&date(0.)), None).unwrap().iter().map(|(k, _)| k).collect::<Vec<_>>()
                ]))
            }
            "mergeable" => {
                let source = Thing::from_dynamic(DynamicValue::from_json(json!({"a":1}))).unwrap();
                let target = Thing::from_dynamic(DynamicValue::from_json(json!({"b":2}))).unwrap();
                let a = PropertyBag::from_values(vec![("x".into(), PropertyInput::Component(source.clone()))], None)
                    .unwrap();
                let b = PropertyBag::from_values(vec![("x".into(), PropertyInput::Component(target.clone()))], None)
                    .unwrap();
                a.merge(&b).unwrap();
                let clone = PropertyBag::new();
                clone.merge(&a).unwrap();
                let av = a.get("x").unwrap();
                let bv = clone.get("x").unwrap();
                ok(json!([
                    record(&a.get_value(Some(&date(0.)), None).unwrap()),
                    record(&clone.get_value(Some(&date(0.)), None).unwrap()),
                    av.strict_equals(&bv),
                    match (source.bag.get("b"), target.bag.get("b")) {
                        (None, None) => true,
                        (Some(a), Some(b)) => a.strict_equals(&b),
                        _ => false,
                    }
                ]))
            }
            _ => None,
        },
    );
}
#[test]
fn entity_cases() {
    run(
        "data_sources",
        "Entity",
        |_| TIGHT,
        |f, a| match f {
            "operations" => {
                let e = Entity::new(EntityOptions {
                    id: Some("item".into()),
                    name: Some("initial".into()),
                    registry: registry(),
                    ..Default::default()
                })
                .unwrap();
                let traces = Rc::new(RefCell::new(vec![]));
                let t = traces.clone();
                let _guard = e.definition_changed().add_listener(move |v| t.borrow_mut().push(trace(&v.property)));
                let mut pool = Pool::default();
                let mut out = vec![];
                for op in a[0].as_array().unwrap() {
                    match s(&op[0]) {
                        "add" => e.add_property(s(&op[1])).unwrap(),
                        "remove" => e.remove_property(s(&op[1])).unwrap(),
                        "set" => e.set_property(s(&op[1]), pool.input(&op[2])).unwrap(),
                        "name" => e.set_name(Some(s(&op[1]).into())),
                        "show" => e.set_show(op[1].as_bool().unwrap()),
                        "tracking" => {
                            e.set_tracking_reference_frame(TrackingReferenceFrame::from_value(n(&op[1]) as u8).unwrap())
                        }
                        _ => panic!("op"),
                    };
                    out.push(json!([capture_entity(&e), std::mem::take(&mut *traces.borrow_mut())]));
                }
                ok(out)
            }
            "hierarchy" => {
                let entities = ["p", "c", "g"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, id)| {
                        Entity::new(EntityOptions {
                            id: Some(id.into()),
                            show: a[0][i].as_bool().unwrap(),
                            registry: registry(),
                            ..Default::default()
                        })
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                let traces = Rc::new(RefCell::new(vec![]));
                let _guards = entities
                    .iter()
                    .map(|e| {
                        let t = traces.clone();
                        e.definition_changed().add_listener(move |v| {
                            t.borrow_mut().push(json!([
                                v.entity.upgrade().unwrap().id(),
                                v.property.name,
                                describe(v.property.new_value.as_ref()),
                                describe(v.property.old_value.as_ref())
                            ]));
                        })
                    })
                    .collect::<Vec<_>>();
                let (p, c, g) = (&entities[0], &entities[1], &entities[2]);
                c.set_parent(Some(p.clone())).unwrap();
                g.set_parent(Some(c.clone())).unwrap();
                let mut out = vec![];
                for op in 0..6 {
                    match op {
                        0 => {}
                        1 => p.set_show(false),
                        2 => p.set_show(true),
                        3 => c.set_show(false),
                        4 => g.set_parent(Some(p.clone())).unwrap(),
                        _ => g.set_parent(None).unwrap(),
                    };
                    out.push(json!([
                        entities
                            .iter()
                            .map(|e| json!([e.id(), e.is_showing(), e.parent().map(|e| e.id().to_owned())]))
                            .collect::<Vec<_>>(),
                        std::mem::take(&mut *traces.borrow_mut())
                    ]));
                }
                ok(out)
            }
            "matrix" => {
                let e = entity("m");
                if !a[0].is_null() {
                    e.set_position(Some(Rc::new(ConstantPositionProperty::new(
                        Some(c3(&a[0])),
                        ReferenceFrame::Fixed,
                    ))))
                    .unwrap();
                }
                if !a[1].is_null() {
                    e.set_orientation(Some(Rc::new(ConstantProperty::new(Some(q(&a[1])))))).unwrap();
                }
                let mut result = Matrix4([7.; 16]);
                let base = e.compute_model_matrix(&date(0.), Some(&mut result)).unwrap().enc();
                let after = result.enc();
                let reference = match n(&a[2]) as u8 {
                    0 => HeightReference::None,
                    1 => HeightReference::ClampToGround,
                    2 => HeightReference::RelativeToGround,
                    3 => HeightReference::ClampToTerrain,
                    4 => HeightReference::RelativeToTerrain,
                    5 => HeightReference::ClampTo3DTile,
                    _ => HeightReference::RelativeTo3DTile,
                };
                let property = ConstantProperty::new(Some(reference));
                let height = e
                    .compute_model_matrix_for_height_reference(
                        &date(0.),
                        HeightReferenceOptions {
                            property: Some(&property),
                            height_offset: n(&a[3]),
                            ellipsoid: &Ellipsoid::WGS84,
                        },
                        Some(&mut result),
                    )
                    .unwrap();
                ok(json!([base, after, height.enc(), result.enc()]))
            }
            "buffers" => {
                let positions = Rc::new(Cell::new(0));
                let orientations = Rc::new(Cell::new(0));
                let pc = positions.clone();
                let qc = orientations.clone();
                let p = Rc::new(CallbackPositionProperty::new(
                    Rc::new(move |_, result| {
                        pc.set(pc.get() + 1);
                        let Some(result) = result else { return Ok(None) };
                        *result = Cartesian3::new(6378137., 0., 0.);
                        Ok(Some(*result))
                    }),
                    false,
                    ReferenceFrame::Fixed,
                ));
                let q = Rc::new(CallbackProperty::new(
                    Rc::new(move |_, result: Option<&mut Quaternion>| {
                        qc.set(qc.get() + 1);
                        let Some(result) = result else { return Ok(None) };
                        *result = Quaternion::IDENTITY;
                        Ok(Some(*result))
                    }),
                    false,
                ));
                let e = Entity::new(EntityOptions {
                    id: Some("s".into()),
                    position: Some(p),
                    orientation: Some(q),
                    ..Default::default()
                })
                .unwrap();
                let base = e.compute_model_matrix(&date(0.), None).unwrap();
                let property = ConstantProperty::new(Some(HeightReference::None));
                let height = e
                    .compute_model_matrix_for_height_reference(
                        &date(0.),
                        HeightReferenceOptions {
                            property: Some(&property),
                            height_offset: 0.,
                            ellipsoid: &Ellipsoid::WGS84,
                        },
                        None,
                    )
                    .unwrap();
                ok(json!([base.enc(), height.enc(), positions.get(), orientations.get()]))
            }
            "propertiesOrder" => {
                let e = entity("n");
                e.set_property(
                    "properties",
                    Some(PropertyInput::Value(DynamicValue::from_json(json!({"10":10,"2":2,"01":1})))),
                )
                .unwrap();
                let component = e.get_property("properties").unwrap().component().unwrap();
                let bag = component.as_any().downcast_ref::<PropertyBag>().unwrap();
                ok(json!([
                    bag.property_names(),
                    bag.get_value(Some(&date(0.)), None).unwrap().iter().map(|(k, _)| k).collect::<Vec<_>>()
                ]))
            }
            "availability" => {
                let e = entity("a");
                let traces = Rc::new(RefCell::new(vec![]));
                let t = traces.clone();
                let _guard = e.definition_changed().add_listener(move |v| t.borrow_mut().push(trace(&v.property)));
                let availability = availability(0., 10.);
                e.set_availability(Some(availability.clone()));
                e.set_availability(Some(availability.clone()));
                let first = json!([
                    ([-1., 0., 5., 10., 11.].map(|t| e.is_available(&date(t)))),
                    std::mem::take(&mut *traces.borrow_mut())
                ]);
                availability.add_interval(TimeInterval::new(date(20.), date(30.), true, true, None));
                let second = json!([
                    ([-1., 0., 10., 15., 20., 30., 31.].map(|t| e.is_available(&date(t)))),
                    std::mem::take(&mut *traces.borrow_mut())
                ]);
                e.set_availability(None);
                let traces = traces.borrow().clone();
                ok(json!([first, second, e.is_available(&date(100.)), traces]))
            }
            "merge" => {
                let mut pool = Pool::default();
                let a = Entity::new(EntityOptions {
                    id: Some("a".into()),
                    name: Some("kept".into()),
                    registry: registry(),
                    values: vec![
                        ("description".into(), PropertyInput::value("A")),
                        ("fixtureData".into(), PropertyInput::Value(DynamicValue::from_json(json!({"a":1})))),
                    ],
                    ..Default::default()
                })
                .unwrap();
                let b = Entity::new(EntityOptions {
                    id: Some("b".into()),
                    name: Some("ignored".into()),
                    registry: registry(),
                    values: vec![
                        ("description".into(), PropertyInput::value("B")),
                        ("position".into(), pool.input(&json!(["vec", [1, 2, 3]])).unwrap()),
                        ("fixtureData".into(), PropertyInput::Value(DynamicValue::from_json(json!({"b":2})))),
                    ],
                    ..Default::default()
                })
                .unwrap();
                a.merge(&b).unwrap();
                let ac = a.get_property("fixtureData").unwrap();
                let bc = b.get_property("fixtureData").unwrap();
                let field_eq = |name: &str| {
                    let ac = ac.component().unwrap().get_property(name);
                    let bc = bc.component().unwrap().get_property(name);
                    match (ac, bc) {
                        (None, None) => true,
                        (Some(a), Some(b)) => a.strict_equals(&b),
                        _ => false,
                    }
                };
                ok(json!([capture_entity(&a), ac.strict_equals(&bc), field_eq("a"), field_eq("b")]))
            }
            _ => None,
        },
    );
}
fn capture_collection(c: &EntityCollection) -> Value {
    json!(c.values().iter().map(|e| json!([e.id(), e.name(), e.is_showing()])).collect::<Vec<_>>())
}
#[test]
fn entity_collection() {
    run(
        "data_sources",
        "EntityCollection",
        |_| EXACT,
        |f, a| match f {
            "operations" => {
                let c = EntityCollection::new();
                let mut entities = Vec::<(String, EntityRef)>::new();
                let traces = Rc::new(RefCell::new(vec![]));
                let t = traces.clone();
                let _guard = c.collection_changed().add_listener(move |v| t.borrow_mut().push(collection_trace(v)));
                let suspended = a[0].as_bool().unwrap();
                if suspended {
                    c.suspend_events();
                }
                let mut out = vec![];
                for op in a[1].as_array().unwrap() {
                    match s(&op[0]) {
                        "add" => {
                            let id = s(&op[1]);
                            if !entities.iter().any(|(n, _)| n == id) {
                                entities.push((id.into(), entity(id)));
                            }
                            c.add(entities.iter().find(|(n, _)| n == id).unwrap().1.clone()).unwrap();
                        }
                        "name" => {
                            entities.iter().find(|(n, _)| n == s(&op[1])).unwrap().1.set_name(Some(s(&op[2]).into()))
                        }
                        "remove" => {
                            c.remove_by_id(s(&op[1]));
                        }
                        "show" => c.set_show(op[1].as_bool().unwrap()).unwrap(),
                        "all" => c.remove_all(),
                        _ => panic!("op"),
                    };
                    out.push(json!([capture_collection(&c), std::mem::take(&mut *traces.borrow_mut())]));
                }
                if suspended {
                    c.resume_events().unwrap();
                }
                let traces = traces.borrow().clone();
                ok(json!([out, traces]))
            }
            "identity" => {
                let c = EntityCollection::new();
                let a = entity("x");
                let b = entity("x");
                c.add(a.clone()).unwrap();
                let before = json!([c.contains(&a), c.contains(&b), Rc::ptr_eq(&c.get_or_create_entity("x"), &a)]);
                let removed = c.remove(Some(&b));
                let n = c.get_or_create_entity("x");
                ok(json!([before, removed, c.contains(&a), Rc::ptr_eq(&n, &a), ids(&c.values())]))
            }
            "reentrant" => {
                let c = EntityCollection::new();
                let traces = Rc::new(RefCell::new(vec![]));
                let t = traces.clone();
                let once = Cell::new(false);
                let _guard = c.collection_changed().add_listener(move |v| {
                    t.borrow_mut().push(collection_trace(v));
                    if !once.replace(true) {
                        let c = v.collection.upgrade().unwrap();
                        c.add(entity("b")).unwrap();
                        c.get_by_id("a").unwrap().set_name(Some("changed".into()));
                        c.remove_by_id("b");
                    }
                });
                c.add(entity("a")).unwrap();
                let traces = traces.borrow().clone();
                ok(json!([traces, c.values().iter().map(|e| json!([e.id(), e.name()])).collect::<Vec<_>>()]))
            }
            "availability" => {
                let c = EntityCollection::new();
                let capture = || {
                    let a = c.compute_availability();
                    json!([a.start.to_iso8601(None), a.stop.to_iso8601(None)])
                };
                let mut out = vec![capture()];
                let e = entity("a");
                e.set_availability(Some(availability(1., 3.)));
                c.add(e).unwrap();
                out.push(capture());
                let e = entity("b");
                let a = availability(-2., 2.);
                a.add_interval(TimeInterval::new(date(10.), date(20.), true, true, None));
                e.set_availability(Some(a));
                c.add(e).unwrap();
                out.push(capture());
                c.add(entity("c")).unwrap();
                out.push(capture());
                c.remove_by_id("a");
                out.push(capture());
                ok(out)
            }
            _ => None,
        },
    );
}
#[test]
fn reference_property() {
    run(
        "data_sources",
        "ReferenceProperty",
        |_| TIGHT,
        |f, a| match f {
            "lifecycle" => {
                let c = EntityCollection::new();
                let r = ReferenceProperty::new(c.clone(), "target", list(&a[0], |v| s(v).to_owned())).unwrap();
                let traces = Rc::new(RefCell::new(vec![]));
                let t = traces.clone();
                let _guard = r.definition_changed().add_listener(move |_| t.borrow_mut().push("changed"));
                let capture = || {
                    json!([
                        r.get_value(Some(&date(0.)), None).unwrap().as_ref().map_or(Value::Null, encode),
                        r.is_constant(),
                        r.reference_frame().map(|f| f as u8),
                        std::mem::take(&mut *traces.borrow_mut())
                    ])
                };
                let mut out = vec![capture()];
                let make = |description: &str, x: f64, y: f64| {
                    Entity::new(EntityOptions {
                        id: Some("target".into()),
                        registry: registry(),
                        values: vec![
                            ("description".into(), PropertyInput::value(description)),
                            ("properties".into(), PropertyInput::Value(DynamicValue::from_json(json!({"x":x})))),
                            ("fixtureData".into(), PropertyInput::Value(DynamicValue::from_json(json!({"a":y})))),
                        ],
                        ..Default::default()
                    })
                    .unwrap()
                };
                let e = make("one", 1., 2.);
                c.add(e.clone()).unwrap();
                out.push(capture());
                match s(&a[0][0]) {
                    "description" => e.set_property("description", Some(PropertyInput::value("two"))).unwrap(),
                    "properties" => {
                        let v = e.get_property("properties").unwrap().component().unwrap();
                        v.as_any()
                            .downcast_ref::<PropertyBag>()
                            .unwrap()
                            .set("x", Some(PropertyInput::value(3.)))
                            .unwrap();
                    }
                    _ => {
                        let v = e.get_property("fixtureData").unwrap().component().unwrap();
                        v.as_any()
                            .downcast_ref::<Thing>()
                            .unwrap()
                            .bag
                            .set("a", Some(PropertyInput::value(4.)))
                            .unwrap();
                    }
                }
                out.push(capture());
                c.remove(Some(&e));
                out.push(capture());
                c.add(make("next", 5., 6.)).unwrap();
                out.push(capture());
                ok(out)
            }
            "parse" => {
                let c = EntityCollection::new();
                match ReferenceProperty::from_string(c, s(&a[0])) {
                    Ok(r) => ok(json!([r.target_id(), r.target_property_names()])),
                    Err(e) => ok(json!({"error":e.message})),
                }
            }
            "type" => {
                let c = EntityCollection::new();
                let r = ReferenceProperty::new(c.clone(), "m", vec!["fixtureData".into()]).unwrap();
                let before = r.get_type(&date(0.)).unwrap();
                let e = entity("m");
                e.set_property("fixtureData", Some(PropertyInput::Value(DynamicValue::from_json(json!({"a":5})))))
                    .unwrap();
                c.add(e).unwrap();
                ok(json!([
                    before,
                    r.get_type(&date(0.)).unwrap(),
                    r.get_value(Some(&date(0.)), None).unwrap().as_ref().map_or(Value::Null, encode)
                ]))
            }
            "position" => {
                let c = EntityCollection::new();
                let e = entity("p");
                e.set_position(Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::new(1., 2., 3.)),
                    ReferenceFrame::Fixed,
                ))))
                .unwrap();
                c.add(e).unwrap();
                let r = ReferenceProperty::new(c.clone(), "p", vec!["position".into()]).unwrap();
                let other = EntityCollection::new();
                ok(json!([
                    r.reference_frame().map(|f| f as u8),
                    r.get_value(Some(&date(0.)), None).unwrap().as_ref().map_or(Value::Null, encode),
                    r.get_value_in_reference_frame(&date(0.), ReferenceFrame::Fixed, None).unwrap().enc(),
                    r.equals(&ReferenceProperty::new(c, "p", vec!["position".into()]).unwrap()),
                    r.equals(&ReferenceProperty::new(other, "p", vec!["position".into()]).unwrap())
                ]))
            }
            _ => None,
        },
    );
}
