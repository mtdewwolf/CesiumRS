//! CesiumJS `CorridorOutlineGeometry`: the outline of a corridor, on the
//! surface or extruded.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::corner_type::CornerType;
use super::corridor_geometry::{at, extruded_offset, prepare, put3, put_index, set_at, splice3};
use super::corridor_geometry_library::{compute_positions, CorridorPositions};
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::math::{equals_epsilon, js_max, js_min, EPSILON2, RADIANS_PER_DEGREE};
use super::polygon_pipeline::scale_to_geodetic_height;
use super::polyline_volume_geometry::corner_type_from;
use super::primitive_type::PrimitiveType;

#[derive(Clone, Debug, PartialEq)]
pub struct CorridorOutlineGeometry {
    pub positions: Vec<Cartesian3>,
    pub ellipsoid: Ellipsoid,
    pub width: f64,
    /// The larger of the two heights.
    pub height: f64,
    pub extruded_height: f64,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub packed_length: usize,
}

struct Combined {
    positions: Vec<f64>,
    indices: Vec<u32>,
    wall_indices: Vec<f64>,
}

fn pair(indices: &mut [u32], index: &mut usize, a: f64, b: f64) {
    put_index(indices, index, a);
    put_index(indices, index, b);
}

fn combine(computed: CorridorPositions, corner_type: CornerType) -> Option<Combined> {
    let CorridorPositions { mut positions, corners, end_positions, .. } = computed;
    let mut wall_indices = Vec::new();
    let (mut left_count, mut right_count, mut indices_length) = (0.0, 0.0, 0.0);
    for p in positions.chunks(2) {
        let length = p[0].len() as f64 - 3.0;
        left_count += length;
        indices_length += (length / 3.0) * 4.0;
        right_count += p.get(1)?.len() as f64 - 3.0;
    }
    left_count += 3.0;
    right_count += 3.0;
    for corner in &corners {
        if let Some(l) = &corner.left_positions {
            left_count += l.len() as f64;
            indices_length += (l.len() as f64 / 3.0) * 2.0;
        } else {
            let r = corner.right_positions.as_ref()?;
            right_count += r.len() as f64;
            indices_length += (r.len() as f64 / 3.0) * 2.0;
        }
    }
    let mut end_position_length = f64::NAN;
    if let Some(e) = &end_positions {
        end_position_length = e[0].len() as f64 - 3.0;
        left_count += end_position_length;
        right_count += end_position_length;
        end_position_length /= 3.0;
        indices_length += end_position_length * 4.0;
    }
    let size = (left_count + right_count) as usize;
    let mut final_positions = vec![0.0; size];
    let mut front: i64 = 0;
    let mut back: i64 = size as i64 - 1;
    let (mut ul, mut ll);
    let (mut ur, mut lr) = (f64::NAN, f64::NAN);
    let half_length = end_position_length / 2.0;
    let mut indices = vec![0u32; (indices_length + 4.0) as usize];
    let mut index = 0;
    let third = |i: i64| i as f64 / 3.0;
    pair(&mut indices, &mut index, third(front), third(back - 2));
    if let Some(e) = &end_positions {
        wall_indices.push(third(front));
        let first = &e[0];
        let mut i = 0.0;
        while i < half_length {
            put3(&mut final_positions, at(first, (half_length + i) * 3.0), Some(front), None);
            put3(&mut final_positions, at(first, (half_length - 1.0 - i) * 3.0), None, Some(back));
            ll = third(front);
            lr = ll + 1.0;
            ul = third(back - 2);
            ur = ul - 1.0;
            pair(&mut indices, &mut index, ul, ur);
            pair(&mut indices, &mut index, ll, lr);
            front += 3;
            back -= 3;
            i += 1.0;
        }
    }
    let right_edge = positions.first()?.clone();
    let left_edge = positions.get(1)?.clone();
    let mut pos_index = 2;
    set_at(&mut final_positions, &right_edge, front)?;
    set_at(&mut final_positions, &left_edge, back - left_edge.len() as i64 + 1)?;
    let length = left_edge.len() as i64 - 3;
    wall_indices.extend([third(front), third(back - 2)]);
    let mut i = 0;
    while i < length {
        ll = third(front);
        lr = ll + 1.0;
        ul = third(back - 2);
        ur = ul - 1.0;
        pair(&mut indices, &mut index, ul, ur);
        pair(&mut indices, &mut index, ll, lr);
        front += 3;
        back -= 3;
        i += 3;
    }
    for corner in &corners {
        if let Some(l) = &corner.left_positions {
            back -= 3;
            let start = ur;
            wall_indices.push(lr);
            for j in 0..l.len() / 3 {
                let jf = j as f64;
                pair(&mut indices, &mut index, start - jf - 1.0, start - jf);
                put3(&mut final_positions, at(l, jf * 3.0), None, Some(back));
                back -= 3;
            }
            wall_indices.push(start - (l.len() as f64 / 6.0).floor());
            if corner_type == CornerType::Beveled {
                wall_indices.push(third(back - 2) + 1.0);
            }
            front += 3;
        } else {
            let r = corner.right_positions.as_ref()?;
            front += 3;
            let start = lr;
            wall_indices.push(ur);
            for j in 0..r.len() / 3 {
                let jf = j as f64;
                pair(&mut indices, &mut index, start + jf, start + jf + 1.0);
                put3(&mut final_positions, at(r, jf * 3.0), Some(front), None);
                front += 3;
            }
            wall_indices.push(start + (r.len() as f64 / 6.0).floor());
            if corner_type == CornerType::Beveled {
                wall_indices.push(third(front) - 1.0);
            }
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
        let mut j = 0;
        while j < left_edge.len() {
            lr = third(front);
            ll = lr - 1.0;
            ur = third(back - 2);
            ul = ur + 1.0;
            pair(&mut indices, &mut index, ul, ur);
            pair(&mut indices, &mut index, ll, lr);
            front += 3;
            back -= 3;
            j += 3;
        }
        front -= 3;
        back += 3;
        wall_indices.extend([third(front), third(back - 2)]);
    }
    if let Some(e) = &end_positions {
        front += 3;
        back -= 3;
        let last = &e[1];
        let mut i = 0.0;
        while i < half_length {
            put3(&mut final_positions, at(last, (end_position_length - i - 1.0) * 3.0), None, Some(back));
            put3(&mut final_positions, at(last, i * 3.0), Some(front), None);
            lr = third(front);
            ll = lr - 1.0;
            ur = third(back - 2);
            ul = ur + 1.0;
            pair(&mut indices, &mut index, ul, ur);
            pair(&mut indices, &mut index, ll, lr);
            front += 3;
            back -= 3;
            i += 1.0;
        }
        wall_indices.push(third(front));
    } else {
        wall_indices.extend([third(front), third(back - 2)]);
    }
    pair(&mut indices, &mut index, third(front), third(back - 2));
    Some(Combined { positions: final_positions, indices, wall_indices })
}

impl CorridorOutlineGeometry {
    /// The height defaults to 0, the extruded height to the height, the
    /// ellipsoid to WGS84, the corner type to rounded and the granularity
    /// to one degree, as in Cesium.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        positions: Vec<Cartesian3>,
        width: f64,
        height: Option<f64>,
        extruded_height: Option<f64>,
        ellipsoid: Option<Ellipsoid>,
        corner_type: Option<CornerType>,
        granularity: Option<f64>,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Self {
        let height = height.unwrap_or(0.0);
        let extruded_height = extruded_height.unwrap_or(height);
        Self {
            packed_length: 1 + positions.len() * 3 + Ellipsoid::PACKED_LENGTH + 6,
            positions,
            ellipsoid: ellipsoid.unwrap_or(Ellipsoid::WGS84),
            width,
            height: js_max(height, extruded_height),
            extruded_height: js_min(height, extruded_height),
            corner_type: corner_type.unwrap_or(CornerType::Rounded),
            granularity: granularity.unwrap_or(RADIANS_PER_DEGREE),
            offset_attribute,
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
        array[i..i + 6].copy_from_slice(&[
            self.width,
            self.height,
            self.extruded_height,
            self.corner_type as u32 as f64,
            self.granularity,
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
        Self::new(
            positions,
            array[i],
            Some(array[i + 1]),
            Some(array[i + 2]),
            Some(ellipsoid),
            Some(corner_type_from(array[i + 3])),
            Some(array[i + 4]),
            GeometryOffsetAttribute::unpack(array[i + 5]),
        )
    }

    /// The outline; `None` for fewer than two distinct positions or a
    /// non-positive width, and where Cesium throws.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let ellipsoid = &self.ellipsoid;
        let clean = prepare(&self.positions, ellipsoid, self.width)?;
        let extrude = !equals_epsilon(self.height, self.extruded_height, 0.0, EPSILON2);
        let computed = compute_positions(&clean, ellipsoid, self.width, self.corner_type, self.granularity, false)?;
        let c = combine(computed, self.corner_type)?;
        let mut positions = c.positions;
        let mut attributes = GeometryAttributes::default();
        let indices = if extrude {
            let length = positions.len();
            let mut extruded = positions.clone();
            scale_to_geodetic_height(&mut positions, Some(self.height), ellipsoid, None)?;
            scale_to_geodetic_height(&mut extruded, Some(self.extruded_height), ellipsoid, None)?;
            positions.extend(extruded);
            let size = length / 3;
            if let Some(offset) = self.offset_attribute {
                attributes.set_other("applyOffset", extruded_offset(offset, size, size * 2, &[(0, 1)]));
            }
            let i_length = c.indices.len();
            let mut new_indices = vec![0u32; (i_length + c.wall_indices.len()) * 2];
            new_indices[..i_length].copy_from_slice(&c.indices);
            let mut index = i_length;
            for p in c.indices.chunks(2) {
                pair(&mut new_indices, &mut index, p[0] as f64 + size as f64, p.get(1).map_or(f64::NAN, |&v| v as f64) + size as f64);
            }
            for &ul in &c.wall_indices {
                pair(&mut new_indices, &mut index, ul, ul + size as f64);
            }
            new_indices
        } else {
            scale_to_geodetic_height(&mut positions, Some(self.height), ellipsoid, None)?;
            if let Some(offset) = self.offset_attribute {
                attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
            }
            c.indices
        };
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let vertex_count = positions.len() / 3;
        attributes.position = Some(GeometryAttribute::doubles(3, positions));
        let mut geometry = Geometry::new(attributes, Some(IndexDatatype::create_typed_array(vertex_count, &indices)), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere);
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
