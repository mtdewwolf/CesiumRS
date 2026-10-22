//! Geoid shift, local ENU transform, outlines, flat normals, and the glTF document
//! `decodeI3S` returns instead of blob URLs.
//!
//! Cesium writes each attribute into its own buffer and always labels the
//! accessor `FLOAT`, even when the bytes are still the integer typed array the
//! decoder produced. Position accessor `min`/`max` are swapped. Opaque triangles
//! are packed in front of transparent ones when color splitting is on.

use super::components::{to_length, to_uint32, to_uint8, FeatureIndex, FeatureKey, FeatureMap, NumMap, Values};
use super::source::DecodedGeometry;
use super::{
    I3sAccessor, I3sBufferView, I3sDecodeParameters, I3sMesh, I3sMeshData, I3sNode, I3sPrimitive,
    I3sPrimitiveExtensions, I3sPropertyTable, I3sRootExtensions,
};
use crate::core::js_math::JsMath;
use crate::core::math::{js_max, js_min, to_radians};
use crate::core::{Cartesian3, Cartographic, Color, Ellipsoid, MapProjection, RuntimeError, WebMercatorProjection};

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

struct Channels {
    positions: Option<Values>,
    normals: Option<Values>,
    uv0s: Option<Values>,
    colors: Option<Values>,
    indices: Option<Values>,
    feature_index: Option<FeatureIndex>,
}

pub(super) fn build(
    mut geometry: DecodedGeometry,
    parameters: &I3sDecodeParameters,
) -> Result<I3sMeshData, RuntimeError> {
    if !parameters.geoid_data_list.is_empty() {
        let center =
            parameters.cartographic_center.ok_or_else(|| RuntimeError::new("cartographicCenter is required"))?;
        apply_geoid(&mut geometry, center, parameters)?;
    }
    transform_to_local(&mut geometry, parameters)?;
    if geometry.get("uv0s").is_some() && geometry.get("uv-region").is_some() {
        let count = geometry.vertex_count as usize;
        let regions = geometry.take("uv-region").unwrap();
        if let Some(uvs) = geometry.attributes.iter_mut().find(|(name, _)| name == "uv0s").map(|(_, v)| v) {
            crop_uvs(count, uvs, &regions);
        }
    }

    let mut channels = Channels {
        positions: geometry.take("positions"),
        normals: geometry.take("normals"),
        uv0s: geometry.take("uv0s"),
        colors: geometry.take("colors"),
        indices: geometry.indices.take(),
        feature_index: feature_index(&geometry, geometry.vertex_count)?,
    };
    // `faceRange` was consumed by feature_index via geometry.get, so take it only as a borrow above.
    // feature_index reads faceRange before we move channels. It already ran.

    let mut vertex_count = geometry.vertex_count;
    let mut vertex_count_is_array = geometry.vertex_count_is_array;
    if parameters.calculate_normals {
        let sourced = sourced_from(&channels, vertex_count, vertex_count_is_array);
        if let Some(generated) = generate_normals(&sourced) {
            channels.normals = Some(generated.normals);
            if let Some(count) = generated.vertex_count {
                vertex_count = count;
                vertex_count_is_array = false;
                channels.indices = None;
                channels.positions = generated.positions;
                channels.uv0s = generated.uv0s;
                channels.colors = generated.colors;
                channels.feature_index = generated.feature_index;
            }
        }
    }

    let mut document = gltf_document(sourced_from(&channels, vertex_count, vertex_count_is_array), parameters);
    document.positions = channels.positions.as_ref().map(Values::to_f64);
    document.indices = match channels.indices.as_ref() {
        Some(Values::U32(values)) => Some(values.clone()),
        Some(other) => Some(other.to_f64().into_iter().map(to_uint32).collect()),
        None => None,
    };
    document.feature_index = channels.feature_index.as_ref().map(FeatureIndex::to_options);
    document.source_url = parameters.url.clone();
    document.cartesian_center = parameters.cartesian_center;
    document.parent_rotation = parameters.parent_rotation;
    Ok(document)
}

/// `faceRange` writes `featureIndex[face * 3 + corner]`. An end of `0xFFFFFFFF`
/// would grow that array without bound and abort the worker while allocating.
/// Cesium hits the same range before `Float32Array` rejects it. Past this cap
/// the decode returns an error instead of reserving the slots.
const MAX_FEATURE_INDEX_LEN: usize = 1 << 24;

fn feature_index(geometry: &DecodedGeometry, vertex_count: u32) -> Result<Option<FeatureIndex>, RuntimeError> {
    if let Some(values) = geometry.get("feature-index") {
        return Ok(Some(FeatureIndex::from_values(values)));
    }
    let Some(face_range) = geometry.get("faceRange") else {
        return Ok(None);
    };
    let mut values = vec![None; vertex_count as usize];
    let mut range = 0;
    while range + 1 < face_range.len() {
        let Some(start) = integer_index(face_range.get(range)) else {
            range += 2;
            continue;
        };
        let Some(end) = integer_index(face_range.get(range + 1)) else {
            range += 2;
            continue;
        };
        let id = (range / 2) as f64;
        if end >= start {
            let slots = end.checked_mul(3).and_then(|count| count.checked_add(3));
            let Some(slots) = slots.filter(|slots| *slots <= MAX_FEATURE_INDEX_LEN) else {
                return Err(RuntimeError::new("faceRange exceeds the feature index limit"));
            };
            if slots > values.len() {
                values.resize(slots, None);
            }
            for face in start..=end {
                for corner in 0..3 {
                    values[face * 3 + corner] = Some(id);
                }
            }
        }
        range += 2;
    }
    Ok(Some(FeatureIndex::Sparse(values)))
}

fn integer_index(value: Option<f64>) -> Option<usize> {
    let value = value?;
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value < usize::MAX as f64 {
        Some(value as usize)
    } else {
        None
    }
}

fn apply_geoid(
    geometry: &mut DecodedGeometry,
    center: Cartographic,
    parameters: &I3sDecodeParameters,
) -> Result<(), RuntimeError> {
    let vertex_count = geometry.vertex_count;
    let scale_x = geometry.scale_x;
    let scale_y = geometry.scale_y;
    let has_positions = geometry.get("positions").is_some();
    // The center sample runs even when the vertex loop does not.
    let center_height = sample_geoid_from_list(center.longitude, center.latitude, parameters)?;
    if !has_positions && vertex_count > 0 {
        return Err(RuntimeError::new("Cannot adjust orthometric heights without positions"));
    }
    if let Some(positions) = geometry.attributes.iter_mut().find(|(name, _)| name == "positions").map(|(_, v)| v) {
        for i in 0..vertex_count as usize {
            let longitude = center.longitude + to_radians(scale_x * positions.get(i * 3).unwrap_or(f64::NAN));
            let latitude = center.latitude + to_radians(scale_y * positions.get(i * 3 + 1).unwrap_or(f64::NAN));
            let height = sample_geoid_from_list(longitude, latitude, parameters)?;
            let z = positions.get(i * 3 + 2).unwrap_or(f64::NAN) + height - center_height;
            positions.set(i * 3 + 2, z);
        }
    }
    Ok(())
}

fn sample_geoid_from_list(
    longitude: f64,
    latitude: f64,
    parameters: &I3sDecodeParameters,
) -> Result<f64, RuntimeError> {
    for geoid in &parameters.geoid_data_list {
        let (x, y) = if let Some(radii) = geoid.web_mercator_radii {
            let projection = WebMercatorProjection::new(Ellipsoid::new(radii.x, radii.y, radii.z));
            let projected = projection.project(&Cartographic::new(longitude, latitude, 0.0));
            (projected.x, projected.y)
        } else if geoid.web_mercator {
            return Err(RuntimeError::new("WebMercator geoid is missing ellipsoid radii"));
        } else {
            (longitude, latitude)
        };
        if x > geoid.west && x < geoid.east && y > geoid.south && y < geoid.north {
            return Ok(sample_geoid(x, y, geoid));
        }
    }
    Ok(0.0)
}

fn sample_geoid(sample_x: f64, sample_y: f64, geoid: &super::I3sGeoid) -> f64 {
    let mut x = ((sample_x - geoid.west) / (geoid.east - geoid.west)) * (geoid.width - 1.0);
    let mut y = ((sample_y - geoid.south) / (geoid.north - geoid.south)) * (geoid.height - 1.0);
    let xi = x.floor();
    let yi = y.floor();
    x -= xi;
    y -= yi;
    let x_next = if xi < geoid.width { xi + 1.0 } else { xi };
    let mut y_next = if yi < geoid.height { yi + 1.0 } else { yi };
    let yi = geoid.height - 1.0 - yi;
    y_next = geoid.height - 1.0 - y_next;
    let h00 = sample_map(xi, yi, geoid.width, &geoid.buffer);
    let h10 = sample_map(x_next, yi, geoid.width, &geoid.buffer);
    let h01 = sample_map(xi, y_next, geoid.width, &geoid.buffer);
    let h11 = sample_map(x_next, y_next, geoid.width, &geoid.buffer);
    let height = (h00 * (1.0 - x) + h10 * x) * (1.0 - y) + (h01 * (1.0 - x) + h11 * x) * y;
    height * geoid.scale + geoid.offset
}

fn sample_map(u: f64, v: f64, width: f64, data: &[f64]) -> f64 {
    let address = u + v * width;
    if !address.is_finite() || address < 0.0 || address.fract() != 0.0 || address >= data.len() as f64 {
        f64::NAN
    } else {
        data[address as usize]
    }
}

fn transform_to_local(geometry: &mut DecodedGeometry, parameters: &I3sDecodeParameters) -> Result<(), RuntimeError> {
    let positions_empty = geometry.get("positions").is_none_or(|values| values.is_empty());
    if (!geometry.vertex_count_is_array && geometry.vertex_count == 0) || positions_empty {
        return Ok(());
    }
    let center = parameters.cartographic_center.ok_or_else(|| RuntimeError::new("cartographicCenter is required"))?;
    let cartesian = parameters.cartesian_center.ok_or_else(|| RuntimeError::new("cartesianCenter is required"))?;
    let rotation = parameters.parent_rotation.ok_or_else(|| RuntimeError::new("parentRotation is required"))?;
    let radii =
        parameters.ellipsoid_radii_square.ok_or_else(|| RuntimeError::new("ellipsoidRadiiSquare is required"))?;
    let ellipsoid = Ellipsoid::new(radii.x.sqrt(), radii.y.sqrt(), radii.z.sqrt());
    let scale_x = geometry.scale_x;
    let scale_y = geometry.scale_y;
    let count = geometry.vertex_count as usize;
    let has_normals = geometry.get("normals").is_some();
    let positions =
        geometry.attributes.iter_mut().find(|(name, _)| name == "positions").map(|(_, values)| values).unwrap();
    for i in 0..count {
        let cartographic = Cartographic::new(
            center.longitude + to_radians(scale_x * positions.get(i * 3).unwrap_or(f64::NAN)),
            center.latitude + to_radians(scale_y * positions.get(i * 3 + 1).unwrap_or(f64::NAN)),
            center.height + positions.get(i * 3 + 2).unwrap_or(f64::NAN),
        );
        let position = ellipsoid.cartographic_to_cartesian(&cartographic).subtract(cartesian);
        let rotated = rotation.multiply_by_vector(position);
        positions.set(i * 3, rotated.x);
        positions.set(i * 3 + 1, rotated.y);
        positions.set(i * 3 + 2, rotated.z);
    }
    if has_normals {
        let normals =
            geometry.attributes.iter_mut().find(|(name, _)| name == "normals").map(|(_, values)| values).unwrap();
        for i in 0..count {
            let normal = Cartesian3::new(
                normals.get(i * 3).unwrap_or(f64::NAN),
                normals.get(i * 3 + 1).unwrap_or(f64::NAN),
                normals.get(i * 3 + 2).unwrap_or(f64::NAN),
            );
            let rotated = rotation.multiply_by_vector(normal);
            normals.set(i * 3, rotated.x);
            normals.set(i * 3 + 1, rotated.y);
            normals.set(i * 3 + 2, rotated.z);
        }
    }
    Ok(())
}

fn crop_uvs(vertex_count: usize, uvs: &mut Values, regions: &Values) {
    for vertex in 0..vertex_count {
        let min_u = regions.get(vertex * 4).unwrap_or(f64::NAN) / 65535.0;
        let min_v = regions.get(vertex * 4 + 1).unwrap_or(f64::NAN) / 65535.0;
        let scale_u =
            (regions.get(vertex * 4 + 2).unwrap_or(f64::NAN) - regions.get(vertex * 4).unwrap_or(f64::NAN)) / 65535.0;
        let scale_v = (regions.get(vertex * 4 + 3).unwrap_or(f64::NAN)
            - regions.get(vertex * 4 + 1).unwrap_or(f64::NAN))
            / 65535.0;
        let u = uvs.get(vertex * 2).unwrap_or(f64::NAN) * scale_u;
        uvs.set(vertex * 2, u);
        let u = uvs.get(vertex * 2).unwrap_or(f64::NAN) + min_u;
        uvs.set(vertex * 2, u);
        let v = uvs.get(vertex * 2 + 1).unwrap_or(f64::NAN) * scale_v;
        uvs.set(vertex * 2 + 1, v);
        let v = uvs.get(vertex * 2 + 1).unwrap_or(f64::NAN) + min_v;
        uvs.set(vertex * 2 + 1, v);
    }
}

#[derive(Clone, Copy)]
struct Sourced<'a> {
    vertex_count: u32,
    vertex_count_is_array: bool,
    indices: Option<&'a Values>,
    positions: Option<&'a Values>,
    normals: Option<&'a Values>,
    uv0s: Option<&'a Values>,
    colors: Option<&'a Values>,
    feature_index: Option<&'a FeatureIndex>,
}

struct GeneratedNormals {
    normals: Values,
    vertex_count: Option<u32>,
    positions: Option<Values>,
    uv0s: Option<Values>,
    colors: Option<Values>,
    feature_index: Option<FeatureIndex>,
}

fn sourced_from(channels: &Channels, vertex_count: u32, vertex_count_is_array: bool) -> Sourced<'_> {
    Sourced {
        vertex_count,
        vertex_count_is_array,
        indices: channels.indices.as_ref(),
        positions: channels.positions.as_ref(),
        normals: channels.normals.as_ref(),
        uv0s: channels.uv0s.as_ref(),
        colors: channels.colors.as_ref(),
        feature_index: channels.feature_index.as_ref(),
    }
}

fn generate_normals(source: &Sourced<'_>) -> Option<GeneratedNormals> {
    let Sourced { vertex_count, vertex_count_is_array, indices, positions, normals, uv0s, colors, feature_index } =
        *source;
    let positions_empty = positions.is_none_or(|values| values.is_empty());
    if (!vertex_count_is_array && vertex_count == 0) || positions_empty || normals.is_some() {
        return None;
    }
    let positions = positions.unwrap();
    if let Some(indices) = indices {
        let count = indices.len();
        let mut out_positions = vec![0.0f32; count * 3];
        let mut out_uvs = uv0s.map(|_| vec![0.0f32; count * 2]);
        let mut out_colors = colors.map(|_| vec![0u8; count * 4]);
        let mut out_features = feature_index.map(|_| vec![None; count]);
        for i in 0..count {
            let index = indices.get(i).map(to_uint32).unwrap_or(0) as usize;
            for k in 0..3 {
                out_positions[i * 3 + k] = positions.get(index * 3 + k).unwrap_or(f64::NAN) as f32;
            }
            if let Some(uvs) = uv0s {
                let dst = out_uvs.as_mut().unwrap();
                dst[i * 2] = uvs.get(index * 2).unwrap_or(f64::NAN) as f32;
                dst[i * 2 + 1] = uvs.get(index * 2 + 1).unwrap_or(f64::NAN) as f32;
            }
            if let Some(colors) = colors {
                let dst = out_colors.as_mut().unwrap();
                for k in 0..4 {
                    dst[i * 4 + k] = colors.get(index * 4 + k).map(to_uint8).unwrap_or(0);
                }
            }
            if let Some(features) = feature_index {
                out_features.as_mut().unwrap()[i] = features.get(index);
            }
        }
        let positions = Values::F32(out_positions);
        let mut flat = vec![0.0f32; count * 3];
        write_flat_normals(&mut flat, count, &positions);
        return Some(GeneratedNormals {
            normals: Values::F32(flat),
            vertex_count: Some(count as u32),
            positions: Some(positions),
            uv0s: out_uvs.map(Values::F32),
            colors: out_colors.map(Values::U8),
            feature_index: out_features.map(FeatureIndex::Sparse),
        });
    }
    let count = vertex_count as usize;
    let mut flat = vec![0.0f32; count * 3];
    write_flat_normals(&mut flat, count, positions);
    Some(GeneratedNormals {
        normals: Values::F32(flat),
        vertex_count: None,
        positions: None,
        uv0s: None,
        colors: None,
        feature_index: None,
    })
}

fn write_flat_normals(normals: &mut [f32], vertex_count: usize, positions: &Values) {
    // The loop steps by three past the last full triangle. Missing corners are
    // `undefined`, so the normal is NaN. Stores past the typed array are no-ops.
    let mut index = 0;
    while index < vertex_count {
        let normal = face_normal(positions, index, index + 1, index + 2);
        for corner in 0..3 {
            let base = (index + corner) * 3;
            if let Some(slot) = normals.get_mut(base..base + 3) {
                slot[0] = normal.x as f32;
                slot[1] = normal.y as f32;
                slot[2] = normal.z as f32;
            }
        }
        index += 3;
    }
}

fn face_normal(positions: &Values, a: usize, b: usize, c: usize) -> Cartesian3 {
    let a = cartesian_at(positions, a);
    let b = cartesian_at(positions, b);
    let c = cartesian_at(positions, c);
    let mut normal = b.subtract(a).cross(c.subtract(a));
    let magnitude = normal.magnitude();
    if magnitude != 0.0 {
        normal = normal.divide_by_scalar(magnitude);
    }
    normal
}

fn cartesian_at(positions: &Values, index: usize) -> Cartesian3 {
    Cartesian3::new(
        positions.get(index * 3).unwrap_or(f64::NAN),
        positions.get(index * 3 + 1).unwrap_or(f64::NAN),
        positions.get(index * 3 + 2).unwrap_or(f64::NAN),
    )
}

fn gltf_document(source: Sourced<'_>, parameters: &I3sDecodeParameters) -> I3sMeshData {
    let Sourced { mut vertex_count, vertex_count_is_array, indices, positions, normals, uv0s, colors, feature_index } =
        source;
    let positions_empty = positions.is_none_or(|values| values.is_empty());
    if (!vertex_count_is_array && vertex_count == 0) || positions_empty {
        return I3sMeshData::empty();
    }
    let Some(positions) = positions else {
        return I3sMeshData::empty();
    };
    if let Some(indices) = indices {
        vertex_count = indices.len() as u32;
    }
    let (index_array, transparent_offset) =
        generate_index_array(vertex_count as usize, indices, colors, parameters.split_geometry_by_color_transparency);
    let end_index = vertex_count as usize;

    let mut doc = GltfBuilder::default();
    let mut attributes = Vec::new();
    // `meshPositions.length / 3` can be fractional. This count is that quotient floored.
    let position_count = (end_index * 3).min(positions.len()) / 3;
    let position_bytes = positions.prefix_bytes(position_count * 3);
    let (min, max) = position_bounds(positions, position_count);
    // Cesium stores the real minimum in `max` and the real maximum in `min`.
    attributes.push(("POSITION", doc.push_position(position_bytes, position_count, min, max)));

    if let Some(normals) = normals {
        let bytes = normals.prefix_bytes(end_index * 3);
        let accessor = doc.push_vec(bytes, VecLayout::array(element_size(normals), 3, "VEC3"));
        attributes.push(("NORMAL", accessor));
    }
    if let Some(uvs) = uv0s {
        let bytes = uvs.prefix_bytes(end_index * 2);
        attributes.push(("TEXCOORD_0", doc.push_vec(bytes, VecLayout::array(element_size(uvs), 2, "VEC2"))));
    }
    if let Some(colors) = colors {
        let converted = convert_colors(colors, end_index * 4);
        let bytes = converted.prefix_bytes(converted.len());
        attributes.push(("COLOR_0", doc.push_vec(bytes, VecLayout::array(4, 4, "VEC4"))));
    }

    let mut extensions = I3sPrimitiveExtensions::default();
    let mut root = I3sRootExtensions::default();
    let mut used = Vec::new();
    if parameters.enable_features {
        if let Some(feature_index) = feature_index {
            let (_, feature_count) = feature_buffers(feature_index);
            // `subarray` clamps. The Float32Array has the full feature-index length.
            let full = feature_id_values(feature_index);
            let id_bytes = Values::F32(full).prefix_bytes(end_index);
            attributes.push(("_FEATURE_ID_0", doc.push_vec(id_bytes, VecLayout::elements(4, 1, FLOAT, "SCALAR"))));
            extensions.feature_count = Some(feature_count);
            used.push("EXT_mesh_features".to_string());
            let table_len = to_length(feature_count);
            let table: Vec<f32> = (0..table_len).map(|i| i as f32).collect();
            let view = doc.push_buffer(Values::F32(table).prefix_bytes(table_len), ELEMENT_ARRAY_BUFFER);
            root.property_table = Some(I3sPropertyTable { count: feature_count, values: view });
            used.push("EXT_structural_metadata".to_string());
        }
    }

    if let Some(outlines) = generate_outlines(&index_array, positions, normals, feature_index, parameters) {
        let bytes = Values::U32(outlines.clone()).prefix_bytes(outlines.len());
        let accessor = doc.push_vec(bytes, VecLayout::elements(4, 1, UNSIGNED_INT, "SCALAR"));
        extensions.outline = Some(accessor);
        used.push("CESIUM_primitive_outline".to_string());
    }

    let index_view =
        doc.push_buffer(Values::U32(index_array).prefix_bytes(vertex_count as usize), ELEMENT_ARRAY_BUFFER);
    let mut primitives = Vec::new();
    if transparent_offset > 0 {
        let accessor = doc.push_index_accessor(index_view, 0, transparent_offset);
        primitives.push(I3sPrimitive {
            attributes: attributes.clone(),
            indices: accessor,
            material: primitives.len(),
            extensions: extensions.clone(),
            transparent: false,
        });
    }
    if (transparent_offset as u32) < vertex_count {
        let accessor =
            doc.push_index_accessor(index_view, transparent_offset * 4, vertex_count as usize - transparent_offset);
        primitives.push(I3sPrimitive {
            attributes,
            indices: accessor,
            material: primitives.len(),
            extensions,
            transparent: true,
        });
    }

    I3sMeshData {
        buffers: doc.buffers,
        buffer_views: doc.views,
        accessors: doc.accessors,
        meshes: vec![I3sMesh { primitives }],
        nodes: vec![I3sNode { mesh: 0 }],
        nodes_in_scene: vec![0],
        root_extensions: Some(root),
        extensions_used: Some(used),
        positions: None,
        indices: None,
        feature_index: None,
        source_url: None,
        cartesian_center: None,
        parent_rotation: None,
    }
}

fn position_bounds(positions: &Values, count: usize) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for i in 0..count {
        for k in 0..3 {
            let value = positions.get(i * 3 + k).unwrap_or(f64::NAN);
            min[k] = js_min(min[k], value);
            max[k] = js_max(max[k], value);
        }
    }
    (min, max)
}

fn feature_id_values(feature_index: &FeatureIndex) -> Vec<f32> {
    (0..feature_index.len()).map(|i| feature_index.get(i).unwrap_or(f64::NAN) as f32).collect()
}

fn feature_buffers(feature_index: &FeatureIndex) -> (Vec<f32>, f64) {
    let mut feature_count = 0.0;
    for index in 0..feature_index.len() {
        // Holes are `undefined`; `undefined + 1` is NaN and does not raise the count.
        // The count uses the original value, before the Float32 store.
        if let Some(value) = feature_index.get(index) {
            let next = value + 1.0;
            if feature_count < next {
                feature_count = next;
            }
        }
    }
    (feature_id_values(feature_index), feature_count)
}

fn convert_colors(colors: &Values, requested: usize) -> Values {
    let count = requested.min(colors.len());
    let mut out = vec![0.0f32; count];
    let mut index = 0;
    while index < count {
        out[index] = srgb_byte(colors, index);
        if index + 1 < count {
            out[index + 1] = srgb_byte(colors, index + 1);
        }
        if index + 2 < count {
            out[index + 2] = srgb_byte(colors, index + 2);
        }
        if index + 3 < count {
            out[index + 3] = Color::byte_to_float(colors.get(index + 3).unwrap_or(f64::NAN)) as f32;
        }
        index += 4;
    }
    Values::F32(out)
}

fn srgb_byte(colors: &Values, index: usize) -> f32 {
    let value = colors.get(index).unwrap_or(f64::NAN);
    crate::core::srgb_to_linear::srgb_to_linear(Color::byte_to_float(value)) as f32
}

fn generate_index_array(
    vertex_count: usize,
    indices: Option<&Values>,
    colors: Option<&Values>,
    split: bool,
) -> (Vec<u32>, usize) {
    let mut index_array = vec![0u32; vertex_count];
    let vertex_of =
        |index: usize| -> Option<f64> { indices.map(|values| values.get(index)).unwrap_or(Some(index as f64)) };
    let transparent = |index: usize, colors: &Values| -> bool {
        let Some(vertex) = vertex_of(index).filter(|value| value.is_finite() && value.fract() == 0.0 && *value >= 0.0)
        else {
            return false;
        };
        let element = (vertex as usize).saturating_mul(4).saturating_add(3);
        colors.get(element).is_some_and(|alpha| alpha < 255.0)
    };
    if let Some(colors) = colors.filter(|_| split) {
        let mut offset = 0;
        let mut index = 0;
        while index < vertex_count {
            if !transparent(index, colors) && !transparent(index + 1, colors) && !transparent(index + 2, colors) {
                for corner in 0..3 {
                    write_index(&mut index_array, offset, vertex_of(index + corner));
                    offset += 1;
                }
            }
            index += 3;
        }
        if offset > 0 {
            let mut cursor = offset;
            index = 0;
            while index < vertex_count {
                if transparent(index, colors) || transparent(index + 1, colors) || transparent(index + 2, colors) {
                    for corner in 0..3 {
                        write_index(&mut index_array, cursor, vertex_of(index + corner));
                        cursor += 1;
                    }
                }
                index += 3;
            }
            (index_array, offset)
        } else {
            for index in 0..vertex_count {
                write_index(&mut index_array, index, vertex_of(index));
            }
            (index_array, 0)
        }
    } else {
        for index in 0..vertex_count {
            write_index(&mut index_array, index, vertex_of(index));
        }
        (index_array, vertex_count)
    }
}

fn write_index(index_array: &mut [u32], slot: usize, value: Option<f64>) {
    if let Some(slot) = index_array.get_mut(slot) {
        *slot = value.map(to_uint32).unwrap_or(0);
    }
}

fn generate_outlines(
    index_array: &[u32],
    positions: &Values,
    normals: Option<&Values>,
    feature_index: Option<&FeatureIndex>,
    parameters: &I3sDecodeParameters,
) -> Option<Vec<u32>> {
    let symbology = parameters.symbology.as_ref()?;
    if symbology.entries.is_empty() {
        return None;
    }
    let symbology: Vec<(FeatureKey, bool)> =
        symbology.entries.iter().map(|(name, edges)| (FeatureKey::parse_property(name), *edges)).collect();
    let mut features: FeatureMap<FeatureRecord> = FeatureMap::new();
    let mut index = 0;
    while index < index_array.len() {
        // A partial last triangle still runs. Past-the-end reads are `undefined`.
        let corner_at = |offset: usize| index_array.get(index + offset).copied();
        let key = match feature_index {
            Some(values) => {
                let feature = corner_at(0).and_then(|vertex| values.get(vertex as usize));
                FeatureKey::from_value(feature)
            }
            None => FeatureKey::Default,
        };
        let record = features.entry(key.clone(), || FeatureRecord::new(has_edges(&symbology, &key)));
        if record.has_outline {
            for corner in 0..3 {
                record.add_vertex(corner_at(corner), positions);
            }
            for corner in 0..3 {
                let a = corner_at(corner);
                let b = corner_at((corner + 1) % 3);
                let unique_a = record.unique(a);
                let unique_b = record.unique(b);
                record.add_edge(a, b, unique_a, unique_b, index as u32);
            }
        }
        index += 3;
    }
    let mut outlines = Vec::new();
    // Replace the normal buffer once, up front, when it cannot cover every corner.
    let mut normal_buf = if normals.is_some_and(|values| index_array.len() * 3 == values.len()) {
        NormalBuf::Source(normals.unwrap())
    } else {
        NormalBuf::Plain(Vec::new())
    };
    for record in features.values_in_key_order() {
        for ends in record.edges.values_in_key_order() {
            for edge in ends.values_in_key_order() {
                if edge_is_smooth(edge, index_array, positions, &mut normal_buf) {
                    continue;
                }
                outlines.extend_from_slice(&edge.outlines);
            }
        }
    }
    if outlines.is_empty() {
        None
    } else {
        Some(outlines)
    }
}

fn has_edges(symbology: &[(FeatureKey, bool)], key: &FeatureKey) -> bool {
    if let Some((_, edges)) = symbology.iter().find(|(name, _)| name == key) {
        return *edges;
    }
    symbology.iter().find(|(name, _)| *name == FeatureKey::Default).map(|(_, edges)| *edges).unwrap_or(false)
}

struct FeatureRecord {
    unique_of: Vec<Option<u32>>,
    coordinates: CoordNode,
    edges: NumMap<NumMap<EdgeData>>,
    has_outline: bool,
}

impl FeatureRecord {
    fn new(has_outline: bool) -> Self {
        Self { unique_of: Vec::new(), coordinates: CoordNode::default(), edges: NumMap::new(), has_outline }
    }

    fn add_vertex(&mut self, vertex: Option<u32>, positions: &Values) {
        let Some(vertex) = vertex else {
            // `undefined * 3` reads `positions[NaN]`, and that object key is `"undefined"`.
            let _node = descend_undefined(&mut self.coordinates);
            return;
        };
        if self.unique(Some(vertex)).is_some() {
            return;
        }
        let coords = [
            positions.get(vertex as usize * 3).unwrap_or(f64::NAN),
            positions.get(vertex as usize * 3 + 1).unwrap_or(f64::NAN),
            positions.get(vertex as usize * 3 + 2).unwrap_or(f64::NAN),
        ];
        let node = descend(&mut self.coordinates, &coords);
        if node.index.is_none() {
            node.index = Some(vertex);
        }
        let unique = node.index.unwrap();
        let slot = vertex as usize;
        if self.unique_of.len() <= slot {
            self.unique_of.resize(slot + 1, None);
        }
        self.unique_of[slot] = Some(unique);
    }

    fn unique(&self, vertex: Option<u32>) -> Option<u32> {
        self.unique_of.get(vertex? as usize).copied().flatten()
    }

    fn add_edge(
        &mut self,
        a: Option<u32>,
        b: Option<u32>,
        unique_a: Option<u32>,
        unique_b: Option<u32>,
        normal_index: u32,
    ) {
        // `undefined < n` is false, so a missing unique index takes the else order.
        let (start, end) = match (unique_a, unique_b) {
            (Some(left), Some(right)) if left < right => (Some(left), Some(right)),
            _ => (unique_b, unique_a),
        };
        let ends = match start {
            Some(index) => self.edges.entry(f64::from(index), NumMap::new),
            None => self.edges.entry_missing(NumMap::new),
        };
        let edge = match end {
            Some(index) => ends.entry(f64::from(index), EdgeData::default),
            None => ends.entry_missing(EdgeData::default),
        };
        edge.normals_index.push(normal_index);
        if edge.outlines.is_empty() || a != unique_a || b != unique_b {
            // `new Uint32Array` turns `undefined` into 0.
            edge.outlines.extend_from_slice(&[a.unwrap_or(0), b.unwrap_or(0)]);
        }
    }
}

#[derive(Clone, Debug, Default)]
struct EdgeData {
    normals_index: Vec<u32>,
    outlines: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
struct CoordNode {
    children: NumMap<CoordNode>,
    index: Option<u32>,
}

fn descend_undefined(node: &mut CoordNode) -> &mut CoordNode {
    let mut node = node;
    for _ in 0..3 {
        node = node.children.entry_missing(CoordNode::default);
    }
    node
}

fn descend<'a>(node: &'a mut CoordNode, coords: &[f64]) -> &'a mut CoordNode {
    let mut node = node;
    for coord in coords {
        node = node.children.entry(*coord, CoordNode::default);
    }
    node
}

enum NormalBuf<'a> {
    Source(&'a Values),
    Plain(Vec<Option<f64>>),
}

fn normal_len(normals: &NormalBuf<'_>) -> usize {
    match normals {
        NormalBuf::Source(values) => values.len(),
        NormalBuf::Plain(values) => values.len(),
    }
}

fn edge_is_smooth(edge: &EdgeData, index_array: &[u32], positions: &Values, normals: &mut NormalBuf<'_>) -> bool {
    if edge.normals_index.len() <= 1 {
        return false;
    }
    // Sampled once per edge, before flat normals grow the plain array.
    let by_position = positions.len() == normal_len(normals);
    for (index_a, &corner_a) in edge.normals_index.iter().enumerate() {
        ensure_face_normal(normals, corner_a as usize, index_array, positions);
        if index_a == 0 {
            continue;
        }
        for &corner_b in edge.normals_index.iter().take(index_a) {
            let normal_a = normal_index(by_position, index_array, corner_a);
            let normal_b = normal_index(by_position, index_array, corner_b);
            if is_edge_smooth(normals, normal_a, normal_b) {
                return true;
            }
        }
    }
    false
}

fn normal_index(by_position: bool, index_array: &[u32], corner: u32) -> usize {
    if by_position {
        match index_array.get(corner as usize) {
            Some(&vertex) => vertex as usize * 3,
            // `undefined * 3` is NaN, and the normal read misses.
            None => usize::MAX / 4,
        }
    } else {
        corner as usize * 3
    }
}

fn ensure_face_normal(normals: &mut NormalBuf<'_>, corner: usize, index_array: &[u32], positions: &Values) {
    let missing = match normals {
        NormalBuf::Source(values) => values.get(corner * 3).is_none(),
        NormalBuf::Plain(values) => values.get(corner * 3).copied().flatten().is_none(),
    };
    if missing {
        let normal = face_normal_indexed(positions, corner, index_array);
        pack_normal(normals, corner * 3, normal);
        pack_normal(normals, (corner + 1) * 3, normal);
        pack_normal(normals, (corner + 2) * 3, normal);
    }
}

fn face_normal_indexed(positions: &Values, corner: usize, index_array: &[u32]) -> Cartesian3 {
    let at = |offset: usize| match index_array.get(corner + offset) {
        Some(&vertex) => cartesian_at(positions, vertex as usize),
        None => Cartesian3::new(f64::NAN, f64::NAN, f64::NAN),
    };
    let mut normal = at(1).subtract(at(0)).cross(at(2).subtract(at(0)));
    let magnitude = normal.magnitude();
    if magnitude != 0.0 {
        normal = normal.divide_by_scalar(magnitude);
    }
    normal
}

fn pack_normal(normals: &mut NormalBuf<'_>, index: usize, normal: Cartesian3) {
    // In-range typed-array elements are always defined, so a source buffer is
    // never written. An out-of-range store on a typed array is a no-op.
    let NormalBuf::Plain(values) = normals else {
        return;
    };
    if values.len() < index + 3 {
        values.resize(index + 3, None);
    }
    values[index] = Some(normal.x);
    values[index + 1] = Some(normal.y);
    values[index + 2] = Some(normal.z);
}

fn is_edge_smooth(normals: &NormalBuf<'_>, a: usize, b: usize) -> bool {
    let a = normal_at(normals, a);
    let b = normal_at(normals, b);
    let cosine = a.dot(b);
    let sine = a.cross(b).magnitude();
    sine.js_atan2(cosine) < 0.25
}

fn normal_at(normals: &NormalBuf<'_>, index: usize) -> Cartesian3 {
    let component = |offset: usize| match normals {
        NormalBuf::Source(values) => values.get(index + offset).unwrap_or(f64::NAN),
        NormalBuf::Plain(values) => values.get(index + offset).copied().flatten().unwrap_or(f64::NAN),
    };
    Cartesian3::new(component(0), component(1), component(2))
}

struct VecLayout {
    element_size: usize,
    components: usize,
    target: u32,
    component_type: u32,
    type_name: &'static str,
}

impl VecLayout {
    fn array(element_size: usize, components: usize, type_name: &'static str) -> Self {
        Self { element_size, components, target: ARRAY_BUFFER, component_type: FLOAT, type_name }
    }

    fn elements(element_size: usize, components: usize, component_type: u32, type_name: &'static str) -> Self {
        Self { element_size, components, target: ELEMENT_ARRAY_BUFFER, component_type, type_name }
    }
}

#[derive(Default)]
struct GltfBuilder {
    buffers: Vec<Vec<u8>>,
    views: Vec<I3sBufferView>,
    accessors: Vec<I3sAccessor>,
}

impl GltfBuilder {
    fn push_buffer(&mut self, bytes: Vec<u8>, target: u32) -> usize {
        let view = self.views.len();
        self.views.push(I3sBufferView { buffer: self.buffers.len(), byte_offset: 0, byte_length: bytes.len(), target });
        self.buffers.push(bytes);
        view
    }

    fn push_vec(&mut self, bytes: Vec<u8>, layout: VecLayout) -> usize {
        // Same floored quotient as the position accessor. JavaScript keeps the fraction.
        let count = if layout.element_size == 0 || layout.components == 0 {
            0
        } else {
            bytes.len() / layout.element_size / layout.components
        };
        let view = self.push_buffer(bytes, layout.target);
        self.accessors.push(I3sAccessor {
            buffer_view: view,
            byte_offset: 0,
            component_type: layout.component_type,
            count,
            type_name: layout.type_name,
            min: None,
            max: None,
        });
        self.accessors.len() - 1
    }

    /// `max` is the real minimum and `min` is the real maximum.
    fn push_position(&mut self, bytes: Vec<u8>, count: usize, max: [f64; 3], min: [f64; 3]) -> usize {
        let view = self.push_buffer(bytes, ARRAY_BUFFER);
        self.accessors.push(I3sAccessor {
            buffer_view: view,
            byte_offset: 0,
            component_type: FLOAT,
            count,
            type_name: "VEC3",
            min: Some(min),
            max: Some(max),
        });
        self.accessors.len() - 1
    }

    fn push_index_accessor(&mut self, view: usize, byte_offset: usize, count: usize) -> usize {
        self.accessors.push(I3sAccessor {
            buffer_view: view,
            byte_offset: byte_offset as u32,
            component_type: UNSIGNED_INT,
            count,
            type_name: "SCALAR",
            min: None,
            max: None,
        });
        self.accessors.len() - 1
    }
}

fn element_size(values: &Values) -> usize {
    match values {
        Values::F32(_) | Values::I32(_) | Values::U32(_) => 4,
        Values::I16(_) | Values::U16(_) => 2,
        Values::I8(_) | Values::U8(_) => 1,
    }
}
