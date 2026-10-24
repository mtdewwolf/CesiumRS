mod common;
mod entity_common;
use cesium_rs::{core::*, data_sources::*};
use entity_common::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[test]
fn payload_events_keep_nested_frames_and_duplicate_listener_identity() {
    let event = ChangeEvent::new();
    let seen = Rc::new(RefCell::new(vec![]));
    let weak = event.downgrade();
    let out = seen.clone();
    let listener: ChangeListener<i32> = Rc::new(move |v| {
        out.borrow_mut().push(('a', *v));
        if *v == 1 {
            weak.upgrade().unwrap().raise(2);
        }
    });
    let first = event.subscribe(listener.clone());
    let duplicate = event.subscribe(listener);
    let out = seen.clone();
    let _second = event.add_listener(move |v| out.borrow_mut().push(('b', *v)));
    event.raise(1);
    assert_eq!(*seen.borrow(), vec![('a', 1), ('a', 2), ('b', 2), ('b', 1)]);
    assert_eq!(event.number_of_listeners(), 2);
    drop(first);
    drop(duplicate);
    assert_eq!(event.number_of_listeners(), 1);
}
#[test]
fn dropping_an_event_payload_can_raise_another_event() {
    struct Payload {
        i: i32,
        on_drop: Option<Box<dyn Fn()>>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            if let Some(f) = self.on_drop.take() {
                f();
            }
        }
    }
    let event = ChangeEvent::<Payload>::new();
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    let _guard = event.add_listener(move |v| out.borrow_mut().push(v.i));
    let weak = event.downgrade();
    event.raise(Payload {
        i: 1,
        on_drop: Some(Box::new(move || weak.upgrade().unwrap().raise(Payload { i: 2, on_drop: None }))),
    });
    assert_eq!(*seen.borrow(), vec![1, 2]);
}
#[test]
fn descriptors_keep_property_identity_across_typed_adapters() {
    let event = ChangeEvent::<PropertyChange>::new();
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    let _guard = event.add_listener(move |_| c.set(c.get() + 1));
    let slot = create_property_descriptor("x", true, None, &event);
    let property = Rc::new(ConstantProperty::new(Some(1.)));
    slot.set(Some(PropertyInput::Component(PropertyComponent::wrap(property.clone())))).unwrap();
    slot.set(Some(PropertyInput::Component(PropertyComponent::wrap(property.clone())))).unwrap();
    assert_eq!(count.get(), 1);
    assert_eq!(property.definition_changed().number_of_listeners(), 1);
    property.set_value(Some(2.));
    assert_eq!(count.get(), 2);
    drop(slot);
    assert_eq!(property.definition_changed().number_of_listeners(), 0);
}
#[test]
fn reentrant_descriptor_replacement_never_subscribes_the_old_value() {
    let event = ChangeEvent::<PropertyChange>::new();
    let link = Rc::new(RefCell::new(None::<PropertyDescriptor>));
    let a = Rc::new(ConstantProperty::new(Some(1.)));
    let b = Rc::new(ConstantProperty::new(Some(2.)));
    let replacement = PropertyComponent::wrap(b.clone());
    let shared = link.clone();
    let once = Cell::new(false);
    let _guard = event.add_listener(move |_| {
        if !once.replace(true) {
            shared.borrow().as_ref().unwrap().set(Some(PropertyInput::Component(replacement.clone()))).unwrap();
        }
    });
    let slot = create_property_descriptor("x", true, None, &event);
    *link.borrow_mut() = Some(slot.clone());
    slot.set(Some(PropertyInput::Component(PropertyComponent::wrap(a.clone())))).unwrap();
    assert_eq!(a.definition_changed().number_of_listeners(), 0);
    assert_eq!(b.definition_changed().number_of_listeners(), 1);
    drop(link.borrow_mut().take());
    drop(slot);
    assert_eq!(b.definition_changed().number_of_listeners(), 0);
}
#[test]
fn descriptors_ignore_retired_child_callbacks_during_dispatch() {
    for edit in ["remove", "replace", "same"] {
        let bag = Rc::new(PropertyBag::new());
        let old = Rc::new(ConstantProperty::new(Some(1.)));
        let new = Rc::new(ConstantProperty::new(Some(2.)));
        let weak_bag = Rc::downgrade(&bag);
        let weak_old = Rc::downgrade(&old);
        let replacement = new.clone();
        let _edit = old.definition_changed().add_listener(move |_| {
            let bag = weak_bag.upgrade().unwrap();
            if edit == "remove" {
                bag.remove_property("x").unwrap();
            } else {
                let property = if edit == "same" { weak_old.upgrade().unwrap() } else { replacement.clone() };
                bag.set("x", Some(PropertyInput::Component(PropertyComponent::wrap(property)))).unwrap();
            }
        });
        bag.add_property("x", Some(PropertyInput::Component(PropertyComponent::wrap(old.clone()))), None).unwrap();
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let _guard = bag.definition_changed().add_listener(move |_| c.set(c.get() + 1));
        old.set_value(Some(3.));
        assert_eq!(count.get(), 1);
        if edit == "replace" {
            new.set_value(Some(4.));
            assert_eq!(count.get(), 2);
        }
    }
}
#[test]
fn failed_factory_preserves_the_current_subscription() {
    let event = ChangeEvent::<PropertyChange>::new();
    let slot = create_property_descriptor("x", true, Some(Rc::new(|_| Err(RuntimeError::new("rejected")))), &event);
    let child = Rc::new(ConstantProperty::new(Some(1.)));
    slot.set(Some(PropertyInput::Component(PropertyComponent::wrap(child.clone())))).unwrap();
    assert!(slot.set(Some(PropertyInput::value(-1.))).is_err());
    assert_eq!(child.definition_changed().number_of_listeners(), 1);
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    let _guard = event.add_listener(move |_| c.set(c.get() + 1));
    child.set_value(Some(2.));
    assert_eq!(count.get(), 1);
}
#[test]
fn bags_snapshot_children_and_keep_one_clock_read() {
    let clock = Rc::new(Cell::new(0));
    let c = clock.clone();
    let bag = Rc::new(PropertyBag::with_clock(Rc::new(move || {
        c.set(c.get() + 1);
        date(c.get() as f64)
    })));
    let weak = Rc::downgrade(&bag);
    let once = Cell::new(false);
    let first = Rc::new(CallbackProperty::new(
        Rc::new(move |time, _| {
            if !once.replace(true) {
                let bag = weak.upgrade().unwrap();
                bag.remove_property("tail").unwrap();
                bag.add_property("next", Some(PropertyInput::value(3.)), None).unwrap();
            }
            Ok(Some(DynamicValue::Number(JulianDate::seconds_difference(time, &date(0.)))))
        }),
        false,
    ));
    let tail = Rc::new(ConstantProperty::new(Some(2.)));
    bag.add_property("first", Some(PropertyInput::Component(PropertyComponent::wrap(first))), None).unwrap();
    bag.add_property("tail", Some(PropertyInput::Component(PropertyComponent::wrap(tail.clone()))), None).unwrap();
    let values = bag.get_value(None, None).unwrap();
    assert_eq!(record(&values), serde_json::json!({"first":1.,"tail":2.}));
    assert_eq!(clock.get(), 1);
    assert_eq!(tail.definition_changed().number_of_listeners(), 0);
    let mut result = values;
    bag.get_value(None, Some(&mut result)).unwrap();
    assert_eq!(record(&result), serde_json::json!({"first":2.,"tail":2.,"next":3.}));
    assert_eq!(clock.get(), 2);
}
#[test]
fn removing_a_bag_field_stops_child_notifications() {
    let bag = PropertyBag::new();
    let child = Rc::new(ConstantProperty::new(Some(1.)));
    bag.add_property("x", Some(PropertyInput::Component(PropertyComponent::wrap(child.clone()))), None).unwrap();
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    let _guard = bag.definition_changed().add_listener(move |_| c.set(c.get() + 1));
    bag.remove_property("x").unwrap();
    assert_eq!(child.definition_changed().number_of_listeners(), 0);
    child.set_value(Some(2.));
    assert_eq!(count.get(), 1);
}
#[test]
fn entity_parent_ownership_avoids_cycles_and_preserves_old_values_in_callbacks() {
    let parent = entity("p");
    let child = entity("c");
    child.set_parent(Some(parent.clone())).unwrap();
    assert!(parent.set_parent(Some(child.clone())).is_err());
    let weak = Rc::downgrade(&parent);
    let observed = Rc::new(Cell::new(false));
    let out = observed.clone();
    let _guard = child.definition_changed().add_listener(move |v| {
        if v.property.name == "parent" {
            if let Some(PropertyInput::Value(DynamicValue::Entity(old))) = &v.property.old_value {
                out.set(old.upgrade().is_some());
            }
        }
    });
    drop(parent);
    assert!(weak.upgrade().is_some());
    child.set_parent(None).unwrap();
    assert!(observed.get());
    assert!(weak.upgrade().is_none());
}
#[test]
fn entity_helpers_reuse_the_same_position_property_identity() {
    let e = entity("e");
    let child = Rc::new(ConstantPositionProperty::new(Some(Cartesian3::UNIT_X), ReferenceFrame::Fixed));
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    let _guard = e.definition_changed().add_listener(move |_| c.set(c.get() + 1));
    e.set_position(Some(child.clone())).unwrap();
    e.set_position(Some(child.clone())).unwrap();
    assert_eq!(count.get(), 1);
    child.set_value(Some(Cartesian3::UNIT_Y), None);
    assert_eq!(count.get(), 2);
    assert_eq!(e.position_at(&date(0.)).unwrap(), Some(Cartesian3::UNIT_Y));
    drop(e);
    assert_eq!(child.definition_changed().number_of_listeners(), 0);
}
#[test]
fn collections_unsubscribe_new_entities_removed_before_resume() {
    let collection = EntityCollection::new();
    let e = entity("e");
    collection.suspend_events();
    collection.add(e.clone()).unwrap();
    collection.remove_all();
    assert_eq!(e.definition_changed().number_of_listeners(), 0);
    collection.resume_events().unwrap();
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    let _guard = collection.collection_changed().add_listener(move |_| c.set(c.get() + 1));
    e.set_name(Some("removed".into()));
    assert_eq!(count.get(), 0);
}
#[test]
fn collections_ignore_entity_callbacks_retired_during_dispatch() {
    let collection = EntityCollection::new();
    let e = entity("e");
    let weak = Rc::downgrade(&collection);
    let _first = e.definition_changed().add_listener(move |_| {
        weak.upgrade().unwrap().remove_by_id("e");
    });
    let seen = Rc::new(RefCell::new(vec![]));
    let s = seen.clone();
    let _guard = collection.collection_changed().add_listener(move |v| s.borrow_mut().push(collection_trace(v)));
    collection.add(e.clone()).unwrap();
    e.set_name(Some("deleted".into()));
    assert_eq!(*seen.borrow(), vec![serde_json::json!([["e"], [], []]), serde_json::json!([[], ["e"], []])]);
}
#[test]
fn references_validate_replaced_ids_and_do_not_retain_the_collection() {
    let collection = EntityCollection::new();
    let old = entity("e");
    old.set_property("description", Some(PropertyInput::value("old"))).unwrap();
    collection.add(old.clone()).unwrap();
    let reference = ReferenceProperty::new(collection.clone(), "e", vec!["description".into()]).unwrap();
    assert_eq!(reference.get_value(Some(&date(0.)), None).unwrap().unwrap().to_json(), serde_json::json!("old"));
    let new = entity("e");
    new.set_property("description", Some(PropertyInput::value("new"))).unwrap();
    collection.suspend_events();
    collection.remove_by_id("e");
    collection.add(new.clone()).unwrap();
    collection.resume_events().unwrap();
    assert_eq!(reference.get_value(Some(&date(0.)), None).unwrap().unwrap().to_json(), serde_json::json!("new"));
    assert_eq!(old.definition_changed().number_of_listeners(), 0);
    let weak = Rc::downgrade(&collection);
    drop(collection);
    assert!(weak.upgrade().is_none());
    assert!(reference.get_value(Some(&date(0.)), None).unwrap().is_none());
    assert_eq!(new.definition_changed().number_of_listeners(), 0);
}
#[test]
fn availability_notifications_keep_the_retired_interval_alive_during_dispatch() {
    let e = entity("e");
    e.set_availability(Some(availability(0., 10.)));
    let seen = Rc::new(Cell::new(false));
    let out = seen.clone();
    let _guard = e.definition_changed().add_listener(move |v| {
        if let Some(PropertyInput::Value(DynamicValue::Availability(old))) = &v.property.old_value {
            out.set(old.upgrade().is_some());
        }
    });
    e.set_availability(None);
    assert!(seen.get());
}
