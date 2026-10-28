//! Targeted CPU reference checks and property validation for CesiumJS `Label` and `LabelCollection`.

use cesium_rs::scene::{label, label_collection};

use cesium_rs::core::*;
use cesium_rs::scene::{HeightReference, HorizontalOrigin, LabelStyle, VerticalOrigin};
use label::*;
use label_collection::*;
use serde_json::Value;

fn load_fixture() -> Value {
    let content = std::fs::read_to_string("tests/data/scene-labels.json")
        .expect("Failed to read tests/data/scene-labels.json");
    serde_json::from_str(&content).expect("Failed to parse tests/data/scene-labels.json")
}

fn assert_approx_eq(a: f64, b: f64, eps: f64) {
    assert!(
        (a - b).abs() <= eps || (a.is_nan() && b.is_nan()),
        "Expected {a} approx equal to {b} within {eps}"
    );
}

#[test]
fn reference_defaults_and_custom_options() {
    let fixture = load_fixture();
    let defaults = &fixture["defaults"];

    let default_options = LabelOptions::default();
    assert_eq!(default_options.show, defaults["show"].as_bool().unwrap());
    assert_eq!(default_options.text, defaults["text"].as_str().unwrap());
    assert_eq!(default_options.font, defaults["font"].as_str().unwrap());
    assert_eq!(default_options.style as i32, defaults["style"].as_i64().unwrap() as i32);
    assert_eq!(default_options.scale, defaults["scale"].as_f64().unwrap());
    assert_eq!(
        default_options.show_background,
        defaults["showBackground"].as_bool().unwrap()
    );
    assert_eq!(
        default_options.horizontal_origin as i32,
        defaults["horizontalOrigin"].as_i64().unwrap() as i32
    );
    assert_eq!(
        default_options.vertical_origin as i32,
        defaults["verticalOrigin"].as_i64().unwrap() as i32
    );
    assert_eq!(
        default_options.height_reference as i32,
        defaults["heightReference"].as_i64().unwrap() as i32
    );
    assert_eq!(default_options.outline_width, defaults["outlineWidth"].as_f64().unwrap());

    let bg_color = &defaults["backgroundColor"];
    assert_approx_eq(default_options.background_color.red, bg_color[0].as_f64().unwrap(), 1e-4);
    assert_approx_eq(default_options.background_color.green, bg_color[1].as_f64().unwrap(), 1e-4);
    assert_approx_eq(default_options.background_color.blue, bg_color[2].as_f64().unwrap(), 1e-4);
    assert_approx_eq(default_options.background_color.alpha, bg_color[3].as_f64().unwrap(), 1e-4);

    let bg_pad = &defaults["backgroundPadding"];
    assert_eq!(default_options.background_padding.x, bg_pad[0].as_f64().unwrap());
    assert_eq!(default_options.background_padding.y, bg_pad[1].as_f64().unwrap());

    // Test custom options from fixture
    let custom = &fixture["custom"];
    let custom_options = LabelOptions {
        position: Cartesian3::new(131075.0, -65538.0, 3.5),
        text: "CesiumRS Label Parity".to_string(),
        font: "24px Helvetica".to_string(),
        style: LabelStyle::FillAndOutline,
        scale: 1.5,
        show: true,
        show_background: true,
        background_color: Color::new(0.2, 0.3, 0.4, 0.85),
        background_padding: Cartesian2::new(10.0, 8.0),
        pixel_offset: Cartesian2::new(15.0, -20.0),
        eye_offset: Cartesian3::new(2.0, 4.0, -10.0),
        horizontal_origin: HorizontalOrigin::Right,
        vertical_origin: VerticalOrigin::Bottom,
        height_reference: HeightReference::None,
        fill_color: Color::new(0.9, 0.8, 0.7, 1.0),
        outline_color: Color::new(0.1, 0.1, 0.1, 1.0),
        outline_width: 2.5,
        scale_by_distance: Some(NearFarScalar::new(100.0, 1.8, 1000.0, 0.4)),
        translucency_by_distance: Some(NearFarScalar::new(50.0, 1.0, 500.0, 0.1)),
        pixel_offset_scale_by_distance: Some(NearFarScalar::new(20.0, 1.2, 200.0, 0.6)),
        distance_display_condition: Some(DistanceDisplayCondition::new(10.0, 2000.0)),
        disable_depth_test_distance: Some(150.0),
        id: Some("custom-label-id".to_string()),
    };

    assert_eq!(custom_options.text, custom["text"].as_str().unwrap());
    assert_eq!(custom_options.scale, custom["scale"].as_f64().unwrap());
    assert_eq!(custom_options.id.as_deref(), custom["id"].as_str());
    assert_eq!(custom_options.outline_width, custom["outlineWidth"].as_f64().unwrap());
}

#[test]
fn validation_rules_matching_cesiumjs() {
    let mut options = LabelOptions::default();
    assert!(options.validate().is_ok());

    // disableDepthTestDistance < 0
    options.disable_depth_test_distance = Some(-1.0);
    assert!(options.validate().is_err());
    options.disable_depth_test_distance = Some(0.0);
    assert!(options.validate().is_ok());

    // translucencyByDistance far <= near
    options.translucency_by_distance = Some(NearFarScalar::new(100.0, 1.0, 50.0, 0.0));
    assert!(options.validate().is_err());
    options.translucency_by_distance = Some(NearFarScalar::new(100.0, 1.0, 100.0, 0.0));
    assert!(options.validate().is_err());
    options.translucency_by_distance = Some(NearFarScalar::new(50.0, 1.0, 100.0, 0.0));
    assert!(options.validate().is_ok());

    // scaleByDistance far <= near
    options.scale_by_distance = Some(NearFarScalar::new(500.0, 1.0, 100.0, 0.5));
    assert!(options.validate().is_err());
    options.scale_by_distance = Some(NearFarScalar::new(100.0, 1.0, 500.0, 0.5));
    assert!(options.validate().is_ok());

    // distanceDisplayCondition far <= near
    options.distance_display_condition = Some(DistanceDisplayCondition::new(500.0, 100.0));
    assert!(options.validate().is_err());
    options.distance_display_condition = Some(DistanceDisplayCondition::new(100.0, 500.0));
    assert!(options.validate().is_ok());

    // Test setters on active label
    let mut collection = LabelCollection::new();
    let handle = collection.add(LabelOptions::default()).unwrap();
    let mut label = handle.borrow_mut();

    assert!(label.set_disable_depth_test_distance(Some(-5.0)).is_err());
    assert!(label.set_disable_depth_test_distance(Some(10.0)).is_ok());

    assert!(label
        .set_translucency_by_distance(Some(NearFarScalar::new(200.0, 1.0, 100.0, 0.0)))
        .is_err());
    assert!(label
        .set_translucency_by_distance(Some(NearFarScalar::new(100.0, 1.0, 200.0, 0.0)))
        .is_ok());
}

#[test]
fn unsupported_characters_filtering() {
    let fixture = load_fixture();
    let filter_cases = fixture["filterCases"].as_array().unwrap();

    for case in filter_cases {
        let input = case["input"].as_str().unwrap();
        let expected = case["expected"].as_str().unwrap();
        let filtered = filter_unsupported_characters(input);
        assert_eq!(filtered, expected, "Mismatch filtering: {input}");
    }
}

#[test]
fn right_to_left_detection_and_reversal() {
    let fixture = load_fixture();
    let rtl_cases = fixture["rtlCases"].as_array().unwrap();

    for case in rtl_cases {
        let input = case["input"].as_str().unwrap();
        let expected = case["expectedRendered"].as_str().unwrap();
        let reversed = reverse_rtl(input);
        assert_eq!(reversed, expected, "Mismatch reversing RTL: {input}");
    }
}

#[test]
fn font_parsing_and_scale() {
    let parsed1 = ParsedFont::parse("30px sans-serif");
    assert_eq!(parsed1.size, 30.0);
    assert_eq!(parsed1.family, "sans-serif");
    assert_eq!(parsed1.style, "normal");
    assert_eq!(parsed1.weight, "normal");
    assert_eq!(parsed1.line_height, None);

    let parsed2 = ParsedFont::parse("italic bold 24px Helvetica");
    assert_eq!(parsed2.size, 24.0);
    assert_eq!(parsed2.family, "Helvetica");
    assert_eq!(parsed2.style, "italic");
    assert_eq!(parsed2.weight, "bold");

    let parsed3 = ParsedFont::parse("16px/1.5 Arial");
    assert_eq!(parsed3.size, 16.0);
    assert_eq!(parsed3.family, "Arial");
    assert_eq!(parsed3.line_height, Some(24.0)); // 1.5 * 16.0

    // Test relative size & total scale
    let mut collection = LabelCollection::new();
    let handle = collection
        .add(LabelOptions {
            font: "24px Helvetica".to_string(),
            scale: 2.0,
            ..Default::default()
        })
        .unwrap();

    let label = handle.borrow();
    assert_approx_eq(label.relative_size(), 24.0 / 48.0, 1e-6);
    assert_approx_eq(label.total_scale(), 2.0 * (24.0 / 48.0), 1e-6);
}

#[test]
fn equality_semantics() {
    let fixture = load_fixture();
    let eq_data = &fixture["equality"];

    let mut collection = LabelCollection::new();
    let l1 = collection.add(LabelOptions::default()).unwrap();
    let l2 = collection.add(LabelOptions::default()).unwrap();

    assert_eq!(l1.borrow().equals(&l2.borrow()), eq_data["defEqualsDef"].as_bool().unwrap());

    let custom_options = LabelOptions {
        position: Cartesian3::new(131075.0, -65538.0, 3.5),
        text: "CesiumRS Label Parity".to_string(),
        font: "24px Helvetica".to_string(),
        style: LabelStyle::FillAndOutline,
        scale: 1.5,
        show: true,
        show_background: true,
        background_color: Color::new(0.2, 0.3, 0.4, 0.85),
        background_padding: Cartesian2::new(10.0, 8.0),
        pixel_offset: Cartesian2::new(15.0, -20.0),
        eye_offset: Cartesian3::new(2.0, 4.0, -10.0),
        horizontal_origin: HorizontalOrigin::Right,
        vertical_origin: VerticalOrigin::Bottom,
        height_reference: HeightReference::None,
        fill_color: Color::new(0.9, 0.8, 0.7, 1.0),
        outline_color: Color::new(0.1, 0.1, 0.1, 1.0),
        outline_width: 2.5,
        scale_by_distance: Some(NearFarScalar::new(100.0, 1.8, 1000.0, 0.4)),
        translucency_by_distance: Some(NearFarScalar::new(50.0, 1.0, 500.0, 0.1)),
        pixel_offset_scale_by_distance: Some(NearFarScalar::new(20.0, 1.2, 200.0, 0.6)),
        distance_display_condition: Some(DistanceDisplayCondition::new(10.0, 2000.0)),
        disable_depth_test_distance: Some(150.0),
        id: Some("custom-label-id".to_string()),
    };

    let custom = collection.add(custom_options.clone()).unwrap();
    let clone_custom = collection.add(custom_options).unwrap();

    assert_eq!(
        l1.borrow().equals(&custom.borrow()),
        eq_data["defEqualsCustom"].as_bool().unwrap()
    );
    assert_eq!(
        custom.borrow().equals(&clone_custom.borrow()),
        eq_data["customEqualsClone"].as_bool().unwrap()
    );

    // Modifying one property breaks equality
    clone_custom.borrow_mut().set_text("Changed".to_string());
    assert!(!custom.borrow().equals(&clone_custom.borrow()));
}

#[test]
fn collection_lifecycle_and_compaction() {
    let mut collection = LabelCollection::new();
    assert!(collection.is_empty());
    assert_eq!(collection.len(), 0);

    let l1 = collection.add(LabelOptions::default()).unwrap();
    let l2 = collection.add(LabelOptions::default()).unwrap();
    let l3 = collection.add(LabelOptions::default()).unwrap();

    assert_eq!(collection.len(), 3);
    assert!(collection.contains(&l2));
    assert_eq!(l1.borrow().collection_index(), 0);
    assert_eq!(l2.borrow().collection_index(), 1);
    assert_eq!(l3.borrow().collection_index(), 2);

    // Remove middle element
    assert!(collection.remove(&l2));
    assert_eq!(collection.len(), 2);
    assert!(!collection.contains(&l2));
    assert!(!l2.borrow().is_attached());

    // Compaction verifies l3 index updated to 1
    assert_eq!(l3.borrow().collection_index(), 1);

    // Second removal returns false
    assert!(!collection.remove(&l2));

    // Remove all
    collection.remove_all();
    assert!(collection.is_empty());
    assert_eq!(collection.len(), 0);
    assert!(!l1.borrow().is_attached());
    assert!(!l3.borrow().is_attached());
}

#[test]
fn bounding_spheres_and_wc_transform() {
    let fixture = load_fixture();
    let expected_bs = &fixture["boundingSphere"];
    let expected_tbs = &fixture["transformedBoundingSphere"];

    let mut collection = LabelCollection::new();
    collection
        .add(LabelOptions {
            position: Cartesian3::new(131075.0, -65538.0, 3.5),
            ..Default::default()
        })
        .unwrap();
    collection
        .add(LabelOptions {
            position: Cartesian3::new(10.0, 20.0, 30.0),
            ..Default::default()
        })
        .unwrap();

    let sphere = collection.base_volume();
    assert_approx_eq(sphere.center.x, expected_bs["center"][0].as_f64().unwrap(), 1e-4);
    assert_approx_eq(sphere.center.y, expected_bs["center"][1].as_f64().unwrap(), 1e-4);
    assert_approx_eq(sphere.center.z, expected_bs["center"][2].as_f64().unwrap(), 1e-4);
    assert_approx_eq(sphere.radius, expected_bs["radius"].as_f64().unwrap(), 1e-4);

    let translation = Matrix4::from_translation(Cartesian3::new(10.0, 20.0, 30.0));
    collection.set_model_matrix(translation);

    let sphere_wc = collection.base_volume_wc();
    assert_approx_eq(sphere_wc.center.x, expected_tbs["center"][0].as_f64().unwrap(), 1e-4);
    assert_approx_eq(sphere_wc.center.y, expected_tbs["center"][1].as_f64().unwrap(), 1e-4);
    assert_approx_eq(sphere_wc.center.z, expected_tbs["center"][2].as_f64().unwrap(), 1e-4);
    assert_approx_eq(sphere_wc.radius, expected_tbs["radius"].as_f64().unwrap(), 1e-4);
}

#[test]
fn cpu_layout_and_screen_space_bounding_box() {
    let fixture = load_fixture();
    let bbox_cases = fixture["bboxCases"].as_array().unwrap();

    let mut collection = LabelCollection::new();
    let handle = collection
        .add(LabelOptions {
            text: "AB".to_string(),
            show_background: true,
            scale: 1.0,
            horizontal_origin: HorizontalOrigin::Left,
            vertical_origin: VerticalOrigin::Baseline,
            ..Default::default()
        })
        .unwrap();

    collection.update();

    let label = handle.borrow();
    let case = &bbox_cases[0];
    let expected_bbox = &case["expectedBbox"];

    let center = Cartesian2::new(100.0, 200.0);
    let bbox = label.screen_space_bounding_box(center);

    assert_approx_eq(bbox.x, expected_bbox[0].as_f64().unwrap(), 1e-4);
    assert_approx_eq(bbox.y, expected_bbox[1].as_f64().unwrap(), 1e-4);
    assert_approx_eq(bbox.width, expected_bbox[2].as_f64().unwrap(), 1e-4);
    assert_approx_eq(bbox.height, expected_bbox[3].as_f64().unwrap(), 1e-4);
}

#[test]
fn single_threaded_memory_safety() {
    // Assert LabelHandle is single-threaded Rc<RefCell<Label>>
    let mut collection = LabelCollection::new();
    let handle = collection.add(LabelOptions::default()).unwrap();

    // Verify interior mutability via RefCell
    {
        let mut mut_ref = handle.borrow_mut();
        mut_ref.set_text("Updated Text".to_string());
    }

    assert_eq!(handle.borrow().text(), "Updated Text");
}

#[test]
fn changed_label_text_and_position_are_seen_by_collection() {
    let mut collection = LabelCollection::new();
    let handle = collection.add(LabelOptions { text: "A".into(), ..Default::default() }).unwrap();
    collection.update();
    assert_eq!(collection.total_glyph_count(), 1);
    assert_eq!(collection.base_volume().center, Cartesian3::ZERO);
    handle.borrow_mut().set_text("ABC".into());
    handle.borrow_mut().set_position(Cartesian3::new(1.0, 2.0, 3.0));
    collection.update();
    assert_eq!(collection.total_glyph_count(), 3);
    assert_eq!(collection.base_volume().center, Cartesian3::new(1.0, 2.0, 3.0));
}

#[test]
fn borrowed_labels_do_not_corrupt_collection_on_remove() {
    let mut collection = LabelCollection::new();
    let first = collection.add(LabelOptions::default()).unwrap();
    let second = collection.add(LabelOptions::default()).unwrap();
    let guard = second.borrow();
    assert!(!collection.remove(&first));
    assert_eq!(collection.len(), 2);
    assert!(collection.contains(&first));
    drop(guard);
    assert!(collection.remove(&first));
    assert_eq!(second.borrow().collection_index(), 0);
    drop(collection);
    assert!(!second.borrow().is_attached());
}

#[test]
fn rtl_mode_changes_apply_to_existing_label_text_changes() {
    let mut collection = LabelCollection::new();
    let label = collection.add(LabelOptions::default()).unwrap();
    collection.set_enable_right_to_left_detection(true);
    label.borrow_mut().set_text("אבג".into());
    assert_eq!(label.borrow().rendered_text(), reverse_rtl("אבג"));
}
