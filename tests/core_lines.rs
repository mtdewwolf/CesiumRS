//! PolylinePipeline and the line and volume geometries against CesiumJS
//! fixtures.

mod common;

use cesium_rs::core::polyline_pipeline as pp;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

/// Float32 attributes can land one f32 step apart when the f64 inputs differ
/// in the last bit; 1e-7 relative covers that and nothing larger.
const GEOMETRY: Tol = Tol { rel: 1e-7, abs: 1e-9, vector: false };

pub fn format(v: &Value) -> VertexFormat {
    match s(v) {
        "POSITION_ONLY" => VertexFormat::POSITION_ONLY,
        "POSITION_AND_NORMAL" => VertexFormat::POSITION_AND_NORMAL,
        "POSITION_NORMAL_AND_ST" => VertexFormat::POSITION_NORMAL_AND_ST,
        "POSITION_AND_ST" => VertexFormat::POSITION_AND_ST,
        "ALL" => VertexFormat::ALL,
        "TANGENT_ONLY" => VertexFormat { position: true, tangent: true, ..Default::default() },
        _ => VertexFormat { position: true, bitangent: true, ..Default::default() },
    }
}

pub fn ellipsoid(v: &Value) -> Ellipsoid {
    if s(v) == "MOON" {
        Ellipsoid::MOON
    } else {
        Ellipsoid::WGS84
    }
}

pub fn arc_type(v: &Value) -> ArcType {
    match u(v) {
        0 => ArcType::None,
        2 => ArcType::Rhumb,
        _ => ArcType::Geodesic,
    }
}

fn packed(length: usize, pack: impl Fn(&mut [f64])) -> Vec<f64> {
    let mut array = vec![0.0; length];
    pack(&mut array);
    array
}

/// The result of a create, or the error Cesium throws in its place.
fn or_throw(g: Option<Geometry>, throws: bool) -> Result<Value, Out> {
    match g {
        None if throws => Err(Out::Err("Cesium throws".into())),
        g => Ok(geometry_enc(g)),
    }
}

#[test]
fn polyline_pipeline() {
    run("core", "PolylinePipeline", |_| TIGHT, |f, a| {
        let arc_options = || {
            let mut o = pp::ArcOptions::new(list(&a[0], c3));
            o.ellipsoid = ellipsoid(&a[1]);
            o.height = if a[2].is_array() { pp::PolylineHeight::PerPosition(nums(&a[2])) } else { pp::PolylineHeight::Constant(n(&a[2])) };
            o.granularity = opt(&a[3]);
            o
        };
        match f {
            "generateArc" => ok(pp::generate_arc(&arc_options())?),
            "generateRhumbArc" => ok(pp::generate_rhumb_arc(&arc_options())?),
            "generateArcMinDistance" => {
                let mut o = pp::ArcOptions::new(list(&a[0], c3));
                o.min_distance = Some(n(&a[1]));
                ok(pp::generate_arc(&o)?)
            }
            "extractHeights" => ok(pp::extract_heights(&list(&a[0], c3), &ellipsoid(&a[1]))),
            "wrapLongitude" => {
                let m = (!a[1].is_null()).then(|| m4(&a[1]));
                let (positions, lengths) = pp::wrap_longitude(&list(&a[0], c3), m.as_ref());
                ok(json!([positions.enc(), lengths.iter().map(|&l| l as f64).collect::<Vec<_>>().enc()]))
            }
            _ => None,
        }
    });
}

fn colors(v: &Value) -> Option<Vec<Color>> {
    (!v.is_null()).then(|| list(v, color))
}

fn polyline_options(a: &[Value]) -> PolylineGeometryOptions {
    let mut o = PolylineGeometryOptions::new(list(&a[0], c3));
    o.colors = colors(&a[1]);
    o.colors_per_vertex = a[2].as_bool().unwrap();
    o.arc_type = arc_type(&a[3]);
    if let Some(g) = opt(&a[4]) {
        o.granularity = g;
    }
    o.ellipsoid = ellipsoid(&a[5]);
    o.vertex_format = format(&a[6]);
    o.width = n(&a[7]);
    o
}

fn simple_options(a: &[Value]) -> SimplePolylineGeometryOptions {
    let p = polyline_options(a);
    SimplePolylineGeometryOptions {
        positions: p.positions,
        colors: p.colors,
        colors_per_vertex: p.colors_per_vertex,
        arc_type: p.arc_type,
        granularity: p.granularity,
        ellipsoid: p.ellipsoid,
    }
}

#[test]
fn polyline_geometries() {
    run("core", "PolylineGeometry", |f| if f.contains("npack") { TIGHT } else { GEOMETRY }, |f, a| match f {
        "polyline" => {
            let p = PolylineGeometry::new(polyline_options(a));
            // Rhumb lines with colors throw a RangeError in Cesium.
            let throws = p.arc_type == ArcType::Rhumb && p.colors.is_some();
            let g = match or_throw(p.create_geometry(), throws) {
                Ok(g) => g,
                Err(e) => return Some(e),
            };
            ok(json!([packed(p.packed_length, |arr| p.pack(arr, 0)).enc(), g]))
        }
        "simplePolyline" => {
            let p = SimplePolylineGeometry::new(simple_options(a));
            let throws = p.arc_type == ArcType::Rhumb && p.colors.is_some() && !p.colors_per_vertex;
            let g = match or_throw(p.create_geometry(), throws) {
                Ok(g) => g,
                Err(e) => return Some(e),
            };
            ok(json!([packed(p.packed_length, |arr| p.pack(arr, 0)).enc(), g]))
        }
        "unpack" => {
            let p = PolylineGeometry::unpack(&nums(&a[0]), 0);
            ok(packed(p.packed_length, |arr| p.pack(arr, 0)))
        }
        "simpleUnpack" => {
            let p = SimplePolylineGeometry::unpack(&nums(&a[0]), 0);
            ok(packed(p.packed_length, |arr| p.pack(arr, 0)))
        }
        _ => None,
    });
}

fn heights(v: &Value) -> Option<Vec<f64>> {
    (!v.is_null()).then(|| nums(v))
}

fn wall_options(a: &[Value]) -> WallGeometryOptions {
    let mut o = WallGeometryOptions::new(list(&a[0], c3));
    o.maximum_heights = heights(&a[1]);
    o.minimum_heights = heights(&a[2]);
    o.vertex_format = format(&a[3]);
    if let Some(g) = opt(&a[4]) {
        o.granularity = g;
    }
    o.ellipsoid = ellipsoid(&a[5]);
    o
}

fn wall_outline(o: WallGeometryOptions) -> WallOutlineGeometry {
    WallOutlineGeometry::new(o.positions, o.maximum_heights, o.minimum_heights, Some(o.granularity), Some(o.ellipsoid))
}

#[test]
fn wall_geometries() {
    run("core", "WallGeometry", |f| if f.contains("npack") { TIGHT } else { GEOMETRY }, |f, a| match f {
        "wall" => {
            let w = WallGeometry::new(wall_options(a));
            ok(json!([packed(w.packed_length, |arr| w.pack(arr, 0)).enc(), geometry_enc(w.create_geometry())]))
        }
        "wallOutline" => {
            let w = wall_outline(wall_options(a));
            ok(json!([packed(w.packed_length, |arr| w.pack(arr, 0)).enc(), geometry_enc(w.create_geometry())]))
        }
        "fromConstantHeights" => {
            let positions = list(&a[0], c3);
            let w = WallGeometry::from_constant_heights(positions.clone(), opt(&a[1]), opt(&a[2]), Ellipsoid::WGS84, format(&a[3]));
            let wo = WallOutlineGeometry::from_constant_heights(positions, opt(&a[1]), opt(&a[2]), Ellipsoid::WGS84);
            ok(json!([
                packed(w.packed_length, |arr| w.pack(arr, 0)).enc(),
                geometry_enc(w.create_geometry()),
                packed(wo.packed_length, |arr| wo.pack(arr, 0)).enc(),
                geometry_enc(wo.create_geometry())
            ]))
        }
        "unpack" => {
            let w = WallGeometry::unpack(&nums(&a[0]), 0);
            ok(packed(w.packed_length, |arr| w.pack(arr, 0)))
        }
        "outlineUnpack" => {
            let w = WallOutlineGeometry::unpack(&nums(&a[0]), 0);
            ok(packed(w.packed_length, |arr| w.pack(arr, 0)))
        }
        _ => None,
    });
}
