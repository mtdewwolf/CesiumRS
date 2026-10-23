//! Parity benchmark workloads: the same inputs, the same calls and the same
//! checksums as `tools/cesium/bench/workloads.mjs`.
//!
//! Fixture workloads read their inputs from `tests/fixtures` (written by real
//! CesiumJS) and check the timed call's output against the recorded result
//! before timing it. Generated workloads read `target/bench-inputs`, written by
//! `tools/cesium/bench/inputs.mjs`; they have no recorded result, so
//! `bench/compare.mjs` checks their checksum against the CesiumJS run.
//!
//! The option decoders below mirror the replay suites in `tests/`. If one
//! drifts from its suite, the fixture check fails before anything is timed.

use crate::common::*;
use crate::harness::{Prepared, Verify};
use base64::Engine as _;
use cesium_rs::core::earcut::earcut;
use cesium_rs::core::heightmap_tessellator::compute_vertices;
use cesium_rs::core::*;
use cesium_rs::workers::draco::{self, AttributeValues, GeometryAttributeType};
use serde_json::{json, Value};

/// Reads a repository-relative file: from disk natively, or from the map the
/// browser host passes to the wasm runner.
pub trait Source {
    fn text(&self, path: &str) -> Result<String, String>;
}

pub struct Workload {
    pub id: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    /// Repository-relative input file.
    pub input: &'static str,
    pub prepare: fn(&dyn Source) -> Result<Prepared, String>,
}

const E: Ellipsoid = Ellipsoid::WGS84;
/// Float32 attributes can land one f32 step apart (the replay suites' GEOMETRY).
const GEOMETRY: Tol = Tol { rel: 1e-7, abs: 1e-9, vector: false };

pub fn all() -> Vec<Workload> {
    vec![
        Workload {
            id: "core.cartographicToCartesian",
            group: "Core math",
            title: "Ellipsoid.cartographicToCartesian",
            input: "tests/fixtures/core/Ellipsoid.json",
            prepare: cartographic_to_cartesian,
        },
        Workload {
            id: "core.cartesianToCartographic",
            group: "Core math",
            title: "Ellipsoid.cartesianToCartographic",
            input: "tests/fixtures/core/Ellipsoid.json",
            prepare: cartesian_to_cartographic,
        },
        Workload {
            id: "core.ellipsoidGeodesic",
            group: "Core math",
            title: "EllipsoidGeodesic + 5 interpolations",
            input: "tests/fixtures/core/Curves.json",
            prepare: ellipsoid_geodesic,
        },
        Workload {
            id: "core.eastNorthUpToFixedFrame",
            group: "Core math",
            title: "Transforms.eastNorthUpToFixedFrame",
            input: "tests/fixtures/core/Transforms.json",
            prepare: east_north_up,
        },
        Workload {
            id: "core.boundingSphereFromPoints",
            group: "Core math",
            title: "BoundingSphere.fromPoints",
            input: "tests/fixtures/core/BoundingSphere.json",
            prepare: bounding_sphere_from_points,
        },
        Workload {
            id: "geometry.earcut",
            group: "Geometry",
            title: "earcut triangulation",
            input: "tests/fixtures/core/Earcut.json",
            prepare: earcut_workload,
        },
        Workload {
            id: "geometry.polygon",
            group: "Geometry",
            title: "PolygonGeometry.createGeometry",
            input: "tests/fixtures/core/PolygonGeometry.json",
            prepare: polygon,
        },
        Workload {
            id: "geometry.corridor",
            group: "Geometry",
            title: "CorridorGeometry.createGeometry",
            input: "tests/fixtures/core/CorridorGeometry.json",
            prepare: corridor,
        },
        Workload {
            id: "geometry.corridorLarge",
            group: "Geometry",
            title: "CorridorGeometry, 20° at 0.00001 rad granularity",
            input: "tests/fixtures/core/CorridorGeometry.json",
            prepare: corridor_large,
        },
        Workload {
            id: "terrain.heightmapFixtures",
            group: "Terrain",
            title: "HeightmapTessellator.computeVertices, fixture grids (2x2 to 6x6)",
            input: "tests/fixtures/core/HeightmapTessellator.json",
            prepare: heightmap_fixtures,
        },
        Workload {
            id: "terrain.heightmap65",
            group: "Terrain",
            title: "HeightmapTessellator.computeVertices, 65x65 Uint16 tile",
            input: "target/bench-inputs/heightmap65.json",
            prepare: |s| heightmap_generated(s, "target/bench-inputs/heightmap65.json"),
        },
        Workload {
            id: "terrain.heightmap257",
            group: "Terrain",
            title: "HeightmapTessellator.computeVertices, 257x257 Uint16 tile",
            input: "target/bench-inputs/heightmap257.json",
            prepare: |s| heightmap_generated(s, "target/bench-inputs/heightmap257.json"),
        },
        Workload {
            id: "workers.dracoMesh5k",
            group: "Workers",
            title: "Draco mesh decode, 5,000 triangles",
            input: "target/bench-inputs/draco5k.json",
            prepare: |s| draco_mesh(s, "target/bench-inputs/draco5k.json"),
        },
        Workload {
            id: "workers.dracoMesh130k",
            group: "Workers",
            title: "Draco mesh decode, 130,050 triangles",
            input: "target/bench-inputs/draco130k.json",
            prepare: |s| draco_mesh(s, "target/bench-inputs/draco130k.json"),
        },
    ]
}

// ---- loading -----------------------------------------------------------------

fn load_json(source: &dyn Source, path: &str) -> Result<Value, String> {
    serde_json::from_str(&source.text(path)?).map_err(|e| format!("{path}: {e}"))
}

/// Cases of one fixture function, without recorded throws (both sides skip them).
fn fixture_cases(source: &dyn Source, path: &str, function: &str) -> Result<Vec<Case>, String> {
    let doc = load_json(source, path)?;
    let cases: Vec<Case> = doc["cases"]
        .as_array()
        .ok_or_else(|| format!("{path}: no cases"))?
        .iter()
        .filter(|c| c["f"] == function && c["r"].get("throws").is_none())
        .map(|c| Case { f: function.to_string(), a: c["a"].as_array().cloned().unwrap_or_default(), r: c["r"].clone() })
        .collect();
    if cases.is_empty() {
        return Err(format!("{path}: no {function} cases"));
    }
    Ok(cases)
}

/// Compares each output with its expected value; keeps the first few misses.
fn check(pairs: impl Iterator<Item = (Value, Value)>, tol: Tol) -> Verify {
    let mut checked = 0;
    let mut mismatches = Vec::new();
    for (i, (got, expected)) in pairs.enumerate() {
        checked += 1;
        if !close(&got, &expected, tol) {
            let got = got.to_string();
            mismatches.push(format!("case {i}: got {}", &got[..got.len().min(160)]));
        }
    }
    Verify { checked, mismatches }
}

fn typed_sum(values: &TypedArray) -> f64 {
    fn sum<T: Copy + Into<f64>>(values: &[T]) -> f64 {
        values.iter().fold(0.0, |s, &v| s + v.into())
    }
    match values {
        TypedArray::Int8(v) => sum(v),
        TypedArray::Uint8(v) => sum(v),
        TypedArray::Int16(v) => sum(v),
        TypedArray::Uint16(v) => sum(v),
        TypedArray::Int32(v) => sum(v),
        TypedArray::Uint32(v) => sum(v),
        TypedArray::Float32(v) => sum(v),
        TypedArray::Float64(v) => sum(v),
    }
}

/// Sum of the position values plus the index count (0 for no geometry).
fn geometry_checksum(geometry: &Option<Geometry>) -> f64 {
    let Some(g) = geometry else { return 0.0 };
    let positions = g.attributes.position.as_ref().map_or(0.0, |p| typed_sum(&p.values));
    positions + g.indices.as_ref().map_or(0.0, |i| i.len() as f64)
}

// ---- core math -----------------------------------------------------------------

fn cartographic_to_cartesian(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/Ellipsoid.json", "cartographicToCartesian")?;
    let inputs: Vec<Cartographic> = cases.iter().map(|c| carto(&c.a[0])).collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for c in std::hint::black_box(&run_inputs) {
                let p = E.cartographic_to_cartesian(c);
                s += p.x + p.y + p.z;
            }
            s
        }),
        verify: Box::new(move || {
            check(
                inputs.iter().zip(&cases).map(|(c, case)| (E.cartographic_to_cartesian(c).enc(), case.r.clone())),
                TIGHT,
            )
        }),
    })
}

fn cartesian_to_cartographic(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/Ellipsoid.json", "cartesianToCartographic")?;
    let inputs: Vec<Cartesian3> = cases.iter().map(|c| c3(&c.a[0])).collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for &p in std::hint::black_box(&run_inputs) {
                if let Some(c) = E.cartesian_to_cartographic(p) {
                    s += c.longitude + c.latitude + c.height;
                }
            }
            s
        }),
        verify: Box::new(move || {
            check(
                inputs.iter().zip(&cases).map(|(&p, case)| (E.cartesian_to_cartographic(p).enc(), case.r.clone())),
                TIGHT,
            )
        }),
    })
}

const FRACTIONS: [f64; 5] = [0.0, 0.25, 0.5, 0.9, 1.0];

fn ellipsoid_geodesic(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/Curves.json", "EllipsoidGeodesic")?;
    let inputs: Vec<(Cartographic, Cartographic)> = cases.iter().map(|c| (carto(&c.a[0]), carto(&c.a[1]))).collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for (a, b) in std::hint::black_box(&run_inputs) {
                if let Some(g) = EllipsoidGeodesic::new(a, b, &E) {
                    s += g.surface_distance + g.start_heading + g.end_heading;
                    for t in FRACTIONS {
                        let p = g.interpolate_using_fraction(t);
                        s += p.longitude + p.latitude;
                    }
                }
            }
            s
        }),
        verify: Box::new(move || {
            let outputs = inputs.iter().map(|(a, b)| match EllipsoidGeodesic::new(a, b, &E) {
                Some(g) => {
                    let mut out = vec![g.surface_distance.enc(), g.start_heading.enc(), g.end_heading.enc()];
                    out.extend(FRACTIONS.iter().map(|&t| g.interpolate_using_fraction(t).enc()));
                    json!(out)
                }
                None => Value::Null,
            });
            check(outputs.zip(cases.iter().map(|c| c.r.clone())), TIGHT)
        }),
    })
}

fn east_north_up(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/Transforms.json", "eastNorthUpToFixedFrame")?;
    let inputs: Vec<Cartesian3> = cases.iter().map(|c| c3(&c.a[0])).collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for &p in std::hint::black_box(&run_inputs) {
                s += Transforms::east_north_up_to_fixed_frame(p, &E).0.iter().sum::<f64>();
            }
            s
        }),
        verify: Box::new(move || {
            check(
                inputs
                    .iter()
                    .zip(&cases)
                    .map(|(&p, case)| (Transforms::east_north_up_to_fixed_frame(p, &E).enc(), case.r.clone())),
                TIGHT,
            )
        }),
    })
}

fn bounding_sphere_from_points(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/BoundingSphere.json", "fromPoints")?;
    let inputs: Vec<Vec<Cartesian3>> = cases.iter().map(|c| list(&c.a[0], c3)).collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for points in std::hint::black_box(&run_inputs) {
                let b = BoundingSphere::from_points(points);
                s += b.center.x + b.center.y + b.center.z + b.radius;
            }
            s
        }),
        verify: Box::new(move || {
            check(
                inputs.iter().zip(&cases).map(|(p, case)| (BoundingSphere::from_points(p).enc(), case.r.clone())),
                TIGHT,
            )
        }),
    })
}

// ---- geometry ------------------------------------------------------------------

fn earcut_workload(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/Earcut.json", "earcut")?;
    let inputs: Vec<(Vec<f64>, Vec<usize>, usize)> = cases
        .iter()
        .map(|c| (nums(&c.a[0]), nums(&c.a[1]).into_iter().map(|x| x as usize).collect(), n(&c.a[2]) as usize))
        .collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for (data, holes, dim) in std::hint::black_box(&run_inputs) {
                s += earcut(data, holes, *dim).iter().sum::<usize>() as f64;
            }
            s
        }),
        verify: Box::new(move || {
            let outputs =
                inputs.iter().map(|(d, h, k)| earcut(d, h, *k).iter().map(|&i| i as f64).collect::<Vec<_>>().enc());
            check(outputs.zip(cases.iter().map(|c| c.r.clone())), EXACT)
        }),
    })
}

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

fn hierarchy(v: &Value) -> PolygonHierarchy {
    PolygonHierarchy::new(
        list(&v["positions"], c3),
        v["holes"].as_array().map_or(Vec::new(), |h| h.iter().map(hierarchy).collect()),
    )
}

fn texture_hierarchy(v: &Value) -> TextureCoordinateHierarchy {
    TextureCoordinateHierarchy {
        positions: list(&v["positions"], c2),
        holes: v["holes"].as_array().map_or(Vec::new(), |h| h.iter().map(texture_hierarchy).collect()),
    }
}

fn arc_type(v: &Value) -> ArcType {
    match u(v) {
        0 => ArcType::None,
        2 => ArcType::Rhumb,
        _ => ArcType::Geodesic,
    }
}

/// `polygonOptions` in tools/cesium/fixtures/core-surfaces.mjs (tests/core_surfaces.rs).
fn polygon_options(a: &[Value]) -> PolygonGeometryOptions {
    let mut o = PolygonGeometryOptions::new(hierarchy(&a[0]));
    o.ellipsoid = ellipsoid(&a[1]);
    if let Some(h) = opt(&a[2]) {
        o.height = h;
    }
    o.extruded_height = opt(&a[3]);
    o.st_rotation = n(&a[4]);
    if let Some(g) = opt(&a[5]) {
        o.granularity = g;
    }
    o.vertex_format = format(&a[6]);
    o.per_position_height = a[7].as_bool().unwrap();
    o.close_top = a[8].as_bool().unwrap();
    o.close_bottom = a[9].as_bool().unwrap();
    o.arc_type = arc_type(&a[10]);
    o.shadow_volume = a[11].as_bool().unwrap();
    o.offset_attribute = opt(&a[12]).and_then(GeometryOffsetAttribute::unpack);
    o.texture_coordinates = (!a[13].is_null()).then(|| texture_hierarchy(&a[13]));
    o
}

fn polygon(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/PolygonGeometry.json", "polygon")?;
    let inputs: Vec<PolygonGeometryOptions> = cases.iter().map(|c| polygon_options(&c.a)).collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for o in std::hint::black_box(&run_inputs) {
                s += geometry_checksum(&PolygonGeometry::new(o.clone()).create_geometry());
            }
            s
        }),
        verify: Box::new(move || {
            let outputs = inputs.iter().map(|o| geometry_enc(PolygonGeometry::new(o.clone()).create_geometry()));
            check(outputs.zip(cases.iter().map(|c| c.r[3].clone())), GEOMETRY)
        }),
    })
}

fn corner(v: &Value) -> CornerType {
    match u(v) {
        1 => CornerType::Mitered,
        2 => CornerType::Beveled,
        _ => CornerType::Rounded,
    }
}

/// `filledOptions` in tools/cesium/fixtures/core-corridors.mjs (tests/core_corridors.rs).
fn corridor_options(a: &[Value]) -> CorridorGeometryOptions {
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

fn corridor(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/CorridorGeometry.json", "filled")?;
    let inputs: Vec<CorridorGeometryOptions> = cases.iter().map(|c| corridor_options(&c.a)).collect();
    let run_inputs = inputs.clone();
    Ok(Prepared {
        cases: inputs.len(),
        run: Box::new(move || {
            let mut s = 0.0;
            for o in std::hint::black_box(&run_inputs) {
                s += geometry_checksum(&CorridorGeometry::new(o.clone()).create_geometry());
            }
            s
        }),
        verify: Box::new(move || {
            let outputs = inputs.iter().map(|o| geometry_enc(CorridorGeometry::new(o.clone()).create_geometry()));
            check(outputs.zip(cases.iter().map(|c| c.r[1].clone())), GEOMETRY)
        }),
    })
}

fn corridor_large(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/CorridorGeometry.json", "largeFilled")?;
    let options = || {
        let mut o = CorridorGeometryOptions::new(Cartesian3::from_degrees_array(&[0.0, 0.0, 20.0, 0.0], &E), 1000.0);
        o.granularity = 0.00001;
        o.corner_type = CornerType::Mitered;
        o.vertex_format = VertexFormat::POSITION_ONLY;
        o
    };
    Ok(Prepared {
        cases: 1,
        run: Box::new(move || {
            geometry_checksum(&CorridorGeometry::new(std::hint::black_box(options())).create_geometry())
        }),
        verify: Box::new(move || {
            let g = CorridorGeometry::new(options()).create_geometry();
            let out = g.as_ref().and_then(|g| {
                let indices = g.indices.as_ref()?;
                let values = indices.to_f64_vec();
                Some(json!([
                    g.attributes.position.as_ref()?.values.len() / 3,
                    typed_array_name(indices),
                    values.len(),
                    values[values.len() - 12..]
                ]))
            });
            check(std::iter::once((out.unwrap_or(Value::Null), cases[0].r.clone())), EXACT)
        }),
    })
}

// ---- terrain -------------------------------------------------------------------

/// Heightmap options from the fixture's plain-data form (tests/core_terrain.rs).
/// The sample buffer is leaked: options borrow it for the life of the run.
fn heightmap_options(o: &Value) -> HeightmapTessellatorOptions<'static> {
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
    let data: &'static TypedArray = Box::leak(Box::new(TypedArray::from_values(datatype, &nums(&o["data"]))));
    let mut options = HeightmapTessellatorOptions::new(
        data,
        u(&o["width"]) as usize,
        u(&o["height"]) as usize,
        rect(&o["native"]),
        n(&o["skirtHeight"]),
    );
    options.rectangle = (!o["rectangle"].is_null()).then(|| rect(&o["rectangle"]));
    options.relative_to_center = (!o["center"].is_null()).then(|| c3(&o["center"]));
    options.is_geographic = o["isGeographic"].as_bool().unwrap_or(true);
    options.ellipsoid = if o["ellipsoid"].as_str() == Some("MOON") { Ellipsoid::MOON } else { Ellipsoid::WGS84 };
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
    options
}

/// Sum of the vertex buffer plus the height range (0 when Cesium returns nothing).
fn heightmap_checksum(options: &HeightmapTessellatorOptions) -> f64 {
    compute_vertices(options)
        .map_or(0.0, |r| r.vertices.iter().fold(0.0, |s, &v| s + v as f64) + r.minimum_height + r.maximum_height)
}

fn heightmap_run(inputs: Vec<HeightmapTessellatorOptions<'static>>) -> Box<dyn FnMut() -> f64> {
    Box::new(move || std::hint::black_box(&inputs).iter().map(heightmap_checksum).sum())
}

fn heightmap_fixtures(source: &dyn Source) -> Result<Prepared, String> {
    let cases = fixture_cases(source, "tests/fixtures/core/HeightmapTessellator.json", "computeVertices")?;
    let inputs: Vec<_> = cases.iter().map(|c| heightmap_options(&c.a[0])).collect();
    Ok(Prepared {
        cases: inputs.len(),
        run: heightmap_run(inputs.clone()),
        verify: Box::new(move || {
            let pairs = inputs.iter().zip(&cases).map(|(o, case)| {
                let got = compute_vertices(o).map_or(Value::Null, |r| {
                    json!({
                        "vertices": r.vertices.iter().map(|&v| v as f64).collect::<Vec<_>>().enc(),
                        "minimumHeight": r.minimum_height.enc(),
                        "maximumHeight": r.maximum_height.enc(),
                    })
                });
                let expected = json!({
                    "vertices": case.r["vertices"],
                    "minimumHeight": case.r["minimumHeight"],
                    "maximumHeight": case.r["maximumHeight"],
                });
                (got, expected)
            });
            check(pairs, TIGHT)
        }),
    })
}

fn heightmap_generated(source: &dyn Source, path: &str) -> Result<Prepared, String> {
    let doc = load_json(source, path)?;
    let options = heightmap_options(&doc["options"]);
    Ok(Prepared {
        cases: 1,
        run: heightmap_run(vec![options]),
        verify: Box::new(|| Verify { checked: 0, mismatches: Vec::new() }),
    })
}

// ---- workers -------------------------------------------------------------------

fn attribute_sum(values: &AttributeValues) -> f64 {
    match values {
        AttributeValues::F32(v) => v.iter().fold(0.0, |s, &x| s + x as f64),
        _ => f64::NAN,
    }
}

/// Decodes the whole mesh, as the `decodeDraco` worker does for the attributes
/// it is asked for (every attribute, in the generated inputs).
fn draco_mesh(source: &dyn Source, path: &str) -> Result<Prepared, String> {
    let doc = load_json(source, path)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s(&doc["base64"]))
        .map_err(|e| format!("{path}: base64: {e}"))?;
    let decode = move || {
        draco::decode_triangular_mesh(std::hint::black_box(&bytes)).map_or(f64::NAN, |mesh| {
            let indices = mesh.indices.iter().fold(0.0, |s, &i| s + i as f64);
            let positions =
                mesh.named_attribute(GeometryAttributeType::Position).map_or(f64::NAN, |a| attribute_sum(&a.values));
            indices + positions
        })
    };
    Ok(Prepared { cases: 1, run: Box::new(decode), verify: Box::new(|| Verify { checked: 0, mismatches: Vec::new() }) })
}
