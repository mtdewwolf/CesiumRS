//! CesiumJS `GeometryPipeline`: in-place operations on geometries and
//! instances — wireframes, vertex-cache reordering, splitting for 16-bit
//! indices, 2D projection, high/low encoding, world transforms, combining
//! instances, normals and tangents, vertex compression, and splitting along
//! the antimeridian.
//!
//! Functions return `None` where Cesium throws or reads past its arrays:
//! missing attributes or indices, attributes of different lengths, and
//! out-of-range indices. Where Cesium builds attributes in plain arrays the
//! values are stored as `Float64` and indices as `Uint32`.

use std::collections::HashMap;

use super::attribute_compression::AttributeCompression;
use super::barycentric_coordinates::barycentric_coordinates;
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::component_datatype::ComponentDatatype;
use super::encoded_cartesian3::EncodedCartesian3;
use super::geographic_projection::GeographicProjection;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryType};
use super::geometry_instance::GeometryInstance;
use super::intersect::Intersect;
use super::intersection_tests::line_segment_plane;
use super::map_projection::MapProjection;
use super::math::{lerp, sign, EPSILON10, EPSILON6, EPSILON9, SIXTY_FOUR_KILOBYTES};
use super::matrix4::Matrix4;
use super::plane::Plane;
use super::primitive_type::PrimitiveType;
use super::tipsify::tipsify;
use super::typed_array::TypedArray;

/// A value of a typed or plain array; NaN past the end, like `undefined`.
fn at(values: &TypedArray, i: usize) -> f64 {
    values.get(i).unwrap_or(f64::NAN)
}

fn c2_at(values: &TypedArray, i: usize) -> Cartesian2 {
    Cartesian2::new(at(values, i), at(values, i + 1))
}

fn c3_at(values: &TypedArray, i: usize) -> Cartesian3 {
    Cartesian3::new(at(values, i), at(values, i + 1), at(values, i + 2))
}

fn c4_at(values: &TypedArray, i: usize) -> Cartesian4 {
    Cartesian4::new(at(values, i), at(values, i + 1), at(values, i + 2), at(values, i + 3))
}

/// Writes into a plain array, growing it with holes (NaN) as JavaScript does.
fn set_at(values: &mut Vec<f64>, i: usize, x: f64) {
    if i >= values.len() {
        values.resize(i + 1, f64::NAN);
    }
    values[i] = x;
}

/// `IndexDatatype.createTypedArray(numberOfVertices, values)`.
fn index_array(number_of_vertices: f64, values: &[f64]) -> TypedArray {
    let datatype = if number_of_vertices >= f64::from(SIXTY_FOUR_KILOBYTES) {
        ComponentDatatype::UnsignedInt
    } else {
        ComponentDatatype::UnsignedShort
    };
    TypedArray::from_values(datatype, values)
}

/// A typed-array length; `None` where JavaScript throws a `RangeError`.
fn array_length(length: f64) -> Option<usize> {
    (length >= 0.0 && length.fract() == 0.0).then_some(length as usize)
}

fn as_index(value: f64) -> Option<usize> {
    (value >= 0.0 && value.fract() == 0.0).then_some(value as usize)
}

// ---- Wireframes ----------------------------------------------------------

fn add_triangle(lines: &mut [f64], index: usize, i0: f64, i1: f64, i2: f64) {
    for (k, v) in [i0, i1, i1, i2, i2, i0].into_iter().enumerate() {
        if let Some(slot) = lines.get_mut(index + k) {
            *slot = v;
        }
    }
}

fn triangles_to_lines(triangles: &[f64]) -> Option<TypedArray> {
    let count = triangles.len();
    let mut lines = vec![0.0; array_length(count as f64 / 3.0 * 6.0)?];
    for (t, tri) in triangles.chunks_exact(3).enumerate() {
        add_triangle(&mut lines, t * 6, tri[0], tri[1], tri[2]);
    }
    Some(index_array(count as f64, &lines))
}

fn triangle_strip_to_lines(triangles: &[f64]) -> TypedArray {
    let count = triangles.len();
    if count < 3 {
        return TypedArray::Uint16(Vec::new());
    }
    let mut lines = vec![0.0; (count - 2) * 6];
    add_triangle(&mut lines, 0, triangles[0], triangles[1], triangles[2]);
    for i in 3..count {
        add_triangle(&mut lines, (i - 2) * 6, triangles[i - 1], triangles[i], triangles[i - 2]);
    }
    index_array(count as f64, &lines)
}

fn triangle_fan_to_lines(triangles: &[f64]) -> Option<TypedArray> {
    if triangles.is_empty() {
        return Some(TypedArray::Uint16(Vec::new()));
    }
    let count = triangles.len() - 1;
    let mut lines = vec![0.0; array_length((count as f64 - 1.0) * 6.0)?];
    let base = triangles[0];
    for i in 1..count {
        add_triangle(&mut lines, (i - 1) * 6, base, triangles[i], triangles[i + 1]);
    }
    Some(index_array(count as f64, &lines))
}

/// Converts triangle indices to line indices for a wireframe.
pub fn to_wireframe(geometry: &mut Geometry) -> Option<()> {
    let Some(indices) = &geometry.indices else { return Some(()) };
    let triangles = indices.to_f64_vec();
    let lines = match geometry.primitive_type {
        PrimitiveType::Triangles => Some(triangles_to_lines(&triangles)?),
        PrimitiveType::TriangleStrip => Some(triangle_strip_to_lines(&triangles)),
        PrimitiveType::TriangleFan => Some(triangle_fan_to_lines(&triangles)?),
        _ => None,
    };
    if let Some(lines) = lines {
        geometry.indices = Some(lines);
    }
    geometry.primitive_type = PrimitiveType::Lines;
    Some(())
}

/// Line segments from each position along a vector attribute (default
/// `normal`), `length` (default 10000) long.
pub fn create_line_segments_for_vectors(
    geometry: &Geometry,
    attribute_name: Option<&str>,
    length: Option<f64>,
) -> Option<Geometry> {
    let length = length.unwrap_or(10000.0);
    let positions = &geometry.attributes.position.as_ref()?.values;
    let vectors = &geometry.attributes.get(attribute_name.unwrap_or("normal"))?.values;
    let positions_length = positions.len();
    let mut new_positions = Vec::with_capacity(2 * positions_length);
    for i in (0..positions_length).step_by(3) {
        let p = c3_at(positions, i);
        let v = c3_at(vectors, i);
        new_positions.extend([p.x, p.y, p.z]);
        new_positions.extend([p.x + v.x * length, p.y + v.y * length, p.z + v.z * length]);
    }
    new_positions.resize(2 * positions_length, 0.0);
    let attributes = GeometryAttributes {
        position: Some(GeometryAttribute::doubles(3, new_positions)),
        ..Default::default()
    };
    let mut result = Geometry::new(attributes, None, PrimitiveType::Lines);
    result.bounding_sphere = geometry.bounding_sphere.map(|bs| BoundingSphere::new(bs.center, bs.radius + length));
    Some(result)
}

/// Shader attribute locations: well-known attributes first, then the rest
/// in iteration order.
pub fn create_attribute_locations(geometry: &Geometry) -> Vec<(String, u32)> {
    const SEMANTICS: [&str; 14] = [
        "position",
        "positionHigh",
        "positionLow",
        "position3DHigh",
        "position3DLow",
        "position2DHigh",
        "position2DLow",
        "pickColor",
        "normal",
        "st",
        "tangent",
        "bitangent",
        "extrudeDirection",
        "compressedAttributes",
    ];
    let names = geometry.attributes.names();
    let mut locations: Vec<(String, u32)> = Vec::new();
    for semantic in SEMANTICS {
        if names.iter().any(|n| n == semantic) {
            locations.push((semantic.to_string(), locations.len() as u32));
        }
    }
    for name in names {
        if !locations.iter().any(|(n, _)| *n == name) {
            let j = locations.len() as u32;
            locations.push((name, j));
        }
    }
    locations
}

// ---- Vertex cache ----------------------------------------------------------

/// Renumbers vertices in the order the indices first use them and reorders
/// the attributes to match, dropping unused vertices.
pub fn reorder_for_pre_vertex_cache(geometry: &mut Geometry) -> Option<()> {
    let Some(indices) = &geometry.indices else { return Some(()) };
    let num_vertices = geometry.compute_number_of_vertices()?;
    let nv = array_length(num_vertices)?;
    let mut cross_reference = vec![-1i64; nv];
    let indices_in = indices.to_f64_vec();
    let mut indices_out = Vec::with_capacity(indices_in.len());
    let mut next_index = 0i64;
    for &index in &indices_in {
        match as_index(index).and_then(|i| cross_reference.get(i).copied().map(|t| (i, t))) {
            // An index past the vertices reads `undefined`, stored as 0.
            None => indices_out.push(f64::NAN),
            Some((_, t)) if t != -1 => indices_out.push(t as f64),
            Some((i, _)) => {
                cross_reference[i] = next_index;
                indices_out.push(next_index as f64);
                next_index += 1;
            }
        }
    }
    geometry.indices = Some(index_array(num_vertices, &indices_out));
    for name in geometry.attributes.names() {
        let attribute = geometry.attributes.get_mut(&name)?;
        let n = usize::from(attribute.components_per_attribute);
        let mut out = vec![0.0; next_index as usize * n];
        for (v, &t) in cross_reference.iter().enumerate() {
            if t != -1 {
                for j in 0..n {
                    out[n * t as usize + j] = at(&attribute.values, n * v + j);
                }
            }
        }
        attribute.values = TypedArray::from_values(attribute.component_datatype, &out);
    }
    Some(())
}

/// Reorders triangles with Tipsify for a post-transform vertex cache of
/// `cache_capacity` vertices (default 24).
pub fn reorder_for_post_vertex_cache(geometry: &mut Geometry, cache_capacity: Option<usize>) {
    if geometry.primitive_type != PrimitiveType::Triangles {
        return;
    }
    let Some(indices) = &geometry.indices else { return };
    let values: Vec<u32> = indices.to_f64_vec().iter().map(|&v| v as u32).collect();
    let maximum_index = values.iter().copied().max().unwrap_or(0);
    if let Some(out) = tipsify(&values, Some(maximum_index), cache_capacity.unwrap_or(24)) {
        geometry.indices = Some(TypedArray::Uint32(out));
    }
}

// ---- Splitting for 16-bit indices -----------------------------------------

struct PlainAttributes(Vec<(String, GeometryAttribute, Vec<f64>)>);

impl PlainAttributes {
    fn describe(attributes: &GeometryAttributes) -> Self {
        Self(attributes.iter().map(|(n, a)| (n.to_string(), a.clone(), Vec::new())).collect())
    }

    fn into_attributes(self) -> GeometryAttributes {
        let mut attributes = GeometryAttributes::default();
        for (name, template, values) in self.0 {
            attributes.set(
                &name,
                GeometryAttribute {
                    component_datatype: template.component_datatype,
                    components_per_attribute: template.components_per_attribute,
                    normalize: template.normalize,
                    values: TypedArray::Float64(values),
                },
            );
        }
        attributes
    }
}

/// Splits a geometry with 64K or more vertices into geometries whose
/// indices fit in 16 bits. Other geometries are returned unchanged.
pub fn fit_to_unsigned_short_indices(geometry: &Geometry) -> Option<Vec<Geometry>> {
    let number_of_vertices = geometry.compute_number_of_vertices()?;
    let Some(original) = geometry.indices.as_ref().filter(|_| number_of_vertices >= f64::from(SIXTY_FOUR_KILOBYTES))
    else {
        return Some(vec![geometry.clone()]);
    };
    let per_primitive = match geometry.primitive_type {
        PrimitiveType::Triangles => 3,
        PrimitiveType::Lines => 2,
        PrimitiveType::Points => 1,
        // Cesium steps by `undefined` and produces nothing.
        _ => return Some(Vec::new()),
    };
    let original = original.to_f64_vec();
    if !original.len().is_multiple_of(per_primitive) {
        return None;
    }
    let limit = SIXTY_FOUR_KILOBYTES as usize;
    let mut geometries = Vec::new();
    let make = |attributes: PlainAttributes, indices: Vec<f64>| {
        let mut g = Geometry::new(
            attributes.into_attributes(),
            Some(TypedArray::Uint32(indices.iter().map(|&i| i as u32).collect())),
            geometry.primitive_type,
        );
        g.bounding_sphere = geometry.bounding_sphere;
        g.bounding_sphere_cv = geometry.bounding_sphere_cv;
        g
    };
    let mut old_to_new: HashMap<u64, usize> = HashMap::new();
    let mut new_indices = Vec::new();
    let mut new_attributes = PlainAttributes::describe(&geometry.attributes);
    for primitive in original.chunks_exact(per_primitive) {
        for &x in primitive {
            let i = match old_to_new.get(&x.to_bits()) {
                Some(&i) => i,
                None => {
                    let i = old_to_new.len();
                    old_to_new.insert(x.to_bits(), i);
                    let vertex = as_index(x)?;
                    for (_, attribute, values) in new_attributes.0.iter_mut() {
                        let n = usize::from(attribute.components_per_attribute);
                        for k in 0..n {
                            values.push(at(&attribute.values, vertex * n + k));
                        }
                    }
                    i
                }
            };
            new_indices.push(i as f64);
        }
        if old_to_new.len() + per_primitive >= limit {
            let attributes = std::mem::replace(&mut new_attributes, PlainAttributes::describe(&geometry.attributes));
            geometries.push(make(attributes, std::mem::take(&mut new_indices)));
            old_to_new.clear();
        }
    }
    if !new_indices.is_empty() {
        geometries.push(make(new_attributes, new_indices));
    }
    Some(geometries)
}

// ---- Projection and encoding -----------------------------------------------

/// Projects a position attribute to 2D: the original is kept as
/// `attribute_name_3d`, the projection is added as `attribute_name_2d`, and
/// `attribute_name` is removed. The projection defaults to geographic on
/// WGS84.
pub fn project_to_2d(
    geometry: &mut Geometry,
    attribute_name: &str,
    attribute_name_3d: &str,
    attribute_name_2d: &str,
    projection: Option<&dyn MapProjection>,
) -> Option<()> {
    let default_projection = GeographicProjection::default();
    let projection = projection.unwrap_or(&default_projection);
    let ellipsoid = projection.ellipsoid();
    let attribute = geometry.attributes.get(attribute_name)?.clone();
    let values_3d = &attribute.values;
    let mut projected = vec![0.0; values_3d.len()];
    for i in (0..values_3d.len()).step_by(3) {
        let lon_lat = ellipsoid.cartesian_to_cartographic(c3_at(values_3d, i))?;
        let p = projection.project(&lon_lat);
        for (k, v) in [p.x, p.y, p.z].into_iter().enumerate() {
            if let Some(slot) = projected.get_mut(i + k) {
                *slot = v;
            }
        }
    }
    geometry.attributes.set(attribute_name_3d, attribute);
    geometry.attributes.set(attribute_name_2d, GeometryAttribute::doubles(3, projected));
    geometry.attributes.remove(attribute_name);
    Some(())
}

/// Splits a double-precision attribute into high and low `Float32`
/// attributes and removes the original.
pub fn encode_attribute(
    geometry: &mut Geometry,
    attribute_name: &str,
    attribute_high_name: &str,
    attribute_low_name: &str,
) -> Option<()> {
    let attribute = geometry.attributes.get(attribute_name)?;
    let components = attribute.components_per_attribute;
    let (high, low): (Vec<f64>, Vec<f64>) = attribute
        .values
        .to_f64_vec()
        .into_iter()
        .map(|v| {
            let e = EncodedCartesian3::encode(v);
            (e.high, e.low)
        })
        .unzip();
    geometry.attributes.set(attribute_high_name, GeometryAttribute::floats(components, &high));
    geometry.attributes.set(attribute_low_name, GeometryAttribute::floats(components, &low));
    geometry.attributes.remove(attribute_name);
    Some(())
}

// ---- World coordinates and combining ---------------------------------------

fn transform_point(matrix: &Matrix4, attribute: Option<&mut GeometryAttribute>) {
    let Some(attribute) = attribute else { return };
    let values = &mut attribute.values;
    for i in (0..values.len()).step_by(3) {
        let p = matrix.multiply_by_point(c3_at(values, i));
        values.set(i, p.x);
        values.set(i + 1, p.y);
        values.set(i + 2, p.z);
    }
}

fn transform_vector(matrix: &super::matrix3::Matrix3, attribute: Option<&mut GeometryAttribute>) {
    let Some(attribute) = attribute else { return };
    let values = &mut attribute.values;
    for i in (0..values.len()).step_by(3) {
        let v = matrix.multiply_by_vector(c3_at(values, i)).normalize();
        values.set(i, v.x);
        values.set(i + 1, v.y);
        values.set(i + 2, v.z);
    }
}

/// Applies the instance's model matrix to its positions, normals, tangents
/// and bounding sphere, then resets the matrix to the identity. `None` if
/// the instance has no geometry or the matrix cannot be inverted for normals.
pub fn transform_to_world_coordinates(instance: &mut GeometryInstance) -> Option<()> {
    let model_matrix = instance.model_matrix;
    if model_matrix == Matrix4::IDENTITY {
        return Some(());
    }
    let geometry = instance.geometry.as_mut()?;
    let attributes = &mut geometry.attributes;
    transform_point(&model_matrix, attributes.position.as_mut());
    transform_point(&model_matrix, attributes.get_mut("prevPosition"));
    transform_point(&model_matrix, attributes.get_mut("nextPosition"));
    if attributes.normal.is_some() || attributes.tangent.is_some() || attributes.bitangent.is_some() {
        let normal_matrix = model_matrix.inverse().ok()?.transpose().get_matrix3();
        transform_vector(&normal_matrix, attributes.normal.as_mut());
        transform_vector(&normal_matrix, attributes.tangent.as_mut());
        transform_vector(&normal_matrix, attributes.bitangent.as_mut());
    }
    if let Some(bs) = geometry.bounding_sphere {
        geometry.bounding_sphere = Some(bs.transform(&model_matrix));
    }
    instance.model_matrix = Matrix4::IDENTITY;
    Some(())
}

fn combine_geometries(geometries: &[&Geometry]) -> Option<Geometry> {
    let first = geometries[0];
    let mut attributes = GeometryAttributes::default();
    for (name, attribute) in first.attributes.iter() {
        let mut values = Vec::new();
        let mut in_all = true;
        for g in geometries {
            match g.attributes.get(name) {
                Some(other)
                    if other.component_datatype == attribute.component_datatype
                        && other.components_per_attribute == attribute.components_per_attribute
                        && other.normalize == attribute.normalize =>
                {
                    values.extend(other.values.to_f64_vec())
                }
                _ => {
                    in_all = false;
                    break;
                }
            }
        }
        if in_all {
            attributes.set(
                name,
                GeometryAttribute {
                    component_datatype: attribute.component_datatype,
                    components_per_attribute: attribute.components_per_attribute,
                    normalize: attribute.normalize,
                    values: TypedArray::from_values(attribute.component_datatype, &values),
                },
            );
        }
    }

    let indices = if first.indices.is_some() {
        let number_of_vertices =
            Geometry::new(attributes.clone(), None, PrimitiveType::Points).compute_number_of_vertices()?;
        let mut dest = Vec::new();
        let mut offset = 0.0;
        for g in geometries {
            dest.extend(g.indices.as_ref()?.to_f64_vec().into_iter().map(|i| offset + i));
            offset += g.compute_number_of_vertices()?;
        }
        Some(index_array(number_of_vertices, &dest))
    } else {
        None
    };

    // The combined bounding sphere exists only if every geometry has one.
    let spheres: Option<Vec<BoundingSphere>> = geometries.iter().map(|g| g.bounding_sphere).collect();
    let bounding_sphere = spheres.map(|spheres| {
        let mut center = Cartesian3::ZERO;
        for bs in &spheres {
            center = bs.center.add(center);
        }
        center = center.divide_by_scalar(spheres.len() as f64);
        let mut radius = 0.0;
        for bs in &spheres {
            let r = bs.center.subtract(center).magnitude() + bs.radius;
            if r > radius {
                radius = r;
            }
        }
        BoundingSphere::new(center, radius)
    });

    let mut combined = Geometry::new(attributes, indices, first.primitive_type);
    combined.bounding_sphere = bounding_sphere;
    Some(combined)
}

/// Combines instances (already in world coordinates) into one geometry, or
/// into west and east geometries for instances split at the antimeridian.
/// Attributes missing from any geometry are dropped.
pub fn combine_instances(instances: &[GeometryInstance]) -> Option<Vec<Geometry>> {
    let whole: Vec<&Geometry> = instances.iter().filter_map(|i| i.geometry.as_ref()).collect();
    let split: Vec<&GeometryInstance> = instances
        .iter()
        .filter(|i| i.geometry.is_none() && i.west_hemisphere_geometry.is_some() && i.east_hemisphere_geometry.is_some())
        .collect();
    let mut geometries = Vec::new();
    if !whole.is_empty() {
        geometries.push(combine_geometries(&whole)?);
    }
    if !split.is_empty() {
        let west: Vec<&Geometry> = split.iter().filter_map(|i| i.west_hemisphere_geometry.as_ref()).collect();
        let east: Vec<&Geometry> = split.iter().filter_map(|i| i.east_hemisphere_geometry.as_ref()).collect();
        geometries.push(combine_geometries(&west)?);
        geometries.push(combine_geometries(&east)?);
    }
    Some(geometries)
}

// ---- Normals, tangents and compression -------------------------------------

/// Computes per-vertex normals by averaging the normals of the triangles
/// that use each vertex.
pub fn compute_normal(geometry: &mut Geometry) -> Option<()> {
    let indices = geometry.indices.as_ref()?.to_f64_vec();
    let vertices = &geometry.attributes.position.as_ref()?.values;
    let num_vertices = array_length(vertices.len() as f64 / 3.0)?;
    if !indices.len().is_multiple_of(3) {
        return None;
    }
    // Each vertex's triangles in order, repeated if it appears twice in one.
    let mut vertex_triangles: Vec<Vec<usize>> = vec![Vec::new(); num_vertices];
    let mut triangle_normals = Vec::with_capacity(indices.len() / 3);
    for (j, tri) in indices.chunks_exact(3).enumerate() {
        let ids = [as_index(tri[0])?, as_index(tri[1])?, as_index(tri[2])?];
        for &i in &ids {
            vertex_triangles.get_mut(i)?.push(j);
        }
        let v0 = c3_at(vertices, ids[0] * 3);
        let v1 = c3_at(vertices, ids[1] * 3).subtract(v0);
        let v2 = c3_at(vertices, ids[2] * 3).subtract(v0);
        triangle_normals.push(v1.cross(v2));
    }
    let mut normal_values = Vec::with_capacity(num_vertices * 3);
    for triangles in &vertex_triangles {
        let mut normal = Cartesian3::ZERO;
        if !triangles.is_empty() {
            for &j in triangles {
                normal = normal.add(triangle_normals[j]);
            }
            // A vertex shared by triangles of opposite winding.
            if Cartesian3::ZERO.equals_epsilon(normal, EPSILON10) {
                normal = triangle_normals[triangles[0]];
            }
        }
        // Probably a degenerate triangle.
        if Cartesian3::ZERO.equals_epsilon(normal, EPSILON10) {
            normal.z = 1.0;
        }
        let n = normal.normalize();
        normal_values.extend([n.x, n.y, n.z]);
    }
    geometry.attributes.normal = Some(GeometryAttribute::floats(3, &normal_values));
    Some(())
}

/// Computes tangents and bitangents from positions, normals and texture
/// coordinates. Returns `None` without replacing attributes if the texture
/// coordinates or normals cannot produce finite tangent vectors.
pub fn compute_tangent_and_bitangent(geometry: &mut Geometry) -> Option<()> {
    let indices = geometry.indices.as_ref()?.to_f64_vec();
    let attributes = &geometry.attributes;
    let vertices = &attributes.position.as_ref()?.values;
    let normals = &attributes.normal.as_ref()?.values;
    let st = &attributes.st.as_ref()?.values;
    let num_vertices = array_length(vertices.len() as f64 / 3.0)?;
    if !indices.len().is_multiple_of(3) {
        return None;
    }
    let mut tan1 = vec![0.0; num_vertices * 3];
    for tri in indices.chunks_exact(3) {
        let [i0, i1, i2] = [as_index(tri[0])?, as_index(tri[1])?, as_index(tri[2])?];
        let (i03, i13, i23) = (i0 * 3, i1 * 3, i2 * 3);
        let (i02, i12, i22) = (i0 * 2, i1 * 2, i2 * 2);
        let (ux, uy, uz) = (at(vertices, i03), at(vertices, i03 + 1), at(vertices, i03 + 2));
        let wx = at(st, i02);
        let wy = at(st, i02 + 1);
        let t1 = at(st, i12 + 1) - wy;
        let t2 = at(st, i22 + 1) - wy;
        let r = 1.0 / ((at(st, i12) - wx) * t2 - (at(st, i22) - wx) * t1);
        let sdirx = (t2 * (at(vertices, i13) - ux) - t1 * (at(vertices, i23) - ux)) * r;
        let sdiry = (t2 * (at(vertices, i13 + 1) - uy) - t1 * (at(vertices, i23 + 1) - uy)) * r;
        let sdirz = (t2 * (at(vertices, i13 + 2) - uz) - t1 * (at(vertices, i23 + 2) - uz)) * r;
        for base in [i03, i13, i23] {
            for (k, d) in [sdirx, sdiry, sdirz].into_iter().enumerate() {
                if let Some(slot) = tan1.get_mut(base + k) {
                    *slot += d;
                }
            }
        }
    }
    let mut tangents = Vec::with_capacity(num_vertices * 3);
    let mut bitangents = Vec::with_capacity(num_vertices * 3);
    for i in 0..num_vertices {
        let i03 = i * 3;
        let n = c3_at(normals, i03);
        let t = Cartesian3::new(tan1[i03], tan1[i03 + 1], tan1[i03 + 2]);
        let scalar = n.dot(t);
        let t = t.subtract(n.multiply_by_scalar(scalar)).normalize();
        if ![t.x, t.y, t.z].iter().all(|v| v.is_finite()) {
            return None;
        }
        tangents.extend([t.x, t.y, t.z]);
        let b = n.cross(t).normalize();
        if ![b.x, b.y, b.z].iter().all(|v| v.is_finite()) {
            return None;
        }
        bitangents.extend([b.x, b.y, b.z]);
    }
    geometry.attributes.tangent = Some(GeometryAttribute::floats(3, &tangents));
    geometry.attributes.bitangent = Some(GeometryAttribute::floats(3, &bitangents));
    Some(())
}

/// Packs normals, texture coordinates, tangents and bitangents (or shadow
/// volume extrude directions) into one `compressedAttributes` attribute.
pub fn compress_vertices(geometry: &mut Geometry) -> Option<()> {
    let attributes = &mut geometry.attributes;
    if let Some(extrude) = attributes.get("extrudeDirection") {
        let directions = &extrude.values;
        let num_vertices = array_length(directions.len() as f64 / 3.0)?;
        let mut compressed = vec![0.0; num_vertices * 2];
        for i in 0..num_vertices {
            let v = c3_at(directions, i * 3);
            if v == Cartesian3::ZERO {
                continue;
            }
            let e = AttributeCompression::oct_encode_in_range(v, 65535.0);
            compressed[i * 2] = e.x;
            compressed[i * 2 + 1] = e.y;
        }
        attributes.set_other("compressedAttributes", GeometryAttribute::floats(2, &compressed));
        attributes.remove("extrudeDirection");
        return Some(());
    }

    let (has_normal, has_st) = (attributes.normal.is_some(), attributes.st.is_some());
    if !has_normal && !has_st {
        return Some(());
    }
    let normals = attributes.normal.as_ref().map(|a| &a.values);
    let st = attributes.st.as_ref().map(|a| &a.values);
    let tangents = attributes.tangent.as_ref().map(|a| &a.values);
    let bitangents = attributes.bitangent.as_ref().map(|a| &a.values);
    let (length, num_components) = match normals {
        Some(n) => (n.len(), 3.0),
        None => (st?.len(), 2.0),
    };
    let num_vertices = length as f64 / num_components;
    let mut num_compressed = if has_st && has_normal { 2.0 } else { 1.0 };
    num_compressed += if tangents.is_some() || bitangents.is_some() { 1.0 } else { 0.0 };
    let mut compressed = vec![0.0; array_length(num_vertices * num_compressed)?];
    let mut write = |index: &mut usize, v: f64| {
        if let Some(slot) = compressed.get_mut(*index) {
            *slot = v;
        }
        *index += 1;
    };
    let mut normal_index = 0;
    let mut i = 0;
    while (i as f64) < num_vertices {
        if let Some(st) = st {
            write(&mut normal_index, AttributeCompression::compress_texture_coordinates(c2_at(st, i * 2)));
        }
        let index = i * 3;
        match (normals, tangents, bitangents) {
            (Some(n), Some(t), Some(b)) => {
                let packed = AttributeCompression::oct_pack(c3_at(n, index), c3_at(t, index), c3_at(b, index));
                write(&mut normal_index, packed.x);
                write(&mut normal_index, packed.y);
            }
            _ => {
                for values in [normals, tangents, bitangents].into_iter().flatten() {
                    write(&mut normal_index, AttributeCompression::oct_encode_float(c3_at(values, index)));
                }
            }
        }
        i += 1;
    }
    attributes.set_other("compressedAttributes", GeometryAttribute::floats(num_compressed as u8, &compressed));
    attributes.normal = None;
    attributes.st = None;
    attributes.bitangent = None;
    attributes.tangent = None;
    Some(())
}

// ---- Indexing primitives -----------------------------------------------------

/// Writes into a fixed-length index list, ignoring out-of-range writes as a
/// typed array does.
fn put(indices: &mut [f64], i: usize, v: f64) {
    if let Some(slot) = indices.get_mut(i) {
        *slot = v;
    }
}

fn index_primitive(geometry: &mut Geometry) -> Option<()> {
    let n = || geometry.compute_number_of_vertices();
    match geometry.primitive_type {
        PrimitiveType::Triangles | PrimitiveType::Lines => {
            if geometry.indices.is_some() {
                return Some(());
            }
            let nv = n()?;
            let indices: Vec<f64> = (0..array_length(nv)?).map(|i| i as f64).collect();
            geometry.indices = Some(index_array(nv, &indices));
        }
        PrimitiveType::TriangleFan => {
            let nv = n()?;
            let mut indices = vec![0.0; array_length((nv - 2.0) * 3.0)?];
            for (k, v) in [1.0, 0.0, 2.0].into_iter().enumerate() {
                put(&mut indices, k, v);
            }
            let mut ii = 3;
            let mut i = 3.0;
            while i < nv {
                for v in [i - 1.0, 0.0, i] {
                    put(&mut indices, ii, v);
                    ii += 1;
                }
                i += 1.0;
            }
            geometry.indices = Some(index_array(nv, &indices));
            geometry.primitive_type = PrimitiveType::Triangles;
        }
        PrimitiveType::TriangleStrip => {
            let nv = n()?;
            let mut indices = vec![0.0; array_length((nv - 2.0) * 3.0)?];
            for (k, v) in [0.0, 1.0, 2.0].into_iter().enumerate() {
                put(&mut indices, k, v);
            }
            if nv > 3.0 {
                for (k, v) in [0.0, 2.0, 3.0].into_iter().enumerate() {
                    put(&mut indices, 3 + k, v);
                }
            }
            let mut ii = 6;
            let mut i = 3.0;
            while i < nv - 1.0 {
                for v in [i, i - 1.0, i + 1.0] {
                    put(&mut indices, ii, v);
                    ii += 1;
                }
                if i + 2.0 < nv {
                    for v in [i, i + 1.0, i + 2.0] {
                        put(&mut indices, ii, v);
                        ii += 1;
                    }
                }
                i += 2.0;
            }
            geometry.indices = Some(index_array(nv, &indices));
            geometry.primitive_type = PrimitiveType::Triangles;
        }
        PrimitiveType::LineStrip | PrimitiveType::LineLoop => {
            let nv = n()?;
            let is_loop = geometry.primitive_type == PrimitiveType::LineLoop;
            let length = if is_loop { nv * 2.0 } else { (nv - 1.0) * 2.0 };
            let mut indices = vec![0.0; array_length(length)?];
            put(&mut indices, 0, 0.0);
            put(&mut indices, 1, 1.0);
            let mut ii = 2;
            let mut i = 2.0;
            while i < nv {
                put(&mut indices, ii, i - 1.0);
                put(&mut indices, ii + 1, i);
                ii += 2;
                i += 1.0;
            }
            if is_loop {
                put(&mut indices, ii, nv - 1.0);
                put(&mut indices, ii + 1, 0.0);
            }
            geometry.indices = Some(index_array(nv, &indices));
            geometry.primitive_type = PrimitiveType::Lines;
        }
        PrimitiveType::Points => {}
    }
    Some(())
}

// ---- Splitting at the antimeridian ---------------------------------------------

const WEST: usize = 0;
const EAST: usize = 1;

/// A geometry under construction from plain arrays, as Cesium builds the
/// two hemisphere geometries.
struct SplitGeometry {
    attributes: Vec<(String, GeometryAttribute, Vec<f64>)>,
    indices: Vec<f64>,
    primitive_type: PrimitiveType,
}

impl SplitGeometry {
    fn copy_for_split(geometry: &Geometry) -> Self {
        Self {
            attributes: geometry.attributes.iter().map(|(n, a)| (n.to_string(), a.clone(), Vec::new())).collect(),
            indices: Vec::new(),
            primitive_type: geometry.primitive_type,
        }
    }

    fn values(&mut self, name: &str) -> Option<&mut Vec<f64>> {
        self.attributes.iter_mut().find(|(n, _, _)| n == name).map(|(_, _, v)| v)
    }

    fn values_ref(&self, name: &str) -> Option<&Vec<f64>> {
        self.attributes.iter().find(|(n, _, _)| n == name).map(|(_, _, v)| v)
    }

    fn push(&mut self, name: &str, values: &[f64]) -> Option<()> {
        self.values(name)?.extend_from_slice(values);
        Some(())
    }

    fn position_count(&self) -> usize {
        self.values_ref("position").map_or(0, |v| v.len() / 3)
    }

    /// `updateGeometryAfterSplit`: typed arrays, 16- or 32-bit indices, and
    /// a bounding sphere if asked. `Some(None)` when it has no positions;
    /// `None` when attribute lengths disagree (Cesium throws).
    fn finish(self, compute_bounding_sphere: bool) -> Option<Option<Geometry>> {
        if self.values_ref("position").is_none_or(|v| v.is_empty()) {
            return Some(None);
        }
        let mut attributes = GeometryAttributes::default();
        for (name, template, values) in self.attributes {
            attributes.set(
                &name,
                GeometryAttribute {
                    values: TypedArray::from_values(template.component_datatype, &values),
                    ..template
                },
            );
        }
        let mut geometry = Geometry::new(attributes, None, self.primitive_type);
        let number_of_vertices = geometry.compute_number_of_vertices()?;
        geometry.indices = Some(index_array(number_of_vertices, &self.indices));
        if compute_bounding_sphere {
            let positions = geometry.attributes.position.as_ref()?.values.to_f64_vec();
            geometry.bounding_sphere = Some(BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3));
        }
        Some(Some(geometry))
    }
}

fn update_instance_after_split(instance: &mut GeometryInstance, west: SplitGeometry, east: SplitGeometry) -> Option<()> {
    let compute = instance.geometry.as_ref().is_some_and(|g| g.bounding_sphere.is_some());
    let west = west.finish(compute)?;
    let east = east.finish(compute)?;
    match (west, east) {
        (None, Some(east)) => instance.geometry = Some(east),
        (Some(west), None) => instance.geometry = Some(west),
        (west, east) => {
            instance.west_hemisphere_geometry = west;
            instance.east_hemisphere_geometry = east;
            instance.geometry = None;
        }
    }
    Some(())
}

/// Appends `point` to a hemisphere unless the source vertex `indices[current]`
/// is already there; pushes the index either way and returns it.
fn insert_split_point(
    geometry: &mut SplitGeometry,
    index_map: &mut [i64],
    indices: &[f64],
    current_index: Option<usize>,
    point: Cartesian3,
) -> Option<usize> {
    let insert_index = geometry.position_count();
    if let Some(current) = current_index {
        let previous = as_index(*indices.get(current)?)?;
        let new_index = *index_map.get(previous)?;
        if new_index != -1 {
            geometry.indices.push(new_index as f64);
            return Some(new_index as usize);
        }
        index_map[previous] = insert_index as i64;
    }
    geometry.push("position", &[point.x, point.y, point.z])?;
    geometry.indices.push(insert_index as f64);
    Some(insert_index)
}

fn offset_point_from_xz_plane(p: &mut Cartesian3, is_behind: bool) {
    if p.y.abs() < EPSILON6 {
        p.y = if is_behind { -EPSILON6 } else { EPSILON6 };
    }
}

fn offset_triangle_from_xz_plane(p: &mut [Cartesian3; 3]) {
    if p[0].y != 0.0 && p[1].y != 0.0 && p[2].y != 0.0 {
        for q in p.iter_mut() {
            let behind = q.y < 0.0;
            offset_point_from_xz_plane(q, behind);
        }
        return;
    }
    let (p0y, p1y, p2y) = (p[0].y.abs(), p[1].y.abs(), p[2].y.abs());
    let s = if p0y > p1y {
        if p0y > p2y {
            sign(p[0].y)
        } else {
            sign(p[2].y)
        }
    } else if p1y > p2y {
        sign(p[1].y)
    } else {
        sign(p[2].y)
    };
    for q in p.iter_mut() {
        offset_point_from_xz_plane(q, s < 0.0);
    }
}

/// The crossing of edge `p`→`p1` with the xz-plane, nudged to each side.
fn xz_intersection_offset_points(p: Cartesian3, p1: Cartesian3) -> (Cartesian3, Cartesian3) {
    let mut u = p.add(p1.subtract(p).multiply_by_scalar(p.y / (p.y - p1.y)));
    let mut v = u;
    offset_point_from_xz_plane(&mut u, true);
    offset_point_from_xz_plane(&mut v, false);
    (u, v)
}

enum TriangleSplit {
    /// The (possibly nudged) corners; the triangle stays on one side.
    Whole([Cartesian3; 3]),
    /// Corners, two crossing points on each side, and three triangles.
    Split([Cartesian3; 7], [usize; 9]),
}

/// `None` unless the triangle is on the far side (x < 0) of the globe.
fn split_triangle(mut p: [Cartesian3; 3]) -> Option<TriangleSplit> {
    if p.iter().any(|q| q.x >= 0.0) {
        return None;
    }
    offset_triangle_from_xz_plane(&mut p);
    let behind = p.map(|q| q.y < 0.0);
    let num_behind = behind.iter().filter(|&&b| b).count();
    let mut indices = [0usize; 9];
    let (u1, u2, q1, q2);
    match num_behind {
        1 => {
            indices[1] = 3;
            indices[2] = 4;
            indices[5] = 6;
            indices[7] = 6;
            indices[8] = 5;
            let k = behind.iter().position(|&b| b)?;
            let (a, b) = ((k + 1) % 3, (k + 2) % 3);
            (u1, q1) = xz_intersection_offset_points(p[k], p[a]);
            (u2, q2) = xz_intersection_offset_points(p[k], p[b]);
            indices[0] = k;
            indices[3] = a;
            indices[4] = b;
            indices[6] = a;
        }
        2 => {
            indices[2] = 4;
            indices[4] = 4;
            indices[5] = 3;
            indices[7] = 5;
            indices[8] = 6;
            let k = behind.iter().position(|&b| !b)?;
            let (a, b) = ((k + 1) % 3, (k + 2) % 3);
            (u1, q1) = xz_intersection_offset_points(p[k], p[a]);
            (u2, q2) = xz_intersection_offset_points(p[k], p[b]);
            indices[0] = a;
            indices[1] = b;
            indices[3] = a;
            indices[6] = k;
        }
        _ => return Some(TriangleSplit::Whole(p)),
    }
    Some(TriangleSplit::Split([p[0], p[1], p[2], u1, u2, q1, q2], indices))
}

/// Attributes that `splitLongitudeTriangles` interpolates by name; all
/// others are interpolated generically.
const NAMED_ATTRIBUTES: [&str; 7] = ["position", "normal", "bitangent", "tangent", "st", "extrudeDirection", "applyOffset"];

/// Barycentric interpolation of an `n`-component attribute, written at
/// `inserted_index`.
#[allow(clippy::too_many_arguments)]
fn interpolate_and_pack(
    n: usize,
    ids: [usize; 3],
    coords: Cartesian3,
    source: &TypedArray,
    current: &mut Vec<f64>,
    inserted_index: usize,
    normalize: bool,
) {
    let c = [coords.x, coords.y, coords.z];
    let out: Vec<f64> = match n {
        4 => {
            let v = ids.map(|i| c4_at(source, i * 4));
            let mut value = v[0].multiply_by_scalar(c[0]).add(v[1].multiply_by_scalar(c[1])).add(v[2].multiply_by_scalar(c[2]));
            if normalize {
                value = value.normalize();
            }
            vec![value.x, value.y, value.z, value.w]
        }
        3 => {
            let v = ids.map(|i| c3_at(source, i * 3));
            let mut value = v[0].multiply_by_scalar(c[0]).add(v[1].multiply_by_scalar(c[1])).add(v[2].multiply_by_scalar(c[2]));
            if normalize {
                value = value.normalize();
            }
            vec![value.x, value.y, value.z]
        }
        2 => {
            let v = ids.map(|i| c2_at(source, i * 2));
            let mut value = v[0].multiply_by_scalar(c[0]).add(v[1].multiply_by_scalar(c[1])).add(v[2].multiply_by_scalar(c[2]));
            if normalize {
                value = value.normalize();
            }
            vec![value.x, value.y]
        }
        _ => vec![at(source, ids[0]) * c[0] + at(source, ids[1]) * c[1] + at(source, ids[2]) * c[2]],
    };
    for (k, v) in out.into_iter().enumerate() {
        set_at(current, inserted_index * n + k, v);
    }
}

fn compute_triangle_attributes(
    ids: [usize; 3],
    point: Cartesian3,
    source: &GeometryAttributes,
    custom_names: &[String],
    current: &mut SplitGeometry,
    inserted_index: usize,
) {
    let get = |name: &str| source.get(name).map(|a| &a.values);
    let (normals, tangents, bitangents, tex_coords, extrude, apply_offset) = (
        get("normal"),
        get("tangent"),
        get("bitangent"),
        get("st"),
        get("extrudeDirection"),
        get("applyOffset"),
    );
    if normals.is_none()
        && tangents.is_none()
        && bitangents.is_none()
        && tex_coords.is_none()
        && extrude.is_none()
        && custom_names.is_empty()
    {
        return;
    }
    let Some(positions) = get("position") else { return };
    let p = ids.map(|i| c3_at(positions, i * 3));
    let Some(coords) = barycentric_coordinates(point, p[0], p[1], p[2]) else { return };

    let mut pack = |name: &str, n: usize, values: Option<&TypedArray>, normalize: bool| {
        if let (Some(values), Some(out)) = (values, current.values(name)) {
            interpolate_and_pack(n, ids, coords, values, out, inserted_index, normalize);
        }
    };
    pack("normal", 3, normals, true);
    if let Some(extrude) = extrude {
        let d = [
            c3_at(extrude, ids[0] * 3).multiply_by_scalar(coords.x),
            c3_at(extrude, ids[1] * 3).multiply_by_scalar(coords.y),
            c3_at(extrude, ids[2] * 3).multiply_by_scalar(coords.z),
        ];
        let direction = if d.iter().any(|v| *v != Cartesian3::ZERO) {
            d[0].add(d[1]).add(d[2]).normalize()
        } else {
            Cartesian3::ZERO
        };
        if let Some(out) = current.values("extrudeDirection") {
            for (k, v) in [direction.x, direction.y, direction.z].into_iter().enumerate() {
                set_at(out, inserted_index * 3 + k, v);
            }
        }
    }
    if let (Some(apply_offset), Some(out)) = (apply_offset, current.values("applyOffset")) {
        let v = at(apply_offset, ids[0]) * coords.x + at(apply_offset, ids[1]) * coords.y + at(apply_offset, ids[2]) * coords.z;
        set_at(out, inserted_index, if v > EPSILON6 { 1.0 } else { 0.0 });
    }
    let mut pack = |name: &str, n: usize, values: Option<&TypedArray>, normalize: bool| {
        if let (Some(values), Some(out)) = (values, current.values(name)) {
            interpolate_and_pack(n, ids, coords, values, out, inserted_index, normalize);
        }
    };
    pack("tangent", 3, tangents, true);
    pack("bitangent", 3, bitangents, true);
    pack("st", 2, tex_coords, false);
    for name in custom_names {
        if let Some(attribute) = source.get(name) {
            let n = usize::from(attribute.components_per_attribute);
            pack(name, n, Some(&attribute.values), false);
        }
    }
}

fn split_longitude_triangles(instance: &mut GeometryInstance) -> Option<()> {
    let geometry = instance.geometry.as_ref()?;
    let attributes = &geometry.attributes;
    let positions = &attributes.position.as_ref()?.values;
    let indices = geometry.indices.as_ref()?.to_f64_vec();
    let custom_names: Vec<String> = attributes.names().into_iter().filter(|n| !NAMED_ATTRIBUTES.contains(&n.as_str())).collect();
    let mut halves = [SplitGeometry::copy_for_split(geometry), SplitGeometry::copy_for_split(geometry)];
    let vertex_count = positions.len() / 3;
    let mut maps = [vec![-1i64; vertex_count], vec![-1i64; vertex_count]];

    for i in (0..indices.len()).step_by(3) {
        let ids = [
            as_index(*indices.get(i)?)?,
            as_index(*indices.get(i + 1)?)?,
            as_index(*indices.get(i + 2)?)?,
        ];
        let p = ids.map(|v| c3_at(positions, v * 3));
        match split_triangle(p) {
            Some(TriangleSplit::Split(points, result_indices)) => {
                for result_index in result_indices {
                    let point = points[result_index];
                    let side = if point.y < 0.0 { WEST } else { EAST };
                    let current_index = (result_index < 3).then_some(i + result_index);
                    let inserted =
                        insert_split_point(&mut halves[side], &mut maps[side], &indices, current_index, point)?;
                    compute_triangle_attributes(ids, point, attributes, &custom_names, &mut halves[side], inserted);
                }
            }
            other => {
                let corners = match other {
                    Some(TriangleSplit::Whole(corners)) => corners,
                    _ => p,
                };
                let side = if corners[0].y < 0.0 { WEST } else { EAST };
                for (k, &point) in corners.iter().enumerate() {
                    let inserted = insert_split_point(&mut halves[side], &mut maps[side], &indices, Some(i + k), point)?;
                    compute_triangle_attributes(ids, point, attributes, &custom_names, &mut halves[side], inserted);
                }
            }
        }
    }
    let [west, east] = halves;
    update_instance_after_split(instance, west, east)
}

fn compute_line_attributes(
    i0: usize,
    i1: usize,
    point: Cartesian3,
    positions: &TypedArray,
    insert_index: usize,
    current: &mut SplitGeometry,
    apply_offset: Option<&TypedArray>,
) {
    let Some(apply_offset) = apply_offset else { return };
    let p0 = c3_at(positions, i0 * 3);
    let value = if p0.equals_epsilon(point, EPSILON10) { at(apply_offset, i0) } else { at(apply_offset, i1) };
    if let Some(out) = current.values("applyOffset") {
        set_at(out, insert_index, value);
    }
}

/// The xz-plane as Cesium builds it with `Plane.fromPointNormal`.
const XZ_PLANE: Plane = Plane { normal: Cartesian3::UNIT_Y, distance: -0.0 };

fn split_longitude_lines(instance: &mut GeometryInstance) -> Option<()> {
    let geometry = instance.geometry.as_ref()?;
    let attributes = &geometry.attributes;
    let positions = &attributes.position.as_ref()?.values;
    let apply_offset = attributes.get("applyOffset").map(|a| &a.values);
    let indices = geometry.indices.as_ref()?.to_f64_vec();
    let mut halves = [SplitGeometry::copy_for_split(geometry), SplitGeometry::copy_for_split(geometry)];
    let vertex_count = positions.len() / 3;
    let mut maps = [vec![-1i64; vertex_count], vec![-1i64; vertex_count]];

    for i in (0..indices.len()).step_by(2) {
        let i0 = as_index(*indices.get(i)?)?;
        let i1 = as_index(*indices.get(i + 1)?)?;
        let mut p0 = c3_at(positions, i0 * 3);
        let mut p1 = c3_at(positions, i1 * 3);
        for p in [&mut p0, &mut p1] {
            if p.y.abs() < EPSILON6 {
                p.y = if p.y < 0.0 { -EPSILON6 } else { EPSILON6 };
            }
        }
        let mut insert = |side: usize, current: Option<usize>, point: Cartesian3| -> Option<()> {
            let index = insert_split_point(&mut halves[side], &mut maps[side], &indices, current, point)?;
            compute_line_attributes(i0, i1, point, positions, index, &mut halves[side], apply_offset);
            Some(())
        };
        if let Some(intersection) = line_segment_plane(p0, p1, &XZ_PLANE) {
            // Move the points on the plane slightly off it.
            let mut offset = Cartesian3::UNIT_Y.multiply_by_scalar(5.0 * EPSILON9);
            let (mut s0, mut s1) = (EAST, WEST);
            if p0.y < 0.0 {
                offset = offset.negate();
                (s0, s1) = (WEST, EAST);
            }
            insert(s0, Some(i), p0)?;
            insert(s0, None, intersection.add(offset))?;
            offset = offset.negate();
            insert(s1, None, intersection.add(offset))?;
            insert(s1, Some(i + 1), p1)?;
        } else {
            let side = if p0.y < 0.0 { WEST } else { EAST };
            insert(side, Some(i), p0)?;
            insert(side, Some(i + 1), p1)?;
        }
    }
    let [west, east] = halves;
    update_instance_after_split(instance, west, east)
}

fn update_adjacency_after_split(geometry: &mut SplitGeometry) -> Option<()> {
    let positions = geometry.values_ref("position")?.clone();
    let length = positions.len();
    let p = |k: usize| positions.get(k).copied().unwrap_or(f64::NAN);
    for name in ["prevPosition", "nextPosition"] {
        let is_prev = name == "prevPosition";
        let adjacent = geometry.values(name)?;
        let a = |values: &Vec<f64>, k: usize| values.get(k).copied().unwrap_or(f64::NAN);
        for j in (0..length).step_by(3) {
            let position = Cartesian3::new(p(j), p(j + 1), p(j + 2));
            if position.x > 0.0 {
                continue;
            }
            let other_y = a(adjacent, j + 1);
            if (position.y < 0.0 && other_y > 0.0) || (position.y > 0.0 && other_y < 0.0) {
                let replacement = if is_prev && j as isize - 3 > 0 {
                    [p(j - 3), p(j - 2), p(j - 1)]
                } else if !is_prev && j + 3 < length {
                    [p(j + 3), p(j + 4), p(j + 5)]
                } else {
                    [position.x, position.y, position.z]
                };
                for (k, v) in replacement.into_iter().enumerate() {
                    if let Some(slot) = adjacent.get_mut(j + k) {
                        *slot = v;
                    }
                }
            }
        }
    }
    Some(())
}

fn split_longitude_polyline(instance: &mut GeometryInstance) -> Option<()> {
    let geometry = instance.geometry.as_ref()?;
    let attributes = &geometry.attributes;
    // Cesium nudges these in place, through their typed arrays.
    let mut positions = attributes.position.as_ref()?.values.clone();
    let mut prev_positions = attributes.get("prevPosition")?.values.clone();
    let mut next_positions = attributes.get("nextPosition")?.values.clone();
    let expand_and_widths = &attributes.get("expandAndWidth")?.values;
    let tex_coords = attributes.st.as_ref().map(|a| &a.values);
    let colors = attributes.color.as_ref().map(|a| &a.values);
    let mut halves = [SplitGeometry::copy_for_split(geometry), SplitGeometry::copy_for_split(geometry)];
    let coplanar_offset = EPSILON6;
    let mut intersection_found = false;
    let length = positions.len() / 3;

    let mut i = 0;
    while i < length {
        let (i0, i2) = (i, i + 2);
        let mut p0 = c3_at(&positions, i0 * 3);
        let mut p2 = c3_at(&positions, i2 * 3);
        // Points near the antimeridian are nudged off it, and the adjacent
        // positions follow so the segment still projects to 2D.
        if p0.y.abs() < coplanar_offset {
            p0.y = coplanar_offset * if p2.y < 0.0 { -1.0 } else { 1.0 };
            positions.set(i * 3 + 1, p0.y);
            positions.set((i + 1) * 3 + 1, p0.y);
            for j in (i0 * 3..i0 * 3 + 12).step_by(3) {
                for k in 0..3 {
                    prev_positions.set(j + k, at(&positions, i * 3 + k));
                }
            }
        }
        if p2.y.abs() < coplanar_offset {
            p2.y = coplanar_offset * if p0.y < 0.0 { -1.0 } else { 1.0 };
            positions.set((i + 2) * 3 + 1, p2.y);
            positions.set((i + 3) * 3 + 1, p2.y);
            for j in (i0 * 3..i0 * 3 + 12).step_by(3) {
                for k in 0..3 {
                    next_positions.set(j + k, at(&positions, (i + 2) * 3 + k));
                }
            }
        }

        if let Some(intersection) = line_segment_plane(p0, p2, &XZ_PLANE) {
            intersection_found = true;
            let mut offset = Cartesian3::UNIT_Y.multiply_by_scalar(5.0 * EPSILON9);
            let (mut s0, mut s2) = (EAST, WEST);
            if p0.y < 0.0 {
                offset = offset.negate();
                (s0, s2) = (WEST, EAST);
            }
            let o0 = intersection.add(offset);
            let o2 = intersection.add(offset.negate());
            let xyz = |c: Cartesian3| [c.x, c.y, c.z];
            let prev = |k: usize| at(&prev_positions, i0 * 3 + k);
            let next = |k: usize| at(&next_positions, i2 * 3 + k);

            let g0 = &mut halves[s0];
            g0.push("position", &[xyz(p0), xyz(p0), xyz(o0), xyz(o0)].concat())?;
            g0.push("prevPosition", &[prev(0), prev(1), prev(2), prev(3), prev(4), prev(5)])?;
            g0.push("prevPosition", &[xyz(p0), xyz(p0)].concat())?;
            g0.push("nextPosition", &[xyz(o0), xyz(o0), xyz(o0), xyz(o0)].concat())?;
            let g2 = &mut halves[s2];
            g2.push("position", &[xyz(o2), xyz(o2), xyz(p2), xyz(p2)].concat())?;
            g2.push("prevPosition", &[xyz(o2), xyz(o2), xyz(o2), xyz(o2)].concat())?;
            g2.push("nextPosition", &[xyz(p2), xyz(p2)].concat())?;
            g2.push("nextPosition", &[next(0), next(1), next(2), next(3), next(4), next(5)])?;

            let width = at(expand_and_widths, i0 * 2 + 1).abs();
            for side in [s0, s2] {
                halves[side].push("expandAndWidth", &[-1.0, width, 1.0, width, -1.0, -width, 1.0, -width])?;
            }
            let t = intersection.subtract(p0).magnitude_squared() / p2.subtract(p0).magnitude_squared();
            if let Some(colors) = colors {
                let c0 = c4_at(colors, i0 * 4);
                let c2 = c4_at(colors, i2 * 4);
                let mid = [lerp(c0.x, c2.x, t), lerp(c0.y, c2.y, t), lerp(c0.z, c2.z, t), lerp(c0.w, c2.w, t)];
                let own0: Vec<f64> = (i0 * 4..i0 * 4 + 8).map(|j| at(colors, j)).collect();
                let own2: Vec<f64> = (i2 * 4..i2 * 4 + 8).map(|j| at(colors, j)).collect();
                halves[s0].push("color", &[own0.as_slice(), &mid, &mid].concat())?;
                halves[s2].push("color", &[&mid[..], &mid, own2.as_slice()].concat())?;
            }
            if let Some(st) = tex_coords {
                let s0v = c2_at(st, i0 * 2);
                let s3v = c2_at(st, (i + 3) * 2);
                let sx = lerp(s0v.x, s3v.x, t);
                let own0: Vec<f64> = (i0 * 2..i0 * 2 + 4).map(|j| at(st, j)).collect();
                let own2: Vec<f64> = (i2 * 2..i2 * 2 + 4).map(|j| at(st, j)).collect();
                halves[s0].push("st", &[own0.as_slice(), &[sx, s0v.y, sx, s3v.y]].concat())?;
                halves[s2].push("st", &[&[sx, s0v.y, sx, s3v.y][..], own2.as_slice()].concat())?;
            }
            for side in [s0, s2] {
                let g = &mut halves[side];
                let index = (g.position_count() - 4) as f64;
                g.indices.extend([index, index + 2.0, index + 1.0, index + 1.0, index + 2.0, index + 3.0]);
            }
        } else {
            let side = if p0.y < 0.0 { WEST } else { EAST };
            let g = &mut halves[side];
            g.push("position", &[p0.x, p0.y, p0.z, p0.x, p0.y, p0.z, p2.x, p2.y, p2.z, p2.x, p2.y, p2.z])?;
            for j in i * 3..i * 3 + 12 {
                g.push("prevPosition", &[at(&prev_positions, j)])?;
                g.push("nextPosition", &[at(&next_positions, j)])?;
            }
            for j in i * 2..i * 2 + 8 {
                g.push("expandAndWidth", &[at(expand_and_widths, j)])?;
                if let Some(st) = tex_coords {
                    g.push("st", &[at(st, j)])?;
                }
            }
            if let Some(colors) = colors {
                let values: Vec<f64> = (i * 4..i * 4 + 16).map(|j| at(colors, j)).collect();
                g.push("color", &values)?;
            }
            let index = (g.position_count() - 4) as f64;
            g.indices.extend([index, index + 2.0, index + 1.0, index + 1.0, index + 2.0, index + 3.0]);
        }
        i += 4;
    }
    if intersection_found {
        for g in halves.iter_mut() {
            update_adjacency_after_split(g)?;
        }
    }
    let [west, east] = halves;
    update_instance_after_split(instance, west, east)
}

/// Splits an instance whose geometry crosses the antimeridian (the
/// negative-x half of the xz-plane) into west and east hemisphere
/// geometries, interpolating attributes at the new vertices. An instance
/// whose bounding sphere does not cross is left alone.
pub fn split_longitude(instance: &mut GeometryInstance) -> Option<()> {
    let geometry = instance.geometry.as_mut()?;
    if let Some(bs) = geometry.bounding_sphere {
        let min_x = bs.center.x - bs.radius;
        if min_x > 0.0 || bs.intersect_plane(&Plane::ORIGIN_ZX_PLANE) != Intersect::Intersecting {
            return Some(());
        }
    }
    match geometry.geometry_type {
        GeometryType::Polylines => split_longitude_polyline(instance),
        GeometryType::Triangles => split_longitude_triangles(instance),
        GeometryType::Lines => split_longitude_lines(instance),
        GeometryType::None => {
            index_primitive(geometry)?;
            match geometry.primitive_type {
                PrimitiveType::Triangles => split_longitude_triangles(instance),
                PrimitiveType::Lines => split_longitude_lines(instance),
                _ => Some(()),
            }
        }
    }
}
