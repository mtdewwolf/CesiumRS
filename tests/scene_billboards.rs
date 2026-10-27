//! CPU reference checks and property validation for Scene Billboard and BillboardCollection.
pub use cesium_rs::{core, scene};
use cesium_rs::core::*;
use cesium_rs::scene::{HeightReference, HorizontalOrigin, SplitDirection, VerticalOrigin};
use serde_json::{json, Value};

use cesium_rs::scene::{billboard, billboard_collection};

use billboard::*;
use billboard_collection::*;

fn test_billboard(options: BillboardOptions, _index: usize) -> Result<BillboardHandle, &'static str> {
    BillboardCollection::new().add(options)
}

fn snapshot(b: &Billboard) -> Value {
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
        "image": o.image,
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
            assert!(
                (fa - fb).abs() < 1e-6 || fa == fb,
                "Numbers mismatch: {fa} vs {fb}"
            );
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
    let expected: Value =
        serde_json::from_str(include_str!("data/scene-billboards.json")).unwrap();

    let mut collection = BillboardCollection::new();
    let def = collection.add(BillboardOptions::default()).unwrap();
    let custom = collection
        .add(BillboardOptions {
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
        })
        .unwrap();

    // Verify snapshot against CesiumJS reference
    compare(&snapshot(&def.borrow()), &expected["defaults"]);
    compare(&snapshot(&custom.borrow()), &expected["custom"]);

    // Bounding box cases
    let bbox_cases = expected["bboxCases"].as_array().unwrap();
    for case in bbox_cases {
        let name = case["name"].as_str().unwrap();
        let expected_rect = &case["expected"];
        let center = Cartesian2::new(
            case["center"][0].as_f64().unwrap(),
            case["center"][1].as_f64().unwrap(),
        );

        let rect = match name {
            "custom_right_bottom" => custom.borrow().screen_space_bounding_box(center),
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
                let bounds = b.borrow().screen_space_bounding_box(center);
                bounds
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
                let bounds = b.borrow().screen_space_bounding_box(center);
                bounds
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
                let bounds = b.borrow().screen_space_bounding_box(center);
                bounds
            }
            _ => panic!("unknown test case {name}"),
        };

        compare(
            &json!([rect.x, rect.y, rect.width, rect.height]),
            expected_rect,
        );
    }

    // Equality tests
    assert_eq!(
        def.borrow().equals(&collection.get(0).unwrap().borrow()),
        expected["defEqualsDef"].as_bool().unwrap()
    );
    assert_eq!(
        def.borrow().equals(&custom.borrow()),
        expected["defEqualsCustom"].as_bool().unwrap()
    );

    // Collection membership and removal
    assert_eq!(collection.len(), expected["initialLength"].as_u64().unwrap() as usize);
    assert_eq!(collection.contains(&custom), expected["containsBefore"].as_bool().unwrap());
    assert_eq!(collection.remove(&custom), expected["firstRemoval"].as_bool().unwrap());
    assert_eq!(collection.remove(&custom), expected["secondRemoval"].as_bool().unwrap());
    assert_eq!(collection.contains(&custom), expected["containsAfter"].as_bool().unwrap());
    assert_eq!(collection.len(), expected["lengthAfter"].as_u64().unwrap() as usize);
    assert_eq!(
        def.borrow().equals(&collection.get(0).unwrap().borrow()),
        expected["retained"].as_bool().unwrap()
    );
    assert!(!custom.borrow().is_attached());
}

#[test]
fn bounding_spheres_and_vertex_attributes() {
    let expected: Value =
        serde_json::from_str(include_str!("data/scene-billboards.json")).unwrap();

    let mut collection = BillboardCollection::new();
    collection
        .add(BillboardOptions {
            position: Cartesian3::new(131075.0, -65538.0, 3.5),
            ..Default::default()
        })
        .unwrap();
    collection
        .add(BillboardOptions {
            position: Cartesian3::new(10.0, 20.0, 30.0),
            ..Default::default()
        })
        .unwrap();

    let sphere = collection.base_volume();
    compare(
        &json!({"center": [sphere.center.x, sphere.center.y, sphere.center.z], "radius": sphere.radius}),
        &expected["boundingSphere"],
    );

    collection.model_matrix = Matrix4::from_translation(Cartesian3::new(25.0, 50.0, 75.0));
    let transformed = collection.base_volume_wc();
    compare(
        &json!({"center": [transformed.center.x, transformed.center.y, transformed.center.z], "radius": transformed.radius}),
        &expected["transformedBoundingSphere"],
    );

    // Verify encoded vertex attributes
    let custom = test_billboard(
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
        0,
    )
    .unwrap();

    let attrs = BillboardVertexAttributes::from_billboard(&custom.borrow(), None);
    let expected_attrs = &expected["encodedAttributes"];

    compare(&json!(attrs.position_high_and_scale), &expected_attrs["positionHighAndScale"]);
    compare(&json!(attrs.position_low_and_rotation), &expected_attrs["positionLowAndRotation"]);
    compare(&json!(attrs.compressed_attribute0), &expected_attrs["compressedAttribute0"]);
    compare(&json!(attrs.compressed_attribute1), &expected_attrs["compressedAttribute1"]);
    compare(&json!(attrs.compressed_attribute2), &expected_attrs["compressedAttribute2"]);
    compare(&json!(attrs.eye_offset), &expected_attrs["eyeOffset"]);
    compare(&json!(attrs.scale_by_distance), &expected_attrs["scaleByDistance"]);
    compare(&json!(attrs.pixel_offset_scale_by_distance), &expected_attrs["pixelOffsetScaleByDistance"]);
    compare(&json!(attrs.compressed_attribute3), &expected_attrs["compressedAttribute3"]);
    assert_eq!(attrs.split_direction, expected_attrs["splitDirection"].as_f64().unwrap());
}

#[test]
fn validation_mutation_and_lifecycle() {
    let mut collection = BillboardCollection::new();

    // Range checks
    let bad_depth = BillboardOptions {
        disable_depth_test_distance: Some(-1.0),
        ..Default::default()
    };
    assert!(collection.add(bad_depth).is_err());

    let bad_scale = BillboardOptions {
        scale_by_distance: Some(NearFarScalar::new(100.0, 1.0, 50.0, 2.0)),
        ..Default::default()
    };
    assert!(collection.add(bad_scale).is_err());

    let bad_translucency = BillboardOptions {
        translucency_by_distance: Some(NearFarScalar::new(200.0, 1.0, 100.0, 0.5)),
        ..Default::default()
    };
    assert!(collection.add(bad_translucency).is_err());

    let bad_pixel_offset_scale = BillboardOptions {
        pixel_offset_scale_by_distance: Some(NearFarScalar::new(300.0, 1.0, 300.0, 2.0)),
        ..Default::default()
    };
    assert!(collection.add(bad_pixel_offset_scale).is_err());

    let bad_ddc = BillboardOptions {
        distance_display_condition: Some(DistanceDisplayCondition::new(500.0, 400.0)),
        ..Default::default()
    };
    assert!(collection.add(bad_ddc).is_err());
    assert!(collection.is_empty());

    // Valid add and mutation
    let b = collection
        .add(BillboardOptions {
            position: Cartesian3::new(10.0, 20.0, 30.0),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(b.borrow().position(), Cartesian3::new(10.0, 20.0, 30.0));
    assert_eq!(b.borrow().actual_position(), Cartesian3::new(10.0, 20.0, 30.0));

    // Clamped position override
    b.borrow_mut().set_clamped_position(Some(Cartesian3::new(11.0, 21.0, 31.0)));
    assert_eq!(b.borrow().actual_position(), Cartesian3::new(11.0, 21.0, 31.0));
    b.borrow_mut().set_clamped_position(None);
    assert_eq!(b.borrow().actual_position(), Cartesian3::new(10.0, 20.0, 30.0));

    // Property change tracking
    assert!(b.borrow().is_dirty());
    let _ = collection.attributes();
    assert!(!b.borrow().is_dirty());

    b.borrow_mut().set_scale(3.0);
    assert!(b.borrow().is_dirty());
    assert!(b.borrow().is_property_changed(SCALE_INDEX));
    assert_eq!(b.borrow().scale(), 3.0);

    // Destroy collection
    collection.destroy();
    assert!(collection.is_destroyed());
    assert!(!b.borrow().is_attached());
    assert!(collection.add(BillboardOptions::default()).is_err());
}

#[test]
fn billboard_position_mutation_refreshes_collection_bounds() {
    let mut collection = BillboardCollection::new();
    let billboard = collection.add(BillboardOptions::default()).unwrap();
    assert_eq!(collection.base_volume().center, Cartesian3::ZERO);
    billboard.borrow_mut().set_position(Cartesian3::new(1.0, 2.0, 3.0));
    // Reading attributes may clear the billboard's dirty flags first.
    collection.attributes();
    assert_eq!(collection.base_volume().center, Cartesian3::new(1.0, 2.0, 3.0));
}

#[test]
fn attribute_alpha_and_fractional_height_follow_cesium_byte_packing() {
    let mut collection = BillboardCollection::new();
    let b = collection.add(BillboardOptions {
        color: Color::new(1.0, 1.0, 1.0, 0.5),
        height: Some(31.5),
        ..Default::default()
    }).unwrap();
    let attrs = BillboardVertexAttributes::from_billboard(&b.borrow(), Some(Color::new(1.0, 1.0, 1.0, 0.25)));
    let packed = attrs.compressed_attribute2[2] as u32;
    assert_eq!((packed >> 16) & 255, 128);
    assert_eq!((packed >> 8) & 255, 64);
    assert_eq!(attrs.compressed_attribute2[3], 126.0);
}

#[test]
fn mutations_invalidate_clamp_and_retain_pending_image_upload() {
    let mut collection = BillboardCollection::new();
    let b = collection.add(BillboardOptions::default()).unwrap();
    b.borrow_mut().set_clamped_position(Some(Cartesian3::new(9.0, 9.0, 9.0)));
    b.borrow_mut().set_position(Cartesian3::new(1.0, 2.0, 3.0));
    assert_eq!(b.borrow().actual_position(), Cartesian3::new(1.0, 2.0, 3.0));
    b.borrow_mut().set_clamped_position(Some(Cartesian3::new(9.0, 9.0, 9.0)));
    b.borrow_mut().set_height_reference(HeightReference::ClampToGround);
    assert_eq!(b.borrow().clamped_position(), None);
    b.borrow_mut().set_load_error(Some("old".into()));
    let mut opts = b.borrow().options().clone();
    opts.image = Some("new.png".into());
    b.borrow_mut().set_options(opts).unwrap();
    assert_eq!(b.borrow().load_error(), None);
    assert!(b.borrow().is_texture_dirty());
    collection.attributes();
    assert!(b.borrow().is_texture_dirty());
}

#[test]
fn dropping_collection_detaches_unborrowed_billboard() {
    let mut collection = BillboardCollection::new();
    let billboard = collection.add(BillboardOptions::default()).unwrap();
    assert!(billboard.borrow().is_attached());
    drop(collection);
    assert!(!billboard.borrow().is_attached());
}

#[test]
fn initial_image_and_setters_keep_loading_state_consistent() {
    let mut collection = BillboardCollection::new();
    let billboard = collection.add(BillboardOptions { image: Some("first.png".into()), ..Default::default() }).unwrap();
    assert_eq!(billboard.borrow().load_state(), scene::BillboardLoadState::Loading);
    assert!(billboard.borrow().is_texture_dirty());
    billboard.borrow_mut().set_image(None);
    assert_eq!(billboard.borrow().load_state(), scene::BillboardLoadState::None);
    billboard.borrow_mut().set_load_error(Some("old".into()));
    billboard.borrow_mut().set_image(Some("second.png".into()));
    assert_eq!(billboard.borrow().load_state(), scene::BillboardLoadState::Loading);
    assert_eq!(billboard.borrow().load_error(), None);
}
