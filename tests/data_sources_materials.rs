mod common;
mod entity_common;

use cesium_rs::core::{clock::ClockOptions, resource::ResourceOptions};
use cesium_rs::{
    core::*,
    data_sources::*,
    scene::{Material, MaterialImage, MaterialValue},
};
use common::*;
use entity_common::{date, encode};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn options(fields: &Value) -> Vec<(String, PropertyInput)> {
    fields
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, v)| {
            let value = match key.as_str() {
                "color" | "evenColor" | "oddColor" | "outlineColor" | "gapColor" => color(v).into(),
                "lineCount" | "lineThickness" | "lineOffset" => c2(v).into(),
                "repeat" if v.is_array() => c2(v).into(),
                "rotation" => q(v).into(),
                "translation" | "scale" => c3(v).into(),
                _ => DynamicValue::from_json(v.clone()),
            };
            (key.clone(), PropertyInput::Value(value))
        })
        .collect()
}
fn make(name: &str, options: Vec<(String, PropertyInput)>) -> ComponentRef {
    match name {
        "ImageMaterialProperty" => Rc::new(ImageMaterialProperty::with_options(options).unwrap()),
        "GridMaterialProperty" => Rc::new(GridMaterialProperty::with_options(options).unwrap()),
        "StripeMaterialProperty" => Rc::new(StripeMaterialProperty::with_options(options).unwrap()),
        "CheckerboardMaterialProperty" => Rc::new(CheckerboardMaterialProperty::with_options(options).unwrap()),
        "PolylineArrowMaterialProperty" => Rc::new(PolylineArrowMaterialProperty::with_options(options).unwrap()),
        "PolylineDashMaterialProperty" => Rc::new(PolylineDashMaterialProperty::with_options(options).unwrap()),
        "PolylineGlowMaterialProperty" => Rc::new(PolylineGlowMaterialProperty::with_options(options).unwrap()),
        "PolylineOutlineMaterialProperty" => Rc::new(PolylineOutlineMaterialProperty::with_options(options).unwrap()),
        "NodeTransformationProperty" => Rc::new(NodeTransformationProperty::with_options(options).unwrap()),
        other => panic!("unknown definition {other}"),
    }
}
fn at(p: &dyn EntityComponent, t: f64) -> Value {
    p.get_value(&date(t), None).unwrap().as_ref().map_or(Value::Null, encode)
}
fn definition(name: &str, fields: &Value) -> Value {
    let inputs = options(fields);
    let p = make(name, inputs.clone());
    let empty = make(name, Vec::new());
    let mut result = DynamicValue::Record(vec![("extra".into(), Some(12.0.into()))]);
    let returned = p.get_value(&date(0.), Some(&mut result)).unwrap().unwrap();
    let variants: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(index, (key, v))| {
            let mut changed = inputs.clone();
            let PropertyInput::Value(value) = v.clone() else { unreachable!() };
            let callback =
                CallbackProperty::new(Rc::new(move |t, _| Ok((*t == date(0.)).then(|| value.clone()))), false);
            changed[index].1 = PropertyInput::Component(PropertyComponent::wrap(Rc::new(callback)));
            let varying = make(name, changed);
            json!([key, varying.is_constant(), varying.equals(&*p).unwrap(), at(&*varying, 0.), at(&*varying, 1.)])
        })
        .collect();
    let kind = if name == "NodeTransformationProperty" { None } else { p.get_type(&date(0.)).unwrap() };
    json!([
        kind,
        at(&*empty, 0.),
        p.is_constant(),
        encode(&result),
        returned.property_equals(&result),
        p.equals(&*make(name, inputs)).unwrap(),
        p.equals(&*empty).unwrap(),
        p.equals(&ColorMaterialProperty::default()).unwrap(),
        variants
    ])
}
fn uniforms(material: &Material) -> Value {
    Value::Object(
        material
            .uniforms
            .iter()
            .map(|(name, v)| {
                let value = match v {
                    MaterialValue::Float(f) => json!(f),
                    MaterialValue::Bool(b) => json!(b),
                    MaterialValue::Vec2(v) => json!(v.to_array()),
                    MaterialValue::Vec4(v) => json!(v.to_array()),
                    MaterialValue::Sampler2D(MaterialImage::Url(url)) => json!(url),
                    MaterialValue::Sampler2D(MaterialImage::Default) => Value::Null,
                    _ => panic!("unexpected uniform {name}"),
                };
                (name.clone(), value)
            })
            .collect(),
    )
}
fn resolve(name: &str, fields: &Value) -> Value {
    let p = make(name, options(fields));
    let mut old = Material::from_type("Color");
    old.translucent = Some(false);
    let mut material = MaterialProperty::get_value(Some(&date(0.)), Some(&*p), Some(old)).unwrap();
    let kept_old = material.translucent == Some(false);
    material.translucent = Some(false);
    let reused = MaterialProperty::get_value(Some(&date(1.)), Some(&*p), Some(material)).unwrap();
    let snapshot = uniforms(&reused);
    let kind = reused.type_name.clone();
    let kept_material = reused.translucent == Some(false);
    let fallback = MaterialProperty::get_value(Some(&date(0.)), None, Some(reused)).unwrap();
    json!([kind, snapshot, kept_old, kept_material, fallback.type_name, uniforms(&fallback)])
}
fn reuse_color() -> Value {
    let p = ColorMaterialProperty::new(Some(PropertyInput::value(Color::new(0.8, 0.7, 0.6, 0.4)))).unwrap();
    let old = Material::from_type("Color");
    let shader = old.shader_source.as_ptr();
    let colored = MaterialProperty::get_value(Some(&date(0.)), Some(&p), Some(old)).unwrap();
    let before = uniforms(&colored);
    let fallback = MaterialProperty::get_value(None, Some(&CompositeMaterialProperty::new()), Some(colored)).unwrap();
    json!([before, shader == fallback.shader_source.as_ptr(), uniforms(&fallback)])
}
fn descriptor() -> Value {
    let original = Rc::new(
        Resource::from_options(ResourceOptions {
            url: "textures/water.png?v=2".into(),
            headers: vec![("Demo".into(), "kept".into())],
            ..Default::default()
        })
        .unwrap(),
    );
    let graphics = BoxGraphics::with_options([("material".into(), PropertyInput::value(original.clone()))]).unwrap();
    let image_material = graphics.material().unwrap().component().unwrap();
    let image = image_material.get_value(&date(0.), None).unwrap().unwrap().get("image").unwrap();
    let DynamicValue::Resource(resource) = image else { panic!("lost image resource") };
    let headers: serde_json::Map<_, _> =
        resource.options().headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let line = PolylineGraphics::with_options([
        ("depthFailMaterial".into(), PropertyInput::value("textures/fail.png")),
        ("material".into(), PropertyInput::value(Color::new(0.1, 0.2, 0.3, 1.))),
    ])
    .unwrap();
    let depth = line.depth_fail_material().unwrap().component().unwrap();
    json!([
        image_material.get_type(&date(0.)).unwrap(),
        image_material.as_any().is::<ImageMaterialProperty>(),
        !Rc::ptr_eq(&resource, &original),
        resource.url(),
        headers,
        depth.get_type(&date(0.)).unwrap(),
        at(&*depth, 0.),
        line.material().unwrap().component().unwrap().get_type(&date(0.)).unwrap()
    ])
}
fn event_counter(changed: &DefinitionChanged) -> (Rc<Cell<usize>>, Subscription) {
    let count = Rc::new(Cell::new(0));
    let counter = count.clone();
    let guard = changed.add_listener(move |_| counter.set(counter.get() + 1));
    (count, guard)
}
fn composite() -> Value {
    let p = CompositeMaterialProperty::new();
    let empty = p.is_constant();
    let grid = Rc::new(GridMaterialProperty::new());
    let (events, _guard) = event_counter(p.definition_changed());
    let fill = |target: &CompositeMaterialProperty, grid: ComponentRef, last: ComponentRef| {
        for (a, b, child) in [(0., 4., grid), (4., 8., Rc::new(StripeMaterialProperty::new())), (10., 12., last)] {
            target.intervals().add_interval(TimeInterval::new(date(a), date(b), true, false, Some(child)));
        }
    };
    fill(&p, grid.clone(), grid.clone());
    let copy = CompositeMaterialProperty::new();
    fill(&copy, Rc::new(GridMaterialProperty::new()), Rc::new(GridMaterialProperty::new()));
    let constant = p.is_constant();
    let equal = p.equals(&copy).unwrap();
    let listeners = grid.definition_changed().number_of_listeners();
    grid.set_cell_alpha(Some(PropertyInput::value(0.9))).unwrap();
    let changed = events.get();
    let still_equal = p.equals(&copy).unwrap();
    let values: Vec<_> = [-1., 0., 3., 4., 8., 10., 12.]
        .into_iter()
        .map(|t| json!([p.get_type(&date(t)).unwrap(), at(&p, t)]))
        .collect();
    p.intervals().remove_all();
    json!([
        empty,
        constant,
        equal,
        listeners,
        changed,
        still_equal,
        values,
        events.get(),
        p.is_constant(),
        grid.definition_changed().number_of_listeners()
    ])
}
fn nodes(fields: &Value) -> Value {
    let fields = options(fields)
        .into_iter()
        .map(|(k, v)| {
            let PropertyInput::Value(v) = v else { unreachable!() };
            (k, Some(v))
        })
        .collect();
    let source = ModelGraphics::with_options([(
        "nodeTransformations".into(),
        PropertyInput::value(DynamicValue::Record(vec![
            ("arm".into(), Some(DynamicValue::Record(fields))),
            ("root".into(), Some(DynamicValue::Record(vec![]))),
        ])),
    )])
    .unwrap();
    let copy = source.clone_definition().unwrap();
    let assigned = ModelGraphics::new();
    source.clone_definition_into(&assigned);
    let target =
        ModelGraphics::from_dynamic(DynamicValue::from_json(json!({"nodeTransformations":{"arm":{}}}))).unwrap();
    target.merge_definition(&source).unwrap();
    let bag = |model: &ModelGraphics| model.node_transformations().unwrap().component().unwrap();
    let child = |bag: &ComponentRef, name: &str| bag.get_property(name).unwrap().component().unwrap();
    let original = bag(&source);
    let copied = bag(&copy);
    let target_bag = bag(&target);
    let before = at(&*target_bag, 0.);
    let (events, _guard) = event_counter(copy.definition_changed());
    let arm = child(&original, "arm");
    let node = arm.as_any().downcast_ref::<NodeTransformationProperty>().unwrap();
    node.set_translation(Some(PropertyInput::value(Cartesian3::new(9., 8., 7.)))).unwrap();
    json!([
        arm.as_any().is::<NodeTransformationProperty>(),
        before,
        !Rc::ptr_eq(&copied, &original),
        Rc::ptr_eq(&bag(&assigned), &original),
        Rc::ptr_eq(&child(&copied, "arm"), &arm),
        Rc::ptr_eq(&child(&target_bag, "root"), &child(&original, "root")),
        events.get(),
        at(&*copied, 0.)
    ])
}
fn rotation(a: f64, b: f64) -> Value {
    let mut packed = [99., 0., 0.];
    Rotation(a).pack(&mut packed, 1);
    Rotation(b).pack(&mut packed, 2);
    let converted = Rotation::convert_packed_array_for_interpolation(&packed, 1, 2);
    let p = SampledProperty::<Rotation>::new(None);
    let other = SampledProperty::<Rotation>::new(None);
    let empty = p.is_constant();
    let (events, _guard) = event_counter(p.definition_changed());
    for property in [&p, &other] {
        property.add_samples(&[date(0.), date(4.)], &[Rotation(a), Rotation(b)], None).unwrap();
    }
    let values: Vec<_> = [-1., 0., 1., 2., 3., 4., 5.]
        .into_iter()
        .map(|t| p.get_value(Some(&date(t)), None).unwrap().map(|v| v.0))
        .collect();
    json!([
        Rotation::PACKED_LENGTH,
        packed,
        Rotation::unpack(&packed, 2).0,
        converted,
        Rotation::unpack_interpolation_result(&converted).0,
        empty,
        p.is_constant(),
        events.get(),
        p.equals(&other).unwrap(),
        values
    ])
}
fn clock() -> Value {
    let p = DataSourceClock::new();
    let trace = Rc::new(RefCell::new(vec![]));
    let listener = trace.clone();
    let _guard = p.definition_changes().add_listener(move |change| listener.borrow_mut().push(change.name.clone()));
    let start = Rc::new(date(-10.));
    p.set_start_time(Some(start.clone()));
    p.set_start_time(Some(start.clone()));
    p.set_start_time(Some(Rc::new(*start)));
    p.set_stop_time(Some(Rc::new(date(20.))));
    p.set_current_time(Some(Rc::new(date(4.))));
    p.set_clock_range(Some(ClockRange::LoopStop));
    p.set_clock_step(Some(ClockStep::TickDependent));
    p.set_multiplier(Some(-2.));
    let clone = p.clone();
    let merged = DataSourceClock::new();
    merged.set_multiplier(Some(3.));
    merged.merge(&p);
    let seconds = |date_: JulianDate| JulianDate::seconds_difference(&date_, &date(0.));
    let snap = |c: &DataSourceClock| {
        json!([
            c.start_time().map(|v| seconds(*v)),
            c.stop_time().map(|v| seconds(*v)),
            c.current_time().map(|v| seconds(*v)),
            c.clock_range().map(|v| v as i32),
            c.clock_step().map(|v| v as i32),
            c.multiplier()
        ])
    };
    let core = |c: &Clock| {
        json!([
            seconds(c.start_time),
            seconds(c.stop_time),
            seconds(c.current_time()),
            c.clock_range as i32,
            c.clock_step() as i32,
            c.multiplier()
        ])
    };
    let mut result =
        Clock::new(ClockOptions { start_time: Some(date(0.)), should_animate: Some(true), ..Default::default() })
            .unwrap();
    let tick_listener = result.on_tick.add_event_listener(|_| {});
    merged.get_value_into(&mut result);
    let full = core(&result);
    let sparse = DataSourceClock::new();
    sparse.set_current_time(Some(Rc::new(date(8.))));
    sparse.get_value_into(&mut result);
    sparse.clone_into(&clone);
    let trace = trace.take();
    json!([
        trace,
        snap(&p),
        p.equals(&p.clone()),
        Rc::ptr_eq(&p.clone().start_time().unwrap(), &p.start_time().unwrap()),
        merged.equals(&p),
        snap(&merged),
        result.on_tick.remove_event_listener(tick_listener),
        full,
        core(&result),
        result.should_animate(),
        snap(&clone),
        core(&p.get_value(None))
    ])
}

#[test]
fn replay_materials() {
    run(
        "data_sources",
        "Materials",
        |name| if name == "rotation" { TIGHT } else { EXACT },
        |name, a| {
            Some(Out::Ok(match name {
                "definition" => definition(s(&a[0]), &a[1]),
                "resolve" => resolve(s(&a[0]), &a[1]),
                "descriptor" => descriptor(),
                "stripeOrientation" => json!([StripeOrientation::Horizontal as u8, StripeOrientation::Vertical as u8]),
                "composite" => composite(),
                "nodes" => nodes(&a[0]),
                "reuseColor" => reuse_color(),
                "rotation" => rotation(n(&a[0]), n(&a[1])),
                "clock" => clock(),
                _ => return None,
            }))
        },
    );
}

#[test]
fn descriptor_events_follow_only_the_current_child() {
    macro_rules! check {
        ($owner:expr, $setter:ident, $name:literal, $value:expr) => {{
            let child = Rc::new(ConstantProperty::<DynamicValue>::new(None));
            let shared = PropertyInput::Component(PropertyComponent::wrap(child.clone()));
            let trace = Rc::new(RefCell::new(Vec::new()));
            {
                let owner = $owner;
                let recorded = trace.clone();
                let _guard = owner.definition_changes().add_listener(move |event| {
                    recorded.borrow_mut().push((event.name.clone(), event.child_changed));
                });
                owner.$setter(Some(shared.clone())).unwrap();
                owner.$setter(Some(shared.clone())).unwrap();
                assert_eq!(child.definition_changed().number_of_listeners(), 1);
                child.set_value(Some($value.into()));
                owner.$setter(None).unwrap();
                assert_eq!(child.definition_changed().number_of_listeners(), 0);
                child.set_value(None);
                owner.$setter(Some(shared)).unwrap();
            }
            assert_eq!(child.definition_changed().number_of_listeners(), 0);
            assert_eq!(*trace.borrow(), [false, true, false, false].map(|b| ($name.to_owned(), b)));
        }};
    }
    check!(ImageMaterialProperty::new(), set_transparent, "transparent", true);
    check!(GridMaterialProperty::new(), set_line_offset, "lineOffset", Cartesian2::new(3., 4.));
    check!(StripeMaterialProperty::new(), set_orientation, "orientation", StripeOrientation::Vertical);
    check!(CheckerboardMaterialProperty::new(), set_repeat, "repeat", Cartesian2::new(2., 3.));
    check!(PolylineArrowMaterialProperty::default(), set_color, "color", Color::WHITE);
    check!(PolylineDashMaterialProperty::new(), set_dash_pattern, "dashPattern", 12.);
    check!(PolylineGlowMaterialProperty::new(), set_glow_power, "glowPower", 0.8);
    check!(PolylineOutlineMaterialProperty::new(), set_outline_width, "outlineWidth", 4.);
    check!(NodeTransformationProperty::new(), set_rotation, "rotation", Quaternion::IDENTITY);
}

#[test]
fn resource_images_snapshot_options_and_resolve_to_texture_urls() {
    let original = Rc::new(Resource::new("textures/tile.png").unwrap());
    let graphics = PolylineGraphics::new();
    graphics.set_material(Some(PropertyInput::value(original.clone()))).unwrap();
    let property = graphics.material().unwrap().component().unwrap();
    original.options_mut().url = "changed.png".into();
    let image = || {
        let value = property.get_value(&date(0.), None).unwrap().unwrap().get("image").unwrap();
        let DynamicValue::Resource(value) = value else { panic!("expected Resource") };
        value
    };
    let first = image();
    first.options_mut().url = "mutated.png".into();
    assert_eq!(image().url(), "textures/tile.png");
    let scene = MaterialProperty::get_value(None, Some(&*property), None).unwrap();
    assert_eq!(scene.uniform("image"), Some(&MaterialValue::Sampler2D(MaterialImage::Url("textures/tile.png".into()))));
    let url = url::Url::parse("https://example.org/tiles/a.png").unwrap();
    graphics.set_depth_fail_material(Some(PropertyInput::value(url.clone()))).unwrap();
    assert_eq!(at(&*graphics.depth_fail_material().unwrap().component().unwrap(), 0.)["image"], url.as_str());
}

#[test]
fn model_node_bag_keeps_the_factory_for_later_assignment() {
    let model = ModelGraphics::new();
    model.set_node_transformations(Some(PropertyInput::value(DynamicValue::from_json(json!({"arm":{}}))))).unwrap();
    let copied = model.clone_definition().unwrap();
    let bag = copied.node_transformations().unwrap().component().unwrap();
    let bag = bag.as_any().downcast_ref::<PropertyBag>().unwrap();
    let old = bag.get("arm").unwrap().component().unwrap();
    let (events, _guard) = event_counter(copied.definition_changed());
    bag.set(
        "arm",
        Some(PropertyInput::value(DynamicValue::Record(vec![(
            "scale".into(),
            Some(Cartesian3::new(2., 3., 4.).into()),
        )]))),
    )
    .unwrap();
    let arm = bag.get("arm").unwrap().component().unwrap();
    assert!(arm.as_any().is::<NodeTransformationProperty>());
    assert_eq!(at(&*arm, 0.)["scale"], json!([2., 3., 4.]));
    old.as_any().downcast_ref::<NodeTransformationProperty>().unwrap().set_scale(None).unwrap();
    old.as_any()
        .downcast_ref::<NodeTransformationProperty>()
        .unwrap()
        .set_translation(Some(PropertyInput::value(Cartesian3::ONE)))
        .unwrap();
    assert_eq!(events.get(), 1);
    arm.as_any().downcast_ref::<NodeTransformationProperty>().unwrap().set_scale(None).unwrap();
    assert_eq!(events.get(), 2);
}

#[test]
fn clock_system_step_is_applied_after_multiplier_and_current_time() {
    let settings = DataSourceClock::new();
    settings.set_current_time(Some(Rc::new(date(4.))));
    settings.set_multiplier(Some(5.));
    settings.set_clock_step(Some(ClockStep::SystemClock));
    let mut clock = settings.get_value(None);
    assert_eq!(clock.clock_step(), ClockStep::SystemClock);
    assert_eq!(clock.multiplier(), 1.);
    assert!(clock.should_animate());
    assert_ne!(clock.current_time(), date(4.));
    settings.set_clock_step(None);
    settings.get_value_into(&mut clock);
    assert_eq!(clock.clock_step(), ClockStep::SystemClockMultiplier);
    assert_eq!(clock.multiplier(), 5.);
    assert_eq!(clock.current_time(), date(4.));
}

#[test]
fn callback_receives_the_requested_time_and_existing_uniform() {
    let time = date(11.);
    let callback = CallbackProperty::new(
        Rc::new(move |t, result: Option<&mut Color>| {
            assert_eq!(*t, time);
            let prior = result.expect("existing material color");
            Ok(Some(Color::new(prior.red, prior.green, prior.blue, 0.4)))
        }),
        false,
    );
    let mut old = Material::from_type("Grid");
    old.set_uniform("color", MaterialValue::from_color(Color::new(0.1, 0.2, 0.3, 1.))).unwrap();
    let grid = GridMaterialProperty::new();
    grid.set_color(Some(PropertyInput::Component(PropertyComponent::wrap(Rc::new(callback))))).unwrap();
    let material = MaterialProperty::get_value(Some(&time), Some(&grid), Some(old)).unwrap();
    assert_eq!(material.uniform("color"), Some(&MaterialValue::from_color(Color::new(0.1, 0.2, 0.3, 0.4))));
    let empty = CompositeMaterialProperty::new();
    let mut scratch = DynamicValue::String("retained".into());
    assert!(empty.get_value(None, Some(&mut scratch)).unwrap().is_none());
    assert_eq!(scratch.to_json(), json!("retained"));
    assert_eq!(
        ImageMaterialProperty::new().get_value(None, None).unwrap().unwrap().get("color").unwrap().to_json(),
        json!([1., 1., 1., 1.])
    );
}
