//! Terrain picker refinement, worker distribution and mesh lifecycle parity.
mod common;
use cesium_rs::core::*;
use cesium_rs::core::{terrain_picker, terrain_provider};
use common::*;
use serde_json::{json, Value};

fn encoding(o: &Value) -> TerrainEncoding {
    TerrainEncoding::new(TerrainEncodingOptions {
        center: Some(c3(&o["center"])),
        axis_aligned_bounding_box: (!o["box"].is_null())
            .then(|| AxisAlignedBoundingBox::from_corners(c3(&o["box"][0]), c3(&o["box"][1]))),
        minimum_height: opt(&o["minimum"]),
        maximum_height: opt(&o["maximum"]),
        from_enu: (!o["frame"].is_null()).then(|| m4(&o["frame"])),
        has_vertex_normals: false,
        has_web_mercator_t: false,
        has_geodetic_surface_normals: o["geo"].as_bool().unwrap_or(false),
        exaggeration: opt(&o["exaggeration"]).unwrap_or(1.0),
        exaggeration_relative_height: opt(&o["relative"]).unwrap_or(0.0),
    })
    .unwrap()
}
fn encode(e: &TerrainEncoding, points: &Value) -> Vec<f32> {
    let points = points.as_array().unwrap();
    let mut buffer = vec![0.0; e.stride * points.len()];
    for (i, p) in points.iter().enumerate() {
        let mut v = TerrainVertex::new(c3(&p[0]), c2(&p[1]), n(&p[2]));
        v.geodetic_surface_normal = Some(c3(&p[3]));
        e.encode(&mut buffer, i * e.stride, &v).unwrap();
    }
    buffer
}
fn projection(merc: bool) -> Box<dyn MapProjection> {
    if merc {
        Box::new(WebMercatorProjection::new(Ellipsoid::WGS84))
    } else {
        Box::new(GeographicProjection::new(Ellipsoid::WGS84))
    }
}
fn shape(p: &TerrainPicker) -> Value {
    json!([p.needs_rebuild, p.node_count()])
}
fn summary(m: &TerrainMesh) -> Value {
    json!([m.oriented_bounding_box.map(|b| json!([b.center.enc(), b.half_axes.enc()])), shape(m.picker())])
}

#[test]
fn terrain_picker() {
    run(
        "core",
        "TerrainPicker",
        |_| TIGHT,
        |f, a| match f {
            "pick" => {
                let e = encoding(&a[0]);
                let vertices = encode(&e, &a[1]);
                let indices = TypedArray::Uint32(nums(&a[2]).into_iter().map(|n| n as u32).collect());
                let transform = m4(&a[3]);
                let projection = projection(false);
                let mut p = TerrainPicker::new();
                let mut results = vec![shape(&p)];
                for operation in a[4].as_array().unwrap() {
                    if operation[3].as_bool().unwrap() {
                        p.needs_rebuild = true;
                    }
                    let ray = Ray::new(c3(&operation[0]), c3(&operation[1]));
                    let hit = p.ray_intersect(
                        TerrainPickMesh { vertices: &vertices, indices: &indices, encoding: &e },
                        TerrainPickContext {
                            ray: &ray,
                            tile_transform: &transform,
                            cull_back_faces: operation[2].as_bool().unwrap(),
                            mode: Some(TerrainSceneMode::Scene3D),
                            projection: projection.as_ref(),
                        },
                    );
                    results.push(json!([hit.enc(), shape(&p)]));
                }
                ok(results)
            }
            _ => None,
        },
    );
}

#[test]
fn terrain_mesh() {
    run(
        "core",
        "TerrainMesh",
        |_| TIGHT,
        |f, a| match f {
            "mesh" => {
                let e = encoding(&a[0]);
                let vertices = encode(&e, &a[1]);
                let topology = terrain_provider::get_regular_grid_indices_and_edge_indices(3, 3)?;
                let mut m = TerrainMesh::new(TerrainMeshOptions {
                    center: c3(&a[0]["center"]),
                    vertices,
                    indices: topology.indices.as_ref().clone(),
                    index_count_without_skirts: Some(24),
                    vertex_count_without_skirts: 9,
                    minimum_height: n(&a[0]["minimum"]),
                    maximum_height: n(&a[0]["maximum"]),
                    rectangle: rect(&a[2]),
                    bounding_sphere_3d: BoundingSphere::new(c3(&a[0]["center"]), 50000.0),
                    occludee_point_in_scaled_space: Some(Cartesian3::new(1.0, 2.0, 3.0)),
                    vertex_stride: (!a[5].as_bool().unwrap()).then_some(e.stride),
                    oriented_bounding_box: a[4].as_bool().unwrap().then(OrientedBoundingBox::default),
                    encoding: e,
                    edges: topology.edges.clone(),
                });
                let proj = projection(a[3].as_bool().unwrap());
                let mut results = vec![
                    json!([
                        m.center.enc(),
                        m.stride,
                        m.indices.to_f64_vec().enc(),
                        m.index_count_without_skirts,
                        m.vertex_count_without_skirts,
                        m.minimum_height,
                        m.maximum_height,
                        m.rectangle.enc(),
                        m.bounding_sphere_3d.enc(),
                        m.occludee_point_in_scaled_space.enc(),
                        [
                            m.edges.west_indices_south_to_north,
                            m.edges.south_indices_east_to_west,
                            m.edges.east_indices_north_to_south,
                            m.edges.north_indices_west_to_east
                        ]
                    ]),
                    m.get_transform(None, proj.as_ref())?.enc(),
                ];
                for mode in [
                    TerrainSceneMode::Scene3D,
                    TerrainSceneMode::Scene3D,
                    TerrainSceneMode::Scene3D,
                    TerrainSceneMode::Scene3D,
                    TerrainSceneMode::Scene2D,
                    TerrainSceneMode::Scene2D,
                    TerrainSceneMode::ColumbusView,
                    TerrainSceneMode::Morphing,
                    TerrainSceneMode::Scene3D,
                ] {
                    let target = m.encoding.get_exaggerated_position(&m.vertices, 4)?;
                    let (origin, direction) = if mode == TerrainSceneMode::Scene3D {
                        let normal = Ellipsoid::WGS84.geodetic_surface_normal(target)?;
                        (target.add(normal.multiply_by_scalar(10000.0)), normal.negate())
                    } else {
                        let p = proj.project(&Ellipsoid::WGS84.cartesian_to_cartographic(target)?);
                        (
                            Cartesian3::new(
                                p.z + 10000.0,
                                p.x + if !a[3].as_bool().unwrap() {
                                    std::f64::consts::TAU * Ellipsoid::WGS84.maximum_radius
                                } else {
                                    0.0
                                },
                                p.y,
                            ),
                            Cartesian3::new(-1.0, 0.0, 0.0),
                        )
                    };
                    let hit = m.pick(&Ray::new(origin, direction), a[3].as_bool().unwrap(), Some(mode), proj.as_ref());
                    results.push(json!([m.get_transform(Some(mode), proj.as_ref())?.enc(), hit.enc(), summary(&m)]));
                }
                m.encoding.exaggeration = 4.0;
                m.encoding.exaggeration_relative_height = -20.0;
                m.update_exaggeration();
                results
                    .push(json!([m.get_transform(Some(TerrainSceneMode::Scene3D), proj.as_ref())?.enc(), summary(&m)]));
                m.update_scene_mode();
                results
                    .push(json!([m.get_transform(Some(TerrainSceneMode::Scene2D), proj.as_ref())?.enc(), summary(&m)]));
                ok(results)
            }
            _ => None,
        },
    );
}

#[test]
fn incrementally_build_terrain_picker() {
    run(
        "core",
        "incrementallyBuildTerrainPicker",
        |_| EXACT,
        |f, a| match f {
            "distribute" => {
                let boxes: Vec<_> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| AxisAlignedBoundingBox::from_corners(c3(&b[0]), c3(&b[1])))
                    .collect();
                let triangles: Vec<_> =
                    a[2].as_array().unwrap().iter().map(|t| [c3(&t[0]), c3(&t[1]), c3(&t[2])]).collect();
                let indices: Vec<_> = (0..triangles.len()).map(|i| i as u32 + 10).collect();
                ok(terrain_picker::distribute_triangles(&boxes, &m4(&a[1]), &indices, &triangles)?)
            }
            _ => None,
        },
    );
}

#[test]
fn picker_rejects_invalid_buffers_and_can_retry_after_a_singular_transform() {
    let encoding = TerrainEncoding::new(TerrainEncodingOptions::new(Cartesian3::ZERO)).unwrap();
    let vertices = vec![0.0; encoding.stride * 3];
    let bad_indices = TypedArray::Uint32(vec![0, 1, 99]);
    let projection = GeographicProjection::new(Ellipsoid::WGS84);
    let ray = Ray::new(Cartesian3::new(0.0, 0.0, 1.0), Cartesian3::new(0.0, 0.0, -1.0));
    let mut picker = TerrainPicker::new();
    let mut context = TerrainPickContext {
        ray: &ray,
        tile_transform: &Matrix4::ZERO,
        cull_back_faces: false,
        mode: Some(TerrainSceneMode::Scene3D),
        projection: &projection,
    };
    let mesh = TerrainPickMesh { vertices: &vertices, indices: &bad_indices, encoding: &encoding };
    assert!(picker.ray_intersect(mesh, context).is_none());
    assert!(picker.needs_rebuild);
    context.tile_transform = &Matrix4::IDENTITY;
    assert!(picker.ray_intersect(mesh, context).is_none());
    assert_eq!(picker.node_count(), 1);
    assert!(terrain_picker::distribute_triangles(&[], &Matrix4::IDENTITY, &[0], &[]).is_none());
}
