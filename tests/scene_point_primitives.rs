//! CPU reference checks against the pinned CesiumJS point implementation.
use cesium_rs::core::*;
use cesium_rs::scene::{point_primitive::*, point_primitive_collection::*, SplitDirection};
use serde_json::{json, Value};

fn snapshot(p: &PointPrimitive) -> Value {
    let o = p.options();
    let scalar = |s: Option<NearFarScalar>| s.map(|v| [v.near, v.near_value, v.far, v.far_value]);
    json!({"show":o.show,"position":[o.position.x,o.position.y,o.position.z],
        "color":[o.color.red,o.color.green,o.color.blue,o.color.alpha],
        "outlineColor":[o.outline_color.red,o.outline_color.green,o.outline_color.blue,o.outline_color.alpha],
        "outlineWidth":o.outline_width,"pixelSize":o.pixel_size,"scale":scalar(o.scale_by_distance),
        "translucency":scalar(o.translucency_by_distance),
        "distance":o.distance_display_condition.map(|v| [v.near,v.far]),
        "depth":o.disable_depth_test_distance,"id":o.id,"split":o.split_direction.value()})
}

fn compare(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                compare(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, v) in a {
                compare(v, &b[k]);
            }
        }
        _ => assert_eq!(a, b),
    }
}

#[test]
fn reference_defaults_custom_and_membership() {
    let expected: Value = serde_json::from_str(include_str!("data/scene-point-primitives.json")).unwrap();
    let mut c = PointPrimitiveCollection::new();
    let a = c.add(PointPrimitiveOptions::default()).unwrap();
    let b = c
        .add(PointPrimitiveOptions {
            position: Cartesian3::new(131075., -65538., 3.5),
            color: Color::new(0.25, 0.5, 0.75, 0.125),
            outline_color: Color::new(1., 0., 0., 0.5),
            pixel_size: 21.,
            outline_width: 3.,
            scale_by_distance: Some(NearFarScalar::new(10., 2., 100., 4.)),
            translucency_by_distance: Some(NearFarScalar::new(2., 0.25, 20., 0.75)),
            distance_display_condition: Some(DistanceDisplayCondition::new(3., 40.)),
            disable_depth_test_distance: 13.,
            split_direction: SplitDirection::Left,
            id: Some("b".into()),
            ..Default::default()
        })
        .unwrap();
    compare(&snapshot(&a.borrow()), &expected["defaults"]);
    compare(&snapshot(&b.borrow()), &expected["custom"]);
    let bb = b.borrow().screen_space_bounding_box(Cartesian2::new(123., 456.));
    compare(&json!([bb.x, bb.y, bb.width, bb.height]), &expected["bbox"]);
    assert_eq!(json!(c.len()), expected["initialLength"]);
    assert_eq!(json!(c.contains(&b)), expected["containsBefore"]);
    assert_eq!(json!(c.remove(&b)), expected["firstRemoval"]);
    assert_eq!(json!(c.remove(&b)), expected["secondRemoval"]);
    assert_eq!(json!(c.contains(&b)), expected["containsAfter"]);
    assert_eq!(json!(c.len()), expected["lengthAfter"]);
    assert_eq!(json!(*a.borrow() == *c.get(0).unwrap().borrow()), expected["retained"]);
    assert!(!b.borrow().is_attached());
}

#[test]
fn validation_attributes_and_owner_lifetime() {
    let mut c = PointPrimitiveCollection::new();
    let bad =
        PointPrimitiveOptions { scale_by_distance: Some(NearFarScalar::new(5., 1., 5., 2.)), ..Default::default() };
    assert!(c.add(bad).is_err());
    assert!(c.is_empty());
    let p = c
        .add(PointPrimitiveOptions {
            position: Cartesian3::new(131075., -65538., 3.5),
            translucency_by_distance: Some(NearFarScalar::new(2., 0.25, 20., 0.75)),
            distance_display_condition: Some(DistanceDisplayCondition::new(3., 40.)),
            disable_depth_test_distance: f64::INFINITY,
            ..Default::default()
        })
        .unwrap();
    let a = c.attributes()[0];
    assert_eq!(a.position_high_and_size, [131072., -65536., 0., 10.]);
    assert_eq!(a.position_low_and_outline, [3., -2., 3.5, 0.]);
    assert_eq!(a.show_and_translucency, [319., 191., 2., 20.]);
    assert_eq!(a.distance_depth_split, [9., 1600., -1., 0.]);
    p.borrow_mut().set_cluster_show(false);
    assert_eq!(c.attributes()[0].show_and_translucency[0], 63.);
    let before = p.borrow().options().clone();
    let mut invalid = before.clone();
    invalid.disable_depth_test_distance = -1.;
    assert!(p.borrow_mut().set_options(invalid).is_err());
    assert_eq!(p.borrow().options(), &before);
    c.destroy();
    assert!(c.is_destroyed());
    assert!(!p.borrow().is_attached());
    assert!(c.add(PointPrimitiveOptions::default()).is_err());
}
