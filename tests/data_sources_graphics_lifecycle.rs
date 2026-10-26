mod common;
mod entity_common;
use cesium_rs::core::*;
use cesium_rs::data_sources::*;
use entity_common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn decode(value: Option<DynamicValue>) -> Value {
    value.as_ref().map_or(Value::Null, encode)
}

fn box_component(value: BoxGraphics) -> Rc<BoxGraphics> {
    Rc::new(value)
}

#[test]
fn entity_graphics_factory_nested_references_and_forwarding() {
    let collection = EntityCollection::new();
    let entity = Entity::with_id("vehicle");
    entity
        .set_property(
            "box",
            Some(PropertyInput::Value(DynamicValue::from_json(json!({
                "show": true,
                "dimensions": [1, 2, 3],
                "fill": false
            })))),
        )
        .unwrap();
    collection.add(entity.clone()).unwrap();

    let graphics = entity.get_property("box").unwrap().component().unwrap();
    let graphics = graphics.as_any().downcast_ref::<BoxGraphics>().unwrap();
    assert!(graphics.show().is_some());
    assert!(graphics.dimensions().is_some());
    assert_eq!(decode(graphics.show().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(true));

    // Nested references resolve through the EntityComponent get_property path.
    let show = ReferenceProperty::from_string(collection.clone(), "vehicle#box.show").unwrap();
    assert_eq!(decode(show.get_value(Some(&date(0.)), None).unwrap()), json!(true));
    let dimensions = ReferenceProperty::from_string(collection.clone(), "vehicle#box.dimensions").unwrap();
    assert_eq!(decode(dimensions.get_value(Some(&date(0.)), None).unwrap()), json!([1, 2, 3]));

    // Graphics child changes are forwarded as child_changed EntityChanges.
    let changes = Rc::new(RefCell::new(vec![]));
    let target = changes.clone();
    let _guard = entity.definition_changed().add_listener(move |change| target.borrow_mut().push(change.clone()));
    graphics.set_show(Some(PropertyInput::value(false))).unwrap();
    let changes = std::mem::take(&mut *changes.borrow_mut());
    let change = changes.last().expect("forwarded graphics change");
    assert_eq!(change.property.name, "box");
    assert!(change.property.child_changed);
    assert_eq!(decode(show.get_value(Some(&date(0.)), None).unwrap()), json!(false));
}

#[test]
fn graphics_clone_and_merge_share_fields_and_reject_other_types() {
    let source = BoxGraphics::with_options([
        ("show".into(), PropertyInput::value(true)),
        ("outline".into(), PropertyInput::value(false)),
        ("material".into(), PropertyInput::value(Color::new(1., 0., 0., 1.))),
    ])
    .unwrap();

    let clone = source.clone_definition().unwrap();
    assert_eq!(
        clone.show().unwrap().component().unwrap().identity(),
        source.show().unwrap().component().unwrap().identity()
    );
    assert_eq!(clone.material().unwrap().component().unwrap().get_type(&date(0.)).unwrap(), Some("Color".into()));

    let target = BoxGraphics::new();
    target.set_outline_width(Some(PropertyInput::value(3.))).unwrap();
    target.merge_definition(&source).unwrap();
    assert_eq!(decode(target.show().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(true));
    // Existing target slots win; source material is shared because it was absent.
    assert_eq!(
        decode(target.outline_width().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()),
        json!(3.)
    );
    assert_eq!(target.material().unwrap().component().unwrap().get_type(&date(0.)).unwrap(), Some("Color".into()));

    let other = CorridorGraphics::new();
    assert!(EntityComponent::merge_component(&target, &other).is_err());
    assert!(!EntityComponent::equals(&target, &other).unwrap());
}

#[test]
fn model_nested_property_bags_merge_when_explicitly_supplied() {
    let source = ModelGraphics::with_options([("uri".into(), PropertyInput::value("model.glb"))]).unwrap();
    let source_bag = PropertyBag::from_values(vec![("node".into(), PropertyInput::value(1.))], None).unwrap();
    source.set_node_transformations(Some(PropertyInput::Component(source_bag.clone() as ComponentRef))).unwrap();

    let target = ModelGraphics::new();
    target.merge_definition(&source).unwrap();
    let merged = target.node_transformations().unwrap().component().unwrap();
    assert!(!Rc::ptr_eq(&merged, &(source_bag.clone() as ComponentRef)));
    let merged = merged.as_any().downcast_ref::<PropertyBag>().unwrap();
    assert_eq!(merged.property_names(), vec!["node"]);

    // A nested raw value is intentionally still a plain ConstantProperty; the
    // special PropertyBag coercion and NodeTransformationProperty remain open.
    let raw = ModelGraphics::with_options([(
        "nodeTransformations".into(),
        PropertyInput::value(DynamicValue::from_json(json!({"node": {"translation": [1, 2, 3]}}))),
    )])
    .unwrap();
    let raw = raw.node_transformations().unwrap().component().unwrap();
    assert!(raw.as_any().is::<PropertyComponent<DynamicValue>>());
}

#[test]
fn model_clone_keeps_shadows_only_on_the_constructor_path() {
    let source = ModelGraphics::with_options([
        ("uri".into(), PropertyInput::value("model.glb")),
        ("shadows".into(), PropertyInput::value(2.)),
    ])
    .unwrap();
    let cloned = source.clone_definition().unwrap();
    assert_eq!(decode(cloned.shadows().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(2.));

    let target = ModelGraphics::new();
    target.set_shadows(Some(PropertyInput::value(7.))).unwrap();
    source.clone_definition_into(&target);
    assert_eq!(decode(target.shadows().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(7.));
    assert_eq!(
        decode(target.uri().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()),
        json!("model.glb")
    );
}

#[test]
fn composite_entity_collections_clone_and_merge_graphics() {
    let defaults = EntityCollection::new();
    let base = Entity::with_id("vehicle");
    base.set_property(
        "box",
        Some(PropertyInput::Component(box_component(
            BoxGraphics::with_options([
                ("show".into(), PropertyInput::value(false)),
                ("fill".into(), PropertyInput::value(true)),
            ])
            .unwrap(),
        ))),
    )
    .unwrap();
    defaults.add(base).unwrap();

    let overrides = EntityCollection::new();
    let top = Entity::with_id("vehicle");
    top.set_property(
        "box",
        Some(PropertyInput::Component(box_component(
            BoxGraphics::with_options([("show".into(), PropertyInput::value(true))]).unwrap(),
        ))),
    )
    .unwrap();
    overrides.add(top).unwrap();

    let composite = CompositeEntityCollection::new(vec![defaults, overrides]).unwrap();
    let entity = composite.get_by_id("vehicle").unwrap();
    let graphics = entity.get_property("box").unwrap().component().unwrap();
    let graphics = graphics.as_any().downcast_ref::<BoxGraphics>().unwrap();
    assert_eq!(decode(graphics.show().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(true));
    assert_eq!(decode(graphics.fill().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(true));
    assert!(graphics.material().is_none());
}

#[test]
fn graphics_events_and_material_nested_reference() {
    let collection = EntityCollection::new();
    let entity = Entity::with_id("vehicle");
    let graphics = box_component(
        BoxGraphics::with_options([("material".into(), PropertyInput::value(Color::new(0.2, 0.4, 0.6, 1.)))]).unwrap(),
    );
    entity.set_property("box", Some(PropertyInput::Component(graphics.clone()))).unwrap();
    collection.add(entity).unwrap();

    let color = ReferenceProperty::from_string(collection.clone(), "vehicle#box.material.color").unwrap();
    assert_eq!(decode(color.get_value(Some(&date(0.)), None).unwrap()), json!([0.2, 0.4, 0.6, 1.]));

    let changes = Rc::new(Cell::new(0));
    let target = changes.clone();
    let _guard = graphics.definition_changes().add_listener(move |_| {
        let next = target.get() + 1;
        target.set(next);
    });
    let material = graphics.material().unwrap().component().unwrap();
    let material = material.as_any().downcast_ref::<ColorMaterialProperty>().unwrap();
    material.set_color(Some(PropertyInput::value(Color::new(0., 1., 0., 1.)))).unwrap();
    assert_eq!(changes.get(), 1);
    assert_eq!(decode(color.get_value(Some(&date(0.)), None).unwrap()), json!([0., 1., 0., 1.]));
    assert_eq!(material.get_type(&date(0.)).unwrap(), Some("Color".into()));
}

#[test]
fn material_descriptor_rejects_unported_inputs() {
    let Err(error) = BoxGraphics::with_options([("material".into(), PropertyInput::value("image.png"))]) else {
        panic!("image materials must be rejected");
    };
    assert!(error.to_string().contains("ImageMaterialProperty"));
    let Err(error) = BoxGraphics::with_options([("material".into(), PropertyInput::value(3.))]) else {
        panic!("unknown materials must be rejected");
    };
    assert!(error.to_string().contains("Unable to infer material type"));
}

#[test]
fn every_graphics_slot_starts_undefined() {
    assert!(BillboardGraphics::new().show().is_none());
    assert!(BoxGraphics::new().show().is_none());
    assert!(Cesium3DTilesetGraphics::new().show().is_none());
    assert!(CorridorGraphics::new().show().is_none());
    assert!(CylinderGraphics::new().show().is_none());
    assert!(EllipseGraphics::new().show().is_none());
    assert!(EllipsoidGraphics::new().show().is_none());
    assert!(LabelGraphics::new().show().is_none());
    assert!(ModelGraphics::new().show().is_none());
    assert!(PathGraphics::new().show().is_none());
    assert!(PlaneGraphics::new().show().is_none());
    assert!(PointGraphics::new().show().is_none());
    assert!(PolygonGraphics::new().show().is_none());
    assert!(PolylineGraphics::new().show().is_none());
    assert!(PolylineVolumeGraphics::new().show().is_none());
    assert!(RectangleGraphics::new().show().is_none());
    assert!(WallGraphics::new().show().is_none());
}

#[test]
fn raw_articulations_merge_into_existing_bag() {
    let model = ModelGraphics::with_options([(
        "articulations".into(),
        PropertyInput::value(DynamicValue::from_json(json!({"bend": 1}))),
    )])
    .unwrap();
    model
        .merge_options([(
            "articulations".into(),
            PropertyInput::value(DynamicValue::from_json(json!({"bend": 2, "twist": 3}))),
        )])
        .unwrap();
    let component = model.articulations().unwrap().component().unwrap();
    let bag = component.as_any().downcast_ref::<PropertyBag>().unwrap();
    assert_eq!(
        DynamicValue::Record(bag.get_value(Some(&date(0.)), None).unwrap()).to_json(),
        json!({"bend": 1.0, "twist": 3.0})
    );
}

#[test]
fn color_material_preserves_result_fields() {
    let material = ColorMaterialProperty::default();
    let mut result = DynamicValue::Record(vec![("sentinel".into(), Some(DynamicValue::Number(7.)))]);
    let value = material.get_value(&date(0.), Some(&mut result)).unwrap().unwrap();
    assert_eq!(value.get("sentinel").unwrap().to_json(), json!(7.));
    assert_eq!(value.get("color").unwrap().to_json(), json!([1., 1., 1., 1.]));
}

#[test]
fn entity_merges_raw_graphics_options() {
    let entity = Entity::with_id("test");
    entity.set_property("box", Some(PropertyInput::Component(Rc::new(BoxGraphics::new())))).unwrap();
    entity
        .merge_values(&[("box".into(), PropertyInput::value(DynamicValue::from_json(json!({"show": true}))))])
        .unwrap();
    let component = entity.get_property("box").unwrap().component().unwrap();
    let graphics = component.as_any().downcast_ref::<BoxGraphics>().unwrap();
    assert_eq!(decode(graphics.show().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(true));
}

#[test]
fn null_articulations_remain_undefined() {
    let model =
        ModelGraphics::with_options([("articulations".into(), PropertyInput::value(DynamicValue::Null))]).unwrap();
    assert!(model.articulations().is_none());
}

#[test]
fn entity_raw_merge_skips_existing_material_before_coercion() {
    let entity = Entity::with_id("test");
    let graphics = Rc::new(BoxGraphics::new());
    graphics.set_material(Some(PropertyInput::value(Color::WHITE))).unwrap();
    let original_material = graphics.material().unwrap().component().unwrap();
    entity.set_property("box", Some(PropertyInput::Component(graphics.clone()))).unwrap();
    entity
        .merge_values(&[(
            "box".into(),
            PropertyInput::value(DynamicValue::from_json(json!({"material": 3, "fill": false}))),
        )])
        .unwrap();
    assert_eq!(graphics.material().unwrap().component().unwrap().identity(), original_material.identity());
    assert_eq!(decode(graphics.fill().unwrap().component().unwrap().get_value(&date(0.), None).unwrap()), json!(false));
}
