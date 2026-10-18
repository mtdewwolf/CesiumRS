//! earcut and PolygonPipeline against CesiumJS fixtures.

mod common;

use cesium_rs::core::earcut::earcut;
use cesium_rs::core::polygon_pipeline as pp;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

fn indices(v: &Value) -> Vec<usize> {
    nums(v).into_iter().map(|x| x as usize).collect()
}

fn ellipsoid(v: &Value) -> Ellipsoid {
    match s(v) {
        "WGS84" => Ellipsoid::WGS84,
        "UNIT_SPHERE" => Ellipsoid::UNIT_SPHERE,
        _ => Ellipsoid::MOON,
    }
}

fn usizes(v: &[usize]) -> Value {
    v.iter().map(|&i| i as f64).collect::<Vec<_>>().enc()
}

#[test]
fn earcut_triangulation() {
    run("core", "Earcut", |_| EXACT, |f, a| match f {
        "earcut" => ok(usizes(&earcut(&nums(&a[0]), &indices(&a[1]), n(&a[2]) as usize))),
        _ => None,
    });
}

#[test]
fn polygon_pipeline() {
    run("core", "PolygonPipeline", |_| TIGHT, |f, a| match f {
        "computeArea2D" => ok(pp::compute_area_2d(&list(&a[0], c2))),
        "computeWindingOrder2D" => ok(pp::compute_winding_order_2d(&list(&a[0], c2)) as u32),
        "triangulate" => ok(usizes(&pp::triangulate(&list(&a[0], c2), &indices(&a[1])))),
        "computeSubdivision" | "computeRhumbLineSubdivision" => {
            let e = ellipsoid(&a[0]);
            let positions: Vec<Cartesian3> = list(&a[1], nums)
                .into_iter()
                .map(|d| Cartesian3::from_degrees(d[0], d[1], d[2], &e))
                .collect();
            let st = (!a[3].is_null()).then(|| list(&a[3], c2));
            let subdivide = if f == "computeSubdivision" {
                pp::compute_subdivision
            } else {
                pp::compute_rhumb_line_subdivision
            };
            let g = subdivide(&e, &positions, &indices(&a[2]), st.as_deref(), opt(&a[4])).expect("subdivision");
            let position = g.attributes.position.as_ref().unwrap();
            ok(json!([
                position.component_datatype as u32,
                position.values.to_f64_vec().enc(),
                g.attributes.st.as_ref().map(|st| json!([st.component_datatype as u32, st.values.to_f64_vec().enc()])),
                g.indices.as_ref().unwrap().to_f64_vec().enc(),
            ]))
        }
        "scaleToGeodeticHeight" => {
            let mut positions = nums(&a[0]);
            let flag = a[3].as_bool();
            pp::scale_to_geodetic_height(&mut positions, opt(&a[1]), &ellipsoid(&a[2]), flag)?;
            ok(positions)
        }
        _ => None,
    });
}

#[test]
fn earcut_handles_empty_and_degenerate_input() {
    assert!(earcut(&[], &[], 2).is_empty());
    assert!(earcut(&[0.0, 0.0, 1.0, 1.0], &[], 2).is_empty());
    // earcut 3.2.4 returns this order (its doc comment shows an older one).
    assert_eq!(earcut(&[10.0, 0.0, 0.0, 50.0, 60.0, 60.0, 70.0, 10.0], &[], 2), vec![1, 0, 3, 1, 3, 2]);
    // A hole with no vertices is skipped rather than panicking.
    assert_eq!(earcut(&[0.0, 0.0, 4.0, 0.0, 4.0, 4.0], &[3], 2).len(), 3);
}

#[test]
fn subdivision_rejects_bad_indices() {
    let e = Ellipsoid::WGS84;
    let p = [Cartesian3::from_degrees(0.0, 0.0, 0.0, &e), Cartesian3::from_degrees(1.0, 0.0, 0.0, &e)];
    assert!(pp::compute_subdivision(&e, &p, &[0, 1, 2], None, None).is_none());
    assert!(pp::compute_subdivision(&e, &p, &[0, 1], None, None).is_none());
    assert!(pp::compute_subdivision(&e, &p, &[0, 1, 1], Some(&[Cartesian2::ZERO]), None).is_none());
}
