//! Corridor positions and full outline buffers against real CesiumJS.
mod common;
use cesium_rs::core::corridor_geometry_library as cl;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

fn ellipsoid(v: &Value) -> Ellipsoid {
    if s(v) == "MOON" {
        Ellipsoid::MOON
    } else {
        Ellipsoid::WGS84
    }
}
fn corner(v: &Value) -> CornerType {
    match u(v) {
        1 => CornerType::Mitered,
        2 => CornerType::Beveled,
        _ => CornerType::Rounded,
    }
}
fn packed(v: &CorridorOutlineGeometry) -> Value {
    let mut a = vec![-7.0; v.packed_length + 4];
    v.pack(&mut a, 2);
    a.enc()
}

#[test]
fn corridor_geometry_library() {
    run(
        "core",
        "CorridorGeometryLibrary",
        |_| TIGHT,
        |f, a| match f {
            "computePositions" => {
                let positions = list(&a[0], c3);
                let mut o = cl::CorridorPositionOptions::new(&positions, n(&a[1]));
                o.corner_type = corner(&a[2]);
                o.granularity = n(&a[3]);
                o.ellipsoid = ellipsoid(&a[4]);
                o.save_attributes = a[5].as_bool().unwrap();
                let p = cl::compute_positions(&o)?;
                let corners: Vec<Value> = p
                    .corners
                    .iter()
                    .map(|c| match c {
                        cl::CorridorCorner::Left(p) => json!({ "leftPositions": p.enc() }),
                        cl::CorridorCorner::Right(p) => json!({ "rightPositions": p.enc() }),
                    })
                    .collect();
                ok(
                    json!({ "positions": p.positions.enc(), "corners": corners, "lefts": p.lefts.enc(), "normals": p.normals.enc(), "endPositions": p.end_positions.map(|p| p.enc()) }),
                )
            }
            "addAttribute" => {
                let mut out = vec![-7.0; 12];
                cl::add_attribute(&mut out, c3(&a[0]), opt(&a[1]).map(|n| n as usize), opt(&a[2]).map(|n| n as usize))?;
                ok(out)
            }
            _ => None,
        },
    );
}

#[test]
fn corridor_outline_geometry() {
    run(
        "core",
        "CorridorOutlineGeometry",
        |_| TIGHT,
        |f, a| match f {
            "outline" => {
                let mut o = CorridorOutlineGeometryOptions::new(list(&a[0], c3), n(&a[1]));
                o.height = n(&a[2]);
                o.extruded_height = opt(&a[3]);
                o.corner_type = corner(&a[4]);
                if let Some(g) = opt(&a[5]) {
                    o.granularity = g;
                }
                o.ellipsoid = ellipsoid(&a[6]);
                o.offset_attribute = opt(&a[7]).and_then(GeometryOffsetAttribute::unpack);
                let v = CorridorOutlineGeometry::new(o);
                ok(json!([packed(&v), geometry_enc(v.create_geometry())]))
            }
            "unpack" => ok(packed(&CorridorOutlineGeometry::unpack(&nums(&a[0]), 2))),
            "largeOutline" => {
                let e = Ellipsoid::WGS84;
                let mut o = CorridorOutlineGeometryOptions::new(
                    Cartesian3::from_degrees_array(&[0.0, 0.0, 20.0, 0.0], &e),
                    1000.0,
                );
                o.granularity = 0.00001;
                o.corner_type = CornerType::Mitered;
                let g = CorridorOutlineGeometry::new(o).create_geometry()?;
                let indices = g.indices.as_ref()?;
                let values = indices.to_f64_vec();
                ok(json!([
                    g.attributes.position.as_ref()?.values.len() / 3,
                    typed_array_name(indices),
                    values.len(),
                    values[values.len() - 8..]
                ]))
            }
            _ => None,
        },
    );
}

#[test]
fn invalid_spacing_and_attribute_offsets_are_rejected() {
    let e = Ellipsoid::WGS84;
    let positions = Cartesian3::from_degrees_array(&[0.0, 0.0, 1.0, 0.0], &e);
    for granularity in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut o = CorridorOutlineGeometryOptions::new(positions.clone(), 1000.0);
        o.granularity = granularity;
        assert!(CorridorOutlineGeometry::new(o).create_geometry().is_none());
    }
    let mut out = vec![1.0; 6];
    assert!(cl::add_attribute(&mut out, Cartesian3::ZERO, Some(0), Some(1)).is_none());
    assert_eq!(out, vec![1.0; 6]);
}

fn filled_options(a: &[Value]) -> CorridorGeometryOptions {
    let mut o = CorridorGeometryOptions::new(list(&a[0], c3), n(&a[1]));
    o.height = n(&a[2]);
    o.extruded_height = opt(&a[3]);
    o.corner_type = corner(&a[4]);
    if let Some(g) = opt(&a[5]) {
        o.granularity = g;
    }
    o.ellipsoid = ellipsoid(&a[6]);
    o.offset_attribute = opt(&a[7]).and_then(GeometryOffsetAttribute::unpack);
    o.vertex_format = match s(&a[8]) {
        "POSITION_ONLY" => VertexFormat::POSITION_ONLY,
        "POSITION_AND_NORMAL" => VertexFormat::POSITION_AND_NORMAL,
        "POSITION_NORMAL_AND_ST" => VertexFormat::POSITION_NORMAL_AND_ST,
        "POSITION_AND_ST" => VertexFormat::POSITION_AND_ST,
        "ALL" => VertexFormat::ALL,
        "TANGENT_ONLY" => VertexFormat { position: true, tangent: true, ..Default::default() },
        "BITANGENT_ONLY" => VertexFormat { position: true, bitangent: true, ..Default::default() },
        "NORMAL_ONLY" => VertexFormat { normal: true, ..Default::default() },
        "POSITION_AND_COLOR" => VertexFormat::POSITION_AND_COLOR,
        _ => VertexFormat::default(),
    };
    o.shadow_volume = a[9].as_bool().unwrap();
    o
}

fn filled_packed(v: &CorridorGeometry) -> Value {
    let mut a = vec![-7.0; v.packed_length + 4];
    v.pack(&mut a, 2);
    a.enc()
}

#[test]
fn corridor_geometry() {
    const GEOMETRY: Tol = Tol { rel: 1e-7, abs: 1e-9, vector: false };
    run(
        "core",
        "CorridorGeometry",
        |f| if f == "properties" || f == "filledUnpack" { TIGHT } else { GEOMETRY },
        |f, a| match f {
            "filled" => {
                let v = CorridorGeometry::new(filled_options(a));
                ok(json!([filled_packed(&v), geometry_enc(v.create_geometry())]))
            }
            "properties" => {
                let o = filled_options(a);
                let rectangle = CorridorGeometry::compute_rectangle(&o.positions, &o.ellipsoid, o.width, o.corner_type);
                let v = CorridorGeometry::new(o);
                ok(json!([v.rectangle().enc(), rectangle.enc(), v.texture_coordinate_rotation_points()]))
            }
            "filledUnpack" => ok(filled_packed(&CorridorGeometry::unpack(&nums(&a[0]), 2))),
            "shadowVolume" => {
                let v = CorridorGeometry::new(filled_options(a)).create_shadow_volume(
                    |gr, e| -100.0 - gr * 1000.0 - e.maximum_radius / 1e6,
                    |gr, e| 400.0 + gr * 2000.0 + e.maximum_radius / 1e6,
                );
                ok(json!([filled_packed(&v), geometry_enc(v.create_geometry())]))
            }
            "largeFilled" => {
                let e = Ellipsoid::WGS84;
                let mut o =
                    CorridorGeometryOptions::new(Cartesian3::from_degrees_array(&[0.0, 0.0, 20.0, 0.0], &e), 1000.0);
                o.granularity = 0.00001;
                o.corner_type = CornerType::Mitered;
                o.vertex_format = VertexFormat::POSITION_ONLY;
                let g = CorridorGeometry::new(o).create_geometry()?;
                let indices = g.indices.as_ref()?;
                let values = indices.to_f64_vec();
                ok(json!([
                    g.attributes.position.as_ref()?.values.len() / 3,
                    typed_array_name(indices),
                    values.len(),
                    values[values.len() - 12..]
                ]))
            }
            _ => None,
        },
    );
}

#[test]
fn filled_corridors_reject_invalid_inputs_and_preserve_their_positions() {
    let positions = Cartesian3::from_degrees_array(&[0.0, 20.0, 1.0, 20.0, 1.0, 21.0], &Ellipsoid::WGS84);
    let mut options = CorridorGeometryOptions::new(positions.clone(), 1000.0);
    options.height = 500.0;
    options.extruded_height = Some(-100.0);
    options.vertex_format = VertexFormat::ALL;
    let corridor = CorridorGeometry::new(options.clone());
    let first = corridor.create_geometry().unwrap();
    assert_eq!(corridor.positions, positions);
    assert_eq!(corridor.create_geometry().unwrap(), first);
    assert!(corridor.rectangle().is_some());
    assert_eq!(corridor.positions, positions);
    for spacing in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut invalid = options.clone();
        invalid.granularity = spacing;
        assert!(CorridorGeometry::new(invalid).create_geometry().is_none());
    }
    let mut invalid = options.clone();
    invalid.positions[0] = Cartesian3::ZERO;
    assert!(CorridorGeometry::new(invalid).create_geometry().is_none());
    // Cesium's extrusion path needs top normals to compute wall tangents.
    // Its tangent-only format throws a DeveloperError; the port returns None.
    options.vertex_format = VertexFormat { position: true, tangent: true, ..Default::default() };
    assert!(CorridorGeometry::new(options).create_geometry().is_none());
}
