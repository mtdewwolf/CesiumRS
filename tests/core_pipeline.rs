//! GeometryPipeline and the geometry instance attributes against CesiumJS
//! fixtures.

mod common;

use cesium_rs::core::geometry_pipeline as gp;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Map, Value};

/// Float32 outputs can land one f32 step apart when f64 inputs differ in
/// the last bit.
const GEOMETRY: Tol = Tol { rel: 1e-7, abs: 1e-9, vector: false };

#[test]
fn degenerate_texture_coordinates_do_not_publish_invalid_tangents() {
    let attributes = GeometryAttributes {
        position: Some(GeometryAttribute::doubles(3, vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])),
        normal: Some(GeometryAttribute::floats(3, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0])),
        st: Some(GeometryAttribute::floats(2, &[0.0; 6])),
        tangent: Some(GeometryAttribute::floats(3, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0])),
        ..Default::default()
    };
    let mut geometry = Geometry::new(attributes, Some(TypedArray::Uint16(vec![0, 1, 2])), PrimitiveType::Triangles);
    let before = geometry.clone();
    assert!(gp::compute_tangent_and_bitangent(&mut geometry).is_none());
    assert_eq!(geometry, before);
}

fn typed(ctor: &str, values: &[f64]) -> TypedArray {
    let datatype = match ctor {
        "Int8Array" => ComponentDatatype::Byte,
        "Uint8Array" => ComponentDatatype::UnsignedByte,
        "Int16Array" => ComponentDatatype::Short,
        "Uint16Array" => ComponentDatatype::UnsignedShort,
        "Int32Array" => ComponentDatatype::Int,
        "Uint32Array" => ComponentDatatype::UnsignedInt,
        "Float32Array" => ComponentDatatype::Float,
        _ => ComponentDatatype::Double,
    };
    TypedArray::from_values(datatype, values)
}

fn typed_name(a: &TypedArray) -> &'static str {
    match a {
        TypedArray::Int8(_) => "Int8Array",
        TypedArray::Uint8(_) => "Uint8Array",
        TypedArray::Int16(_) => "Int16Array",
        TypedArray::Uint16(_) => "Uint16Array",
        TypedArray::Int32(_) => "Int32Array",
        TypedArray::Uint32(_) => "Uint32Array",
        TypedArray::Float32(_) => "Float32Array",
        TypedArray::Float64(_) => "Float64Array",
    }
}

fn datatype(v: &Value) -> ComponentDatatype {
    ComponentDatatype::from_gl(u(v)).expect("component datatype")
}

/// Rebuilds a geometry from the fixtures' `geomIn` data.
fn geometry(v: &Value) -> Geometry {
    let mut attributes = GeometryAttributes::default();
    for a in v["attributes"].as_array().unwrap() {
        let a = a.as_array().unwrap();
        let ctor = s(&a[4]);
        // Plain arrays keep full precision, as Float64.
        let values = typed(if ctor == "Array" { "Float64Array" } else { ctor }, &nums(&a[5]));
        attributes.set(
            s(&a[0]),
            GeometryAttribute {
                component_datatype: datatype(&a[1]),
                components_per_attribute: u(&a[2]) as u8,
                normalize: a[3].as_bool().unwrap(),
                values,
            },
        );
    }
    let indices = v["indices"].as_array().map(|i| {
        let ctor = s(&i[0]);
        typed(if ctor == "Array" { "Uint32Array" } else { ctor }, &nums(&i[1]))
    });
    let primitive_type = PrimitiveType::from_gl(u(&v["primitiveType"])).unwrap();
    let mut g = Geometry::new(attributes, indices, primitive_type);
    g.bounding_sphere = (!v["boundingSphere"].is_null()).then(|| bsphere(&v["boundingSphere"]));
    g.bounding_sphere_cv = (!v["boundingSphereCV"].is_null()).then(|| bsphere(&v["boundingSphereCV"]));
    g.geometry_type = match u(&v["geometryType"]) {
        1 => GeometryType::Triangles,
        2 => GeometryType::Lines,
        3 => GeometryType::Polylines,
        _ => GeometryType::None,
    };
    g.offset_attribute = opt(&v["offsetAttribute"]).and_then(GeometryOffsetAttribute::unpack);
    g
}

fn enc(g: Option<&Geometry>) -> Value {
    let Some(g) = g else { return Value::Null };
    let mut attributes = Map::new();
    for (name, a) in g.attributes.iter() {
        attributes.insert(
            name.to_string(),
            json!([
                a.component_datatype as u32,
                a.components_per_attribute,
                a.normalize,
                typed_name(&a.values),
                a.values.to_f64_vec().enc()
            ]),
        );
    }
    json!({
        "attributes": attributes,
        "indices": g.indices.as_ref().map(|i| json!([typed_name(i), i.to_f64_vec().enc()])),
        "primitiveType": g.primitive_type as u32,
        "boundingSphere": g.bounding_sphere.enc(),
        "boundingSphereCV": g.bounding_sphere_cv.enc(),
        "geometryType": g.geometry_type as u32,
    })
}

fn instance_enc(i: &GeometryInstance) -> Value {
    json!({
        "geometry": enc(i.geometry.as_ref()),
        "west": enc(i.west_hemisphere_geometry.as_ref()),
        "east": enc(i.east_hemisphere_geometry.as_ref()),
        "modelMatrix": i.model_matrix.enc(),
    })
}

fn digest(g: &Geometry) -> Value {
    let mut attributes = Map::new();
    for (name, a) in g.attributes.iter() {
        let v = a.values.to_f64_vec();
        attributes.insert(
            name.to_string(),
            json!([v.len(), v.iter().sum::<f64>(), v[..12.min(v.len())].to_vec().enc()]),
        );
    }
    let idx = g.indices.as_ref().map(|i| i.to_f64_vec()).unwrap_or_default();
    json!({
        "attributes": attributes,
        "indices": [idx.len(), idx.iter().sum::<f64>(), idx[..24.min(idx.len())].to_vec().enc(), idx[idx.len().saturating_sub(24)..].to_vec().enc()],
    })
}

/// Runs an in-place operation and encodes the geometry it leaves.
fn mutate(a: &Value, f: impl FnOnce(&mut Geometry) -> Option<()>) -> Option<Out> {
    let mut g = geometry(a);
    f(&mut g)?;
    ok(enc(Some(&g)))
}

#[test]
fn geometry_pipeline() {
    run(
        "core",
        "GeometryPipeline",
        |f| if f == "fitToUnsignedShortIndicesBig" { TIGHT } else { GEOMETRY },
        |f, a| match f {
            "toWireframe" => mutate(&a[0], gp::to_wireframe),
            "createAttributeLocations" => {
                let locations = gp::create_attribute_locations(&geometry(&a[0]));
                ok(Value::Array(locations.into_iter().map(|(n, l)| json!([n, l])).collect()))
            }
            "reorderForPreVertexCache" => mutate(&a[0], gp::reorder_for_pre_vertex_cache),
            "reorderForPostVertexCache" => {
                let capacity = opt(&a[1]).map(|c| c as usize);
                mutate(&a[0], |g| {
                    gp::reorder_for_post_vertex_cache(g, capacity);
                    Some(())
                })
            }
            "fitToUnsignedShortIndices" => {
                let out = gp::fit_to_unsigned_short_indices(&geometry(&a[0]))?;
                ok(Value::Array(out.iter().map(|g| enc(Some(g))).collect()))
            }
            "fitToUnsignedShortIndicesBig" => {
                let n = n(&a[0]);
                let e = EllipsoidGeometry::new(
                    Cartesian3::new(1.0, 2.0, 3.0),
                    None,
                    0.0,
                    std::f64::consts::TAU,
                    0.0,
                    std::f64::consts::PI,
                    n,
                    n,
                    VertexFormat::POSITION_AND_NORMAL,
                    None,
                )?;
                let out = gp::fit_to_unsigned_short_indices(&e.create_geometry()?)?;
                ok(Value::Array(out.iter().map(digest).collect()))
            }
            "encodeAttribute" => mutate(&a[0], |g| gp::encode_attribute(g, "position", "positionHigh", "positionLow")),
            "compressVertices" => mutate(&a[0], gp::compress_vertices),
            "computeNormal" => mutate(&a[0], gp::compute_normal),
            "computeTangentAndBitangent" => mutate(&a[0], gp::compute_tangent_and_bitangent),
            "createLineSegmentsForVectors" => {
                let name = a[1].as_str();
                ok(enc(Some(&gp::create_line_segments_for_vectors(&geometry(&a[0]), name, opt(&a[2]))?)))
            }
            "projectTo2D" => {
                let geographic = GeographicProjection::default();
                let mercator = WebMercatorProjection::default();
                let projection: Option<&dyn MapProjection> = match a[1].as_str() {
                    Some("geographic") => Some(&geographic),
                    Some(_) => Some(&mercator),
                    None => None,
                };
                mutate(&a[0], |g| gp::project_to_2d(g, "position", "position3D", "position2D", projection))
            }
            "transformToWorldCoordinates" => {
                let mut instance = GeometryInstance::new(geometry(&a[0]), Some(m4(&a[1])));
                gp::transform_to_world_coordinates(&mut instance)?;
                ok(instance_enc(&instance))
            }
            "combineInstances" => {
                let instances: Vec<GeometryInstance> = list(&a[0], |x| GeometryInstance::new(geometry(x), None));
                let out = gp::combine_instances(&instances)?;
                ok(Value::Array(out.iter().map(|g| enc(Some(g))).collect()))
            }
            "splitLongitude" => {
                let mut instance = GeometryInstance::new(geometry(&a[0]), None);
                gp::split_longitude(&mut instance)?;
                ok(instance_enc(&instance))
            }
            _ => None,
        },
    );
}

fn bytes<const K: usize>(v: [u8; K]) -> Vec<f64> {
    v.iter().map(|&b| f64::from(b)).collect()
}

fn floats<const K: usize>(v: [f32; K]) -> Vec<f64> {
    v.iter().map(|&b| f64::from(b)).collect()
}

#[test]
fn geometry_instance_attributes() {
    run("core", "GeometryInstanceAttributes", |_| EXACT, |f, a| match f {
        "color" => ok(bytes(ColorGeometryInstanceAttribute::new(opt(&a[0]), opt(&a[1]), opt(&a[2]), opt(&a[3])).value)),
        "colorToValue" => ok(bytes(ColorGeometryInstanceAttribute::to_value(&Color::new(n(&a[0]), n(&a[1]), n(&a[2]), n(&a[3]))))),
        "show" => ok(bytes(ShowGeometryInstanceAttribute::new(a[0].as_bool()).value)),
        "offset" => {
            let v: Vec<Option<f64>> = list(&a[0], opt);
            ok(floats(OffsetGeometryInstanceAttribute::new(v[0], v[1], v[2]).value))
        }
        "distanceDisplayCondition" => {
            let v: Vec<Option<f64>> = list(&a[0], opt);
            ok(floats(DistanceDisplayConditionGeometryInstanceAttribute::new(v[0], v[1]).value))
        }
        "distanceDisplayConditionToValue" => {
            let v = nums(&a[0]);
            ok(floats(DistanceDisplayConditionGeometryInstanceAttribute::to_value(&DistanceDisplayCondition::new(v[0], v[1]))))
        }
        _ => None,
    });
}

#[test]
fn instance_attributes_convert_to_generic() {
    let a: GeometryInstanceAttribute = ColorGeometryInstanceAttribute::new(Some(1.0), Some(0.0), Some(0.5), None).into();
    assert_eq!(a.component_datatype, ComponentDatatype::UnsignedByte);
    assert_eq!(a.components_per_attribute, 4);
    assert!(a.normalize);
    assert_eq!(a.value, TypedArray::Uint8(vec![255, 0, 128, 255]));
    let s: GeometryInstanceAttribute = ShowGeometryInstanceAttribute::new(Some(false)).into();
    assert_eq!(s.value, TypedArray::Uint8(vec![0]));
}
