//! CesiumJS `RectangleGeometry`: a cartographic rectangle on the ellipsoid,
//! optionally rotated and extruded, with texture coordinates and normal
//! frames.

use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::geometry_instance::GeometryInstance;
use super::geometry_pipeline::combine_instances;
use super::index_datatype::IndexDatatype;
use super::math::{equals_epsilon, js_max, js_min, EPSILON10, EPSILON2, RADIANS_PER_DEGREE};
use super::matrix2::Matrix2;
use super::matrix3::Matrix3;
use super::polygon_pipeline::scale_to_geodetic_height;
use super::primitive_type::PrimitiveType;
use super::quaternion::Quaternion;
use super::rectangle::Rectangle;
use super::rectangle_geometry_library::{compute_options, compute_position, RectangleComputedOptions};
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleGeometryOptions {
    pub rectangle: Rectangle,
    pub height: f64,
    /// Defaults to `height`.
    pub extruded_height: Option<f64>,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    pub rotation: f64,
    pub st_rotation: f64,
    pub vertex_format: VertexFormat,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl RectangleGeometryOptions {
    pub fn new(rectangle: Rectangle) -> Self {
        Self {
            rectangle,
            height: 0.0,
            extruded_height: None,
            granularity: RADIANS_PER_DEGREE,
            ellipsoid: Ellipsoid::WGS84,
            rotation: 0.0,
            st_rotation: 0.0,
            vertex_format: VertexFormat::DEFAULT,
            shadow_volume: false,
            offset_attribute: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleGeometry {
    pub rectangle: Rectangle,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    /// The higher of the two heights.
    pub surface_height: f64,
    pub rotation: f64,
    pub st_rotation: f64,
    pub vertex_format: VertexFormat,
    /// The lower of the two heights.
    pub extruded_height: f64,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

/// Writes ignore out-of-range indices, as typed arrays do.
fn put<T: Copy>(values: &mut [T], i: usize, v: T) {
    if let Some(slot) = values.get_mut(i) {
        *slot = v;
    }
}

fn get(values: &[f64], i: usize) -> f64 {
    values.get(i).copied().unwrap_or(f64::NAN)
}

fn neg(values: &[f32]) -> Vec<f32> {
    values.iter().map(|v| -v).collect()
}

fn floats(values: Vec<f32>) -> GeometryAttribute {
    GeometryAttribute::new(ComponentDatatype::Float, 3, TypedArray::Float32(values))
}

fn st_attribute(values: Vec<f32>) -> GeometryAttribute {
    GeometryAttribute::new(ComponentDatatype::Float, 2, TypedArray::Float32(values))
}

fn uint8(values: Vec<u8>) -> GeometryAttribute {
    GeometryAttribute::new(ComponentDatatype::UnsignedByte, 1, TypedArray::Uint8(values))
}

/// Normals, tangents and bitangents as `Float32Array`s.
#[derive(Default)]
struct Frames {
    normals: Option<Vec<f32>>,
    tangents: Option<Vec<f32>>,
    bitangents: Option<Vec<f32>>,
}

fn calculate_attributes(positions: &[f64], vf: VertexFormat, ellipsoid: &Ellipsoid, tangent_rotation: &Matrix3) -> Frames {
    let length = positions.len();
    let mut f = Frames {
        normals: vf.normal.then(|| vec![0f32; length]),
        tangents: vf.tangent.then(|| vec![0f32; length]),
        bitangents: vf.bitangent.then(|| vec![0f32; length]),
    };
    if !(vf.normal || vf.tangent || vf.bitangent) {
        return f;
    }
    let mut tangent = Cartesian3::ZERO;
    let mut bitangent = Cartesian3::ZERO;
    for i in (0..length).step_by(3) {
        let p = Cartesian3::from_array(positions, i);
        let normal = ellipsoid.geodetic_surface_normal(p).unwrap_or(Cartesian3::new(f64::NAN, f64::NAN, f64::NAN));
        if vf.tangent || vf.bitangent {
            tangent = tangent_rotation.multiply_by_vector(Cartesian3::UNIT_Z.cross(normal)).normalize();
            if vf.bitangent {
                bitangent = normal.cross(tangent).normalize();
            }
        }
        for (values, v) in [(&mut f.normals, normal), (&mut f.tangents, tangent), (&mut f.bitangents, bitangent)] {
            if let Some(values) = values {
                values[i..i + 3].copy_from_slice(&[v.x as f32, v.y as f32, v.z as f32]);
            }
        }
    }
    f
}

fn calculate_attributes_wall(positions: &[f64], vf: VertexFormat, ellipsoid: &Ellipsoid) -> Frames {
    let length = positions.len();
    let mut f = Frames {
        normals: vf.normal.then(|| vec![0f32; length]),
        tangents: vf.tangent.then(|| vec![0f32; length]),
        bitangents: vf.bitangent.then(|| vec![0f32; length]),
    };
    if !(vf.normal || vf.tangent || vf.bitangent) || length == 0 {
        return f;
    }
    let mut recompute_normal = true;
    let (mut normal, mut tangent, mut bitangent) = (Cartesian3::ZERO, Cartesian3::ZERO, Cartesian3::ZERO);
    let mut index = 0;
    for i in (0..length).step_by(6) {
        let p = Cartesian3::from_array(positions, i);
        let mut p1 = Cartesian3::from_array(positions, (i + 6) % length);
        if recompute_normal {
            let p2 = Cartesian3::from_array(positions, (i + 3) % length).subtract(p);
            p1 = p1.subtract(p);
            normal = p2.cross(p1).normalize();
            recompute_normal = false;
        }
        // Cesium compares the edge vector here when it just recomputed the
        // normal, and the next position otherwise.
        if p1.equals_epsilon(p, EPSILON10) {
            recompute_normal = true;
        }
        if vf.tangent || vf.bitangent {
            bitangent = ellipsoid.geodetic_surface_normal(p).unwrap_or(bitangent);
            if vf.tangent {
                tangent = bitangent.cross(normal).normalize();
            }
        }
        for (values, v) in [(&mut f.normals, normal), (&mut f.tangents, tangent), (&mut f.bitangents, bitangent)] {
            if let Some(values) = values {
                let v = [v.x as f32, v.y as f32, v.z as f32];
                for k in 0..6 {
                    put(values, index + k, v[k % 3]);
                }
            }
        }
        index += 6;
    }
    f
}

/// The top surface: positions on the ellipsoid, texture coordinates,
/// frames and indices.
struct Surface {
    positions: Vec<f64>,
    st: Option<Vec<f32>>,
    frames: Frames,
    indices: Vec<u32>,
    size: usize,
}

fn construct_rectangle(vf: VertexFormat, ellipsoid: &Ellipsoid, o: &RectangleComputedOptions) -> Option<Surface> {
    // Cesium writes positions unconditionally and throws without them.
    if !vf.position {
        return None;
    }
    let (height, width) = (o.height, o.width);
    let (mut row_start, mut row_end, mut row_height, mut size) = (0, height, height as isize, 0usize);
    if o.north_cap {
        row_start = 1;
        row_height -= 1;
        size += 1;
    }
    if o.south_cap {
        row_end = row_end.saturating_sub(1);
        row_height -= 1;
        size += 1;
    }
    size += width * row_height.max(0) as usize;
    let mut positions = vec![0.0; size * 3];
    let mut st = vf.st.then(|| vec![0f32; size * 2]);
    let (mut pos_index, mut st_index) = (0, 0);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, -f64::MAX, -f64::MAX);
    let mut write = |position: Cartesian3, t: Cartesian2, st: &mut Option<Vec<f32>>| {
        for v in [position.x, position.y, position.z] {
            put(&mut positions, pos_index, v);
            pos_index += 1;
        }
        if let Some(st) = st.as_mut() {
            put(st, st_index, t.x as f32);
            put(st, st_index + 1, t.y as f32);
            st_index += 2;
        }
    };
    for row in row_start..row_end {
        for col in 0..width {
            let (p, t) = compute_position(o, ellipsoid, vf.st, row as f64, col as f64);
            write(p, t, &mut st);
            if vf.st {
                min_x = js_min(min_x, t.x);
                min_y = js_min(min_y, t.y);
                max_x = js_max(max_x, t.x);
                max_y = js_max(max_y, t.y);
            }
        }
    }
    if o.north_cap {
        let (p, t) = compute_position(o, ellipsoid, vf.st, 0.0, 0.0);
        write(p, t, &mut st);
        if vf.st {
            (min_x, min_y, max_x, max_y) = (t.x, t.y, t.x, t.y);
        }
    }
    if o.south_cap {
        let (p, t) = compute_position(o, ellipsoid, vf.st, height as f64 - 1.0, 0.0);
        write(p, t, &mut st);
        if vf.st {
            min_x = js_min(min_x, t.x);
            min_y = js_min(min_y, t.y);
            max_x = js_max(max_x, t.x);
            max_y = js_max(max_y, t.y);
        }
    }
    if let Some(st) = st.as_mut() {
        if min_x < 0.0 || min_y < 0.0 || max_x > 1.0 || max_y > 1.0 {
            for k in (0..st.len()).step_by(2) {
                st[k] = ((f64::from(st[k]) - min_x) / (max_x - min_x)) as f32;
                st[k + 1] = ((f64::from(st[k + 1]) - min_y) / (max_y - min_y)) as f32;
            }
        }
    }
    let frames = calculate_attributes(&positions, vf, ellipsoid, &o.tangent_rotation_matrix);

    let w = width as isize;
    let mut indices_size = 6 * (w - 1) * (row_height - 1);
    if o.north_cap {
        indices_size += 3 * (w - 1);
    }
    if o.south_cap {
        indices_size += 3 * (w - 1);
    }
    if indices_size < 0 {
        return None;
    }
    let mut indices = vec![0u32; indices_size as usize];
    let mut ii = 0;
    let mut push = |indices: &mut Vec<u32>, v: usize| {
        put(indices, ii, v as u32);
        ii += 1;
    };
    let mut index = 0;
    for _ in 0..(row_height - 1).max(0) {
        for _ in 0..width.saturating_sub(1) {
            let upper_left = index;
            let lower_left = upper_left + width;
            let lower_right = lower_left + 1;
            let upper_right = upper_left + 1;
            for v in [upper_left, lower_left, upper_right, upper_right, lower_left, lower_right] {
                push(&mut indices, v);
            }
            index += 1;
        }
        index += 1;
    }
    if o.north_cap || o.south_cap {
        let mut north_index = size.wrapping_sub(1);
        let south_index = size.wrapping_sub(1);
        if o.north_cap && o.south_cap {
            north_index = size.wrapping_sub(2);
        }
        if o.north_cap {
            for i in 0..width.saturating_sub(1) {
                for v in [north_index, i, i + 1] {
                    push(&mut indices, v);
                }
            }
        }
        if o.south_cap {
            let start = ((row_height - 1).max(0) as usize) * width;
            for i in 0..width.saturating_sub(1) {
                for v in [start + i, south_index, start + i + 1] {
                    push(&mut indices, v);
                }
            }
        }
    }
    Some(Surface { positions, st, frames, indices, size })
}

impl RectangleGeometry {
    pub const PACKED_LENGTH: usize = 4 + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH + 7;

    pub fn new(options: RectangleGeometryOptions) -> Self {
        let height = options.height;
        let extruded_height = options.extruded_height.unwrap_or(height);
        Self {
            rectangle: options.rectangle,
            granularity: options.granularity,
            ellipsoid: options.ellipsoid,
            surface_height: js_max(height, extruded_height),
            rotation: options.rotation,
            st_rotation: options.st_rotation,
            vertex_format: options.vertex_format,
            extruded_height: js_min(height, extruded_height),
            shadow_volume: options.shadow_volume,
            offset_attribute: options.offset_attribute,
        }
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        self.rectangle.pack(array, i);
        i += 4;
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        let values = [
            self.granularity,
            self.surface_height,
            self.rotation,
            self.st_rotation,
            self.extruded_height,
            if self.shadow_volume { 1.0 } else { 0.0 },
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ];
        array[i..i + 7].copy_from_slice(&values);
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let rectangle = Rectangle::unpack(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i + 4);
        let vertex_format = VertexFormat::unpack(array, i + 4 + Ellipsoid::PACKED_LENGTH);
        let o = i + 4 + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH;
        let a = &array[o..o + 7];
        Self::new(RectangleGeometryOptions {
            rectangle,
            height: a[1],
            extruded_height: Some(a[4]),
            granularity: a[0],
            ellipsoid,
            rotation: a[2],
            st_rotation: a[3],
            vertex_format,
            shadow_volume: a[5] == 1.0,
            offset_attribute: GeometryOffsetAttribute::unpack(a[6]),
        })
    }

    /// The bounds of the rectangle rotated by `rotation`.
    pub fn compute_rectangle(rectangle: &Rectangle, granularity: f64, rotation: f64, ellipsoid: &Ellipsoid) -> Rectangle {
        if rotation == 0.0 {
            return *rectangle;
        }
        let o = compute_options(rectangle, granularity, rotation, 0.0);
        let (h, w) = (o.height as f64 - 1.0, o.width as f64 - 1.0);
        let corners = [(0.0, 0.0), (0.0, w), (h, 0.0), (h, w)].map(|(r, c)| compute_position(&o, ellipsoid, false, r, c).0);
        Rectangle::from_cartesian_array(&corners, ellipsoid)
    }

    /// The rotated rectangle's bounds.
    pub fn rectangle(&self) -> Rectangle {
        Self::compute_rectangle(&self.rectangle, self.granularity, self.rotation, &self.ellipsoid)
    }

    /// Three corners of the rotated texture space, as six numbers.
    pub fn texture_coordinate_rotation_points(&self) -> [f64; 6] {
        if self.st_rotation == 0.0 {
            return [0.0, 0.0, 0.0, 1.0, 1.0, 0.0];
        }
        let unrotated =
            Self::compute_rectangle(&self.rectangle, self.granularity, self.rotation - self.st_rotation, &self.ellipsoid);
        let bounding = self.rectangle();
        let to_desired = Matrix2::from_rotation(self.st_rotation);
        let center = bounding.center();
        let points = [
            Cartesian2::new(unrotated.west, unrotated.south),
            Cartesian2::new(unrotated.west, unrotated.north),
            Cartesian2::new(unrotated.east, unrotated.south),
        ]
        .map(|p| {
            let p = Cartesian2::new(p.x - center.longitude, p.y - center.latitude);
            let p = to_desired.multiply_by_vector(p);
            let p = Cartesian2::new(p.x + center.longitude, p.y + center.latitude);
            Cartesian2::new((p.x - bounding.west) / bounding.width(), (p.y - bounding.south) / bounding.height())
        });
        [points[0].x, points[0].y, points[1].x, points[1].y, points[2].x, points[2].y]
    }

    /// A shadow volume between the heights the two functions return.
    pub fn create_shadow_volume(
        &self,
        min_height: impl Fn(f64, &Ellipsoid) -> f64,
        max_height: impl Fn(f64, &Ellipsoid) -> f64,
    ) -> Self {
        let mut options = RectangleGeometryOptions::new(self.rectangle);
        options.rotation = self.rotation;
        options.ellipsoid = self.ellipsoid;
        options.st_rotation = self.st_rotation;
        options.granularity = self.granularity;
        options.extruded_height = Some(max_height(self.granularity, &self.ellipsoid));
        options.height = min_height(self.granularity, &self.ellipsoid);
        options.vertex_format = VertexFormat::POSITION_ONLY;
        options.shadow_volume = true;
        Self::new(options)
    }

    fn construct_extruded(&self, o: &RectangleComputedOptions) -> Option<Geometry> {
        let vf = self.vertex_format;
        let shadow = self.shadow_volume;
        let ellipsoid = &self.ellipsoid;
        // A shadow volume needs normals for its extrusion directions;
        // with requested normals too, Cesium throws a RangeError.
        if shadow && vf.normal {
            return None;
        }
        let top = construct_rectangle(VertexFormat { normal: vf.normal || shadow, ..vf }, ellipsoid, o)?;
        let (height, width) = (o.height, o.width);

        // Cesium scales the same array twice: up to the top height, then
        // from there to the surface and down to the bottom.
        let mut top_positions = top.positions.clone();
        scale_to_geodetic_height(&mut top_positions, Some(self.surface_height), ellipsoid, Some(false))?;
        let mut bottom_positions = top_positions.clone();
        scale_to_geodetic_height(&mut bottom_positions, Some(self.extruded_height), ellipsoid, None)?;
        let length = top_positions.len();
        let positions = [top_positions.as_slice(), &bottom_positions].concat();

        let mut attributes = GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
        let mut top_normals = Vec::new();
        if let Some(n) = &top.frames.normals {
            top_normals = neg(n);
            if vf.normal {
                attributes.normal = Some(floats([n.as_slice(), &top_normals].concat()));
            }
        }
        if shadow {
            let mut extrude = vec![0f32; length];
            extrude.extend_from_slice(&top_normals);
            attributes.set_other("extrudeDirection", floats(extrude));
        }
        let has_offsets = self.offset_attribute.is_some();
        let top_offsets = self.offset_attribute == Some(GeometryOffsetAttribute::Top);
        if let Some(offset) = self.offset_attribute {
            let size = (length / 3) * 2;
            let mut values = vec![0u8; size];
            match offset {
                GeometryOffsetAttribute::Top => values[..size / 2].fill(1),
                GeometryOffsetAttribute::None => {}
                GeometryOffsetAttribute::All => values.fill(1),
            }
            attributes.set_other("applyOffset", uint8(values));
        }
        if let Some(t) = &top.frames.tangents {
            attributes.tangent = Some(floats([t.as_slice(), &neg(t)].concat()));
        }
        if let Some(b) = &top.frames.bitangents {
            attributes.bitangent = Some(floats([b.as_slice(), b].concat()));
        }
        let top_st = top.st.clone().unwrap_or_default();
        if let Some(st) = &top.st {
            attributes.st = Some(st_attribute([st.as_slice(), st].concat()));
        }
        let indices_length = top.indices.len();
        let pos_length = (length / 3) as u32;
        let mut new_indices = top.indices.clone();
        new_indices.resize(indices_length * 2, 0);
        for i in (0..indices_length).step_by(3) {
            new_indices[i + indices_length] = top.indices[i + 2] + pos_length;
            new_indices[i + 1 + indices_length] = top.indices[i + 1] + pos_length;
            new_indices[i + 2 + indices_length] = top.indices[i] + pos_length;
        }
        let top_bottom = Geometry::new(
            attributes,
            Some(IndexDatatype::create_typed_array(length / 3 * 2, &new_indices)),
            PrimitiveType::Triangles,
        );

        // The walls.
        let (north_cap, south_cap) = (o.north_cap, o.south_cap);
        let mut row_height = height as isize;
        let mut width_multiplier = 2isize;
        let mut perimeter_positions = 0isize;
        let mut corners = 4isize;
        let mut duplicate_corners = 4isize;
        for cap in [north_cap, south_cap] {
            if cap {
                width_multiplier -= 1;
                row_height -= 1;
                perimeter_positions += 1;
                corners -= 2;
                duplicate_corners -= 1;
            }
        }
        perimeter_positions += width_multiplier * width as isize + 2 * row_height - corners;
        let wall_count = ((perimeter_positions + duplicate_corners) * 2).max(0) as usize;
        let mut wall_positions = vec![0.0; wall_count * 3];
        let mut wall_extrude = shadow.then(|| vec![0f32; wall_count * 3]);
        let mut wall_offsets = has_offsets.then(|| vec![0u8; wall_count]);
        let mut wall_st = vf.st.then(|| vec![0f32; wall_count * 2]);
        if has_offsets && !top_offsets {
            let value = u8::from(self.offset_attribute == Some(GeometryOffsetAttribute::All));
            if let Some(w) = wall_offsets.as_mut() {
                w.fill(value);
            }
        }
        let (mut pos_index, mut st_index, mut extrude_index, mut offset_index) = (0, 0, 0, 0);
        let mut add = |i: usize| {
            let three_i = i * 3;
            for k in 0..3 {
                put(&mut wall_positions, pos_index + k, get(&top_positions, three_i + k));
                put(&mut wall_positions, pos_index + 3 + k, get(&bottom_positions, three_i + k));
            }
            pos_index += 6;
            if let Some(st) = wall_st.as_mut() {
                let (s, t) = (
                    top_st.get(i * 2).copied().unwrap_or(f32::NAN),
                    top_st.get(i * 2 + 1).copied().unwrap_or(f32::NAN),
                );
                for (k, v) in [s, t, s, t].into_iter().enumerate() {
                    put(st, st_index + k, v);
                }
                st_index += 4;
            }
            if let Some(e) = wall_extrude.as_mut() {
                extrude_index += 3;
                for k in 0..3 {
                    put(e, extrude_index, top_normals.get(three_i + k).copied().unwrap_or(f32::NAN));
                    extrude_index += 1;
                }
            }
            if top_offsets {
                if let Some(w) = wall_offsets.as_mut() {
                    put(w, offset_index, 1);
                }
                offset_index += 2;
            }
        };
        let area = width * row_height.max(0) as usize;
        let mut i = 0;
        while i < area {
            add(i);
            i += width.max(1);
        }
        if !south_cap {
            for i in area.saturating_sub(width)..area {
                add(i);
            }
        } else {
            let south_index = if north_cap { area + 1 } else { area };
            add(south_index);
            add(south_index);
        }
        let mut i = area as isize - 1;
        while i > 0 {
            add(i as usize);
            i -= width as isize;
        }
        if !north_cap {
            for i in (0..width).rev() {
                add(i);
            }
        } else {
            add(area);
            add(area);
        }

        let frames = calculate_attributes_wall(&wall_positions, vf, ellipsoid);
        let mut wall_indices = vec![0u32; (perimeter_positions * 6).max(0) as usize];
        let wall_length = wall_positions.len() / 3;
        let mut index = 0;
        let mut i = 0;
        while i + 1 < wall_length {
            let upper_left = i;
            let upper_right = (upper_left + 2) % wall_length;
            let p1 = Cartesian3::from_array(&wall_positions, upper_left * 3);
            let p2 = Cartesian3::from_array(&wall_positions, upper_right * 3);
            i += 2;
            if p1.equals_epsilon(p2, EPSILON10) {
                continue;
            }
            let lower_left = (upper_left + 1) % wall_length;
            let lower_right = (lower_left + 2) % wall_length;
            for v in [upper_left, lower_left, upper_right, upper_right, lower_left, lower_right] {
                put(&mut wall_indices, index, v as u32);
                index += 1;
            }
        }
        let mut wall_attributes =
            GeometryAttributes { position: Some(GeometryAttribute::doubles(3, wall_positions)), ..Default::default() };
        wall_attributes.normal = frames.normals.map(floats);
        wall_attributes.tangent = frames.tangents.map(floats);
        wall_attributes.bitangent = frames.bitangents.map(floats);
        wall_attributes.st = wall_st.map(st_attribute);
        if let Some(e) = wall_extrude {
            wall_attributes.set_other("extrudeDirection", floats(e));
        }
        if let Some(w) = wall_offsets {
            wall_attributes.set_other("applyOffset", uint8(w));
        }
        let wall = Geometry::new(
            wall_attributes,
            Some(IndexDatatype::create_typed_array(wall_count, &wall_indices)),
            PrimitiveType::Triangles,
        );
        combine_instances(&[GeometryInstance::new(top_bottom, None), GeometryInstance::new(wall, None)])?.into_iter().next()
    }

    /// The rectangle's geometry; `None` for a rectangle with no area (or a
    /// vertex format without positions, where Cesium throws).
    pub fn create_geometry(&self) -> Option<Geometry> {
        let r = self.rectangle;
        if equals_epsilon(r.north, r.south, EPSILON10, EPSILON10) || equals_epsilon(r.east, r.west, EPSILON10, EPSILON10) {
            return None;
        }
        let ellipsoid = &self.ellipsoid;
        let mut o = compute_options(&r, self.granularity, self.rotation, self.st_rotation);
        o.tangent_rotation_matrix = if self.st_rotation != 0.0 || self.rotation != 0.0 {
            let axis = ellipsoid.geodetic_surface_normal_cartographic(&r.center());
            Matrix3::from_quaternion(&Quaternion::from_axis_angle(axis, -self.st_rotation))
        } else {
            Matrix3::IDENTITY
        };
        let extrude = !equals_epsilon(self.surface_height, self.extruded_height, 0.0, EPSILON2);
        o.lon_scalar = 1.0 / r.width();
        o.lat_scalar = 1.0 / r.height();

        let (geometry, bounding_sphere) = if extrude {
            let geometry = self.construct_extruded(&o)?;
            let top = BoundingSphere::from_rectangle_3d(&r, ellipsoid, self.surface_height);
            let bottom = BoundingSphere::from_rectangle_3d(&r, ellipsoid, self.extruded_height);
            (geometry, top.union(&bottom))
        } else {
            let mut surface = construct_rectangle(self.vertex_format, ellipsoid, &o)?;
            scale_to_geodetic_height(&mut surface.positions, Some(self.surface_height), ellipsoid, Some(false))?;
            let count = surface.positions.len() / 3;
            let mut attributes =
                GeometryAttributes { position: Some(GeometryAttribute::doubles(3, surface.positions)), ..Default::default() };
            attributes.normal = surface.frames.normals.map(floats);
            attributes.tangent = surface.frames.tangents.map(floats);
            attributes.bitangent = surface.frames.bitangents.map(floats);
            attributes.st = surface.st.map(st_attribute);
            if let Some(offset) = self.offset_attribute {
                attributes.set_other("applyOffset", GeometryAttribute::apply_offset(count, offset));
            }
            let indices = IndexDatatype::create_typed_array(surface.size, &surface.indices);
            (
                Geometry::new(attributes, Some(indices), PrimitiveType::Triangles),
                BoundingSphere::from_rectangle_3d(&r, ellipsoid, self.surface_height),
            )
        };
        let mut result = Geometry::new(geometry.attributes, geometry.indices, geometry.primitive_type);
        result.bounding_sphere = Some(bounding_sphere);
        result.offset_attribute = self.offset_attribute;
        Some(result)
    }
}
