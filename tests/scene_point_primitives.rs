//! CPU reference checks against the pinned CesiumJS point implementation.
use cesium_rs::core::*;
use cesium_rs::renderer::{Gl, GlValue};
use cesium_rs::scene::{point_primitive::*, point_primitive_collection::*, ScenePrimitive, SplitDirection};
use serde_json::{json, Value};

/// No update runs here, so the host is never called.
struct NoGl;
impl Gl for NoGl {
    type Object = ();
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        unreachable!()
    }
    fn invoke(&self, _: &str, _: &[GlValue<()>]) -> Result<GlValue<()>, RuntimeError> {
        unreachable!()
    }
}

fn collection() -> PointPrimitiveCollection<NoGl> {
    PointPrimitiveCollection::default()
}

fn snapshot(p: &PointPrimitive) -> Value {
    let o = &p.options();
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
    let mut c = collection();
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
    compare(&snapshot(&a), &expected["defaults"]);
    compare(&snapshot(&b), &expected["custom"]);
    let bb = b.screen_space_bounding_box(Cartesian2::new(123., 456.));
    compare(&json!([bb.x, bb.y, bb.width, bb.height]), &expected["bbox"]);
    assert_eq!(json!(c.length()), expected["initialLength"]);
    assert_eq!(json!(c.contains(&b)), expected["containsBefore"]);
    assert_eq!(json!(c.remove(&b).unwrap()), expected["firstRemoval"]);
    assert_eq!(json!(c.remove(&b).unwrap()), expected["secondRemoval"]);
    assert_eq!(json!(c.contains(&b)), expected["containsAfter"]);
    assert_eq!(json!(c.length()), expected["lengthAfter"]);
    assert_eq!(json!(a.equals(&c.get(0).unwrap())), expected["retained"]);
    assert!(!b.is_attached());
}

#[test]
fn validation_dirty_marking_and_owner_lifetime() {
    let mut c = collection();
    let bad =
        PointPrimitiveOptions { scale_by_distance: Some(NearFarScalar::new(5., 1., 5., 2.)), ..Default::default() };
    assert_eq!(c.add(bad).unwrap_err().message, "scaleByDistance.far must be greater than scaleByDistance.near.");
    assert_eq!(c.length(), 0);
    let p = c.add(PointPrimitiveOptions::default()).unwrap();
    let q = c.add(PointPrimitiveOptions::default()).unwrap();
    assert!(p.equals(&q) && !p.ptr_eq(&q), "equals compares properties");

    // Setters mark a point dirty once and count every changed property;
    // setting an equal value changes nothing.
    p.set_color(Color::WHITE);
    assert_eq!(c.points_to_update(), 0);
    p.set_cluster_show(false);
    p.set_pixel_size(4.);
    q.set_split_direction(SplitDirection::Right);
    assert!(p.is_dirty() && q.is_dirty());
    assert_eq!(c.points_to_update(), 2);
    let mut changed = [0; NUMBER_OF_PROPERTIES];
    (changed[SHOW_INDEX], changed[PIXEL_SIZE_INDEX], changed[SPLIT_DIRECTION_INDEX]) = (1, 1, 1);
    assert_eq!(c.properties_changed(), changed);

    // Range checks leave the point unchanged.
    assert_eq!(
        p.set_disable_depth_test_distance(-1.).unwrap_err().message,
        "disableDepthTestDistance must be greater than or equal to 0.0."
    );
    assert_eq!(
        p.set_translucency_by_distance(Some(NearFarScalar::new(2., 1., 1., 0.))).unwrap_err().message,
        "far distance must be greater than near distance."
    );
    assert_eq!(
        p.set_distance_display_condition(Some(DistanceDisplayCondition::new(5., 5.))).unwrap_err().message,
        "far must be greater than near"
    );
    assert_eq!(p.options().disable_depth_test_distance, 0.);

    // Setting the position also sets the actual position.
    p.set_position(Cartesian3::new(1., 2., 3.));
    assert_eq!(p.actual_position(), Cartesian3::new(1., 2., 3.));

    c.destroy().unwrap();
    assert!(c.is_destroyed());
    assert!(!p.is_attached() && !q.is_attached());
    assert!(c.add(PointPrimitiveOptions::default()).is_err());
    // A detached point no longer queues updates.
    q.set_show(false);
    assert!(!q.show());
}
