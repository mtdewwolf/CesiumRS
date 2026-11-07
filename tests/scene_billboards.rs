//! Billboard properties, screen-space bounds, equality, membership and
//! validation against pinned CesiumJS snapshots
//! (`tests/data/scene-billboards.json`). The update pipeline is checked
//! against a SwiftShader capture in `tests/scene_billboard_collection.rs`.
use cesium_rs::core::*;
use cesium_rs::renderer::{Gl, GlValue};
use cesium_rs::scene::billboard::*;
use cesium_rs::scene::billboard_collection::*;
use cesium_rs::scene::{HeightReference, HorizontalOrigin, ScenePrimitive, SplitDirection, VerticalOrigin};
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

type B = Billboard<NoGl>;

fn test_billboard(options: BillboardOptions, _index: usize) -> Result<B, RuntimeError> {
    BillboardCollection::<NoGl>::default().add(options, None)
}

fn snapshot(b: &B) -> Value {
    let o = b.options();
    let scalar = |s: Option<NearFarScalar>| s.map(|v| [v.near, v.near_value, v.far, v.far_value]);
    json!({
        "show": o.show,
        "position": [o.position.x, o.position.y, o.position.z],
        "pixelOffset": [o.pixel_offset.x, o.pixel_offset.y],
        "eyeOffset": [o.eye_offset.x, o.eye_offset.y, o.eye_offset.z],
        "horizontalOrigin": o.horizontal_origin as i32,
        "verticalOrigin": o.vertical_origin as i32,
        "heightReference": o.height_reference as i32,
        "scale": o.scale,
        "color": [o.color.red, o.color.green, o.color.blue, o.color.alpha],
        "rotation": o.rotation,
        "alignedAxis": [o.aligned_axis.x, o.aligned_axis.y, o.aligned_axis.z],
        "width": o.width,
        "height": o.height,
        "scaleByDistance": scalar(o.scale_by_distance),
        "translucencyByDistance": scalar(o.translucency_by_distance),
        "pixelOffsetScaleByDistance": scalar(o.pixel_offset_scale_by_distance),
        "sizeInMeters": o.size_in_meters,
        "distanceDisplayCondition": o.distance_display_condition.map(|v| [v.near, v.far]),
        "disableDepthTestDistance": o.disable_depth_test_distance,
        "id": o.id,
        "image": b.image(),
        "ready": b.ready(),
        "clusterShow": b.cluster_show(),
        "outlineColor": [o.outline_color.red, o.outline_color.green, o.outline_color.blue, o.outline_color.alpha],
        "outlineWidth": o.outline_width,
        "splitDirection": o.split_direction.value(),
    })
}

fn compare(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let fa = a.as_f64().unwrap();
            let fb = b.as_f64().unwrap();
            assert!((fa - fb).abs() < 1e-6 || fa == fb, "Numbers mismatch: {fa} vs {fb}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "Array length mismatch: {a:?} vs {b:?}");
            for (ea, eb) in a.iter().zip(b) {
                compare(ea, eb);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "Object key counts differ");
            for (k, v) in a {
                assert!(b.get(k).is_some(), "Key {k} missing in expected object");
                compare(v, &b[k]);
            }
        }
        _ => assert_eq!(a, b),
    }
}

#[test]
fn reference_defaults_custom_and_membership() {
    let expected: Value = serde_json::from_str(include_str!("data/scene-billboards.json")).unwrap();

    let mut collection = BillboardCollection::<NoGl>::default();
    let def = collection.add(BillboardOptions::default(), None).unwrap();
    let custom = collection
        .add(
            BillboardOptions {
                position: Cartesian3::new(131075.0, -65538.0, 3.5),
                show: true,
                scale: 2.5,
                pixel_offset: Cartesian2::new(12.0, -24.0),
                eye_offset: Cartesian3::new(5.0, 10.0, -50.0),
                horizontal_origin: HorizontalOrigin::Right,
                vertical_origin: VerticalOrigin::Bottom,
                height_reference: HeightReference::None,
                color: Color::new(0.25, 0.5, 0.75, 0.8),
                rotation: std::f64::consts::FRAC_PI_4,
                aligned_axis: Cartesian3::new(0.0, 1.0, 0.0),
                width: Some(64.0),
                height: Some(32.0),
                scale_by_distance: Some(NearFarScalar::new(100.0, 2.0, 1000.0, 0.5)),
                translucency_by_distance: Some(NearFarScalar::new(50.0, 1.0, 500.0, 0.2)),
                pixel_offset_scale_by_distance: Some(NearFarScalar::new(20.0, 1.5, 200.0, 0.5)),
                size_in_meters: true,
                distance_display_condition: Some(DistanceDisplayCondition::new(15.0, 2500.0)),
                disable_depth_test_distance: Some(100.0),
                split_direction: SplitDirection::Left,
                id: Some("test-billboard-custom".into()),
                outline_color: Color::new(1.0, 0.0, 0.0, 1.0),
                outline_width: 2.0,
                ..Default::default()
            },
            None,
        )
        .unwrap();

    // Verify snapshot against CesiumJS reference
    compare(&snapshot(&def), &expected["defaults"]);
    compare(&snapshot(&custom), &expected["custom"]);

    // Bounding box cases
    let bbox_cases = expected["bboxCases"].as_array().unwrap();
    for case in bbox_cases {
        let name = case["name"].as_str().unwrap();
        let expected_rect = &case["expected"];
        let center = Cartesian2::new(case["center"][0].as_f64().unwrap(), case["center"][1].as_f64().unwrap());

        let rect = match name {
            "custom_right_bottom" => custom.screen_space_bounding_box(center),
            "center_center" => {
                let b = test_billboard(
                    BillboardOptions {
                        width: Some(100.0),
                        height: Some(50.0),
                        scale: 2.0,
                        horizontal_origin: HorizontalOrigin::Center,
                        vertical_origin: VerticalOrigin::Center,
                        ..Default::default()
                    },
                    0,
                )
                .unwrap();
                b.screen_space_bounding_box(center)
            }
            "left_top" => {
                let b = test_billboard(
                    BillboardOptions {
                        width: Some(100.0),
                        height: Some(50.0),
                        scale: 2.0,
                        horizontal_origin: HorizontalOrigin::Left,
                        vertical_origin: VerticalOrigin::Top,
                        ..Default::default()
                    },
                    0,
                )
                .unwrap();
                b.screen_space_bounding_box(center)
            }
            "left_baseline" => {
                let b = test_billboard(
                    BillboardOptions {
                        width: Some(100.0),
                        height: Some(50.0),
                        scale: 2.0,
                        horizontal_origin: HorizontalOrigin::Left,
                        vertical_origin: VerticalOrigin::Baseline,
                        ..Default::default()
                    },
                    0,
                )
                .unwrap();
                b.screen_space_bounding_box(center)
            }
            _ => panic!("unknown test case {name}"),
        };

        compare(&json!([rect.x, rect.y, rect.width, rect.height]), expected_rect);
    }

    // Equality tests
    assert_eq!(def.equals(&collection.get(0).unwrap()), expected["defEqualsDef"].as_bool().unwrap());
    assert_eq!(def.equals(&custom), expected["defEqualsCustom"].as_bool().unwrap());

    // Collection membership and removal
    assert_eq!(collection.length(), expected["initialLength"].as_u64().unwrap() as usize);
    assert_eq!(collection.contains(&custom), expected["containsBefore"].as_bool().unwrap());
    assert_eq!(collection.remove(&custom).unwrap(), expected["firstRemoval"].as_bool().unwrap());
    assert_eq!(collection.remove(&custom).unwrap(), expected["secondRemoval"].as_bool().unwrap());
    assert_eq!(collection.contains(&custom), expected["containsAfter"].as_bool().unwrap());
    assert_eq!(collection.length(), expected["lengthAfter"].as_u64().unwrap() as usize);
    assert_eq!(def.equals(&collection.get(0).unwrap()), expected["retained"].as_bool().unwrap());
    assert!(!custom.is_attached());
}

#[test]
fn validation_dirty_marking_and_lifecycle() {
    let mut c = BillboardCollection::<NoGl>::default();
    let bad = BillboardOptions { scale_by_distance: Some(NearFarScalar::new(5., 1., 5., 2.)), ..Default::default() };
    assert_eq!(c.add(bad, None).unwrap_err().message, "scaleByDistance.far must be greater than scaleByDistance.near.");
    let clamped = BillboardOptions { height_reference: HeightReference::ClampToGround, ..Default::default() };
    assert_eq!(c.add(clamped, None).unwrap_err().message, "Height reference is not supported without a scene.");
    assert_eq!(c.length(), 0);

    let a = c.add(BillboardOptions::default(), None).unwrap();
    let b = c.add(BillboardOptions::default(), None).unwrap();
    assert!(a.equals(&b) && !a.ptr_eq(&b));
    a.set_color(Color::WHITE);
    assert_eq!(c.billboards_to_update(), 0, "an equal value changes nothing");
    a.set_scale(2.);
    a.set_rotation(1.);
    b.set_size_in_meters(true);
    b.set_outline_width(1.);
    assert_eq!(c.billboards_to_update(), 2, "each billboard is queued once");
    let mut changed = [0; NUMBER_OF_PROPERTIES];
    (changed[SCALE_INDEX], changed[ROTATION_INDEX], changed[COLOR_INDEX], changed[SDF_INDEX]) = (1, 1, 1, 1);
    assert_eq!(c.properties_changed(), changed);

    assert_eq!(
        a.set_disable_depth_test_distance(Some(-1.)).unwrap_err().message,
        "disableDepthTestDistance must be greater than or equal to 0.0."
    );
    assert_eq!(
        a.set_pixel_offset_scale_by_distance(Some(NearFarScalar::new(2., 1., 1., 0.))).unwrap_err().message,
        "far distance must be greater than near distance."
    );
    assert!(a.set_height_reference(HeightReference::RelativeToGround).is_err());
    a.set_position(Cartesian3::new(1., 2., 3.));
    assert_eq!(a.actual_position(), Cartesian3::new(1., 2., 3.));
    assert!(a.set_image_sub_region("missing", BoundingRectangle::new(0., 0., 1., 1.)).is_err());

    c.destroy().unwrap();
    assert!(c.is_destroyed());
    assert!(!a.is_attached() && !b.is_attached());
    assert!(c.add(BillboardOptions::default(), None).is_err());
}
