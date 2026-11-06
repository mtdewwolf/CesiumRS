//! Polyline properties, loops, antimeridian splitting, bounding volumes and
//! membership against pinned CesiumJS snapshots (`tests/data/scene-polylines.json`).
//! The collection's update is checked against a SwiftShader capture in
//! `tests/scene_polyline_collection.rs`.
use cesium_rs::core;
use cesium_rs::renderer::{Gl, GlValue};
use cesium_rs::scene;
use core::{Cartesian3, DistanceDisplayCondition, Ellipsoid, Matrix4, RuntimeError};
use scene::polyline::*;
use scene::polyline_collection::*;
use scene::ScenePrimitive;
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

fn snapshot(p: &Polyline) -> Value {
    let segs = p.segments();
    json!({
        "show": p.show(),
        "width": p.width(),
        "loop": p.r#loop(),
        "materialType": p.material().borrow().type_name.clone(),
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
                let a_val = a_obj.get(k).unwrap_or_else(|| panic!("Missing key {k} in actual object"));
                compare_values(a_val, e_val, tolerance);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

#[test]
fn reference_defaults_custom_and_collection_lifecycle() {
    let fixture: Value = serde_json::from_str(include_str!("data/scene-polylines.json")).unwrap();

    let mut c = PolylineCollection::<NoGl>::default();
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
            width: Some(4.5),
            show: Some(true),
            r#loop: Some(true),
            id: Some("custom-polyline-1".into()),
            distance_display_condition: Some(DistanceDisplayCondition::new(12.0, 850.0)),
            ..Default::default()
        })
        .unwrap();

    compare_values(&snapshot(&p_defaults), &fixture["defaults"], 1e-6);

    assert_eq!(c.length(), fixture["initialLength"].as_u64().unwrap() as usize);
    assert_eq!(c.contains(&p_custom), fixture["containsBefore"].as_bool().unwrap());

    // Transform model matrix
    let translation = Matrix4::from_translation(Cartesian3::new(25.0, 50.0, 75.0));
    c.set_model_matrix(translation);
    p_custom.update();

    compare_values(&snapshot(&p_custom), &fixture["transformed"], 1e-4);

    // Toggle loop to false
    p_custom.set_loop(false);
    compare_values(&snapshot(&p_custom), &fixture["loopFalse"], 1e-4);

    // Toggle loop to true
    p_custom.set_loop(true);
    compare_values(&snapshot(&p_custom), &fixture["loopTrueAgain"], 1e-4);

    // Mutate positions
    p_custom.set_positions(vec![Cartesian3::new(10.0, 20.0, 30.0), Cartesian3::new(40.0, 50.0, 60.0)]);
    compare_values(&snapshot(&p_custom), &fixture["mutatedPositions"], 1e-4);

    // Antimeridian crossing polyline
    let p_antimeridian = c
        .add(PolylineOptions {
            positions: vec![
                Cartesian3::from_degrees(179.0, 0.0, 0.0, &Ellipsoid::WGS84),
                Cartesian3::from_degrees(-179.0, 0.0, 0.0, &Ellipsoid::WGS84),
            ],
            width: Some(2.0),
            ..Default::default()
        })
        .unwrap();

    compare_values(&snapshot(&p_antimeridian), &fixture["antimeridian"], 1e-4);

    // Removal
    assert_eq!(c.remove(&p_custom).unwrap(), fixture["firstRemoval"].as_bool().unwrap());
    assert_eq!(c.remove(&p_custom).unwrap(), fixture["secondRemoval"].as_bool().unwrap());
    assert_eq!(c.contains(&p_custom), fixture["containsAfter"].as_bool().unwrap());
    assert!(p_custom.is_destroyed());
    assert_eq!(c.length(), fixture["lengthAfter"].as_u64().unwrap() as usize);
}

#[test]
fn validation_and_dirty_marking() {
    let mut c = PolylineCollection::<NoGl>::default();
    let p = c.add(PolylineOptions::default()).unwrap();
    // Cesium accepts a reversed distance condition at construction but not in the setter.
    assert_eq!(
        p.set_distance_display_condition(Some(DistanceDisplayCondition::new(5., 5.))).unwrap_err().message,
        "far distance must be greater than near distance."
    );
    p.set_width(1.0);
    assert!(!p.is_dirty(), "an equal value changes nothing");
    p.set_width(3.0);
    p.set_show(false);
    assert!(p.is_dirty());
    assert!(c.polylines_updated());
    assert_eq!(c.polylines_to_update().len(), 1, "queued once");
    assert_eq!(p.properties_changed()[WIDTH_INDEX], 1);
    assert_eq!(p.properties_changed()[SHOW_INDEX], 1);
    let same = p.material();
    p.set_material(same);
    assert_eq!(p.properties_changed()[MATERIAL_INDEX], 0, "the same material by identity");
    p.set_material(default_material());
    assert_eq!(p.properties_changed()[MATERIAL_INDEX], 1, "an equal but distinct material");
    c.destroy().unwrap();
    assert!(p.is_destroyed());
    assert!(c.add(PolylineOptions::default()).is_err());
}
