mod common;
mod entity_common;
use cesium_rs::core::*;
use cesium_rs::data_sources::*;
use common::*;
use entity_common::*;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

type SharedProperty = Rc<ConstantProperty<DynamicValue>>;

fn property(id: &str, pool: &[(String, SharedProperty)]) -> Option<SharedProperty> {
    pool.iter().find(|(name, _)| name == id).map(|(_, p)| p.clone())
}

fn put_property(id: &str, value: f64, pool: &mut Vec<(String, SharedProperty)>) -> SharedProperty {
    if let Some(property) = property(id, pool) {
        return property;
    }
    let property = Rc::new(ConstantProperty::new(Some(DynamicValue::Number(value))));
    pool.push((id.into(), property.clone()));
    property
}

fn names<T: Graphics>() -> &'static [&'static str] {
    T::empty().property_names()
}

/// ModelGraphics nested PropertyBag coercion is not part of this batch; the
/// fixture and native operations skip those two slots and document them.
fn supported_names<T: Graphics>() -> Vec<&'static str> {
    names::<T>().iter().copied().filter(|name| !matches!(*name, "nodeTransformations" | "articulations")).collect()
}

fn component(property: SharedProperty) -> PropertyInput {
    PropertyInput::Component(PropertyComponent::wrap(property))
}

/// Mirrors data-sources-graphics.mjs option construction exactly.
fn options<T: Graphics>(pool: &mut Vec<(String, SharedProperty)>) -> Vec<(String, PropertyInput)> {
    supported_names::<T>()
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let value = if matches!(*name, "material" | "depthFailMaterial") {
                PropertyInput::value(Color::new(0.01 * index as f64, 0.2, 0.3, 0.4))
            } else if index % 2 == 0 {
                PropertyInput::value(index as f64 + 0.25)
            } else {
                component(put_property(&format!("p{index}"), index as f64 + 0.75, pool))
            };
            ((*name).to_owned(), value)
        })
        .collect()
}

fn listener_count(value: Option<&PropertyInput>) -> usize {
    value.and_then(PropertyInput::component).map_or(0, |p| p.definition_changed().number_of_listeners())
}

fn capture<T: Graphics>(graphics: &T, events: &RefCell<Vec<Value>>) -> Value {
    let fields = names::<T>()
        .iter()
        .map(|name| {
            let value = graphics.get(name);
            json!([name, describe(value.as_ref()), listener_count(value.as_ref())])
        })
        .collect::<Vec<_>>();
    let events = std::mem::take(&mut *events.borrow_mut());
    json!([fields, events])
}

fn run_case<T: Graphics + 'static>() -> Vec<Value> {
    let mut pool = Vec::new();
    let value = Rc::new(T::from_options_values(options::<T>(&mut pool)).unwrap());
    let events = Rc::new(RefCell::new(Vec::new()));
    let target_events = events.clone();
    let _guard = value.definition_changes().add_listener(move |change| target_events.borrow_mut().push(trace(change)));

    let mut out = vec![];
    out.push(capture(&*value, &events));

    let q = put_property("q", 99., &mut pool);
    for name in supported_names::<T>() {
        value.set(name, Some(component(q.clone()))).unwrap();
    }
    out.push(capture(&*value, &events));

    q.set_value(Some(DynamicValue::Number(100.)));
    out.push(capture(&*value, &events));

    // clone() without a result routes through the constructor merge path.
    let cloned = EntityComponent::clone_component(&*value, None).unwrap();
    let cloned = cloned.as_any().downcast_ref::<T>().unwrap();
    out.push(capture(cloned, &RefCell::new(vec![])));

    // clone(result) uses the upstream clone field list. ModelGraphics omits
    // shadows, so a pre-set result keeps the old shadows property.
    let target: ComponentRef = Rc::new(T::empty());
    {
        let target_graphics = target.as_any().downcast_ref::<T>().unwrap();
        if names::<T>().contains(&"shadows") {
            target_graphics.set("shadows", Some(component(q.clone()))).unwrap();
        }
    }
    EntityComponent::clone_component(&*value, Some(target.clone())).unwrap();
    let target_graphics = target.as_any().downcast_ref::<T>().unwrap();
    out.push(capture(target_graphics, &RefCell::new(vec![])));

    // merge fills only missing slots and shares the source property objects.
    let preset = T::empty();
    preset.set("show", Some(component(q.clone()))).unwrap();
    preset.merge_graphics(&*value).unwrap();
    out.push(capture(&preset, &RefCell::new(vec![])));

    for name in supported_names::<T>() {
        value.set(name, None).unwrap();
    }
    out.push(capture(&*value, &events));
    value.merge_graphics(cloned).unwrap();
    out.push(capture(&*value, &events));

    let from_options = T::from_options_values(options::<T>(&mut Vec::new())).unwrap();
    out.push(capture(&from_options, &RefCell::new(vec![])));

    out
}

fn dynamic(value: Option<DynamicValue>) -> Value {
    value.as_ref().map_or(Value::Null, encode)
}

fn polygon_hierarchy_case() -> Vec<Value> {
    let positions = DynamicValue::from_json(json!([[1., 2., 3.], [4., 5., 6.]]));
    let object = DynamicValue::from_json(json!({
        "positions": [[1., 2., 3.], [4., 5., 6.]],
        "holes": []
    }));
    let from_array =
        PolygonGraphics::with_options([("hierarchy".into(), PropertyInput::value(positions.clone()))]).unwrap();
    let from_object = PolygonGraphics::with_options([("hierarchy".into(), PropertyInput::value(object))]).unwrap();
    let direct = PolygonGraphics::new();
    direct.set_hierarchy(Some(PropertyInput::value(positions.clone()))).unwrap();
    let wrapped = PolygonGraphics::new();
    wrapped.set_hierarchy(Some(component(Rc::new(ConstantProperty::new(Some(positions)))))).unwrap();
    vec![
        capture(&from_array, &RefCell::new(vec![])),
        capture(&from_object, &RefCell::new(vec![])),
        capture(&direct, &RefCell::new(vec![])),
        capture(&wrapped, &RefCell::new(vec![])),
    ]
}

fn model_bags_case() -> Vec<Value> {
    let raw = ModelGraphics::with_options([
        ("articulations".into(), PropertyInput::value(DynamicValue::from_json(json!({"arm bend": 2})))),
        ("environmentMapOptions".into(), PropertyInput::value(DynamicValue::from_json(json!({"specular": 0.5})))),
    ])
    .unwrap();
    let source = ModelGraphics::with_options([
        ("articulations".into(), PropertyInput::value(DynamicValue::from_json(json!({"arm bend": 3})))),
        ("environmentMapOptions".into(), PropertyInput::value(DynamicValue::from_json(json!({"specular": 0.7})))),
    ])
    .unwrap();
    let target = ModelGraphics::new();
    target.merge_definition(&source).unwrap();
    vec![capture(&raw, &RefCell::new(vec![])), capture(&target, &RefCell::new(vec![]))]
}

fn color_material_case() -> Vec<Value> {
    let value = ColorMaterialProperty::new(Some(PropertyInput::value(Color::new(0.25, 0.5, 0.75, 1.)))).unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let target = events.clone();
    let _guard = value.definition_changes().add_listener(move |change| target.borrow_mut().push(trace(change)));

    let mut out = vec![json!([
        value.get_type(&date(0.)).unwrap(),
        dynamic(value.get_value(&date(0.), None).unwrap()),
        std::mem::take(&mut *events.borrow_mut())
    ])];

    let color = Rc::new(ConstantProperty::new(Some(DynamicValue::Color(Color::new(1., 0., 0., 0.5)))));
    value.set_color(Some(component(color.clone()))).unwrap();
    out.push(json!([
        value.is_constant(),
        dynamic(value.get_value(&date(0.), None).unwrap()),
        std::mem::take(&mut *events.borrow_mut())
    ]));

    color.set_value(Some(DynamicValue::Color(Color::new(0., 1., 0., 1.))));
    out.push(json!([
        value.is_constant(),
        dynamic(value.get_value(&date(0.), None).unwrap()),
        std::mem::take(&mut *events.borrow_mut())
    ]));

    let other = ColorMaterialProperty::new(Some(PropertyInput::value(Color::new(1., 0., 0., 0.5)))).unwrap();
    let green = ColorMaterialProperty::new(Some(PropertyInput::value(Color::new(0., 1., 0., 1.)))).unwrap();
    out.push(json!([
        EntityComponent::equals(&value, &value).unwrap(),
        EntityComponent::equals(&value, &other).unwrap(),
        EntityComponent::equals(&value, &green).unwrap(),
        describe(value.color().as_ref())
    ]));
    out
}

macro_rules! definition {
    ($name:expr) => {
        match $name {
            "BillboardGraphics" => run_case::<BillboardGraphics>(),
            "BoxGraphics" => run_case::<BoxGraphics>(),
            "Cesium3DTilesetGraphics" => run_case::<Cesium3DTilesetGraphics>(),
            "CorridorGraphics" => run_case::<CorridorGraphics>(),
            "CylinderGraphics" => run_case::<CylinderGraphics>(),
            "EllipseGraphics" => run_case::<EllipseGraphics>(),
            "EllipsoidGraphics" => run_case::<EllipsoidGraphics>(),
            "LabelGraphics" => run_case::<LabelGraphics>(),
            "ModelGraphics" => run_case::<ModelGraphics>(),
            "PathGraphics" => run_case::<PathGraphics>(),
            "PlaneGraphics" => run_case::<PlaneGraphics>(),
            "PointGraphics" => run_case::<PointGraphics>(),
            "PolygonGraphics" => run_case::<PolygonGraphics>(),
            "PolylineGraphics" => run_case::<PolylineGraphics>(),
            "PolylineVolumeGraphics" => run_case::<PolylineVolumeGraphics>(),
            "RectangleGraphics" => run_case::<RectangleGraphics>(),
            "WallGraphics" => run_case::<WallGraphics>(),
            other => panic!("unknown graphics class {other}"),
        }
    };
}

fn graphics_port(f: &str, a: &[Value]) -> Option<Out> {
    match f {
        "definition" => ok(definition!(s(&a[0]))),
        "polygonHierarchy" => ok(polygon_hierarchy_case()),
        "modelBags" => ok(model_bags_case()),
        "colorMaterial" => ok(color_material_case()),
        _ => None,
    }
}

#[test]
fn graphics_definitions_and_materials() {
    run("data_sources", "Graphics", |_| EXACT, graphics_port);
}
