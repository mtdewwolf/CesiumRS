//! CesiumJS `CorridorOutlineGeometry`: geodesic corridor edges with rounded,
//! mitered or beveled corners, optional extrusion and vertex offsets.

use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::corner_type::CornerType;
use super::corridor_geometry_library::{compute_positions, CorridorCorner, CorridorPositionOptions, CorridorPositions};
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::math::{equals_epsilon, js_max, js_min, EPSILON2, RADIANS_PER_DEGREE};
use super::polygon_pipeline::scale_to_geodetic_height;
use super::polyline_volume_geometry::corner_type_from;
use super::primitive_type::PrimitiveType;

#[derive(Clone, Debug, PartialEq)]
pub struct CorridorOutlineGeometryOptions {
    pub positions: Vec<Cartesian3>,
    pub width: f64,
    pub ellipsoid: Ellipsoid,
    pub height: f64,
    pub extruded_height: Option<f64>,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl CorridorOutlineGeometryOptions {
    pub fn new(positions: Vec<Cartesian3>, width: f64) -> Self {
        Self {
            positions,
            width,
            ellipsoid: Ellipsoid::WGS84,
            height: 0.0,
            extruded_height: None,
            corner_type: CornerType::Rounded,
            granularity: RADIANS_PER_DEGREE,
            offset_attribute: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorridorOutlineGeometry {
    pub positions: Vec<Cartesian3>,
    pub width: f64,
    pub ellipsoid: Ellipsoid,
    pub height: f64,
    pub extruded_height: f64,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub packed_length: usize,
}

fn put(positions: &mut [f64], p: Cartesian3, index: usize) {
    positions[index..index + 3].copy_from_slice(&[p.x, p.y, p.z]);
}

/// Flat positions, line indices, and vertices to connect vertically on extrusion.
fn combine(computed: CorridorPositions, corner_type: CornerType) -> (Vec<f64>, Vec<u32>, Vec<u32>) {
    let CorridorPositions { positions, corners, end_positions, .. } = computed;
    let mut size = positions.iter().map(|p| p.len() - 3).sum::<usize>() + 6;
    for corner in &corners {
        let (CorridorCorner::Left(p) | CorridorCorner::Right(p)) = corner;
        size += p.len();
    }
    let end_length = end_positions.as_ref().map_or(0, |p| p[0].len() / 3 - 1);
    size += end_length * 6;
    let mut out = vec![0.0; size];
    // Use xyz start indices for both ends, avoiding reverse usize underflow.
    let mut front = 0;
    let mut back = size - 3;
    let mut indices = vec![0, (back / 3) as u32];
    let mut walls = Vec::new();
    let segment = |indices: &mut Vec<u32>, front: usize, back: usize| {
        indices.extend([(back / 3) as u32, (back / 3 - 1) as u32, (front / 3) as u32, (front / 3 + 1) as u32]);
    };
    if let Some(ends) = &end_positions {
        walls.push((front / 3) as u32);
        for i in 0..end_length / 2 {
            let left = Cartesian3::from_array(&ends[0], (end_length / 2 - 1 - i) * 3);
            let right = Cartesian3::from_array(&ends[0], (end_length / 2 + i) * 3);
            put(&mut out, right, front);
            put(&mut out, left, back);
            segment(&mut indices, front, back);
            front += 3;
            back -= 3;
        }
    }
    let right = &positions[0];
    let left = &positions[1];
    out[front..front + right.len()].copy_from_slice(right);
    out[back + 3 - left.len()..back + 3].copy_from_slice(left);
    walls.extend([(front / 3) as u32, (back / 3) as u32]);
    // LR/UR from the final segment are used as the corner's anchor vertices.
    let mut lr = front / 3;
    let mut ur = back / 3;
    for _ in (0..left.len() - 3).step_by(3) {
        lr = front / 3 + 1;
        ur = back / 3 - 1;
        segment(&mut indices, front, back);
        front += 3;
        back -= 3;
    }
    for (i, corner) in corners.iter().enumerate() {
        match corner {
            CorridorCorner::Left(p) => {
                back -= 3;
                let start = ur;
                walls.push(lr as u32);
                for j in 0..p.len() / 3 {
                    indices.extend([(start - j - 1) as u32, (start - j) as u32]);
                    put(&mut out, Cartesian3::from_array(p, j * 3), back);
                    back -= 3;
                }
                walls.push((start - p.len() / 6) as u32);
                if corner_type == CornerType::Beveled {
                    walls.push((back / 3 + 1) as u32);
                }
                front += 3;
            }
            CorridorCorner::Right(p) => {
                front += 3;
                let start = lr;
                walls.push(ur as u32);
                for j in 0..p.len() / 3 {
                    indices.extend([(start + j) as u32, (start + j + 1) as u32]);
                    put(&mut out, Cartesian3::from_array(p, j * 3), front);
                    front += 3;
                }
                walls.push((start + p.len() / 6) as u32);
                if corner_type == CornerType::Beveled {
                    walls.push((front / 3 - 1) as u32);
                }
                back -= 3;
            }
        }
        let right = &positions[i * 2 + 2][3..];
        let left = &positions[i * 2 + 3];
        let left = &left[..left.len() - 3];
        out[front..front + right.len()].copy_from_slice(right);
        out[back + 3 - left.len()..back + 3].copy_from_slice(left);
        for _ in (0..left.len()).step_by(3) {
            lr = front / 3;
            ur = back / 3;
            indices.extend([(ur + 1) as u32, ur as u32, (lr - 1) as u32, lr as u32]);
            front += 3;
            back -= 3;
        }
        front -= 3;
        back += 3;
        walls.extend([(front / 3) as u32, (back / 3) as u32]);
    }
    if let Some(ends) = &end_positions {
        front += 3;
        back -= 3;
        for i in 0..end_length / 2 {
            let left = Cartesian3::from_array(&ends[1], (end_length - i - 1) * 3);
            let right = Cartesian3::from_array(&ends[1], i * 3);
            put(&mut out, left, back);
            put(&mut out, right, front);
            let lr = front / 3;
            let ur = back / 3;
            indices.extend([(ur + 1) as u32, ur as u32, (lr - 1) as u32, lr as u32]);
            front += 3;
            back -= 3;
        }
        walls.push((front / 3) as u32);
    } else {
        walls.extend([(front / 3) as u32, (back / 3) as u32]);
    }
    indices.extend([(front / 3) as u32, (back / 3) as u32]);
    (out, indices, walls)
}

impl CorridorOutlineGeometry {
    pub fn new(options: CorridorOutlineGeometryOptions) -> Self {
        let extruded = options.extruded_height.unwrap_or(options.height);
        let packed_length = 1 + options.positions.len() * 3 + Ellipsoid::PACKED_LENGTH + 6;
        Self {
            positions: options.positions,
            width: options.width,
            ellipsoid: options.ellipsoid,
            height: js_max(options.height, extruded),
            extruded_height: js_min(options.height, extruded),
            corner_type: options.corner_type,
            granularity: options.granularity,
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
        let count = array[i] as usize;
        i += 1;
        let positions = (0..count).map(|k| Cartesian3::unpack(array, i + k * 3)).collect();
        i += count * 3;
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        Self::new(CorridorOutlineGeometryOptions {
            positions,
            ellipsoid,
            width: array[i],
            height: array[i + 1],
            extruded_height: Some(array[i + 2]),
            corner_type: corner_type_from(array[i + 3]),
            granularity: array[i + 4],
            offset_attribute: GeometryOffsetAttribute::unpack(array[i + 5]),
        })
    }

    /// Creates the outline without mutating the stored positions. Returns
    /// `None` for nonpositive width, fewer than two distinct surface points,
    /// invalid spacing or positions that cannot be projected.
    pub fn create_geometry(&self) -> Option<Geometry> {
        if self.width <= 0.0 {
            return None;
        }
        let positions =
            self.positions.iter().map(|p| self.ellipsoid.scale_to_geodetic_surface(*p)).collect::<Option<Vec<_>>>()?;
        let positions = array_remove_duplicates(&positions, |a, b, e| a.equals_epsilon(*b, e), false, None);
        let mut options = CorridorPositionOptions::new(&positions, self.width);
        options.ellipsoid = self.ellipsoid;
        options.corner_type = self.corner_type;
        options.granularity = self.granularity;
        let (mut positions, mut indices, walls) = combine(compute_positions(&options)?, self.corner_type);
        let extrude = !equals_epsilon(self.height, self.extruded_height, 0.0, EPSILON2);
        let top_count = positions.len() / 3;
        if extrude {
            let mut bottom = positions.clone();
            scale_to_geodetic_height(&mut positions, Some(self.height), &self.ellipsoid, None)?;
            scale_to_geodetic_height(&mut bottom, Some(self.extruded_height), &self.ellipsoid, None)?;
            positions.extend(bottom);
            let bottom_indices: Vec<u32> = indices.iter().map(|i| *i + top_count as u32).collect();
            indices.extend(bottom_indices);
            for i in walls {
                indices.extend([i, i + top_count as u32]);
            }
        } else {
            scale_to_geodetic_height(&mut positions, Some(self.height), &self.ellipsoid, None)?;
        }
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let count = positions.len() / 3;
        let mut attributes =
            GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
        if let Some(offset) = self.offset_attribute {
            let mut attribute = GeometryAttribute::apply_offset(count, offset);
            if extrude && offset == GeometryOffsetAttribute::Top {
                if let super::typed_array::TypedArray::Uint8(values) = &mut attribute.values {
                    values[top_count..].fill(0);
                }
            }
            attributes.set_other("applyOffset", attribute);
        }
        let mut geometry =
            Geometry::new(attributes, Some(IndexDatatype::create_typed_array(count, &indices)), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere);
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
