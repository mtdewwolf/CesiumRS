//! CesiumJS `CorridorGeometry`: a corridor of fixed width along a polyline,
//! on the surface or extruded.

use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::component_datatype::ComponentDatatype;
use super::corner_type::CornerType;
use super::corridor_geometry_library::{add_attribute, compute_positions, CorridorPositions};
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::js_math::JsMath;
use super::math::{equals_epsilon, js_max, js_min, EPSILON2, PI_OVER_TWO, RADIANS_PER_DEGREE};
use super::polygon_pipeline::scale_to_geodetic_height;
use super::polyline_volume_geometry::corner_type_from;
use super::primitive_type::PrimitiveType;
use super::rectangle::Rectangle;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct CorridorGeometryOptions {
    pub positions: Vec<Cartesian3>,
    pub width: f64,
    pub height: f64,
    /// Defaults to `height`.
    pub extruded_height: Option<f64>,
    pub ellipsoid: Ellipsoid,
    pub vertex_format: VertexFormat,
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
            height: 0.0,
            extruded_height: None,
            ellipsoid: Ellipsoid::WGS84,
            vertex_format: VertexFormat::DEFAULT,
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
    pub ellipsoid: Ellipsoid,
    pub vertex_format: VertexFormat,
    pub width: f64,
    /// The larger of the two heights.
    pub height: f64,
    pub extruded_height: f64,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub packed_length: usize,
}

/// The positions on the surface without duplicates; `None` for fewer than
/// two or a non-positive width, and where Cesium throws (a position at the
/// center).
pub(crate) fn prepare(positions: &[Cartesian3], ellipsoid: &Ellipsoid, width: f64) -> Option<Vec<Cartesian3>> {
    let scaled = positions.iter().map(|p| ellipsoid.scale_to_geodetic_surface(*p)).collect::<Option<Vec<_>>>()?;
    let clean = array_remove_duplicates(&scaled, |a, b, e| a.equals_epsilon(*b, e), false, None);
    (clean.len() >= 2 && width > 0.0).then_some(clean)
}

/// The point at a JavaScript array index: NaN components where the index
/// is fractional or out of range, as `Cartesian3.fromArray` reads
/// `undefined`.
pub(crate) fn at(array: &[f64], index: f64) -> Cartesian3 {
    let get = |k: f64| {
        let i = index + k;
        if i >= 0.0 && i.fract() == 0.0 {
            array.get(i as usize).copied().unwrap_or(f64::NAN)
        } else {
            f64::NAN
        }
    };
    Cartesian3::new(get(0.0), get(1.0), get(2.0))
}

/// ECMAScript ToUint32, as a typed array stores an index.
pub(crate) fn to_index(v: f64) -> u32 {
    if v.is_finite() {
        v.trunc().rem_euclid(4_294_967_296.0) as u32
    } else {
        0
    }
}

pub(crate) fn put_index(indices: &mut [u32], index: &mut usize, v: f64) {
    if let Some(slot) = indices.get_mut(*index) {
        *slot = to_index(v);
    }
    *index += 1;
}

/// `TypedArray.prototype.set`; `None` where it throws (out of range).
pub(crate) fn set_at(target: &mut [f64], source: &[f64], offset: i64) -> Option<()> {
    let offset = usize::try_from(offset).ok()?;
    target.get_mut(offset..offset + source.len())?.copy_from_slice(source);
    Some(())
}

/// `Array.prototype.splice(start, 3)` with JavaScript's negative starts.
pub(crate) fn splice3(array: &mut Vec<f64>, start: i64) {
    let len = array.len() as i64;
    let start = if start < 0 { js_max((len + start) as f64, 0.0) as i64 } else { start.min(len) };
    let end = (start + 3).min(len);
    array.drain(start as usize..end as usize);
}

pub(crate) fn put3(array: &mut [f64], v: Cartesian3, front: Option<i64>, back: Option<i64>) {
    add_attribute(array, [v.x, v.y, v.z], front, back, |x| x);
}

fn put3f(array: &mut Option<Vec<f32>>, v: Cartesian3, front: Option<i64>, back: Option<i64>) {
    if let Some(a) = array.as_mut() {
        add_attribute(a, [v.x, v.y, v.z], front, back, |x| x as f32);
    }
}

#[derive(Default)]
struct Frames {
    normals: Option<Vec<f32>>,
    tangents: Option<Vec<f32>>,
    bitangents: Option<Vec<f32>>,
}

fn add_normals(attr: &mut Frames, normal: Cartesian3, left: Cartesian3, front: Option<i64>, back: Option<i64>, vf: VertexFormat) {
    let forward = left.cross(normal).normalize();
    if vf.normal {
        put3f(&mut attr.normals, normal, front, back);
    }
    if vf.tangent {
        put3f(&mut attr.tangents, forward, front, back);
    }
    if vf.bitangent {
        put3f(&mut attr.bitangents, left, front, back);
    }
}

struct Combined {
    positions: Vec<f64>,
    frames: Frames,
    st: Option<Vec<f32>>,
    indices: Vec<u32>,
}

fn quad(indices: &mut [u32], index: &mut usize, ul: f64, ll: f64, ur: f64, lr: f64) {
    for v in [ul, ll, ur, ur, ll, lr] {
        put_index(indices, index, v);
    }
}

fn combine(computed: CorridorPositions, vf: VertexFormat, ellipsoid: &Ellipsoid) -> Option<Combined> {
    let CorridorPositions { mut positions, corners, lefts, normals: computed_normals, end_positions } = computed;
    let computed_lefts = lefts?;
    let computed_normals = computed_normals?;
    let gsn = |p: Cartesian3| ellipsoid.geodetic_surface_normal(p);
    let (mut left_count, mut right_count, mut indices_length) = (0.0, 0.0, 0.0);
    for pair in positions.chunks(2) {
        let length = pair[0].len() as f64 - 3.0;
        left_count += length;
        indices_length += length * 2.0;
        right_count += pair.get(1)?.len() as f64 - 3.0;
    }
    left_count += 3.0;
    right_count += 3.0;
    for corner in &corners {
        if let Some(l) = &corner.left_positions {
            left_count += l.len() as f64;
            indices_length += l.len() as f64;
        } else {
            let r = corner.right_positions.as_ref()?;
            right_count += r.len() as f64;
            indices_length += r.len() as f64;
        }
    }
    let mut end_position_length = f64::NAN;
    if let Some(e) = &end_positions {
        end_position_length = e[0].len() as f64 - 3.0;
        left_count += end_position_length;
        right_count += end_position_length;
        end_position_length /= 3.0;
        indices_length += end_position_length * 6.0;
    }
    let size = left_count + right_count;
    let size_u = size as usize;
    let mut final_positions = vec![0.0; size_u];
    let mut attr = Frames {
        normals: vf.normal.then(|| vec![0.0; size_u]),
        tangents: vf.tangent.then(|| vec![0.0; size_u]),
        bitangents: vf.bitangent.then(|| vec![0.0; size_u]),
    };
    let mut front: i64 = 0;
    let mut back: i64 = size_u as i64 - 1;
    // Cesium reads these before setting them only when an edge is empty.
    let (mut ul, mut ll);
    let (mut ur, mut lr) = (f64::NAN, f64::NAN);
    let half_length = end_position_length / 2.0;
    let mut indices = vec![0u32; indices_length as usize];
    let mut index = 0;
    let mut normal;
    let mut left;
    let third = |i: i64| i as f64 / 3.0;

    if let Some(e) = &end_positions {
        let first = &e[0];
        normal = at(&computed_normals, 0.0);
        left = at(&computed_lefts, 0.0);
        let mut i = 0.0;
        while i < half_length {
            let left_pos = at(first, (half_length - 1.0 - i) * 3.0);
            let right_pos = at(first, (half_length + i) * 3.0);
            put3(&mut final_positions, right_pos, Some(front), None);
            put3(&mut final_positions, left_pos, None, Some(back));
            add_normals(&mut attr, normal, left, Some(front), Some(back), vf);
            ll = third(front);
            lr = ll + 1.0;
            ul = third(back - 2);
            ur = ul - 1.0;
            quad(&mut indices, &mut index, ul, ll, ur, lr);
            front += 3;
            back -= 3;
            i += 1.0;
        }
    }
    let mut pos_index = 0;
    let mut comp_index = 0.0;
    let right_edge = positions.get(pos_index)?.clone();
    let left_edge = positions.get(pos_index + 1)?.clone();
    pos_index += 2;
    set_at(&mut final_positions, &right_edge, front)?;
    set_at(&mut final_positions, &left_edge, back - left_edge.len() as i64 + 1)?;
    left = at(&computed_lefts, comp_index);
    let length = left_edge.len() as i64 - 3;
    let mut i = 0;
    while i < length {
        let right_normal = gsn(at(&right_edge, i as f64))?;
        let left_normal = gsn(at(&left_edge, (length - i) as f64))?;
        normal = right_normal.add(left_normal).normalize();
        add_normals(&mut attr, normal, left, Some(front), Some(back), vf);
        ll = third(front);
        lr = ll + 1.0;
        ul = third(back - 2);
        ur = ul - 1.0;
        quad(&mut indices, &mut index, ul, ll, ur, lr);
        front += 3;
        back -= 3;
        i += 3;
    }
    comp_index += 3.0;

    for corner in &corners {
        normal = at(&computed_normals, comp_index);
        if let Some(l) = &corner.left_positions {
            add_normals(&mut attr, normal, left, None, Some(back), vf);
            back -= 3;
            let (pivot, start) = (lr, ur);
            let n = l.len() / 3;
            for j in 0..n {
                let jf = j as f64;
                let outside_point = at(l, jf * 3.0);
                for v in [pivot, start - jf - 1.0, start - jf] {
                    put_index(&mut indices, &mut index, v);
                }
                put3(&mut final_positions, outside_point, None, Some(back));
                let previous_point = at(&final_positions, (start - jf - 1.0) * 3.0);
                let next_point = at(&final_positions, pivot * 3.0);
                left = previous_point.subtract(next_point).normalize();
                add_normals(&mut attr, normal, left, None, Some(back), vf);
                back -= 3;
            }
            let outside_point = at(&final_positions, pivot * 3.0);
            let previous_point = at(&final_positions, start * 3.0).subtract(outside_point);
            let next_point = at(&final_positions, (start - n as f64) * 3.0).subtract(outside_point);
            left = previous_point.add(next_point).normalize();
            add_normals(&mut attr, normal, left, Some(front), None, vf);
            front += 3;
        } else {
            let r = corner.right_positions.as_ref()?;
            add_normals(&mut attr, normal, left, Some(front), None, vf);
            front += 3;
            let (pivot, start) = (ur, lr);
            let n = r.len() / 3;
            for j in 0..n {
                let jf = j as f64;
                let outside_point = at(r, jf * 3.0);
                for v in [pivot, start + jf, start + jf + 1.0] {
                    put_index(&mut indices, &mut index, v);
                }
                put3(&mut final_positions, outside_point, Some(front), None);
                let previous_point = at(&final_positions, pivot * 3.0);
                let next_point = at(&final_positions, (start + jf) * 3.0);
                left = previous_point.subtract(next_point).normalize();
                add_normals(&mut attr, normal, left, Some(front), None, vf);
                front += 3;
            }
            let outside_point = at(&final_positions, pivot * 3.0);
            let previous_point = at(&final_positions, (start + n as f64) * 3.0).subtract(outside_point);
            let next_point = at(&final_positions, start * 3.0).subtract(outside_point);
            left = next_point.add(previous_point).negate().normalize();
            add_normals(&mut attr, normal, left, None, Some(back), vf);
            back -= 3;
        }
        let mut right_edge = std::mem::take(positions.get_mut(pos_index)?);
        let mut left_edge = std::mem::take(positions.get_mut(pos_index + 1)?);
        pos_index += 2;
        splice3(&mut right_edge, 0);
        let start = left_edge.len() as i64 - 3;
        splice3(&mut left_edge, start);
        set_at(&mut final_positions, &right_edge, front)?;
        set_at(&mut final_positions, &left_edge, back - left_edge.len() as i64 + 1)?;
        let length = left_edge.len() as i64 - 3;
        comp_index += 3.0;
        left = at(&computed_lefts, comp_index);
        let mut j = 0;
        while j < left_edge.len() as i64 {
            let right_normal = gsn(at(&right_edge, j as f64))?;
            let left_normal = gsn(at(&left_edge, (length - j) as f64))?;
            normal = right_normal.add(left_normal).normalize();
            add_normals(&mut attr, normal, left, Some(front), Some(back), vf);
            lr = third(front);
            ll = lr - 1.0;
            ur = third(back - 2);
            ul = ur + 1.0;
            quad(&mut indices, &mut index, ul, ll, ur, lr);
            front += 3;
            back -= 3;
            j += 3;
        }
        front -= 3;
        back += 3;
    }
    normal = at(&computed_normals, computed_normals.len() as f64 - 3.0);
    add_normals(&mut attr, normal, left, Some(front), Some(back), vf);
    if let Some(e) = &end_positions {
        front += 3;
        back -= 3;
        let last = &e[1];
        let mut i = 0.0;
        while i < half_length {
            let left_pos = at(last, (end_position_length - i - 1.0) * 3.0);
            let right_pos = at(last, i * 3.0);
            put3(&mut final_positions, left_pos, None, Some(back));
            put3(&mut final_positions, right_pos, Some(front), None);
            add_normals(&mut attr, normal, left, Some(front), Some(back), vf);
            lr = third(front);
            ll = lr - 1.0;
            ur = third(back - 2);
            ul = ur + 1.0;
            quad(&mut indices, &mut index, ul, ll, ur, lr);
            front += 3;
            back -= 3;
            i += 1.0;
        }
    }

    let st = vf.st.then(|| {
        let mut st = vec![0f32; (size / 3.0 * 2.0) as usize];
        let mut k = 0;
        let mut push = |v: f64| {
            if let Some(slot) = st.get_mut(k) {
                *slot = v as f32;
            }
            k += 1;
        };
        let left_count = left_count / 3.0;
        let right_count = right_count / 3.0;
        if end_positions.is_some() {
            let epl = end_position_length;
            let theta = std::f64::consts::PI / (epl + 1.0);
            let left_st = 1.0 / (left_count - epl + 1.0);
            let right_st = 1.0 / (right_count - epl + 1.0);
            let half_end_pos = epl / 2.0;
            let mut i = half_end_pos + 1.0;
            while i < epl + 1.0 {
                let a = PI_OVER_TWO + theta * i;
                push(right_st * (1.0 + a.js_cos()));
                push(0.5 * (1.0 + a.js_sin()));
                i += 1.0;
            }
            let mut i = 1.0;
            while i < right_count - epl + 1.0 {
                push(i * right_st);
                push(0.0);
                i += 1.0;
            }
            let mut i = epl;
            while i > half_end_pos {
                let a = PI_OVER_TWO - i * theta;
                push(1.0 - right_st * (1.0 + a.js_cos()));
                push(0.5 * (1.0 + a.js_sin()));
                i -= 1.0;
            }
            let mut i = half_end_pos;
            while i > 0.0 {
                let a = PI_OVER_TWO - theta * i;
                push(1.0 - left_st * (1.0 + a.js_cos()));
                push(0.5 * (1.0 + a.js_sin()));
                i -= 1.0;
            }
            let mut i = left_count - epl;
            while i > 0.0 {
                push(i * left_st);
                push(1.0);
                i -= 1.0;
            }
            let mut i = 1.0;
            while i < half_end_pos + 1.0 {
                let a = PI_OVER_TWO + theta * i;
                push(left_st * (1.0 + a.js_cos()));
                push(0.5 * (1.0 + a.js_sin()));
                i += 1.0;
            }
        } else {
            let left_st = 1.0 / (left_count - 1.0);
            let right_st = 1.0 / (right_count - 1.0);
            let mut i = 0.0;
            while i < right_count {
                push(i * right_st);
                push(0.0);
                i += 1.0;
            }
            let mut i = left_count;
            while i > 0.0 {
                push((i - 1.0) * left_st);
                push(1.0);
                i -= 1.0;
            }
        }
        st
    });
    Some(Combined { positions: final_positions, frames: attr, st, indices })
}

/// Fills `wall` from `index` with each position but the first twice, and the
/// first at both ends.
pub(crate) fn add_wall_positions<T: Copy>(positions: &[T], mut index: usize, wall: &mut [T]) {
    let mut put = |v: T| {
        if let Some(slot) = wall.get_mut(index) {
            *slot = v;
        }
        index += 1;
    };
    let first = &positions[..3.min(positions.len())];
    first.iter().for_each(|&v| put(v));
    for p in positions[3.min(positions.len())..].chunks(3) {
        p.iter().for_each(|&v| put(v));
        p.iter().for_each(|&v| put(v));
    }
    first.iter().for_each(|&v| put(v));
}

fn float_attribute(components: u8, values: Vec<f32>) -> GeometryAttribute {
    GeometryAttribute::new(ComponentDatatype::Float, components, TypedArray::Float32(values))
}

/// Top, bottom and wall normals, tangents, bitangents and texture
/// coordinates; `None` where Cesium throws (tangents without normals or
/// bitangents).
fn extruded_attributes(c: &mut Combined, positions: &[f64], vf: VertexFormat) -> Option<()> {
    if !vf.normal && !vf.tangent && !vf.bitangent && !vf.st {
        return Some(());
    }
    let three_size = positions.len() / 6;
    let two_size = three_size / 3 * 2;
    let six_size = three_size * 2;
    if vf.normal || vf.bitangent || vf.tangent {
        let top_normals = if vf.normal || vf.bitangent { c.frames.normals.clone() } else { None };
        let top_bitangents = c.frames.bitangents.clone();
        let mut normals = vf.normal.then(|| vec![0f32; three_size * 6]);
        let mut tangents = vf.tangent.then(|| vec![0f32; three_size * 6]);
        let mut bitangents = vf.bitangent.then(|| vec![0f32; three_size * 6]);
        let mut attr_index = six_size as i64;
        for i in (0..three_size).step_by(3) {
            let offset = attr_index + six_size as i64;
            let top = Cartesian3::from_array(positions, i);
            let bottom = Cartesian3::from_array(positions, i + three_size).subtract(top);
            let previous = Cartesian3::from_array(positions, (i + 3) % three_size).subtract(top);
            let normal = bottom.cross(previous).normalize();
            for at_index in [offset, offset + 3, attr_index, attr_index + 3] {
                put3f(&mut normals, normal, Some(at_index), None);
            }
            if vf.tangent || vf.bitangent {
                let tn = top_normals.as_ref()?;
                let bitangent = Cartesian3::new(tn[i] as f64, tn[i + 1] as f64, tn[i + 2] as f64);
                for at_index in [offset, offset + 3, attr_index, attr_index + 3] {
                    put3f(&mut bitangents, bitangent, Some(at_index), None);
                }
                if vf.tangent {
                    let tangent = bitangent.cross(normal).normalize();
                    for at_index in [offset, offset + 3, attr_index, attr_index + 3] {
                        put3f(&mut tangents, tangent, Some(at_index), None);
                    }
                }
            }
            attr_index += 6;
        }
        if let Some(n) = normals.as_mut() {
            let tn = top_normals.as_ref()?;
            n[..tn.len()].copy_from_slice(tn);
            for i in (0..three_size).step_by(3) {
                for k in 0..3 {
                    n[i + three_size + k] = -tn[i + k];
                }
            }
            c.frames.normals = normals;
        } else {
            c.frames.normals = None;
        }
        if let Some(b) = bitangents.as_mut() {
            let tb = top_bitangents.as_ref()?;
            b[..tb.len()].copy_from_slice(tb);
            b.get_mut(three_size..three_size + tb.len())?.copy_from_slice(tb);
            c.frames.bitangents = bitangents;
        } else {
            c.frames.bitangents = None;
        }
        if let Some(t) = tangents.as_mut() {
            let tt = c.frames.tangents.as_ref()?;
            t[..tt.len()].copy_from_slice(tt);
            t.get_mut(three_size..three_size + tt.len())?.copy_from_slice(tt);
            c.frames.tangents = tangents;
        }
    }
    if vf.st {
        let top_st = c.st.as_ref()?;
        let mut st = vec![0f32; two_size * 6];
        st[..top_st.len()].copy_from_slice(top_st);
        st.get_mut(two_size..two_size + top_st.len())?.copy_from_slice(top_st);
        let mut index = two_size * 2;
        let mut push = |v: f32| {
            if let Some(slot) = st.get_mut(index) {
                *slot = v;
            }
            index += 1;
        };
        let get = |i: usize| top_st.get(i).copied().unwrap_or(f32::NAN);
        for _ in 0..2 {
            push(get(0));
            push(get(1));
            for i in (2..two_size).step_by(2) {
                let (s, t) = (get(i), get(i + 1));
                push(s);
                push(t);
                push(s);
                push(t);
            }
            push(get(0));
            push(get(1));
        }
        c.st = Some(st);
    }
    Some(())
}

/// The `applyOffset` values for an extruded volume of `size` top vertices.
pub(crate) fn extruded_offset(offset: GeometryOffsetAttribute, size: usize, total: usize, top: &[(usize, usize)]) -> GeometryAttribute {
    let mut values = vec![0u8; total];
    match offset {
        GeometryOffsetAttribute::Top => top.iter().for_each(|&(a, b)| values[a * size..b * size].fill(1)),
        GeometryOffsetAttribute::None => {}
        GeometryOffsetAttribute::All => values.fill(1),
    }
    GeometryAttribute::new(ComponentDatatype::UnsignedByte, 1, TypedArray::Uint8(values))
}

fn assemble(c: Combined, positions: Vec<f64>) -> GeometryAttributes {
    let mut attributes = GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
    attributes.st = c.st.map(|v| float_attribute(2, v));
    attributes.normal = c.frames.normals.map(|v| float_attribute(3, v));
    attributes.tangent = c.frames.tangents.map(|v| float_attribute(3, v));
    attributes.bitangent = c.frames.bitangents.map(|v| float_attribute(3, v));
    attributes
}

/// Updates the bounds with the cartographic of `p`; at the center Cesium
/// leaves its scratch cartographic unchanged.
fn extend_bounds(ellipsoid: &Ellipsoid, p: Cartesian3, carto: &mut Cartographic, min: &mut Cartographic, max: &mut Cartographic) {
    if let Some(c) = ellipsoid.cartesian_to_cartographic(p) {
        *carto = c;
    }
    min.latitude = js_min(min.latitude, carto.latitude);
    min.longitude = js_min(min.longitude, carto.longitude);
    max.latitude = js_max(max.latitude, carto.latitude);
    max.longitude = js_max(max.longitude, carto.longitude);
}

/// The corridor's bounds; all zeros for fewer than two distinct positions
/// or a non-positive width, `None` where Cesium throws.
pub fn compute_rectangle(positions: &[Cartesian3], ellipsoid: &Ellipsoid, width: f64, corner_type: CornerType) -> Option<Rectangle> {
    let scaled = positions.iter().map(|p| ellipsoid.scale_to_geodetic_surface(*p)).collect::<Option<Vec<_>>>()?;
    let clean = array_remove_duplicates(&scaled, |a, b, e| a.equals_epsilon(*b, e), false, None);
    let length = clean.len();
    if length < 2 || width <= 0.0 {
        return Some(Rectangle::default());
    }
    let half_width = width * 0.5;
    let mut carto = Cartographic::default();
    let mut min = Cartographic::new(f64::INFINITY, f64::INFINITY, 0.0);
    let mut max = Cartographic::new(f64::NEG_INFINITY, f64::NEG_INFINITY, 0.0);
    let offset_points = |p1: Cartesian3, p2: Cartesian3, carto: &mut Cartographic, min: &mut Cartographic, max: &mut Cartographic| -> Option<()> {
        let direction = p2.subtract(p1).normalize();
        let normal = ellipsoid.geodetic_surface_normal(p1)?;
        let offset = direction.cross(normal).multiply_by_scalar(half_width);
        extend_bounds(ellipsoid, p1.add(offset), carto, min, max);
        extend_bounds(ellipsoid, p1.subtract(offset), carto, min, max);
        Some(())
    };
    if corner_type == CornerType::Rounded {
        let first = clean[0];
        let offset = first.subtract(clean[1]).normalize().multiply_by_scalar(half_width);
        extend_bounds(ellipsoid, first.add(offset), &mut carto, &mut min, &mut max);
    }
    for i in 0..length - 1 {
        offset_points(clean[i], clean[i + 1], &mut carto, &mut min, &mut max)?;
    }
    let last = clean[length - 1];
    let offset = last.subtract(clean[length - 2]).normalize().multiply_by_scalar(half_width);
    let ends = last.add(offset);
    offset_points(last, ends, &mut carto, &mut min, &mut max)?;
    if corner_type == CornerType::Rounded {
        extend_bounds(ellipsoid, ends, &mut carto, &mut min, &mut max);
    }
    Some(Rectangle { north: max.latitude, south: min.latitude, east: max.longitude, west: min.longitude })
}

impl CorridorGeometry {
    pub fn new(options: CorridorGeometryOptions) -> Self {
        let extruded_height = options.extruded_height.unwrap_or(options.height);
        Self {
            packed_length: 1 + options.positions.len() * 3 + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH + 7,
            positions: options.positions,
            ellipsoid: options.ellipsoid,
            vertex_format: options.vertex_format,
            width: options.width,
            height: js_max(options.height, extruded_height),
            extruded_height: js_min(options.height, extruded_height),
            corner_type: options.corner_type,
            granularity: options.granularity,
            shadow_volume: options.shadow_volume,
            offset_attribute: options.offset_attribute,
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
        let n = array[i] as usize;
        i += 1;
        let positions = (0..n).map(|k| Cartesian3::unpack(array, i + k * 3)).collect();
        i += n * 3;
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        Self::new(CorridorGeometryOptions {
            positions,
            width: array[i],
            height: array[i + 1],
            extruded_height: Some(array[i + 2]),
            ellipsoid,
            vertex_format,
            corner_type: corner_type_from(array[i + 3]),
            granularity: array[i + 4],
            shadow_volume: array[i + 5] == 1.0,
            offset_attribute: GeometryOffsetAttribute::unpack(array[i + 6]),
        })
    }

    pub fn rectangle(&self) -> Option<Rectangle> {
        compute_rectangle(&self.positions, &self.ellipsoid, self.width, self.corner_type)
    }

    pub fn texture_coordinate_rotation_points(&self) -> [f64; 6] {
        [0.0, 0.0, 0.0, 1.0, 1.0, 0.0]
    }

    /// A position-only volume from `min_height` to `max_height`, for ground
    /// primitives.
    pub fn create_shadow_volume(&self, min_height: f64, max_height: f64) -> Self {
        Self::new(CorridorGeometryOptions {
            positions: self.positions.clone(),
            width: self.width,
            height: max_height,
            extruded_height: Some(min_height),
            ellipsoid: self.ellipsoid,
            vertex_format: VertexFormat::POSITION_ONLY,
            corner_type: self.corner_type,
            granularity: self.granularity,
            shadow_volume: true,
            offset_attribute: None,
        })
    }

    fn extruded(&self, clean: &[Cartesian3]) -> Option<(GeometryAttributes, Vec<u32>, Vec<f64>)> {
        let vf = self.vertex_format;
        let top_vf = VertexFormat {
            position: vf.position,
            normal: vf.normal || vf.bitangent || self.shadow_volume,
            tangent: vf.tangent,
            bitangent: vf.normal || vf.bitangent,
            st: vf.st,
            color: false,
        };
        let ellipsoid = &self.ellipsoid;
        let computed = compute_positions(clean, ellipsoid, self.width, self.corner_type, self.granularity, true)?;
        let mut c = combine(computed, top_vf, ellipsoid)?;
        let mut positions = std::mem::take(&mut c.positions);
        let length = positions.len();
        let mut extruded = positions.clone();
        let mut wall = vec![0.0; length * 4];
        scale_to_geodetic_height(&mut positions, Some(self.height), ellipsoid, None)?;
        add_wall_positions(&positions, 0, &mut wall);
        scale_to_geodetic_height(&mut extruded, Some(self.extruded_height), ellipsoid, None)?;
        add_wall_positions(&extruded, length * 2, &mut wall);
        let mut new_positions = positions;
        new_positions.extend(extruded);
        new_positions.extend(wall);
        extruded_attributes(&mut c, &new_positions, vf)?;
        let size = length / 3;
        let mut extrude_direction = None;
        if self.shadow_volume {
            let top_normals = c.frames.normals.as_mut()?;
            let n = top_normals.len();
            top_normals.iter_mut().for_each(|v| *v = -*v);
            let mut extrude_normals = vec![0f32; n * 6];
            extrude_normals[n..2 * n].copy_from_slice(top_normals);
            add_wall_positions(top_normals, n * 4, &mut extrude_normals);
            extrude_direction = Some(float_attribute(3, extrude_normals));
            if !vf.normal {
                c.frames.normals = None;
            }
        }
        let i_length = c.indices.len();
        let two_size = size * 2;
        let mut new_indices = vec![0u32; i_length * 2 + two_size * 3];
        new_indices[..i_length].copy_from_slice(&c.indices);
        let mut index = i_length;
        for t in c.indices.chunks(3) {
            for v in [t[2], t[1], t[0]] {
                put_index(&mut new_indices, &mut index, v as f64 + size as f64);
            }
        }
        for i in (0..two_size).step_by(2) {
            let ul = (i + two_size) as f64;
            let ll = ul + two_size as f64;
            quad(&mut new_indices, &mut index, ul, ll, ul + 1.0, ll + 1.0);
        }
        let mut attributes = assemble(c, Vec::new());
        if let Some(d) = extrude_direction {
            attributes.set_other("extrudeDirection", d);
        }
        if let Some(offset) = self.offset_attribute {
            attributes.set_other("applyOffset", extruded_offset(offset, size, size * 6, &[(0, 1), (2, 4)]));
        }
        Some((attributes, new_indices, new_positions))
    }

    /// The corridor; `None` for fewer than two distinct positions or a
    /// non-positive width, and where Cesium throws (an extruded corridor
    /// with tangents but neither normals nor bitangents).
    pub fn create_geometry(&self) -> Option<Geometry> {
        let ellipsoid = &self.ellipsoid;
        let clean = prepare(&self.positions, ellipsoid, self.width)?;
        let extrude = !equals_epsilon(self.height, self.extruded_height, 0.0, EPSILON2);
        let vf = self.vertex_format;
        let (mut attributes, indices, positions) = if extrude {
            self.extruded(&clean)?
        } else {
            let computed = compute_positions(&clean, ellipsoid, self.width, self.corner_type, self.granularity, true)?;
            let mut c = combine(computed, vf, ellipsoid)?;
            let mut positions = std::mem::take(&mut c.positions);
            scale_to_geodetic_height(&mut positions, Some(self.height), ellipsoid, None)?;
            let indices = std::mem::take(&mut c.indices);
            let mut attributes = assemble(c, Vec::new());
            if let Some(offset) = self.offset_attribute {
                attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
            }
            (attributes, indices, positions)
        };
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let vertex_count = positions.len() / 3;
        attributes.position = vf.position.then(|| GeometryAttribute::doubles(3, positions));
        let mut geometry = Geometry::new(attributes, Some(IndexDatatype::create_typed_array(vertex_count, &indices)), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(bounding_sphere);
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
