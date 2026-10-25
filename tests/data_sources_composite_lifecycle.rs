mod common;
mod entity_common;
use cesium_rs::{core::*, data_sources::*};
use entity_common::*;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
};
fn layer(e: EntityRef) -> Rc<EntityCollection> {
    let c = EntityCollection::new();
    c.add(e).unwrap();
    c
}
fn description(e: &Entity, value: &str) {
    e.set_property("description", Some(PropertyInput::value(value))).unwrap();
}
fn text(c: &CompositeEntityCollection) -> String {
    let p = c.get_by_id("x").unwrap().get_property("description").unwrap().component().unwrap();
    if let Some(DynamicValue::String(s)) = p.get_value(&date(0.), None).unwrap() {
        s
    } else {
        panic!("string")
    }
}
#[test]
fn composite_subscriptions_and_owner_links_are_weak() {
    let e = entity("x");
    let source = layer(e.clone());
    let c = CompositeEntityCollection::new(vec![source.clone()]).unwrap();
    let output = c.get_by_id("x").unwrap();
    let weak = Rc::downgrade(&c);
    assert_eq!(source.collection_changed().number_of_listeners(), 1);
    assert_eq!(e.definition_changed().number_of_listeners(), 2);
    let internal = output.entity_collection().unwrap();
    let owner = internal.owner().unwrap();
    assert!(owner.is::<CompositeEntityCollection>());
    drop(owner);
    drop(internal);
    drop(c);
    assert!(weak.upgrade().is_none());
    assert!(output.entity_collection().is_none());
    assert_eq!(source.collection_changed().number_of_listeners(), 0);
    assert_eq!(e.definition_changed().number_of_listeners(), 1);
}
#[test]
fn duplicate_layers_and_shared_entities_have_one_subscription() {
    let e = entity("x");
    description(&e, "first");
    let a = layer(e.clone());
    let b = layer(e.clone());
    let a_source = a.clone() as EntitySourceRef;
    let c = CompositeEntityCollection::new(vec![a.clone(), a.clone(), b.clone()]).unwrap();
    assert_eq!(a.collection_changed().number_of_listeners(), 1);
    assert_eq!(e.definition_changed().number_of_listeners(), 3);
    let count = Rc::new(Cell::new(0));
    let seen = count.clone();
    let _guard = c.collection_changed().add_listener(move |_| seen.set(seen.get() + 1));
    description(&e, "second");
    assert_eq!(text(&c), "second");
    assert_eq!(count.get(), 1);
    c.remove_collection(&a_source).unwrap();
    c.remove_collection(&a_source).unwrap();
    assert_eq!(a.collection_changed().number_of_listeners(), 0);
    description(&e, "third");
    assert_eq!(text(&c), "third");
    c.remove_all_collections().unwrap();
    assert_eq!(e.definition_changed().number_of_listeners(), 2);
}
#[test]
fn nested_composites_resolve_references_and_reject_cycles() {
    let e = entity("x");
    description(&e, "one");
    let source = layer(e.clone());
    let a = CompositeEntityCollection::new(vec![source]).unwrap();
    let b = CompositeEntityCollection::new(vec![a.clone()]).unwrap();
    assert!(a.add_collection(b.clone(), None).is_err());
    assert!(a.add_collection(a.clone(), None).is_err());
    let reference = ReferenceProperty::from_string(b.clone(), "x#description").unwrap();
    description(&e, "two");
    assert_eq!(text(&b), "two");
    assert!(matches!(reference.get_value(Some(&date(0.)),None).unwrap(),Some(DynamicValue::String(s)) if s=="two"));
    let weak = Rc::downgrade(&b);
    drop(b);
    assert!(weak.upgrade().is_none());
    assert!(reference.get_value(Some(&date(0.)), None).unwrap().is_none());
}
#[test]
fn retired_entity_callbacks_do_not_restore_removed_layers() {
    let e = entity("x");
    description(&e, "one");
    let source = layer(e.clone());
    let link = Rc::new(RefCell::new(None::<std::rc::Weak<CompositeEntityCollection>>));
    let target = link.clone();
    let source_ref = source.clone() as EntitySourceRef;
    let _first = e.definition_changed().add_listener(move |_| {
        target.borrow().as_ref().unwrap().upgrade().unwrap().remove_collection(&source_ref).unwrap();
    });
    let c = CompositeEntityCollection::new(vec![source.clone()]).unwrap();
    *link.borrow_mut() = Some(Rc::downgrade(&c));
    description(&e, "two");
    assert!(c.is_empty());
    assert!(c.take_error().is_none());
    assert_eq!(source.collection_changed().number_of_listeners(), 0);
}
#[test]
fn collection_callbacks_retired_by_an_earlier_listener_are_ignored() {
    let source = EntityCollection::new();
    let link = Rc::new(RefCell::new(None::<std::rc::Weak<CompositeEntityCollection>>));
    let target = link.clone();
    let _first = source.collection_changed().add_listener(move |_| {
        target.borrow().as_ref().unwrap().upgrade().unwrap().recompose().unwrap();
    });
    let c = CompositeEntityCollection::new(vec![source.clone()]).unwrap();
    *link.borrow_mut() = Some(Rc::downgrade(&c));
    let count = Rc::new(Cell::new(0));
    let out = count.clone();
    let _guard = c.collection_changed().add_listener(move |_| out.set(out.get() + 1));
    let e = entity("x");
    description(&e, "new");
    source.add(e).unwrap();
    assert_eq!(count.get(), 1);
    assert_eq!(text(&c), "new");
    assert!(c.take_error().is_none());
}
#[test]
fn staged_entities_keep_the_composite_as_their_collection_owner() {
    let e = entity("x");
    description(&e, "value");
    let c = CompositeEntityCollection::new(vec![layer(e)]).unwrap();
    let output = c.get_by_id("x").unwrap();
    let target = Rc::downgrade(&output);
    let observed = Rc::new(Cell::new(0));
    let count = observed.clone();
    let _guard = output.definition_changed().add_listener(move |_| {
        let owner = target.upgrade().unwrap().entity_collection().unwrap().owner().unwrap();
        assert!(owner.is::<CompositeEntityCollection>());
        count.set(count.get() + 1);
    });
    c.recompose().unwrap();
    assert!(observed.get() >= 2);
}
#[test]
fn layer_edits_from_output_callbacks_are_serialized() {
    let low = entity("x");
    description(&low, "low");
    let high = entity("x");
    description(&high, "high");
    let a = layer(low.clone());
    let b = layer(high.clone());
    let a_source = a.clone() as EntitySourceRef;
    let c = CompositeEntityCollection::new(vec![a, b]).unwrap();
    let original = c.get_by_id("x").unwrap();
    let weak = Rc::downgrade(&c);
    let once = Cell::new(false);
    let _guard = c.collection_changed().add_listener(move |_| {
        if !once.replace(true) {
            weak.upgrade().unwrap().raise_collection_to_top(&a_source).unwrap();
        }
    });
    description(&high, "new high");
    assert_eq!(text(&c), "low");
    assert!(Rc::ptr_eq(&original, &c.get_by_id("x").unwrap()));
    assert!(c.take_error().is_none());
}
#[test]
fn nested_suspension_coalesces_reordering_and_membership() {
    let x = entity("x");
    description(&x, "low");
    let y = entity("x");
    description(&y, "high");
    let a = layer(x.clone());
    let b = layer(y.clone());
    let a_source = a.clone() as EntitySourceRef;
    let c = CompositeEntityCollection::new(vec![a.clone(), b.clone()]).unwrap();
    let count = Rc::new(Cell::new(0));
    let out = count.clone();
    let _guard = c.collection_changed().add_listener(move |_| out.set(out.get() + 1));
    c.suspend_events();
    c.suspend_events();
    c.raise_collection_to_top(&a_source).unwrap();
    description(&x, "next");
    b.add(entity("y")).unwrap();
    c.resume_events().unwrap();
    assert_eq!(count.get(), 0);
    c.resume_events().unwrap();
    assert_eq!(count.get(), 1);
    assert_eq!(text(&c), "next");
    assert_eq!(c.len(), 2);
    assert!(c.resume_events().is_err());
}
struct Fallible {
    changed: DefinitionChanged,
    fail: Rc<Cell<bool>>,
}
impl EntityComponent for Fallible {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        Ok(other.as_any().is::<Self>())
    }
    fn supports_clone(&self) -> bool {
        true
    }
    fn supports_merge(&self) -> bool {
        true
    }
    fn clone_component(&self, result: Option<ComponentRef>) -> Result<ComponentRef, RuntimeError> {
        if self.fail.get() {
            return Err(RuntimeError::new("application clone failed"));
        }
        Ok(result.unwrap_or_else(|| Rc::new(Self { changed: DefinitionChanged::new(), fail: self.fail.clone() })))
    }
    fn merge_component(&self, _: &dyn EntityComponent) -> Result<(), RuntimeError> {
        Ok(())
    }
}
#[test]
fn automatic_component_errors_are_observable_and_recoverable() {
    let fail = Rc::new(Cell::new(false));
    let property = Rc::new(Fallible { changed: DefinitionChanged::new(), fail: fail.clone() });
    let e = entity("x");
    e.set_property("fixtureData", Some(PropertyInput::Component(property.clone()))).unwrap();
    let source = layer(e.clone());
    let c = CompositeEntityCollection::new(vec![source]).unwrap();
    let seen = Rc::new(Cell::new(0));
    let out = seen.clone();
    let _errors = c.errors().add_listener(move |_| out.set(out.get() + 1));
    fail.set(true);
    property.changed.raise();
    assert_eq!(seen.get(), 1);
    assert_eq!(c.take_error().unwrap().message, "application clone failed");
    assert!(c.recompose().is_err());
    fail.set(false);
    c.recompose().unwrap();
    let count = Rc::new(Cell::new(0));
    let out = count.clone();
    let _guard = c.collection_changed().add_listener(move |_| out.set(out.get() + 1));
    e.set_name(Some("after recovery".into()));
    assert!(count.get() > 0);
}
#[test]
fn constructor_serializes_notifications_from_shared_property_bags() {
    let low = entity("x");
    let high = entity("x");
    low.set_property("properties", Some(PropertyInput::Value(DynamicValue::from_json(serde_json::json!({"a":1})))))
        .unwrap();
    high.set_property("properties", Some(PropertyInput::Value(DynamicValue::from_json(serde_json::json!({"b":2})))))
        .unwrap();
    let c = CompositeEntityCollection::new(vec![layer(low), layer(high.clone())]).unwrap();
    assert!(c.take_error().is_none());
    let bag = c.get_by_id("x").unwrap().get_property("properties").unwrap().component().unwrap();
    assert!(bag.get_property("a").is_some());
    assert!(bag.get_property("b").is_some());
    let high_bag = high.get_property("properties").unwrap().component().unwrap();
    assert_eq!(bag.identity(), high_bag.identity());
}
