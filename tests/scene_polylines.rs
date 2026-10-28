//! CPU reference checks and property validation for Scene polylines.
use cesium_rs::core;
use cesium_rs::scene;
use core::{Cartesian3, DistanceDisplayCondition, Ellipsoid, Matrix4};
use scene::polyline::*;
use scene::polyline_collection::*;
use serde_json::{json, Value};

fn snapshot(p: &Polyline) -> Value {
    let segs = p.segments();
    json!({
        "show": p.show(),
        "width": p.width(),
        "loop": p.r#loop(),
        "materialType": p.material().type_name,
        "positions": p.positions().iter().map(|pos| [pos.x, pos.y, pos.z]).collect::<Vec<_>>(),
        "actualPositions": p.actual_positions().iter().map(|pos| [pos.x, pos.y, pos.z]).collect::<Vec<_>>(),
        "distanceDisplayCondition": p.distance_display_condition().map(|d| [d.near, d.far]),
        "id": p.id(),
        "boundingVolume": {
            "center": [p.bounding_volume().center.x, p.bounding_volume().center.y, p.bounding_volume().center.z],
            "radius": p.bounding_volume().radius,
        },
        "boundingVolumeWC": {
            "center": [p.bounding_volume_wc().center.x, p.bounding_volume_wc().center.y, p.bounding_volume_wc().center.z],
            "radius": p.bounding_volume_wc().radius,
        },
        "segmentLengths": segs.1,
        "segmentPositionsCount": segs.0.len(),
    })
}

fn compare_values(actual: &Value, expected: &Value, tolerance: f64) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let a_f = a.as_f64().unwrap();
            let b_f = b.as_f64().unwrap();
            assert!(
                (a_f - b_f).abs() <= tolerance,
                "Number mismatch: actual {a_f} vs expected {b_f} (tolerance {tolerance})"
            );
        }
        (Value::Array(a_arr), Value::Array(e_arr)) => {
            assert_eq!(a_arr.len(), e_arr.len(), "Array len mismatch");
            for (a, e) in a_arr.iter().zip(e_arr.iter()) {
                compare_values(a, e, tolerance);
            }
        }
        (Value::Object(a_obj), Value::Object(e_obj)) => {
            for (k, e_val) in e_obj {
                let a_val = a_obj
                    .get(k)
                    .unwrap_or_else(|| panic!("Missing key {k} in actual object"));
                compare_values(a_val, e_val, tolerance);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

#[test]
fn reference_defaults_custom_and_collection_lifecycle() {
    let fixture: Value = serde_json::from_str(include_str!("data/scene-polylines.json")).unwrap();

    let mut c = PolylineCollection::new();
    let p_defaults = c.add(PolylineOptions::default()).unwrap();

    let custom_positions = vec![
        Cartesian3::new(1000.0, 2000.0, 3000.0),
        Cartesian3::new(1000.0, 2000.0, 3000.0), // duplicate
        Cartesian3::new(4000.0, 5000.0, 6000.0),
        Cartesian3::new(7000.0, 8000.0, 9000.0),
    ];

    let p_custom = c
        .add(PolylineOptions {
            positions: custom_positions,
            width: 4.5,
            show: true,
            r#loop: true,
            id: Some("custom-polyline-1".into()),
            distance_display_condition: Some(DistanceDisplayCondition::new(12.0, 850.0)),
            ..Default::default()
        })
        .unwrap();

    compare_values(&snapshot(&p_defaults.borrow()), &fixture["defaults"], 1e-6);

    assert_eq!(c.len(), fixture["initialLength"].as_u64().unwrap() as usize);
    assert_eq!(c.contains(&p_custom), fixture["containsBefore"].as_bool().unwrap());

    // Transform model matrix
    let translation = Matrix4::from_translation(Cartesian3::new(25.0, 50.0, 75.0));
    c.model_matrix = translation;
    p_custom.borrow_mut().update(&c.model_matrix);

    compare_values(&snapshot(&p_custom.borrow()), &fixture["transformed"], 1e-4);

    // Toggle loop to false
    p_custom.borrow_mut().set_loop(false);
    compare_values(&snapshot(&p_custom.borrow()), &fixture["loopFalse"], 1e-4);

    // Toggle loop to true
    p_custom.borrow_mut().set_loop(true);
    compare_values(&snapshot(&p_custom.borrow()), &fixture["loopTrueAgain"], 1e-4);

    // Mutate positions
    p_custom.borrow_mut().set_positions(vec![
        Cartesian3::new(10.0, 20.0, 30.0),
        Cartesian3::new(40.0, 50.0, 60.0),
    ]);
    compare_values(&snapshot(&p_custom.borrow()), &fixture["mutatedPositions"], 1e-4);

    // Antimeridian crossing polyline
    let p_antimeridian = c
        .add(PolylineOptions {
            positions: vec![
                Cartesian3::from_degrees(179.0, 0.0, 0.0, &Ellipsoid::WGS84),
                Cartesian3::from_degrees(-179.0, 0.0, 0.0, &Ellipsoid::WGS84),
            ],
            width: 2.0,
            ..Default::default()
        })
        .unwrap();

    compare_values(&snapshot(&p_antimeridian.borrow()), &fixture["antimeridian"], 1e-4);

    // Removal
    assert_eq!(c.remove(&p_custom), fixture["firstRemoval"].as_bool().unwrap());
    assert_eq!(c.remove(&p_custom), fixture["secondRemoval"].as_bool().unwrap());
    assert_eq!(c.contains(&p_custom), fixture["containsAfter"].as_bool().unwrap());
    assert!(!p_custom.borrow().is_attached());
    assert_eq!(c.len(), fixture["lengthAfter"].as_u64().unwrap() as usize);
}

#[test]
fn validation_and_error_handling() {
    let mut c = PolylineCollection::new();

    // Cesium accepts a reversed distance condition at construction.
    let bad_ddc = PolylineOptions {
        distance_display_condition: Some(DistanceDisplayCondition::new(100.0, 50.0)),
        ..Default::default()
    };
    assert!(c.add(bad_ddc).is_ok());

    // Cesium accepts negative width at construction.
    let bad_width = PolylineOptions {
        width: -2.0,
        ..Default::default()
    };
    assert!(c.add(bad_width).is_ok());

    // Adding valid polyline
    let p = c
        .add(PolylineOptions {
            positions: vec![Cartesian3::new(0.0, 0.0, 0.0), Cartesian3::new(10.0, 0.0, 0.0)],
            width: 3.0,
            ..Default::default()
        })
        .unwrap();

    // Width setters accept negative values; distance-condition setters validate.
    assert!(p.borrow_mut().set_width(-1.0).is_ok());
    assert_eq!(p.borrow().width(), -1.0);

    assert!(p
        .borrow_mut()
        .set_distance_display_condition(Some(DistanceDisplayCondition::new(50.0, 20.0)))
        .is_err());
    assert!(p.borrow().distance_display_condition().is_none());

    // Destroy collection
    c.destroy();
    assert!(c.is_destroyed());
    assert!(c.is_empty());
    assert!(!p.borrow().is_attached());

    // Adding to destroyed collection
    assert!(c.add(PolylineOptions::default()).is_err());
}

#[test]
fn batch_attributes_and_vertex_generation() {
    let mut c = PolylineCollection::new();
    let p1 = c
        .add(PolylineOptions {
            positions: vec![
                Cartesian3::new(0.0, 0.0, 0.0),
                Cartesian3::new(10.0, 0.0, 0.0),
                Cartesian3::new(10.0, 10.0, 0.0),
            ],
            width: 4.0,
            show: true,
            distance_display_condition: Some(DistanceDisplayCondition::new(10.0, 500.0)),
            ..Default::default()
        })
        .unwrap();

    let _p2 = c
        .add(PolylineOptions {
            positions: vec![
                Cartesian3::new(100.0, 0.0, 0.0),
                Cartesian3::new(200.0, 0.0, 0.0),
            ],
            width: 0.0, // width 0 means show should be 0 in batch attributes
            show: true,
            ..Default::default()
        })
        .unwrap();

    let batch = c.batch_attributes();
    assert_eq!(batch.len(), 2);
    // Polyline 1: width 4.0, show 1.0
    assert_eq!(batch[0].width_and_show, [4.0, 1.0]);
    assert_eq!(batch[0].distance_display_condition, [10.0, 500.0]);
    // Polyline 2: width 0.0, show 0.0 (width <= 0)
    assert_eq!(batch[1].width_and_show, [0.0, 0.0]);

    // Tessellation data
    let geom = c.generate_vertex_data();
    assert_eq!(geom.len(), 1); // 1 bucket (both Color material)
    let color_geom = &geom[0];
    assert_eq!(color_geom.material_type, "Color");

    // Polyline 1 has 3 points (1 segment, length 3).
    // Segment length L = 3.
    // In CesiumJS: 4 * L - 4 = 8 vertices. 6 * (L - 1) = 12 indices.
    // Polyline 2 has 2 points (1 segment, length 2).
    // Segment length L = 2.
    // In CesiumJS: 4 * 2 - 4 = 4 vertices. 6 * (2 - 1) = 6 indices.
    // Total vertices = 8 + 4 = 12.
    // Total indices = 12 + 6 = 18.
    assert_eq!(color_geom.vertices.len(), 12);
    assert_eq!(color_geom.indices.len(), 18);

    // Verify first quad indices
    assert_eq!(&color_geom.indices[0..6], &[0, 2, 1, 1, 2, 3]);

    // Mutate p1 show to false
    p1.borrow_mut().set_show(false);
    let updated_batch = c.batch_attributes();
    assert_eq!(updated_batch[0].width_and_show, [4.0, 0.0]);
}

#[test]
fn loop_edits_refresh_geometry_and_preserve_near_equal_endpoints() {
    let mut collection = PolylineCollection::new();
    let points = vec![Cartesian3::ZERO, Cartesian3::new(10.0, 0.0, 0.0), Cartesian3::new(1e-12, 0.0, 0.0)];
    let line = collection.add(PolylineOptions { positions: points.clone(), ..Default::default() }).unwrap();
    line.borrow_mut().set_loop(true);
    assert_eq!(line.borrow().actual_positions().len(), 4);
    assert_eq!(collection.generate_vertex_data()[0].vertices.len(), 12);
    line.borrow_mut().set_loop(false);
    assert_eq!(line.borrow().actual_positions(), points);
    assert_eq!(collection.generate_vertex_data()[0].vertices.len(), 8);
}

#[test]
fn three_dimensional_geometry_uses_unsplit_positions_and_current_matrix() {
    let mut collection = PolylineCollection::new();
    let line = collection.add(PolylineOptions { positions: vec![
        Cartesian3::from_degrees(179.0, 0.0, 0.0, &Ellipsoid::WGS84),
        Cartesian3::from_degrees(-179.0, 0.0, 0.0, &Ellipsoid::WGS84),
    ], ..Default::default() }).unwrap();
    let geometry = collection.generate_vertex_data();
    assert_eq!(geometry[0].vertices.len(), 4);
    assert_eq!(geometry[0].indices.len(), 6);
    collection.model_matrix = Matrix4::from_translation(Cartesian3::new(1.0, 2.0, 3.0));
    let center = line.borrow().bounding_volume().center;
    let attributes = collection.batch_attributes();
    let actual = line.borrow().bounding_volume_wc().center;
    assert!((actual.x - (center.x + 1.0)).abs() < 1e-8);
    assert!((actual.y - (center.y + 2.0)).abs() < 1e-8);
    assert!((actual.z - (center.z + 3.0)).abs() < 1e-8);
    assert_eq!(attributes.len(), 1);
}

#[test]
fn default_polyline_color_is_opaque_white() {
    let options = PolylineOptions::default();
    assert_eq!(options.material.uniform("color"), Some(&scene::MaterialValue::from_color(core::Color::WHITE)));
}

#[test]
fn geometry_build_cleans_dirty_polylines_and_preserves_first_material_order() {
    let mut collection = PolylineCollection::new();
    let points = vec![Cartesian3::ZERO, Cartesian3::new(1.0, 0.0, 0.0)];
    let line = collection.add(PolylineOptions {
        positions: points.clone(),
        material: scene::Material::from_type("PolylineDash"),
        ..Default::default()
    }).unwrap();
    collection.add(PolylineOptions { positions: points.clone(), ..Default::default() }).unwrap();
    line.borrow_mut().set_positions(points);
    assert!(line.borrow().is_dirty());
    let geometry = collection.generate_vertex_data();
    assert_eq!(geometry.iter().map(|bucket| bucket.material_type.as_str()).collect::<Vec<_>>(), ["PolylineDash", "Color"]);
    assert!(!line.borrow().is_dirty());
    assert_eq!(line.borrow().properties_changed()[POSITION_INDEX], 0);
}
