//! Native glTF/meshopt terrain decoding, border skirts and barycentric child clipping.
use super::terrain_encoding::{TerrainEncodingOptions, TerrainVertex};
use super::terrain_mesh::TerrainMeshOptions;
use super::terrain_provider::{self, TerrainEdgeIndices};
use super::*;
use serde_json::Value;
use std::collections::HashMap;
#[derive(Clone, Debug)]
pub struct TerrainGltf {
    pub json: Value,
    pub buffers: Vec<Vec<u8>>,
}
impl TerrainGltf {
    pub fn from_glb(bytes: &[u8]) -> Result<Self, RuntimeError> {
        let mut r = super::network_terrain::Reader::new(bytes);
        if r.u32()? != 0x46546c67 || r.u32()? != 2 {
            return Err(error("Expected a glTF 2 GLB."));
        }
        if r.u32()? as usize != bytes.len() {
            return Err(error("Invalid GLB length."));
        }
        let mut json = None;
        let mut buffers = vec![];
        while r.pos < bytes.len() {
            let len = r.u32()? as usize;
            let kind = r.u32()?;
            let b = r.take(len)?;
            match kind {
                0x4e4f534a => json = Some(serde_json::from_slice(b).map_err(|e| error(e.to_string()))?),
                0x004e4942 => buffers.push(b.to_vec()),
                _ => {}
            }
        }
        Ok(Self { json: json.ok_or_else(|| error("GLB JSON is missing."))?, buffers })
    }
}
fn error(s: impl Into<String>) -> RuntimeError {
    RuntimeError::new(s)
}
fn integer(v: &Value) -> Result<usize, RuntimeError> {
    v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| error("Invalid glTF integer."))
}
fn member<'a>(v: &'a Value, key: &str) -> Result<&'a Value, RuntimeError> {
    v.get(key).ok_or_else(|| error(format!("Missing glTF {key}.")))
}
fn index<'a>(v: &'a Value, i: usize) -> Result<&'a Value, RuntimeError> {
    v.get(i).ok_or_else(|| error("glTF index is out of bounds."))
}
fn slice(bytes: &[u8], offset: usize, len: usize) -> Result<&[u8], RuntimeError> {
    bytes
        .get(offset..offset.checked_add(len).ok_or_else(|| error("glTF buffer range overflow."))?)
        .ok_or_else(|| error("glTF buffer is truncated."))
}
fn decode_vertices<const N: usize>(count: usize, bytes: &[u8]) -> Result<Vec<u8>, RuntimeError> {
    super::meshopt_vertex::decode(bytes, count, N)
}
fn accessor(g: &TerrainGltf, id: usize, kind: u8) -> Result<Vec<f64>, RuntimeError> {
    let a = index(member(&g.json, "accessors")?, id)?;
    let view = index(member(&g.json, "bufferViews")?, integer(member(a, "bufferView")?)?)?;
    let count = integer(member(a, "count")?)?;
    if kind == 2 && count % 3 != 0 {
        return Err(error("Terrain triangle count is invalid."));
    }
    let offset = a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let ext = &view["extensions"]["EXT_meshopt_compression"];
    let compressed = !ext.is_null();
    let source = if compressed { ext } else { view };
    let b = g.buffers.get(integer(member(source, "buffer")?)?).ok_or_else(|| error("glTF buffer is missing."))?;
    let start = (source["byteOffset"].as_u64().unwrap_or(0) as usize)
        .checked_add(offset)
        .ok_or_else(|| error("glTF offset overflow."))?;
    if compressed {
        let len = integer(member(ext, "byteLength")?)?;
        let bytes = slice(b, start, len)?;
        let stride = integer(member(ext, "byteStride")?)?;
        if count > 100_000_000 {
            return Err(error("glTF accessor is too large."));
        }
        if kind < 2 {
            let decoded = match stride {
                4 => decode_vertices::<4>(count, bytes)?,
                8 => decode_vertices::<8>(count, bytes)?,
                _ => return Err(error("Unsupported meshopt terrain attribute stride.")),
            };
            if kind == 0 {
                let unit = if stride == 4 { 1 } else { 2 };
                let maximum = if unit == 1 { 255. } else { 65535. };
                let mut out = Vec::with_capacity(count * 3);
                for i in 0..count {
                    for c in 0..3 {
                        let p = i * stride + c * unit;
                        let v = if unit == 1 {
                            decoded[p] as f64
                        } else {
                            u16::from_le_bytes(decoded[p..p + 2].try_into().unwrap()) as f64
                        };
                        out.push((v / maximum) as f32 as f64);
                    }
                }
                return Ok(out);
            }
            if stride != 4 {
                return Err(error("Invalid meshopt normal stride."));
            }
            let mut out = vec![];
            for i in 0..count {
                let mut x = (decoded[i * 4] as i8 as f64 / 127.).max(-1.);
                let mut y = (decoded[i * 4 + 1] as i8 as f64 / 127.).max(-1.);
                let z = 1. - (x.abs() + y.abs());
                if z < 0. {
                    let old = x;
                    x = (1. - y.abs()) * if old < 0. { -1. } else { 1. };
                    y = (1. - old.abs()) * if y < 0. { -1. } else { 1. };
                }
                let n = Cartesian3::new(x, y, z).normalize();
                out.extend([n.x as f32 as f64, n.y as f32 as f64, n.z as f32 as f64]);
            }
            return Ok(out);
        }
        let mut out = vec![0u32; count];
        let result = if kind == 2 {
            meshopt_rs::index::buffer::decode_index_buffer(&mut out, bytes)
        } else {
            meshopt_rs::index::sequence::decode_index_sequence(&mut out, bytes)
        };
        result.map_err(|e| error(format!("Invalid meshopt indices: {e:?}")))?;
        return Ok(out
            .into_iter()
            .map(|i| if a["componentType"] == 5123 { i as u16 as f64 } else { i as f64 })
            .collect());
    }
    let size = if kind < 2 {
        4
    } else if a["componentType"] == 5123 {
        2
    } else {
        4
    };
    let components = if kind < 2 { 3 } else { 1 };
    let n = count.checked_mul(components).ok_or_else(|| error("glTF accessor overflow."))?;
    let bytes = slice(b, start, n.checked_mul(size).ok_or_else(|| error("glTF accessor overflow."))?)?;
    Ok(bytes
        .chunks_exact(size)
        .map(|b| {
            if kind < 2 {
                f32::from_le_bytes(b.try_into().unwrap()) as f64
            } else if size == 2 {
                u16::from_le_bytes(b.try_into().unwrap()) as f64
            } else {
                u32::from_le_bytes(b.try_into().unwrap()) as f64
            }
        })
        .collect())
}
#[derive(Clone, Copy)]
pub struct TilesTerrainMeshOptions<'a> {
    pub gltf: &'a TerrainGltf,
    pub ellipsoid: Ellipsoid,
    pub rectangle: Rectangle,
    pub has_vertex_normals: bool,
    pub has_web_mercator_t: bool,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub bounding_sphere: BoundingSphere,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub horizon_occlusion_point: Cartesian3,
    pub skirt_height: f64,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
}
pub struct Cesium3DTilesTerrainGeometryProcessor;
impl Cesium3DTilesTerrainGeometryProcessor {
    pub fn create_mesh(o: TilesTerrainMeshOptions<'_>) -> Result<TerrainMesh, RuntimeError> {
        let primitive = &o.gltf.json["meshes"][0]["primitives"][0];
        let positions = accessor(o.gltf, integer(member(&primitive["attributes"], "POSITION")?)?, 0)?;
        let normals = if o.has_vertex_normals {
            Some(accessor(o.gltf, integer(member(&primitive["attributes"], "NORMAL")?)?, 1)?)
        } else {
            None
        };
        let ids = accessor(o.gltf, integer(member(primitive, "indices")?)?, 2)?
            .into_iter()
            .map(|v| v as u32)
            .collect::<Vec<_>>();
        let edge = |name| {
            accessor(o.gltf, integer(member(&primitive["extensions"]["CESIUM_tile_edges"], name)?)?, 3)
                .map(|v| v.into_iter().map(|n| n as u32).collect())
        };
        let edges = TerrainEdgeIndices {
            west_indices_south_to_north: edge("left")?,
            south_indices_east_to_west: edge("bottom")?,
            east_indices_north_to_south: edge("right")?,
            north_indices_west_to_east: edge("top")?,
        };
        let a = o.gltf.json["nodes"][0]["matrix"]
            .as_array()
            .filter(|v| v.len() == 16)
            .ok_or_else(|| error("Terrain glTF node matrix is missing."))?;
        let mut array = [0.; 16];
        for (i, v) in a.iter().enumerate() {
            array[i] = v.as_f64().ok_or_else(|| error("Invalid terrain node matrix."))?;
        }
        let axis = Matrix4([1., 0., 0., 0., 0., 0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1.]);
        let transform = axis.multiply(&Matrix4(array));
        let mut center = o.rectangle.center();
        center.height = (o.minimum_height + o.maximum_height) * 0.5;
        let approximate = o.ellipsoid.cartographic_to_cartesian(&center);
        let mut samples = vec![];
        for (i, pos) in positions.chunks_exact(3).enumerate() {
            let position = transform.multiply_by_point(Cartesian3::new(pos[0], pos[1], pos[2]));
            let c =
                o.ellipsoid.cartesian_to_cartographic(position).ok_or_else(|| error("Invalid terrain position."))?;
            let mut uv = Cartesian2::new(
                ((c.longitude - o.rectangle.west) / o.rectangle.width()).clamp(0., 1.),
                ((c.latitude - o.rectangle.south) / o.rectangle.height()).clamp(0., 1.),
            );
            if edges.west_indices_south_to_north.contains(&(i as u32)) {
                uv.x = 0.;
            } else if edges.east_indices_north_to_south.contains(&(i as u32)) {
                uv.x = 1.;
            }
            if edges.south_indices_east_to_west.contains(&(i as u32)) {
                uv.y = 0.;
            } else if edges.north_indices_west_to_east.contains(&(i as u32)) {
                uv.y = 1.;
            }
            let mut v = TerrainVertex::new(position, uv, c.height);
            if let Some(n) = &normals {
                let n = n.get(i * 3..i * 3 + 3).ok_or_else(|| error("Terrain normals are truncated."))?;
                v.normal_to_pack = Some(AttributeCompression::oct_encode(
                    transform.multiply_by_point_as_vector(Cartesian3::new(n[0], n[1], n[2])).normalize(),
                ));
            }
            set_fields(&mut v, c.latitude, &o.rectangle, &o.ellipsoid, o.has_web_mercator_t, o.exaggeration);
            samples.push(v);
        }
        build_mesh(
            samples,
            ids,
            edges,
            o.rectangle,
            o.ellipsoid,
            o.bounding_sphere.center,
            approximate,
            [o.minimum_height, o.maximum_height],
            Some((o.bounding_sphere, o.oriented_bounding_box, Some(o.horizon_occlusion_point))),
            o.skirt_height,
            o.has_vertex_normals,
            o.has_web_mercator_t,
            o.exaggeration,
            o.exaggeration_relative_height,
        )
    }
    pub fn upsample_mesh(
        parent: &TerrainMesh,
        ellipsoid: Ellipsoid,
        rectangle: Rectangle,
        east: bool,
        north: bool,
        skirt_height: f64,
    ) -> Result<TerrainMesh, RuntimeError> {
        let count = parent.vertex_count_without_skirts;
        let indices = parent.indices.to_f64_vec();
        let index_count = parent.index_count_without_skirts.unwrap_or(indices.len());
        let min = Cartesian2::new(if east { 0.5 } else { 0. }, if north { 0.5 } else { 0. });
        let max = Cartesian2::new(min.x + 0.5, min.y + 0.5);
        let start = if east {
            if north {
                3
            } else {
                0
            }
        } else if north {
            2
        } else {
            1
        };
        let mut map: HashMap<u64, u32> = HashMap::new();
        let mut samples = vec![];
        let mut output = vec![];
        let mut edge: [Vec<u32>; 4] = Default::default();
        let (mut hmin, mut hmax) = (f64::INFINITY, f64::NEG_INFINITY);
        for tri in indices[..index_count].chunks_exact(3) {
            let ids = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            if ids.iter().any(|&i| i >= count) {
                return Err(error("Invalid terrain triangle."));
            }
            let mut polygon = ids
                .iter()
                .enumerate()
                .map(|(j, &i)| {
                    let uv = parent.encoding.decode_texture_coordinates(&parent.vertices, i).unwrap();
                    let mut b = [0.; 3];
                    b[j] = 1.;
                    (uv, b)
                })
                .collect::<Vec<_>>();
            for e in 0..2 {
                let side = (start + e) % 4;
                let inside = |p: Cartesian2| match side {
                    0 => p.x - min.x,
                    1 => max.y - p.y,
                    2 => max.x - p.x,
                    _ => p.y - min.y,
                };
                let mut next = vec![];
                if polygon.is_empty() {
                    break;
                }
                let mut previous = *polygon.last().unwrap();
                for &current in &polygon {
                    let a = inside(previous.0);
                    let b = inside(current.0);
                    if (a < 0. && b > 0.) || (a > 0. && b < 0.) {
                        let t = match side {
                            0 => (min.x - previous.0.x) / (current.0.x - previous.0.x),
                            2 => (max.x - previous.0.x) / (current.0.x - previous.0.x),
                            3 => (min.y - previous.0.y) / (current.0.y - previous.0.y),
                            _ => (max.y - previous.0.y) / (current.0.y - previous.0.y),
                        };
                        let mut p = previous.0.lerp(current.0, t);
                        match side {
                            0 => p.x = min.x,
                            2 => p.x = max.x,
                            3 => p.y = min.y,
                            _ => p.y = max.y,
                        }
                        let mut bary = [0.; 3];
                        for j in 0..3 {
                            bary[j] = previous.1[j] * (1. - t) + current.1[j] * t;
                        }
                        next.push((p, bary));
                    }
                    if b >= 0. {
                        next.push(current);
                    }
                    previous = current;
                }
                polygon = next;
            }
            if polygon.len() < 3 {
                continue;
            }
            let mut poly_ids = vec![];
            for (p, (uv, bary)) in polygon.iter().enumerate() {
                let bits = |b: &[f64; 3]| (b[0].ceil() as u8) | ((b[1].ceil() as u8) << 1) | ((b[2].ceil() as u8) << 2);
                let prim = (0..3).filter(|&i| bits(bary) & (1 << i) != 0).collect::<Vec<_>>();
                let key = if prim.len() == 1 {
                    Some(ids[prim[0]] as u64)
                } else if prim.len() == 2 {
                    let a = ids[prim[0]] as u64;
                    let b = ids[prim[1]] as u64;
                    let base = count as u64 + 2 * (a.min(b) * count as u64 + a.max(b));
                    let same = bits(bary) == bits(&polygon[(p + polygon.len() - 1) % polygon.len()].1);
                    let first = map.contains_key(&base);
                    let second = map.contains_key(&(base + 1));
                    let use_first = (!same) == (!first || !second);
                    Some(base + u64::from(!use_first))
                } else {
                    None
                };
                let existing = key.and_then(|k| map.get(&k).copied());
                let i = if let Some(i) = existing {
                    i
                } else {
                    let i = samples.len() as u32;
                    if let Some(k) = key {
                        map.insert(k, i);
                    }
                    let uv = Cartesian2::new((uv.x - min.x) * 2., (uv.y - min.y) * 2.);
                    let weights = [bary[0], bary[1], 1. - bary[0] - bary[1]];
                    let mut height = 0.;
                    let mut normal = Cartesian3::ZERO;
                    for j in 0..3 {
                        height += parent
                            .encoding
                            .decode_height(&parent.vertices, ids[j])
                            .ok_or_else(|| error("Invalid terrain height."))?
                            * weights[j];
                        if parent.encoding.has_vertex_normals {
                            normal = normal.add(
                                parent
                                    .encoding
                                    .decode_normal(&parent.vertices, ids[j])
                                    .unwrap()
                                    .multiply_by_scalar(weights[j]),
                            );
                        }
                    }
                    hmin = hmin.min(height);
                    hmax = hmax.max(height);
                    let c = Cartographic::new(
                        super::math::lerp(rectangle.west, rectangle.east, uv.x),
                        super::math::lerp(rectangle.south, rectangle.north, uv.y),
                        height,
                    );
                    let mut v = TerrainVertex::new(ellipsoid.cartographic_to_cartesian(&c), uv, height);
                    if parent.encoding.has_vertex_normals {
                        v.normal_to_pack = Some(AttributeCompression::oct_encode(normal.normalize()));
                    }
                    set_fields(
                        &mut v,
                        c.latitude,
                        &rectangle,
                        &ellipsoid,
                        parent.encoding.has_web_mercator_t,
                        parent.encoding.exaggeration,
                    );
                    if uv.x == 0. {
                        edge[0].push(i);
                    } else if uv.x == 1. {
                        edge[2].push(i);
                    }
                    if uv.y == 0. {
                        edge[1].push(i);
                    } else if uv.y == 1. {
                        edge[3].push(i);
                    }
                    samples.push(v);
                    i
                };
                poly_ids.push(i);
            }
            for j in 2..poly_ids.len() {
                output.extend([poly_ids[0], poly_ids[j - 1], poly_ids[j]]);
            }
        }
        edge[0].sort_by(|&a, &b| {
            samples[a as usize].texture_coordinates.y.total_cmp(&samples[b as usize].texture_coordinates.y)
        });
        edge[1].sort_by(|&a, &b| {
            samples[b as usize].texture_coordinates.x.total_cmp(&samples[a as usize].texture_coordinates.x)
        });
        edge[2].sort_by(|&a, &b| {
            samples[b as usize].texture_coordinates.y.total_cmp(&samples[a as usize].texture_coordinates.y)
        });
        edge[3].sort_by(|&a, &b| {
            samples[a as usize].texture_coordinates.x.total_cmp(&samples[b as usize].texture_coordinates.x)
        });
        let [west, south, east, north] = edge;
        let edges = TerrainEdgeIndices {
            west_indices_south_to_north: west,
            south_indices_east_to_west: south,
            east_indices_north_to_south: east,
            north_indices_west_to_east: north,
        };
        let mut c = rectangle.center();
        c.height = (parent.minimum_height + parent.maximum_height) * 0.5;
        let center = ellipsoid.cartographic_to_cartesian(&c);
        build_mesh(
            samples,
            output,
            edges,
            rectangle,
            ellipsoid,
            center,
            center,
            [hmin, hmax],
            None,
            skirt_height,
            parent.encoding.has_vertex_normals,
            parent.encoding.has_web_mercator_t,
            parent.encoding.exaggeration,
            parent.encoding.exaggeration_relative_height,
        )
    }
}
fn set_fields(v: &mut TerrainVertex, latitude: f64, r: &Rectangle, e: &Ellipsoid, merc: bool, exag: f64) {
    if merc {
        let south = WebMercatorProjection::geodetic_latitude_to_mercator_angle(r.south);
        v.web_mercator_t = Some(
            (WebMercatorProjection::geodetic_latitude_to_mercator_angle(latitude) - south)
                / (WebMercatorProjection::geodetic_latitude_to_mercator_angle(r.north) - south),
        );
    }
    if exag != 1. {
        v.geodetic_surface_normal = e.geodetic_surface_normal(v.position);
    }
}
fn decoded(encoding: &TerrainEncoding, buffer: &[f32], i: usize) -> Result<TerrainVertex, RuntimeError> {
    Ok(TerrainVertex {
        position: encoding.decode_position(buffer, i).ok_or_else(|| error("Invalid encoded terrain vertex."))?,
        texture_coordinates: encoding.decode_texture_coordinates(buffer, i).unwrap(),
        height: encoding.decode_height(buffer, i).unwrap(),
        normal_to_pack: if encoding.has_vertex_normals { encoding.get_oct_encoded_normal(buffer, i) } else { None },
        web_mercator_t: if encoding.has_web_mercator_t { encoding.decode_web_mercator_t(buffer, i) } else { None },
        geodetic_surface_normal: if encoding.has_geodetic_surface_normals {
            encoding.decode_geodetic_surface_normal(buffer, i)
        } else {
            None
        },
    })
}
#[allow(clippy::too_many_arguments)]
fn build_mesh(
    samples: Vec<TerrainVertex>,
    ids: Vec<u32>,
    edges: TerrainEdgeIndices,
    r: Rectangle,
    e: Ellipsoid,
    center: Cartesian3,
    approx: Cartesian3,
    heights: [f64; 2],
    bounds: Option<(BoundingSphere, OrientedBoundingBox, Option<Cartesian3>)>,
    skirt: f64,
    normals: bool,
    merc: bool,
    exag: f64,
    relative: f64,
) -> Result<TerrainMesh, RuntimeError> {
    let count = samples.len();
    let edge_slices = [
        &edges.west_indices_south_to_north,
        &edges.south_indices_east_to_west,
        &edges.east_indices_north_to_south,
        &edges.north_indices_west_to_east,
    ];
    if count == 0
        || edge_slices.iter().any(|s| s.is_empty() || s.iter().any(|&i| i as usize >= count))
        || ids.iter().any(|&i| i as usize >= count)
    {
        return Err(error("Invalid terrain topology."));
    }
    let skirt_count =
        terrain_provider::get_skirt_vertex_count(&edges).ok_or_else(|| error("Invalid terrain edges."))?;
    let total = count + skirt_count;
    let from = Transforms::east_north_up_to_fixed_frame(approx, &e);
    let to = from.inverse_transformation();
    let temp = TerrainEncoding::new(TerrainEncodingOptions {
        center: Some(center),
        has_vertex_normals: normals,
        has_web_mercator_t: merc,
        has_geodetic_surface_normals: exag != 1.,
        exaggeration: exag,
        exaggeration_relative_height: relative,
        ..Default::default()
    })
    .unwrap();
    let mut buffer = vec![0.; total * temp.stride];
    let (mut min, mut max) = (
        Cartesian3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Cartesian3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    let mut offset = 0;
    for v in &samples {
        offset = temp.encode(&mut buffer, offset, v).ok_or_else(|| error("Invalid terrain encoding."))?;
        let p = to.multiply_by_point(v.position);
        min = min.minimum_by_component(p);
        max = max.maximum_by_component(p);
    }
    let bounds = if let Some(b) = bounds {
        b
    } else {
        let values = buffer.iter().map(|&v| v as f64).collect::<Vec<_>>();
        let sphere = BoundingSphere::from_vertices(&values, center, temp.stride);
        let obb = OrientedBoundingBox::from_rectangle(&r, heights[0], heights[1], &e)
            .ok_or_else(|| error("Invalid terrain box."))?;
        let horizon = EllipsoidalOccluder::new(&e, None)
            .compute_horizon_culling_point_from_vertices_possibly_under_ellipsoid(
                center,
                &values,
                temp.stride,
                center,
                Some(heights[0]),
            );
        (sphere, obb, horizon)
    };
    for (k, edge) in edge_slices.iter().enumerate() {
        for &i in edge.iter() {
            let mut v = decoded(&temp, &buffer, i as usize)?;
            let uv = v.texture_coordinates;
            let dx = [-1., 0., 1., 0.][k] * 1e-4;
            let dy = [0., -1., 0., 1.][k] * 1e-4;
            let c = Cartographic::new(
                super::math::lerp(r.west, r.east, uv.x + dx),
                super::math::lerp(r.south, r.north, uv.y + dy)
                    .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
                v.height - skirt,
            );
            v.position = e.cartographic_to_cartesian(&c);
            v.height = c.height;
            if exag != 1. {
                v.geodetic_surface_normal = e.geodetic_surface_normal(v.position);
            }
            let p = to.multiply_by_point(v.position);
            min = min.minimum_by_component(p);
            max = max.maximum_by_component(p);
            offset = temp.encode(&mut buffer, offset, &v).ok_or_else(|| error("Invalid skirt encoding."))?;
        }
    }
    let final_encoding = TerrainEncoding::new(TerrainEncodingOptions {
        center: Some(center),
        axis_aligned_bounding_box: Some(AxisAlignedBoundingBox::new(min, max, None)),
        minimum_height: Some(heights[0] - skirt),
        maximum_height: Some(heights[1]),
        from_enu: Some(from),
        has_vertex_normals: normals,
        has_web_mercator_t: merc,
        has_geodetic_surface_normals: exag != 1.,
        exaggeration: exag,
        exaggeration_relative_height: relative,
    })
    .ok_or_else(|| error("Invalid terrain encoding."))?;
    let encoding = if final_encoding.quantization != temp.quantization {
        let mut final_buffer = vec![0.; total * final_encoding.stride];
        let mut offset = 0;
        for i in 0..total {
            offset = final_encoding.encode(&mut final_buffer, offset, &decoded(&temp, &buffer, i)?).unwrap();
        }
        buffer = final_buffer;
        final_encoding
    } else {
        temp
    };
    let index_count = ids.len();
    let skirt_indices = terrain_provider::get_skirt_index_count_with_filled_corners(skirt_count)
        .ok_or_else(|| error("Invalid terrain skirts."))?;
    let mut padded = ids;
    padded.resize(index_count + skirt_indices, 0);
    let mut indices = IndexDatatype::create_typed_array(total, &padded);
    terrain_provider::add_skirt_indices_with_filled_corners(&edges, count as u32, &mut indices, index_count)
        .ok_or_else(|| error("Invalid terrain skirt indices."))?;
    Ok(TerrainMesh::new(TerrainMeshOptions {
        center,
        vertices: buffer,
        indices,
        index_count_without_skirts: Some(index_count),
        vertex_count_without_skirts: count,
        minimum_height: heights[0],
        maximum_height: heights[1],
        rectangle: r,
        bounding_sphere_3d: bounds.0,
        occludee_point_in_scaled_space: bounds.2,
        vertex_stride: Some(encoding.stride),
        oriented_bounding_box: Some(bounds.1),
        encoding,
        edges,
    }))
}
