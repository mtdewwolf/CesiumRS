//! Filled surface geometries (ellipses and circles) against CesiumJS
//! fixtures.

mod common;

use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

/// Float32 attributes can land one f32 step apart when the f64 inputs differ
/// in the last bit; 1e-7 relative covers that and nothing larger.
const GEOMETRY: Tol = Tol { rel: 1e-7, abs: 1e-9, vector: false };

fn format(v: &Value) -> VertexFormat {
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

fn ellipsoid(v: &Value) -> Ellipsoid {
    if s(v) == "MOON" {
        Ellipsoid::MOON
    } else {
        Ellipsoid::WGS84
    }
}

/// Options from `ellipseArgs`: center, axes, ellipsoid, heights, rotations,
/// granularity, format, shadow volume and offset.
fn ellipse_options(a: &[Value]) -> EllipseGeometryOptions {
    let mut o = EllipseGeometryOptions::new(c3(&a[0]), n(&a[1]), n(&a[2]));
    o.ellipsoid = ellipsoid(&a[3]);
    if let Some(h) = opt(&a[4]) {
        o.height = h;
    }
    o.extruded_height = opt(&a[5]);
    o.rotation = n(&a[6]);
    o.st_rotation = n(&a[7]);
    if let Some(g) = opt(&a[8]) {
        o.granularity = g;
    }
    o.vertex_format = format(&a[9]);
    o.shadow_volume = a[10].as_bool().unwrap();
    o.offset_attribute = opt(&a[11]).and_then(GeometryOffsetAttribute::unpack);
    o
}

fn packed(length: usize, pack: impl Fn(&mut [f64])) -> Vec<f64> {
    let mut array = vec![0.0; length];
    pack(&mut array);
    array
}

#[test]
fn ellipse_and_circle_geometry() {
    run(
        "core",
        "EllipseGeometry",
        |f| if f == "computeRectangle" || f.contains("npack") { TIGHT } else { GEOMETRY },
        |f, a| match f {
            "ellipse" => {
                let e = EllipseGeometry::new(ellipse_options(a))?;
                let pack = packed(EllipseGeometry::PACKED_LENGTH, |arr| e.pack(arr, 0));
                ok(json!([
                    geometry_enc(e.create_geometry()),
                    pack.enc(),
                    e.rectangle().enc(),
                    e.texture_coordinate_rotation_points().to_vec().enc()
                ]))
            }
            "circle" => {
                let c = CircleGeometry::new(n(&a[1]), ellipse_options(a))?;
                let pack = packed(CircleGeometry::PACKED_LENGTH, |arr| c.pack(arr, 0));
                ok(json!([
                    geometry_enc(c.create_geometry()),
                    pack.enc(),
                    c.rectangle().enc(),
                    c.texture_coordinate_rotation_points().to_vec().enc()
                ]))
            }
            "unpack" => {
                let e = EllipseGeometry::unpack(&nums(&a[0]), 0)?;
                ok(packed(EllipseGeometry::PACKED_LENGTH, |arr| e.pack(arr, 0)))
            }
            "circleUnpack" => {
                let c = CircleGeometry::unpack(&nums(&a[0]), 0)?;
                ok(packed(CircleGeometry::PACKED_LENGTH, |arr| c.pack(arr, 0)))
            }
            "createShadowVolume" => {
                let e = EllipseGeometry::new(ellipse_options(a))?.create_shadow_volume(|_, _| -1000.0, |_, _| 3000.0)?;
                let pack = packed(EllipseGeometry::PACKED_LENGTH, |arr| e.pack(arr, 0));
                ok(json!([pack.enc(), geometry_enc(e.create_geometry())]))
            }
            "computeRectangle" => {
                let granularity = opt(&a[4]).unwrap_or(cesium_rs::core::math::RADIANS_PER_DEGREE);
                ok(EllipseGeometry::compute_rectangle(c3(&a[0]), n(&a[1]), n(&a[2]), n(&a[3]), granularity, &Ellipsoid::WGS84))
            }
            _ => None,
        },
    );
}

/// Options from `rectangleArgs`: rectangle, ellipsoid, heights, rotations,
/// granularity, format, shadow volume and offset.
fn rectangle_options(a: &[Value]) -> RectangleGeometryOptions {
    let mut o = RectangleGeometryOptions::new(rect(&a[0]));
    o.ellipsoid = ellipsoid(&a[1]);
    if let Some(h) = opt(&a[2]) {
        o.height = h;
    }
    o.extruded_height = opt(&a[3]);
    o.rotation = n(&a[4]);
    o.st_rotation = n(&a[5]);
    if let Some(g) = opt(&a[6]) {
        o.granularity = g;
    }
    o.vertex_format = format(&a[7]);
    o.shadow_volume = a[8].as_bool().unwrap();
    o.offset_attribute = opt(&a[9]).and_then(GeometryOffsetAttribute::unpack);
    o
}

fn outline_options(a: &[Value]) -> RectangleOutlineOptions {
    let r = rectangle_options(a);
    RectangleOutlineOptions {
        rectangle: r.rectangle,
        granularity: r.granularity,
        ellipsoid: r.ellipsoid,
        rotation: r.rotation,
        height: r.height,
        extruded_height: r.extruded_height,
        offset_attribute: r.offset_attribute,
    }
}

/// Cesium throws a RangeError for an extruded shadow volume that also asks
/// for normals; anything else that yields no geometry is `undefined`.
fn rectangle_geometry(r: &RectangleGeometry) -> Result<Value, Out> {
    match r.create_geometry() {
        None if r.shadow_volume && r.vertex_format.normal => Err(Out::Err("RangeError".into())),
        g => Ok(geometry_enc(g)),
    }
}

#[test]
fn rectangle_geometry_and_outline() {
    run(
        "core",
        "RectangleGeometry",
        |f| if f == "computeRectangle" || f.contains("npack") { TIGHT } else { GEOMETRY },
        |f, a| match f {
            "rectangle" => {
                let r = RectangleGeometry::new(rectangle_options(a));
                let pack = packed(RectangleGeometry::PACKED_LENGTH, |arr| r.pack(arr, 0));
                let geometry = match rectangle_geometry(&r) {
                    Ok(g) => g,
                    Err(e) => return Some(e),
                };
                ok(json!([pack.enc(), r.rectangle().enc(), r.texture_coordinate_rotation_points().to_vec().enc(), geometry]))
            }
            "outline" => {
                let r = RectangleOutlineGeometry::new(outline_options(a));
                let pack = packed(RectangleOutlineGeometry::PACKED_LENGTH, |arr| r.pack(arr, 0));
                ok(json!([pack.enc(), geometry_enc(r.create_geometry())]))
            }
            "unpack" => {
                let r = RectangleGeometry::unpack(&nums(&a[0]), 0);
                ok(packed(RectangleGeometry::PACKED_LENGTH, |arr| r.pack(arr, 0)))
            }
            "outlineUnpack" => {
                let r = RectangleOutlineGeometry::unpack(&nums(&a[0]), 0);
                ok(packed(RectangleOutlineGeometry::PACKED_LENGTH, |arr| r.pack(arr, 0)))
            }
            "createShadowVolume" => {
                let r = RectangleGeometry::new(rectangle_options(a)).create_shadow_volume(|_, _| -1000.0, |_, _| 3000.0);
                let pack = packed(RectangleGeometry::PACKED_LENGTH, |arr| r.pack(arr, 0));
                let geometry = match rectangle_geometry(&r) {
                    Ok(g) => g,
                    Err(e) => return Some(e),
                };
                ok(json!([pack.enc(), geometry]))
            }
            "computeRectangle" => {
                let granularity = opt(&a[2]).unwrap_or(cesium_rs::core::math::RADIANS_PER_DEGREE);
                ok(RectangleGeometry::compute_rectangle(&rect(&a[0]), granularity, n(&a[1]), &Ellipsoid::WGS84))
            }
            _ => None,
        },
    );
}
