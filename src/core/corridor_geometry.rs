//! CesiumJS `CorridorGeometry`: a filled geodesic corridor, optionally
//! extruded, with surface frames, texture coordinates and shadow directions.
//!
//! ```
//! use cesium_rs::core::{Cartesian3, CorridorGeometry, CorridorGeometryOptions, Ellipsoid};
//! let positions = Cartesian3::from_degrees_array(&[-72.0, 40.0, -70.0, 35.0], &Ellipsoid::WGS84);
//! let mut options = CorridorGeometryOptions::new(positions, 100_000.0);
//! options.height = 1000.0;
//! options.extruded_height = Some(0.0);
//! let geometry = CorridorGeometry::new(options).create_geometry().unwrap();
//! assert!(geometry.indices.is_some());
//! ```

use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::corner_type::CornerType;
use super::corridor_geometry_library::{compute_positions, CorridorCorner, CorridorPositionOptions, CorridorPositions};
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::js_math::JsMath;
use super::math::{equals_epsilon, js_max, js_min, EPSILON2, PI, PI_OVER_TWO, RADIANS_PER_DEGREE};
use super::polygon_pipeline::scale_to_geodetic_height;
use super::polyline_volume_geometry::corner_type_from;
use super::primitive_type::PrimitiveType;
use super::rectangle::Rectangle;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

#[derive(Clone, Debug, PartialEq)]
pub struct CorridorGeometryOptions {
    pub positions: Vec<Cartesian3>,
    pub width: f64,
    pub ellipsoid: Ellipsoid,
    pub vertex_format: VertexFormat,
    pub height: f64,
    pub extruded_height: Option<f64>,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl CorridorGeometryOptions {
    pub fn new(positions: Vec<Cartesian3>, width: f64) -> Self {
        Self {
            positions,
            width,
            ellipsoid: Ellipsoid::WGS84,
            vertex_format: VertexFormat::DEFAULT,
            height: 0.0,
            extruded_height: None,
            corner_type: CornerType::Rounded,
            granularity: RADIANS_PER_DEGREE,
            shadow_volume: false,
            offset_attribute: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorridorGeometry {
    pub positions: Vec<Cartesian3>,
    pub width: f64,
    pub ellipsoid: Ellipsoid,
    pub vertex_format: VertexFormat,
    pub height: f64,
    pub extruded_height: f64,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub packed_length: usize,
}

fn put(out: &mut [f64], i: usize, p: Cartesian3) {
    out[i..i + 3].copy_from_slice(&[p.x, p.y, p.z]);
}

fn put_f32(out: &mut [f32], i: usize, p: Cartesian3) {
    out[i..i + 3].copy_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
}

fn at_f32(out: &[f32], i: usize) -> Cartesian3 {
    Cartesian3::new(out[i] as f64, out[i + 1] as f64, out[i + 2] as f64)
}

#[derive(Default)]
struct Frames {
    normal: Option<Vec<f32>>,
    tangent: Option<Vec<f32>>,
    bitangent: Option<Vec<f32>>,
}

impl Frames {
    fn new(size: usize, vf: VertexFormat) -> Self {
        Self {
            normal: vf.normal.then(|| vec![0.0; size]),
            tangent: vf.tangent.then(|| vec![0.0; size]),
            bitangent: vf.bitangent.then(|| vec![0.0; size]),
        }
    }

    fn write(&mut self, normal: Cartesian3, left: Cartesian3, front: Option<usize>, back: Option<usize>) {
        let forward = left.cross(normal).normalize();
        for i in [front, back].into_iter().flatten() {
            for (values, p) in [(&mut self.normal, normal), (&mut self.tangent, forward), (&mut self.bitangent, left)] {
                if let Some(values) = values {
                    put_f32(values, i, p);
                }
            }
        }
    }

    fn into_attributes(self, attributes: &mut GeometryAttributes) {
        let float = |v| GeometryAttribute::new(ComponentDatatype::Float, 3, TypedArray::Float32(v));
        attributes.normal = self.normal.map(float);
        attributes.tangent = self.tangent.map(float);
        attributes.bitangent = self.bitangent.map(float);
    }
}

fn quad(indices: &mut Vec<u32>, ll: usize, ul: usize, lr: usize, ur: usize) {
    indices.extend([ul as u32, ll as u32, ur as u32, ur as u32, ll as u32, lr as u32]);
}

fn average_normal(right: &[f64], ri: usize, left: &[f64], li: usize, ellipsoid: &Ellipsoid) -> Option<Cartesian3> {
    Some(
        ellipsoid
            .geodetic_surface_normal(Cartesian3::from_array(right, ri))?
            .add(ellipsoid.geodetic_surface_normal(Cartesian3::from_array(left, li))?)
            .normalize(),
    )
}

fn texture_coordinates(left_count: usize, right_count: usize, end_length: Option<usize>) -> Vec<f32> {
    let mut st = Vec::with_capacity((left_count + right_count) * 2);
    let mut push = |s: f64, t: f64| st.extend([s as f32, t as f32]);
    if let Some(end) = end_length {
        let theta = PI / (end + 1) as f64;
        let left_st = 1.0 / (left_count - end + 1) as f64;
        let right_st = 1.0 / (right_count - end + 1) as f64;
        for i in end / 2 + 1..end + 1 {
            let a = PI_OVER_TWO + theta * i as f64;
            push(right_st * (1.0 + a.js_cos()), 0.5 * (1.0 + a.js_sin()));
        }
        for i in 1..right_count - end + 1 {
            push(i as f64 * right_st, 0.0);
        }
        for i in (end / 2 + 1..=end).rev() {
            let a = PI_OVER_TWO - i as f64 * theta;
            push(1.0 - right_st * (1.0 + a.js_cos()), 0.5 * (1.0 + a.js_sin()));
        }
        for i in (1..=end / 2).rev() {
            let a = PI_OVER_TWO - theta * i as f64;
            push(1.0 - left_st * (1.0 + a.js_cos()), 0.5 * (1.0 + a.js_sin()));
        }
        for i in (1..=left_count - end).rev() {
            push(i as f64 * left_st, 1.0);
        }
        for i in 1..end / 2 + 1 {
            let a = PI_OVER_TWO + theta * i as f64;
            push(left_st * (1.0 + a.js_cos()), 0.5 * (1.0 + a.js_sin()));
        }
    } else {
        let left_st = 1.0 / (left_count - 1) as f64;
        let right_st = 1.0 / (right_count - 1) as f64;
        for i in 0..right_count {
            push(i as f64 * right_st, 0.0);
        }
        for i in (1..=left_count).rev() {
            push((i - 1) as f64 * left_st, 1.0);
        }
    }
    st
}

fn combine(
    computed: CorridorPositions,
    vf: VertexFormat,
    ellipsoid: &Ellipsoid,
) -> Option<(GeometryAttributes, Vec<u32>)> {
    let CorridorPositions { positions, corners, end_positions, lefts, normals } = computed;
    let lefts = lefts?;
    let normals = normals?;
    let mut left_count = 3;
    let mut right_count = 3;
    for pair in positions.as_chunks::<2>().0 {
        left_count += pair[0].len() - 3;
        right_count += pair[1].len() - 3;
    }
    for c in &corners {
        match c {
            CorridorCorner::Left(p) => left_count += p.len(),
            CorridorCorner::Right(p) => right_count += p.len(),
        }
    }
    let end_length = end_positions.as_ref().map(|p| p[0].len() / 3 - 1);
    if let Some(end) = end_length {
        left_count += end * 3;
        right_count += end * 3;
    }
    let size = left_count + right_count;
    let mut out = vec![0.0; size];
    let mut frames = Frames::new(size, vf);
    let mut front = 0;
    let mut back = size - 3;
    let mut indices = Vec::new();
    let mut normal = Cartesian3::from_array(&normals, 0);
    let mut left = Cartesian3::from_array(&lefts, 0);
    if let Some(ends) = &end_positions {
        let end = end_length?;
        for i in 0..end / 2 {
            put(&mut out, back, Cartesian3::from_array(&ends[0], (end / 2 - 1 - i) * 3));
            put(&mut out, front, Cartesian3::from_array(&ends[0], (end / 2 + i) * 3));
            frames.write(normal, left, Some(front), Some(back));
            quad(&mut indices, front / 3, back / 3, front / 3 + 1, back / 3 - 1);
            front += 3;
            back -= 3;
        }
    }
    let right_edge = &positions[0];
    let left_edge = &positions[1];
    out[front..front + right_edge.len()].copy_from_slice(right_edge);
    out[back + 3 - left_edge.len()..back + 3].copy_from_slice(left_edge);
    let length = left_edge.len() - 3;
    let mut lr = front / 3;
    let mut ur = back / 3;
    for i in (0..length).step_by(3) {
        normal = average_normal(right_edge, i, left_edge, length - i, ellipsoid)?;
        frames.write(normal, left, Some(front), Some(back));
        lr = front / 3 + 1;
        ur = back / 3 - 1;
        quad(&mut indices, front / 3, back / 3, lr, ur);
        front += 3;
        back -= 3;
    }
    for (i, corner) in corners.iter().enumerate() {
        normal = Cartesian3::from_array(&normals, (i + 1) * 3);
        match corner {
            CorridorCorner::Left(p) => {
                frames.write(normal, left, None, Some(back));
                back -= 3;
                let (pivot, start) = (lr, ur);
                for j in 0..p.len() / 3 {
                    indices.extend([pivot as u32, (start - j - 1) as u32, (start - j) as u32]);
                    put(&mut out, back, Cartesian3::from_array(p, j * 3));
                    left = Cartesian3::from_array(&out, (start - j - 1) * 3)
                        .subtract(Cartesian3::from_array(&out, pivot * 3))
                        .normalize();
                    frames.write(normal, left, None, Some(back));
                    back -= 3;
                }
                let outside = Cartesian3::from_array(&out, pivot * 3);
                let previous = Cartesian3::from_array(&out, start * 3).subtract(outside);
                let next = Cartesian3::from_array(&out, (start - p.len() / 3) * 3).subtract(outside);
                left = previous.add(next).normalize();
                frames.write(normal, left, Some(front), None);
                front += 3;
            }
            CorridorCorner::Right(p) => {
                frames.write(normal, left, Some(front), None);
                front += 3;
                let (pivot, start) = (ur, lr);
                for j in 0..p.len() / 3 {
                    indices.extend([pivot as u32, (start + j) as u32, (start + j + 1) as u32]);
                    put(&mut out, front, Cartesian3::from_array(p, j * 3));
                    left = Cartesian3::from_array(&out, pivot * 3)
                        .subtract(Cartesian3::from_array(&out, (start + j) * 3))
                        .normalize();
                    frames.write(normal, left, Some(front), None);
                    front += 3;
                }
                let outside = Cartesian3::from_array(&out, pivot * 3);
                let previous = Cartesian3::from_array(&out, (start + p.len() / 3) * 3).subtract(outside);
                let next = Cartesian3::from_array(&out, start * 3).subtract(outside);
                left = next.add(previous).negate().normalize();
                frames.write(normal, left, None, Some(back));
                back -= 3;
            }
        }
        let right_edge = &positions[i * 2 + 2][3..];
        let left_edge = &positions[i * 2 + 3];
        let left_edge = &left_edge[..left_edge.len() - 3];
        out[front..front + right_edge.len()].copy_from_slice(right_edge);
        out[back + 3 - left_edge.len()..back + 3].copy_from_slice(left_edge);
        let length = left_edge.len() - 3;
        left = Cartesian3::from_array(&lefts, (i + 2) * 3);
        for j in (0..left_edge.len()).step_by(3) {
            normal = average_normal(right_edge, j, left_edge, length - j, ellipsoid)?;
            frames.write(normal, left, Some(front), Some(back));
            lr = front / 3;
            ur = back / 3;
            quad(&mut indices, lr - 1, ur + 1, lr, ur);
            front += 3;
            back -= 3;
        }
        front -= 3;
        back += 3;
    }
    normal = Cartesian3::from_array(&normals, normals.len() - 3);
    frames.write(normal, left, Some(front), Some(back));
    if let Some(ends) = &end_positions {
        let end = end_length?;
        front += 3;
        back -= 3;
        for i in 0..end / 2 {
            put(&mut out, back, Cartesian3::from_array(&ends[1], (end - i - 1) * 3));
            put(&mut out, front, Cartesian3::from_array(&ends[1], i * 3));
            frames.write(normal, left, Some(front), Some(back));
            quad(&mut indices, front / 3 - 1, back / 3 + 1, front / 3, back / 3);
            front += 3;
            back -= 3;
        }
    }
    let mut attributes =
        GeometryAttributes { position: Some(GeometryAttribute::doubles(3, out)), ..Default::default() };
    frames.into_attributes(&mut attributes);
    if vf.st {
        attributes.st = Some(GeometryAttribute::new(
            ComponentDatatype::Float,
            2,
            TypedArray::Float32(texture_coordinates(left_count / 3, right_count / 3, end_length)),
        ));
    }
    Some((attributes, indices))
}

/// Duplicates the perimeter vertices so each edge has its own wall frame.
fn wall_positions<T: Copy>(positions: &[T], components: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(positions.len() * 2);
    out.extend_from_slice(&positions[..components]);
    for p in positions[components..].chunks_exact(components) {
        out.extend_from_slice(p);
        out.extend_from_slice(p);
    }
    out.extend_from_slice(&positions[..components]);
    out
}

fn f32_values(attribute: &Option<GeometryAttribute>) -> Option<Vec<f32>> {
    match &attribute.as_ref()?.values {
        TypedArray::Float32(v) => Some(v.clone()),
        _ => None,
    }
}

fn extruded_attributes(attributes: &mut GeometryAttributes, vf: VertexFormat, positions: &[f64]) -> Option<()> {
    if !(vf.normal || vf.tangent || vf.bitangent || vf.st) {
        return Some(());
    }
    let size = positions.len() / 18;
    let three = size * 3;
    let top_normals = if vf.normal || vf.bitangent { Some(f32_values(&attributes.normal)?) } else { None };
    let top_bitangents = if vf.normal || vf.bitangent { Some(f32_values(&attributes.bitangent)?) } else { None };
    if vf.normal || vf.bitangent || vf.tangent {
        let mut frames = Frames::new(three * 6, vf);
        for i in (0..three).step_by(3) {
            let top = Cartesian3::from_array(positions, i);
            let bottom = Cartesian3::from_array(positions, i + three).subtract(top);
            let previous = Cartesian3::from_array(positions, (i + 3) % three).subtract(top);
            let normal = bottom.cross(previous).normalize();
            let bitangent =
                if vf.tangent || vf.bitangent { at_f32(top_normals.as_ref()?, i) } else { Cartesian3::ZERO };
            let tangent = bitangent.cross(normal).normalize();
            let start = three * 2 + i * 2;
            for index in [start, start + 3, start + three * 2, start + three * 2 + 3] {
                if let Some(values) = &mut frames.normal {
                    put_f32(values, index, normal);
                }
                if let Some(values) = &mut frames.bitangent {
                    put_f32(values, index, bitangent);
                }
                if let Some(values) = &mut frames.tangent {
                    put_f32(values, index, tangent);
                }
            }
        }
        if let Some(values) = &mut frames.normal {
            let top = top_normals.as_ref()?;
            values[..three].copy_from_slice(top);
            for i in 0..three {
                values[i + three] = -top[i];
            }
        }
        if let Some(values) = &mut frames.bitangent {
            let top = top_bitangents.as_ref()?;
            values[..three].copy_from_slice(top);
            values[three..three * 2].copy_from_slice(top);
        }
        if let Some(values) = &mut frames.tangent {
            let top = f32_values(&attributes.tangent)?;
            values[..three].copy_from_slice(&top);
            values[three..three * 2].copy_from_slice(&top);
        }
        frames.into_attributes(attributes);
    }
    if vf.st {
        let top = f32_values(&attributes.st)?;
        let mut st = top.clone();
        st.extend_from_slice(&top);
        let wall = wall_positions(&top, 2);
        st.extend_from_slice(&wall);
        st.extend_from_slice(&wall);
        attributes.st = Some(GeometryAttribute::new(ComponentDatatype::Float, 2, TypedArray::Float32(st)));
    }
    Some(())
}

fn extrude(attributes: &mut GeometryAttributes, indices: &mut Vec<u32>, corridor: &CorridorGeometry) -> Option<()> {
    let mut top = attributes.position.as_ref()?.values.to_f64_vec();
    let mut bottom = top.clone();
    let size = top.len() / 3;
    scale_to_geodetic_height(&mut top, Some(corridor.height), &corridor.ellipsoid, None)?;
    scale_to_geodetic_height(&mut bottom, Some(corridor.extruded_height), &corridor.ellipsoid, None)?;
    let mut positions = top.clone();
    positions.extend_from_slice(&bottom);
    positions.extend(wall_positions(&top, 3));
    positions.extend(wall_positions(&bottom, 3));
    let vf = corridor.vertex_format;
    extruded_attributes(attributes, vf, &positions)?;
    if corridor.shadow_volume {
        let mut normals = f32_values(&attributes.normal)?;
        for n in &mut normals {
            *n = -*n;
        }
        let length = normals.len();
        let mut directions = vec![0.0; length * 6];
        directions[length..length * 2].copy_from_slice(&normals);
        directions[length * 4..].copy_from_slice(&wall_positions(&normals, 3));
        // Cesium negates this temporary attribute in place.
        attributes.normal =
            vf.normal.then(|| GeometryAttribute::new(ComponentDatatype::Float, 3, TypedArray::Float32(normals)));
        attributes.set_other(
            "extrudeDirection",
            GeometryAttribute::new(ComponentDatatype::Float, 3, TypedArray::Float32(directions)),
        );
    }
    if let Some(offset) = corridor.offset_attribute {
        let mut attribute = GeometryAttribute::apply_offset(size * 6, offset);
        if offset == GeometryOffsetAttribute::Top {
            if let TypedArray::Uint8(values) = &mut attribute.values {
                values.fill(0);
                values[..size].fill(1);
                values[size * 2..size * 4].fill(1);
            }
        }
        attributes.set_other("applyOffset", attribute);
    }
    let original_length = indices.len();
    for i in (0..original_length).step_by(3) {
        let [v0, v1, v2] = [indices[i], indices[i + 1], indices[i + 2]];
        indices.extend([v2 + size as u32, v1 + size as u32, v0 + size as u32]);
    }
    for i in (0..size * 2).step_by(2) {
        let ul = i + size * 2;
        quad(indices, ul + size * 2, ul, ul + size * 2 + 1, ul + 1);
    }
    attributes.position = Some(GeometryAttribute::doubles(3, positions));
    Some(())
}

fn clean_positions(positions: &[Cartesian3], ellipsoid: &Ellipsoid) -> Option<Vec<Cartesian3>> {
    let positions = positions.iter().map(|p| ellipsoid.scale_to_geodetic_surface(*p)).collect::<Option<Vec<_>>>()?;
    Some(array_remove_duplicates(&positions, |a, b, e| a.equals_epsilon(*b, e), false, None))
}

impl CorridorGeometry {
    pub fn new(options: CorridorGeometryOptions) -> Self {
        let extruded = options.extruded_height.unwrap_or(options.height);
        let packed_length =
            1 + options.positions.len() * 3 + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH + 7;
        Self {
            positions: options.positions,
            width: options.width,
            ellipsoid: options.ellipsoid,
            vertex_format: options.vertex_format,
            height: js_max(options.height, extruded),
            extruded_height: js_min(options.height, extruded),
            corner_type: options.corner_type,
            granularity: options.granularity,
            shadow_volume: options.shadow_volume,
            offset_attribute: options.offset_attribute,
            packed_length,
        }
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        array[i] = self.positions.len() as f64;
        i += 1;
        for p in &self.positions {
            p.pack(array, i);
            i += 3;
        }
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        array[i..i + 7].copy_from_slice(&[
            self.width,
            self.height,
            self.extruded_height,
            self.corner_type as u32 as f64,
            self.granularity,
            if self.shadow_volume { 1.0 } else { 0.0 },
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ]);
    }

    pub fn unpack(array: &[f64], mut i: usize) -> Self {
        let count = array[i] as usize;
        i += 1;
        let positions = (0..count).map(|k| Cartesian3::unpack(array, i + k * 3)).collect();
        i += count * 3;
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        Self::new(CorridorGeometryOptions {
            positions,
            width: array[i],
            ellipsoid,
            vertex_format,
            height: array[i + 1],
            extruded_height: Some(array[i + 2]),
            corner_type: corner_type_from(array[i + 3]),
            granularity: array[i + 4],
            shadow_volume: array[i + 5] == 1.0,
            offset_attribute: GeometryOffsetAttribute::unpack(array[i + 6]),
        })
    }

    /// Bounding rectangle, computed on demand without changing the inputs.
    /// As in Cesium, antimeridian corridors can span nearly the whole globe.
    pub fn compute_rectangle(
        positions: &[Cartesian3],
        ellipsoid: &Ellipsoid,
        width: f64,
        corner_type: CornerType,
    ) -> Option<Rectangle> {
        let positions = clean_positions(positions, ellipsoid)?;
        if positions.len() < 2 || width <= 0.0 {
            return Some(Rectangle::default());
        }
        let half_width = width * 0.5;
        let mut rect = Rectangle::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut include = |p: Cartesian3| -> Option<()> {
            let p = ellipsoid.cartesian_to_cartographic(p)?;
            rect.west = js_min(rect.west, p.longitude);
            rect.south = js_min(rect.south, p.latitude);
            rect.east = js_max(rect.east, p.longitude);
            rect.north = js_max(rect.north, p.latitude);
            Some(())
        };
        let mut offset_points = |start: Cartesian3, end: Cartesian3| -> Option<()> {
            let direction = end.subtract(start).normalize();
            let normal = ellipsoid.geodetic_surface_normal(start)?;
            let offset = direction.cross(normal).multiply_by_scalar(half_width);
            include(start.add(offset))?;
            include(start.subtract(offset))
        };
        // Offset edges first; min/max are order independent for finite values.
        for pair in positions.windows(2) {
            offset_points(pair[0], pair[1])?;
        }
        let last = positions[positions.len() - 1];
        let offset = last.subtract(positions[positions.len() - 2]).normalize().multiply_by_scalar(half_width);
        let end = last.add(offset);
        offset_points(last, end)?;
        if corner_type == CornerType::Rounded {
            let first = positions[0];
            let offset = first.subtract(positions[1]).normalize().multiply_by_scalar(half_width);
            include(first.add(offset))?;
            include(end)?;
        }
        Some(rect)
    }

    pub fn rectangle(&self) -> Option<Rectangle> {
        Self::compute_rectangle(&self.positions, &self.ellipsoid, self.width, self.corner_type)
    }

    pub fn texture_coordinate_rotation_points(&self) -> [f64; 6] {
        [0.0, 0.0, 0.0, 1.0, 1.0, 0.0]
    }

    pub fn create_shadow_volume(
        &self,
        min_height: impl Fn(f64, &Ellipsoid) -> f64,
        max_height: impl Fn(f64, &Ellipsoid) -> f64,
    ) -> Self {
        let mut options = CorridorGeometryOptions::new(self.positions.clone(), self.width);
        options.ellipsoid = self.ellipsoid;
        options.corner_type = self.corner_type;
        options.granularity = self.granularity;
        options.extruded_height = Some(min_height(self.granularity, &self.ellipsoid));
        options.height = max_height(self.granularity, &self.ellipsoid);
        options.vertex_format = VertexFormat::POSITION_ONLY;
        options.shadow_volume = true;
        Self::new(options)
    }

    /// Returns `None` for degenerate paths, invalid spacing, unsupported
    /// geodesics or vertex-format combinations that Cesium cannot construct.
    pub fn create_geometry(&self) -> Option<Geometry> {
        if self.width <= 0.0 {
            return None;
        }
        let positions = clean_positions(&self.positions, &self.ellipsoid)?;
        let mut options = CorridorPositionOptions::new(&positions, self.width);
        options.ellipsoid = self.ellipsoid;
        options.corner_type = self.corner_type;
        options.granularity = self.granularity;
        options.save_attributes = true;
        let vf = self.vertex_format;
        let is_extruded = !equals_epsilon(self.height, self.extruded_height, 0.0, EPSILON2);
        let top_vf = if is_extruded {
            VertexFormat {
                normal: vf.normal || vf.bitangent || self.shadow_volume,
                bitangent: vf.normal || vf.bitangent,
                ..vf
            }
        } else {
            vf
        };
        let (mut attributes, mut indices) = combine(compute_positions(&options)?, top_vf, &self.ellipsoid)?;
        if is_extruded {
            extrude(&mut attributes, &mut indices, self)?;
        } else {
            let mut positions = attributes.position.as_ref()?.values.to_f64_vec();
            scale_to_geodetic_height(&mut positions, Some(self.height), &self.ellipsoid, None)?;
            if let Some(offset) = self.offset_attribute {
                attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
            }
            attributes.position = Some(GeometryAttribute::doubles(3, positions));
        }
        let values = &attributes.position.as_ref()?.values.to_f64_vec();
        let count = values.len() / 3;
        let sphere = BoundingSphere::from_vertices(values, Cartesian3::ZERO, 3);
        if !vf.position {
            attributes.position = None;
        }
        let mut geometry = Geometry::new(
            attributes,
            Some(IndexDatatype::create_typed_array(count, &indices)),
            PrimitiveType::Triangles,
        );
        geometry.bounding_sphere = Some(sphere);
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
