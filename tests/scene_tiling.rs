//! Tile bounding volumes, bounding volume semantics and implicit tiling
//! replayed against the pinned CesiumJS fixtures.
//!
//! Script results are `{ "ok": value }` or `{ "throws": errorName }`. A
//! negative index, which Cesium rejects with a `DeveloperError`, cannot be
//! passed to the port's unsigned indices and is answered here.
mod common;
use cesium_rs::core::{Cartesian3, Cartographic, GeographicProjection, MapProjection, Resource, WebMercatorProjection};
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Map, Value};
use std::sync::Arc;

fn kind(e: &MetadataError) -> &'static str {
    match e {
        MetadataError::Developer(_) => "DeveloperError",
        MetadataError::Type(_) => "TypeError",
        MetadataError::Range(_) => "RangeError",
        MetadataError::Runtime(_) => "RuntimeError",
        MetadataError::Syntax(_) => "SyntaxError",
    }
}

fn attempt<T>(r: Result<T, MetadataError>, f: impl FnOnce(T) -> Value) -> Value {
    match r {
        Ok(v) => json!({ "ok": f(v) }),
        Err(e) => json!({ "throws": kind(&e) }),
    }
}

fn developer() -> Value {
    json!({ "throws": "DeveloperError" })
}

/// A metadata value as the fixtures tag it.
fn tag(value: &MetadataValue) -> Value {
    let all = |c: &[f64]| c.iter().map(|&x| num(x)).collect::<Vec<_>>();
    match value {
        MetadataValue::Null => json!({ "$null": true }),
        MetadataValue::Boolean(b) => json!(b),
        MetadataValue::Number(x) => num(*x),
        MetadataValue::BigInt(b) => json!({ "$bigint": b.to_string() }),
        MetadataValue::String(s) => json!(s),
        MetadataValue::Array(items) => Value::Array(items.iter().map(tag).collect()),
        MetadataValue::Cartesian2(c) => json!({ "$Cartesian2": all(&[c.x, c.y]) }),
        MetadataValue::Cartesian3(c) => json!({ "$Cartesian3": all(&[c.x, c.y, c.z]) }),
        MetadataValue::Cartesian4(c) => json!({ "$Cartesian4": all(&[c.x, c.y, c.z, c.w]) }),
        MetadataValue::Matrix2(m) => json!({ "$Matrix2": all(&m.0) }),
        MetadataValue::Matrix3(m) => json!({ "$Matrix3": all(&m.0) }),
        MetadataValue::Matrix4(m) => json!({ "$Matrix4": all(&m.0) }),
        MetadataValue::Object(v) => v.clone(),
    }
}

fn val(value: Option<MetadataValue>) -> Value {
    value.map_or(Value::Null, |v| tag(&v))
}

/// Every property of an entity, as `{ id: value }`.
fn entity(e: &dyn MetadataEntity) -> Value {
    let mut out = Map::new();
    for id in e.get_property_ids() {
        out.insert(id.clone(), val(e.get_property(&id).unwrap()));
    }
    Value::Object(out)
}

// ---- bounding volumes ------------------------------------------------------

struct Frame {
    mode: SceneMode,
    position_wc: Cartesian3,
    cartographic: Cartographic,
    projection: Box<dyn MapProjection>,
}

fn frames(v: &Value) -> Vec<Frame> {
    list(v, |f| Frame {
        mode: SceneMode::from_value(n(&f[0]) as i32).unwrap(),
        position_wc: c3(&f[1]),
        cartographic: carto(&f[2]),
        projection: if s(&f[3]) == "geographic" {
            Box::new(GeographicProjection::default())
        } else {
            Box::new(WebMercatorProjection::default())
        },
    })
}

fn distances(volume: &dyn TileBoundingVolume, frames: &[Frame]) -> Value {
    Value::Array(
        frames
            .iter()
            .map(|f| {
                let frame = TileBoundingVolumeFrame {
                    mode: f.mode,
                    camera_position_wc: f.position_wc,
                    camera_position_cartographic: f.cartographic,
                    map_projection: f.projection.as_ref(),
                };
                num(volume.distance_to_camera(&frame))
            })
            .collect(),
    )
}

fn debug(volume: &DebugVolume) -> Value {
    json!({
        "instances": volume.geometry_instances.iter().map(|i| {
            let color = i.attributes.iter().find(|(name, _)| name == "color").map(|(_, a)| a.value.to_f64_vec()).unwrap();
            json!({
                "geometry": geometry_enc(i.geometry.clone()),
                "modelMatrix": i.model_matrix.enc(),
                "id": i.id,
                "color": color.enc(),
            })
        }).collect::<Vec<_>>(),
        "translucent": false,
        "flat": true,
        "asynchronous": false,
    })
}

fn volume_ops(volume: &dyn TileBoundingVolume, a: &[Value], frame_arg: usize) -> Map<String, Value> {
    let planes = list(&a[frame_arg + 1], plane);
    let mut out = Map::new();
    out.insert("boundingSphere".into(), volume.bounding_sphere().enc());
    out.insert("distances".into(), distances(volume, &frames(&a[frame_arg])));
    out.insert("intersections".into(), Value::Array(planes.iter().map(|p| volume.intersect_plane(p).enc()).collect()));
    out.insert("debug".into(), debug(&volume.create_debug_volume(&color(&a[frame_arg + 2]))));
    out
}

#[test]
fn tile_bounding_sphere() {
    run(
        "scene",
        "TileBoundingSphere",
        |_| TIGHT_VECTOR,
        |f, a| {
            if f != "sphere" {
                return None;
            }
            let center = c3(&a[0]);
            let mut v = TileBoundingSphere::new(center, n(&a[1]));
            let mut out = volume_ops(&v, a, 2);
            out.insert("center".into(), v.center().enc());
            out.insert("radius".into(), num(v.radius()));
            v.update(center + Cartesian3::UNIT_Z, 0.0);
            out.insert("updated".into(), json!([v.center().enc(), num(v.radius())]));
            Some(Out::Ok(Value::Object(out)))
        },
    );
}

#[test]
fn tile_oriented_bounding_box() {
    run(
        "scene",
        "TileOrientedBoundingBox",
        |_| TIGHT_VECTOR,
        |f, a| {
            if f != "box" {
                return None;
            }
            let center = c3(&a[0]);
            let mut v = TileOrientedBoundingBox::new(center, m3(&a[1]));
            let mut out = volume_ops(&v, a, 2);
            out.insert(
                "boundingVolume".into(),
                json!([v.bounding_volume().center.enc(), v.bounding_volume().half_axes.enc()]),
            );
            v.update(center + Cartesian3::UNIT_X, cesium_rs::core::Matrix3::ZERO);
            let obb = v.bounding_volume();
            out.insert("updated".into(), json!([obb.center.enc(), obb.half_axes.enc(), v.bounding_sphere().enc()]));
            Some(Out::Ok(Value::Object(out)))
        },
    );
}

#[test]
fn tile_bounding_region() {
    run(
        "scene",
        "TileBoundingRegion",
        |_| TIGHT_VECTOR,
        |f, a| match f {
            "region" => {
                let mut options = TileBoundingRegionOptions::new(rect(&a[0]));
                options.minimum_height = Some(n(&a[1]));
                options.maximum_height = Some(n(&a[2]));
                let v = TileBoundingRegion::new(options);
                let mut out = volume_ops(&v, a, 3);
                out.insert("rectangle".into(), v.rectangle.enc());
                out.insert("heights".into(), json!([num(v.minimum_height), num(v.maximum_height)]));
                out.insert(
                    "corners".into(),
                    json!([v.southwest_corner_cartesian.enc(), v.northeast_corner_cartesian.enc()]),
                );
                out.insert(
                    "normals".into(),
                    json!([v.west_normal.enc(), v.south_normal.enc(), v.east_normal.enc(), v.north_normal.enc()]),
                );
                let obb = v.bounding_volume().unwrap();
                out.insert("boundingVolume".into(), json!([obb.center.enc(), obb.half_axes.enc()]));
                Some(Out::Ok(Value::Object(out)))
            }
            "lazy" => {
                let mut options = TileBoundingRegionOptions::new(rect(&a[0]));
                options.compute_bounding_volumes = Some(false);
                let v = TileBoundingRegion::new(options);
                Some(Out::Ok(json!({
                    "boundingVolume": Value::Null,
                    "boundingSphere": v.bounding_sphere_opt().map(|b| b.enc()),
                    "heights": [num(v.minimum_height), num(v.maximum_height)],
                    "distances": distances(&v, &frames(&a[1])),
                })))
            }
            _ => None,
        },
    );
}

#[test]
fn tile_bounding_s2_cell() {
    run(
        "scene",
        "TileBoundingS2Cell",
        |_| TIGHT_VECTOR,
        |f, a| {
            if f != "s2" {
                return None;
            }
            let v = TileBoundingS2Cell::new(TileBoundingS2CellOptions {
                token: s(&a[0]).to_owned(),
                minimum_height: Some(n(&a[1])),
                maximum_height: Some(n(&a[2])),
                ellipsoid: None,
            })?;
            let mut out = volume_ops(&v, a, 3);
            out.insert("center".into(), v.center.enc());
            out.insert("heights".into(), json!([num(v.minimum_height), num(v.maximum_height)]));
            out.insert("planes".into(), Value::Array(v.bounding_planes().iter().map(|p| p.enc()).collect()));
            out.insert("vertices".into(), Value::Array(v.vertices().iter().map(|p| p.enc()).collect()));
            Some(Out::Ok(Value::Object(out)))
        },
    );
}

// ---- implicit tiling -------------------------------------------------------

fn scheme(v: &Value) -> ImplicitSubdivisionScheme {
    ImplicitSubdivisionScheme::from_str_value(s(v)).unwrap()
}

fn int(v: &Value) -> i64 {
    n(v) as i64
}

fn make(o: &Value) -> Result<ImplicitTileCoordinates, MetadataError> {
    ImplicitTileCoordinates::new(
        scheme(&o["subdivisionScheme"]),
        u(&o["subtreeLevels"]),
        int(&o["level"]),
        int(&o["x"]),
        int(&o["y"]),
        o.get("z").filter(|z| !z.is_null()).map(int),
    )
}

fn coord(c: &ImplicitTileCoordinates) -> Value {
    json!({
        "scheme": c.subdivision_scheme.as_str(),
        "subtreeLevels": c.subtree_levels,
        "level": c.level,
        "x": c.x,
        "y": c.y,
        "z": c.z,
    })
}

fn morton(c: &ImplicitTileCoordinates) -> Result<u32, MetadataError> {
    c.morton_index().ok_or_else(|| MetadataError::Developer("morton range".into()))
}

fn describe_coordinates(o: &Value, offsets: &Value) -> Result<Value, MetadataError> {
    let c = make(o)?;
    let others: Vec<_> = list(offsets, |d| make(d).unwrap());
    let morton_index = morton(&c)?;
    let ok = |r: Result<ImplicitTileCoordinates, MetadataError>| attempt(r, |c| coord(&c));
    let template: Map<String, Value> =
        c.get_template_values().into_iter().map(|(k, v)| (k.to_owned(), json!(v))).collect();
    let quad_root = ImplicitTileCoordinates::new(ImplicitSubdivisionScheme::Quadtree, c.subtree_levels, 0, 0, 0, None)?;
    Ok(json!({
        "coord": coord(&c),
        "childIndex": c.child_index(),
        "mortonIndex": morton_index,
        "tileIndex": num(c.tile_index().unwrap()),
        "descendants": others.iter().map(|d| ok(c.get_descendant_coordinates(d))).collect::<Vec<_>>(),
        "ancestors": vec![-1i64, 0, 1, 2, 3, 40].into_iter().map(|k| ok(c.get_ancestor_coordinates(k))).collect::<Vec<_>>(),
        "offsets": others.iter().map(|d| match c.get_descendant_coordinates(d) {
            Ok(desc) => ok(c.get_offset_coordinates(&desc)),
            Err(e) => json!({ "throws": kind(&e) }),
        }).collect::<Vec<_>>(),
        "offsetSelf": ok(c.get_offset_coordinates(&c)),
        "offsetNotDescendant": ok(c.get_offset_coordinates(&quad_root)),
        "children": vec![-1i64, 0, 1, 2, 3, 4, 7, 8].into_iter().map(|i| ok(c.get_child_coordinates(i))).collect::<Vec<_>>(),
        "subtree": ok(c.get_subtree_coordinates()),
        "parentSubtree": ok(c.get_parent_subtree_coordinates()),
        "isAncestor": others.iter().map(|d| attempt(c.is_ancestor(d), |b| json!(b))).collect::<Vec<_>>(),
        "isAncestorOfChild": attempt(c.get_child_coordinates(1).and_then(|child| c.is_ancestor(&child)), |b| json!(b)),
        "isEqual": std::iter::once(&c).chain(others.iter()).map(|d| c.is_equal(d)).collect::<Vec<_>>(),
        "isRoot": c.is_implicit_tileset_root(),
        "isSubtreeRoot": c.is_subtree_root(),
        "isBottom": c.is_bottom_of_subtree(),
        "template": template,
    }))
}

#[test]
fn implicit_tile_coordinates() {
    run(
        "scene",
        "ImplicitTileCoordinates",
        |_| EXACT,
        |f, a| {
            let ok = |r: Result<ImplicitTileCoordinates, MetadataError>| attempt(r, |c| coord(&c));
            Some(Out::Ok(json!({ "result": match f {
                "coordinates" => attempt(describe_coordinates(&a[0], &a[1]), |v| v),
                "mismatch" => return Some(Out::Ok({
                    let (ca, cb, co) = (make(&a[0]).unwrap(), make(&a[1]).unwrap(), make(&a[2]).unwrap());
                    json!([
                        attempt(ca.is_ancestor(&cb), |b| json!(b)),
                        attempt(ca.is_ancestor(&co), |b| json!(b)),
                        attempt(ca.get_descendant_coordinates(&cb), |_| json!({})),
                        ca.is_equal(&cb),
                        ca.is_equal(&co),
                    ])
                })),
                "fromMortonIndex" => {
                    ok(ImplicitTileCoordinates::from_morton_index(scheme(&a[0]), u(&a[1]), int(&a[2]), u(&a[3])))
                }
                "fromTileIndex" => ok(ImplicitTileCoordinates::from_tile_index(scheme(&a[0]), 3, u(&a[1]))),
                _ => return None,
            }})))
        },
    );
}

#[test]
fn implicit_availability_bitstream() {
    run(
        "scene",
        "ImplicitAvailabilityBitstream",
        |_| EXACT,
        |f, a| {
            if f != "bitstream" {
                return None;
            }
            let o = &a[0];
            let length_bits = u(&o["lengthBits"]) as usize;
            let result = ImplicitAvailabilityBitstream::new(ImplicitAvailabilityBitstreamOptions {
                length_bits,
                available_count: o.get("availableCount").map(|c| u(c) as usize),
                constant: o.get("constant").and_then(Value::as_bool),
                bitstream: o.get("bitstream").map(|b| nums(b).into_iter().map(|x| x as u8).collect()),
                compute_available_count_enabled: o
                    .get("computeAvailableCountEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
            Some(Out::Ok(json!({ "result": match result {
                Err(_) => json!({ "throws": "RuntimeError" }),
                Ok(b) => {
                    let mut bits = vec![developer()];
                    for i in 0..=length_bits {
                        bits.push(b.get_bit(i).map_or_else(developer, |bit| json!({ "ok": bit })));
                    }
                    json!({ "ok": { "lengthBits": b.length_bits(), "availableCount": b.available_count(), "bits": bits } })
                }
            }})))
        },
    );
}

fn base_resource() -> Resource {
    Resource::new("https://example.com/tileset/tileset.json").unwrap()
}

fn tileset(tile_json: &Value, schema: &Value) -> Result<ImplicitTileset, MetadataError> {
    ImplicitTileset::new(base_resource(), tile_json, Some(Arc::new(MetadataSchema::from_json(schema).unwrap())))
}

#[test]
fn implicit_tileset() {
    let schema = load("scene", "ImplicitSubtree")[0].a[0]["schema"].clone();
    run(
        "scene",
        "ImplicitTileset",
        |_| EXACT,
        |f, a| {
            if f != "tileset" {
                return None;
            }
            Some(Out::Ok(json!({ "result": attempt(tileset(&a[0], &schema), |t| {
            json!({
                "geometricError": t.geometric_error.map(num),
                "boundingVolume": t.bounding_volume,
                "refine": t.refine,
                "subtreeUri": t.subtree_uri_template.url(),
                "contentUris": t.content_uri_templates.iter().map(Resource::url).collect::<Vec<_>>(),
                "contentHeaders": t.content_headers,
                "contentCount": t.content_count,
                "tileHeader": t.tile_header,
                "subdivisionScheme": t.subdivision_scheme.as_str(),
                "branchingFactor": t.branching_factor,
                "subtreeLevels": t.subtree_levels,
                "availableLevels": num(t.available_levels),
            })
        }) })))
        },
    );
}

/// `getBit` and friends at a possibly negative index.
fn at_index(i: i64, f: impl FnOnce(usize) -> Result<bool, MetadataError>) -> Value {
    if i < 0 {
        return developer();
    }
    attempt(f(i as usize), |b| json!(b))
}

fn describe_subtree(subtree: &ImplicitSubtree, root: &ImplicitTileCoordinates, octree: bool) -> Value {
    let mut tiles = Vec::new();
    for level in 0..=2u32 {
        let n = 1i64 << level;
        for x in 0..n {
            for y in 0..n {
                for z in 0..(if octree { n } else { 1 }) {
                    let scheme =
                        if octree { ImplicitSubdivisionScheme::Octree } else { ImplicitSubdivisionScheme::Quadtree };
                    let local =
                        ImplicitTileCoordinates::new(scheme, 2, level as i64, x, y, octree.then_some(z)).unwrap();
                    let c = root.get_descendant_coordinates(&local).unwrap();
                    let view = |v: Option<ImplicitMetadataView>| v.map_or(Value::Null, |v| entity(&v));
                    tiles.push(json!({
                        "coordinates": [c.level, c.x, c.y, c.z],
                        "tileIndex": attempt(subtree.get_tile_index(&c), |i| json!(i)),
                        "tile": attempt(subtree.tile_is_available_at_coordinates(&c), |b| json!(b)),
                        "content": attempt(subtree.content_is_available_at_coordinates(&c, None), |b| json!(b)),
                        "content1": attempt(subtree.content_is_available_at_coordinates(&c, Some(1)), |b| json!(b)),
                        "childIndex": attempt(subtree.get_child_subtree_index(&c), |i| json!(i)),
                        "child": attempt(subtree.child_subtree_is_available_at_coordinates(&c), |b| json!(b)),
                        "tileView": attempt(subtree.get_tile_metadata_view(&c), view),
                        "contentView": attempt(subtree.get_content_metadata_view(&c, 0), view),
                    }));
                }
            }
        }
    }
    let count = |t: &Arc<std::sync::Mutex<MetadataTable>>| t.lock().unwrap().count();
    json!({
        "ready": subtree.ready(),
        "metadata": subtree.metadata().map(|m| entity(m)),
        "tileTableCount": subtree.tile_metadata_table().map(count),
        "tilePropertyTableJson": subtree.tile_property_table_json(),
        "contentTableCounts": subtree.content_metadata_tables().iter().map(count).collect::<Vec<_>>(),
        "tileAvailableCount": subtree.tile_availability().available_count(),
        "contentAvailableCounts": subtree.content_availability_bitstreams().iter().map(|b| b.available_count()).collect::<Vec<_>>(),
        "childAvailableCount": subtree.child_subtree_availability().available_count(),
        "tileJumpBuffer": subtree.tile_jump_buffer(),
        "contentJumpBuffers": subtree.content_jump_buffers(),
        "levelOffsets": (0..4).map(|l| num(subtree.get_level_offset(l))).collect::<Vec<_>>(),
        "parentMorton": vec![0u32, 3, 4, 17, 63].into_iter().map(|m| subtree.get_parent_morton_index(m)).collect::<Vec<_>>(),
        "indexBits": vec![-1i64, 0, 4, 5].into_iter().map(|i| json!([
            at_index(i, |i| subtree.tile_is_available_at_index(i)),
            at_index(i, |i| subtree.child_subtree_is_available_at_index(i)),
            at_index(i, |i| subtree.content_is_available_at_index(i, None)),
            at_index(i, |i| subtree.content_is_available_at_index(0, Some(i))),
        ])).collect::<Vec<_>>(),
        "tiles": tiles,
    })
}

#[test]
fn implicit_subtree() {
    run(
        "scene",
        "ImplicitSubtree",
        |_| EXACT,
        |f, a| {
            if f != "subtree" {
                return None;
            }
            let o = &a[0];
            let tileset = tileset(&o["tileJson"], &o["schema"]).unwrap();
            let coordinates = make(&o["coordinates"]).unwrap();
            let resource = Resource::new("https://example.com/tileset/subtrees/0.0.0.subtree").unwrap();
            let file: Option<Vec<u8>> =
                (!o["file"].is_null()).then(|| nums(&o["file"]).into_iter().map(|b| b as u8).collect());
            let source = match &file {
                Some(bytes) => SubtreeSource::Binary(bytes),
                None => SubtreeSource::Json(o["json"].clone()),
            };
            let external = o["externalBuffers"].clone();
            let mut load_buffer = |r: &Resource| -> Result<Vec<u8>, MetadataError> {
                external
                    .get(r.url())
                    .map(|b| nums(b).into_iter().map(|x| x as u8).collect())
                    .ok_or_else(|| MetadataError::Runtime(format!("no fixture for {}", r.url())))
            };
            let result = ImplicitSubtree::from_subtree_json(&resource, source, &tileset, coordinates, &mut load_buffer);
            Some(Out::Ok(
                json!({ "result": attempt(result, |subtree| describe_subtree(&subtree, &coordinates, o["octree"] == json!(true))) }),
            ))
        },
    );
}

struct FakeSubtree(ImplicitTileCoordinates);

impl CachedSubtree for FakeSubtree {
    fn implicit_coordinates(&self) -> &ImplicitTileCoordinates {
        &self.0
    }
}

#[test]
fn implicit_subtree_cache() {
    run(
        "scene",
        "ImplicitSubtreeCache",
        |_| EXACT,
        |f, a| {
            if f != "cache" {
                return None;
            }
            let maximum = u(&a[0]) as usize;
            let adds: Vec<ImplicitTileCoordinates> = list(&a[1], |o| make(o).unwrap());
            let mut cache = ImplicitSubtreeCache::new(Some(maximum));
            let mut steps = Vec::new();
            for c in &adds {
                let add = attempt(cache.add_subtree(FakeSubtree(*c)), |_| Value::Null);
                let queue: Vec<Value> = cache
                    .nodes()
                    .iter()
                    .map(|node| {
                        let c = node.subtree.implicit_coordinates();
                        json!([c.level, c.x, c.y, c.z, node.stamp])
                    })
                    .collect();
                let found: Vec<bool> = adds.iter().map(|q| cache.find(q).is_some()).collect();
                steps.push(json!({ "add": add, "queue": queue, "found": found }));
            }
            Some(Out::Ok(Value::Array(steps)))
        },
    );
}

#[test]
fn bounding_volume_semantics() {
    run(
        "scene",
        "BoundingVolumeSemantics",
        |_| EXACT,
        |f, a| {
            if f != "semantics" {
                return None;
            }
            let schema = MetadataSchema::from_json(&a[0]).unwrap();
            let class = schema.classes.get(s(&a[1])).unwrap().clone();
            let metadata = TileMetadata::new(&json!({ "class": a[1], "properties": a[2] }), class).unwrap();
            let mut out = Map::new();
            for prefix in [BoundingVolumePrefix::Tile, BoundingVolumePrefix::Content] {
                let all = bounding_volume_semantics::parse_all_bounding_volume_semantics(prefix, &metadata).unwrap();
                let volume = all.bounding_volume.map(|v| match v {
                    SemanticBoundingVolume::Box(v) => json!({ "box": tag(&v) }),
                    SemanticBoundingVolume::Region(v) => json!({ "region": tag(&v) }),
                    SemanticBoundingVolume::Sphere(v) => json!({ "sphere": tag(&v) }),
                });
                out.insert(
                prefix.as_str().into(),
                json!({ "boundingVolume": volume, "minimumHeight": val(all.minimum_height), "maximumHeight": val(all.maximum_height) }),
            );
            }
            Some(Out::Ok(Value::Object(out)))
        },
    );
}
