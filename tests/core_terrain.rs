//! Terrain encodings, full buffers and layout conversions against CesiumJS.
mod common;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

fn create(o: &Value) -> TerrainEncoding {
    let options = TerrainEncodingOptions {
        center: (!o["center"].is_null()).then(|| c3(&o["center"])),
        axis_aligned_bounding_box: (!o["box"].is_null())
            .then(|| AxisAlignedBoundingBox::from_corners(c3(&o["box"][0]), c3(&o["box"][1]))),
        minimum_height: opt(&o["heights"][0]),
        maximum_height: opt(&o["heights"][1]),
        from_enu: (!o["frame"].is_null()).then(|| m4(&o["frame"])),
        has_vertex_normals: o["normals"].as_bool().unwrap_or(false),
        has_web_mercator_t: o["merc"].as_bool().unwrap_or(false),
        has_geodetic_surface_normals: o["geo"].as_bool().unwrap_or(false),
        exaggeration: opt(&o["exaggeration"]).unwrap_or(1.0),
        exaggeration_relative_height: opt(&o["relative"]).unwrap_or(0.0),
    };
    TerrainEncoding::new(options).unwrap()
}

fn metadata(e: &TerrainEncoding) -> Value {
    let attributes: Vec<_> = e
        .get_attributes()
        .iter()
        .map(|a| {
            json!([
                a.index,
                a.component_datatype as u32,
                a.components_per_attribute,
                a.offset_in_bytes,
                a.stride_in_bytes
            ])
        })
        .collect();
    let locations: serde_json::Map<String, Value> =
        e.get_attribute_locations().iter().map(|(name, i)| ((*name).into(), json!(i))).collect();
    json!({
        "quantization": e.quantization as u8, "minimumHeight": e.minimum_height.enc(), "maximumHeight": e.maximum_height.enc(),
        "center": e.center.enc(), "toScaledENU": e.to_scaled_enu.enc(), "fromScaledENU": e.from_scaled_enu.enc(), "matrix": e.matrix.enc(),
        "hasVertexNormals": e.has_vertex_normals, "hasWebMercatorT": e.has_web_mercator_t, "hasGeodeticSurfaceNormals": e.has_geodetic_surface_normals,
        "exaggeration": e.exaggeration, "exaggerationRelativeHeight": e.exaggeration_relative_height, "stride": e.stride, "attributes": attributes, "locations": locations,
    })
}

fn buffer_enc(buffer: &[f32]) -> Value {
    buffer.iter().map(|v| *v as f64).collect::<Vec<_>>().enc()
}

fn encode(e: &TerrainEncoding, vertices: &Value) -> Vec<f32> {
    let vertices = vertices.as_array().unwrap();
    let mut out = vec![0.0; e.stride * vertices.len()];
    let mut i = 0;
    for v in vertices {
        let mut vertex = TerrainVertex::new(c3(&v[0]), c2(&v[1]), n(&v[2]));
        vertex.normal_to_pack = Some(c2(&v[3]));
        vertex.web_mercator_t = Some(n(&v[4]));
        vertex.geodetic_surface_normal = Some(c3(&v[5]));
        i = e.encode(&mut out, i, &vertex).unwrap();
    }
    out
}

fn decode(e: &TerrainEncoding, buffer: &[f32]) -> Value {
    (0..buffer.len() / e.stride).map(|i| json!({
        "position": e.decode_position(buffer,i).enc(), "uv": e.decode_texture_coordinates(buffer,i).enc(), "height": e.decode_height(buffer,i).enc(),
        "merc": e.decode_web_mercator_t(buffer,i).enc(), "oct": e.get_oct_encoded_normal(buffer,i).enc(), "normal": e.decode_normal(buffer,i).enc(),
        "geo": e.decode_geodetic_surface_normal(buffer,i).enc(), "exaggerated": e.get_exaggerated_position(buffer,i).enc(),
    })).collect::<Vec<_>>().enc()
}

#[test]
fn terrain_encoding() {
    run(
        "core",
        "TerrainEncoding",
        |_| TIGHT,
        |f, a| match f {
            "constructor" => ok(metadata(&create(&a[0]))),
            "codec" => {
                let e = create(&a[0]);
                let b = encode(&e, &a[1]);
                ok(json!([metadata(&e), buffer_enc(&b), decode(&e, &b), metadata(&e.clone())]))
            }
            "surfaceNormals" => {
                let mut e = create(&a[0]);
                let old = encode(&e, &a[1]);
                let count = old.len() / e.stride;
                let mut b = vec![0.0; (e.stride + 3) * count];
                e.add_geodetic_surface_normals(&old, &mut b, &Ellipsoid::WGS84)?;
                let added = json!([metadata(&e), buffer_enc(&b), decode(&e, &b)]);
                let mut no_op = vec![-7.0; b.len()];
                e.add_geodetic_surface_normals(&b, &mut no_op, &Ellipsoid::WGS84)?;
                let mut removed = vec![0.0; (e.stride - 3) * count];
                e.remove_geodetic_surface_normals(&b, &mut removed)?;
                let mut no_op_remove = vec![-7.0; removed.len()];
                e.remove_geodetic_surface_normals(&removed, &mut no_op_remove)?;
                ok(json!([added, metadata(&e), buffer_enc(&removed), buffer_enc(&no_op), buffer_enc(&no_op_remove)]))
            }
            _ => None,
        },
    );
}

#[test]
fn invalid_writes_and_layout_conversions_are_atomic() {
    let mut o = TerrainEncodingOptions::new(Cartesian3::new(6378137.0, 0.0, 0.0));
    o.has_vertex_normals = true;
    let mut e = TerrainEncoding::new(o).unwrap();
    let before = e.clone();
    let vertex = TerrainVertex::new(Cartesian3::new(6378137.0, 1.0, 2.0), Cartesian2::ZERO, 3.0);
    let mut out = vec![-7.0; e.stride];
    assert!(e.encode(&mut out, 0, &vertex).is_none());
    assert!(e.encode(&mut out, usize::MAX, &vertex).is_none());
    assert_eq!(out, vec![-7.0; e.stride]);
    let old = vec![0.0; e.stride];
    assert!(e.add_geodetic_surface_normals(&old, &mut out, &Ellipsoid::WGS84).is_none());
    assert_eq!(e, before);
    assert_eq!(out, vec![-7.0; e.stride]);
    assert!(e.decode_position(&[], 0).is_none());
    assert!(e.decode_height(&old, usize::MAX).is_none());
    assert!(e.decode_web_mercator_t(&old, 0).is_none());
    assert!(e.decode_geodetic_surface_normal(&old, 0).is_none());
    e.has_geodetic_surface_normals = true;
    e.calculate_stride_and_offsets();
    let before = e.clone();
    assert!(e.remove_geodetic_surface_normals(&vec![0.0; e.stride], &mut []).is_none());
    assert_eq!(e, before);
}

#[test]
fn heightmap_tessellator() {
    use cesium_rs::core::heightmap_tessellator::compute_vertices;
    run(
        "core",
        "HeightmapTessellator",
        |_| TIGHT,
        |f, a| match f {
            "computeVertices" => {
                let o = &a[0];
                let datatype = match s(&o["type"]) {
                    "Int8Array" => ComponentDatatype::Byte,
                    "Uint8Array" => ComponentDatatype::UnsignedByte,
                    "Int16Array" => ComponentDatatype::Short,
                    "Uint16Array" => ComponentDatatype::UnsignedShort,
                    "Int32Array" => ComponentDatatype::Int,
                    "Uint32Array" => ComponentDatatype::UnsignedInt,
                    "Float32Array" => ComponentDatatype::Float,
                    _ => ComponentDatatype::Double,
                };
                let data = TypedArray::from_values(datatype, &nums(&o["data"]));
                let mut options = HeightmapTessellatorOptions::new(
                    &data,
                    u(&o["width"]) as usize,
                    u(&o["height"]) as usize,
                    rect(&o["native"]),
                    n(&o["skirtHeight"]),
                );
                options.rectangle = (!o["rectangle"].is_null()).then(|| rect(&o["rectangle"]));
                options.relative_to_center = (!o["center"].is_null()).then(|| c3(&o["center"]));
                options.is_geographic = o["isGeographic"].as_bool().unwrap_or(true);
                options.ellipsoid =
                    if o["ellipsoid"].as_str() == Some("MOON") { Ellipsoid::MOON } else { Ellipsoid::WGS84 };
                options.include_web_mercator_t = o["includeWebMercatorT"].as_bool().unwrap_or(false);
                options.exaggeration = opt(&o["exaggeration"]).unwrap_or(1.0);
                options.exaggeration_relative_height = opt(&o["exaggerationRelativeHeight"]).unwrap_or(0.0);
                let st = &o["structure"];
                options.structure.height_scale = opt(&st["heightScale"]).unwrap_or(1.0);
                options.structure.height_offset = opt(&st["heightOffset"]).unwrap_or(0.0);
                options.structure.elements_per_height = opt(&st["elementsPerHeight"]).unwrap_or(1.0) as usize;
                options.structure.stride = opt(&st["stride"]).unwrap_or(1.0) as usize;
                options.structure.element_multiplier = opt(&st["elementMultiplier"]).unwrap_or(256.0);
                options.structure.is_big_endian = st["isBigEndian"].as_bool().unwrap_or(false);
                let result = compute_vertices(&options)?;
                let bbox =
                    result.oriented_bounding_box.map(|b| json!({"center":b.center.enc(),"halfAxes":b.half_axes.enc()}));
                ok(json!({
                    "vertices":buffer_enc(&result.vertices), "minimumHeight":result.minimum_height.enc(), "maximumHeight":result.maximum_height.enc(),
                    "encoding":metadata(&result.encoding), "boundingSphere3D":result.bounding_sphere_3d.enc(), "orientedBoundingBox":bbox,
                    "occludeePointInScaledSpace":result.occludee_point_in_scaled_space.enc(), "decoded":decode(&result.encoding,&result.vertices),
                }))
            }
            _ => None,
        },
    );
}

#[test]
fn invalid_heightmap_layouts_return_none() {
    use cesium_rs::core::heightmap_tessellator::compute_vertices;
    let data = TypedArray::Float64(vec![1.0, 2.0, 3.0, 4.0]);
    let mut options = HeightmapTessellatorOptions::new(&data, 2, 2, Rectangle::new(10.0, 20.0, 11.0, 21.0), 0.0);
    assert!(compute_vertices(&options).is_some());
    options.width = 1;
    assert!(compute_vertices(&options).is_none());
    options.width = 3;
    assert!(compute_vertices(&options).is_none());
    options.width = 2;
    options.structure.stride = 0;
    assert!(compute_vertices(&options).is_none());
    options.structure.stride = 1;
    options.structure.elements_per_height = 0;
    assert!(compute_vertices(&options).is_none());
}
