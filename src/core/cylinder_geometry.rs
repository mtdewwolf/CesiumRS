//! CesiumJS `CylinderGeometry`, `CylinderOutlineGeometry`, and
//! `CylinderGeometryLibrary`: a cylinder or cone along the z-axis, centered
//! at the origin.

use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::js_math::JsMath;
use super::math::{js_max, js_round, TWO_PI};
use super::primitive_type::PrimitiveType;
use super::vertex_format::VertexFormat;

/// Cesium's `CylinderGeometryLibrary.computePositions`: the bottom and top
/// rings, and with `fill` first the side's interleaved bottom/top pairs.
pub fn compute_positions(length: f64, top_radius: f64, bottom_radius: f64, slices: usize, fill: bool) -> Vec<f64> {
    let top_z = length * 0.5;
    let bottom_z = -top_z;
    let two_slice = slices + slices;
    let size = if fill { 2 * two_slice } else { two_slice };
    let mut positions = vec![0.0; size * 3];
    let mut index = 0;
    let mut tb_index = 0;
    let bottom_offset = if fill { two_slice * 3 } else { 0 };
    let top_offset = if fill { (two_slice + slices) * 3 } else { slices * 3 };
    for i in 0..slices {
        let angle = (i as f64 / slices as f64) * TWO_PI;
        let x = angle.js_cos();
        let y = angle.js_sin();
        let (bottom_x, bottom_y) = (x * bottom_radius, y * bottom_radius);
        let (top_x, top_y) = (x * top_radius, y * top_radius);
        positions[tb_index + bottom_offset..tb_index + bottom_offset + 3]
            .copy_from_slice(&[bottom_x, bottom_y, bottom_z]);
        positions[tb_index + top_offset..tb_index + top_offset + 3].copy_from_slice(&[top_x, top_y, top_z]);
        tb_index += 3;
        if fill {
            positions[index..index + 6].copy_from_slice(&[bottom_x, bottom_y, bottom_z, top_x, top_y, top_z]);
            index += 6;
        }
    }
    positions
}

fn bounding_sphere(length: f64, top_radius: f64, bottom_radius: f64) -> BoundingSphere {
    let radius = Cartesian2::new(length * 0.5, js_max(bottom_radius, top_radius));
    BoundingSphere::new(Cartesian3::ZERO, radius.magnitude())
}

fn is_degenerate(length: f64, top_radius: f64, bottom_radius: f64) -> bool {
    length <= 0.0 || top_radius < 0.0 || bottom_radius < 0.0 || (top_radius == 0.0 && bottom_radius == 0.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CylinderGeometry {
    pub length: f64,
    pub top_radius: f64,
    pub bottom_radius: f64,
    pub vertex_format: VertexFormat,
    pub slices: usize,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl CylinderGeometry {
    pub const PACKED_LENGTH: usize = VertexFormat::PACKED_LENGTH + 5;

    /// Cesium defaults to 128 slices. `None` for fewer than three slices or a
    /// [`GeometryOffsetAttribute::Top`] offset (Cesium throws).
    pub fn new(
        length: f64,
        top_radius: f64,
        bottom_radius: f64,
        slices: usize,
        vertex_format: VertexFormat,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Option<Self> {
        if slices < 3 || offset_attribute == Some(GeometryOffsetAttribute::Top) {
            return None;
        }
        Some(Self { length, top_radius, bottom_radius, vertex_format, slices, offset_attribute })
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        let values = [
            self.length,
            self.top_radius,
            self.bottom_radius,
            self.slices as f64,
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ];
        array[i..i + 5].copy_from_slice(&values);
    }

    pub fn unpack(array: &[f64], i: usize) -> Option<Self> {
        let vertex_format = VertexFormat::unpack(array, i);
        let a = &array[i + VertexFormat::PACKED_LENGTH..i + VertexFormat::PACKED_LENGTH + 5];
        Self::new(a[0], a[1], a[2], a[3] as usize, vertex_format, GeometryOffsetAttribute::unpack(a[4]))
    }

    /// The triangles; `None` for a non-positive length, a negative radius,
    /// or two zero radii.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let (length, top_radius, bottom_radius) = (self.length, self.top_radius, self.bottom_radius);
        let vertex_format = self.vertex_format;
        let slices = self.slices;
        if is_degenerate(length, top_radius, bottom_radius) {
            return None;
        }
        let two_slices = slices + slices;
        let three_slices = slices + two_slices;
        let num_vertices = two_slices + two_slices;
        let positions = compute_positions(length, top_radius, bottom_radius, slices, true);

        let mut normals = Vec::new();
        let mut tangents = Vec::new();
        let mut bitangents = Vec::new();
        let compute_normal = vertex_format.normal || vertex_format.tangent || vertex_format.bitangent;
        if compute_normal {
            let compute_tangent = vertex_format.tangent || vertex_format.bitangent;
            let theta = (bottom_radius - top_radius).js_atan2(length);
            let normal_z = theta.js_sin();
            let normal_scale = theta.js_cos();
            let mut tangent = Cartesian3::ZERO;
            for i in 0..slices {
                let angle = (i as f64 / slices as f64) * TWO_PI;
                let normal = Cartesian3::new(normal_scale * angle.js_cos(), normal_scale * angle.js_sin(), normal_z);
                if compute_tangent {
                    tangent = Cartesian3::UNIT_Z.cross(normal).normalize();
                }
                if vertex_format.normal {
                    normals.extend([normal.x, normal.y, normal.z, normal.x, normal.y, normal.z]);
                }
                if vertex_format.tangent {
                    tangents.extend([tangent.x, tangent.y, tangent.z, tangent.x, tangent.y, tangent.z]);
                }
                if vertex_format.bitangent {
                    let bitangent = normal.cross(tangent).normalize();
                    bitangents.extend([bitangent.x, bitangent.y, bitangent.z, bitangent.x, bitangent.y, bitangent.z]);
                }
            }
            for (cap_normal_z, bitangent_y) in [(-1.0, -1.0), (1.0, 1.0)] {
                for _ in 0..slices {
                    if vertex_format.normal {
                        normals.extend([0.0, 0.0, cap_normal_z]);
                    }
                    if vertex_format.tangent {
                        tangents.extend([1.0, 0.0, 0.0]);
                    }
                    if vertex_format.bitangent {
                        bitangents.extend([0.0, bitangent_y, 0.0]);
                    }
                }
            }
        }

        let mut indices: Vec<u32> = Vec::with_capacity(12 * slices - 12);
        let mut j = 0;
        for _ in 0..slices - 1 {
            indices.extend([j, j + 2, j + 3, j, j + 3, j + 1]);
            j += 2;
        }
        let ts = two_slices as u32;
        indices.extend([ts - 2, 0, 1, ts - 2, 1, ts - 1]);
        for i in 1..slices as u32 - 1 {
            indices.extend([ts + i + 1, ts + i, ts]);
        }
        let th = three_slices as u32;
        for i in 1..slices as u32 - 1 {
            indices.extend([th, th + i, th + i + 1]);
        }
        let indices = IndexDatatype::create_typed_array(num_vertices, &indices);

        let mut attributes = GeometryAttributes::default();
        if vertex_format.position {
            attributes.position = Some(GeometryAttribute::doubles(3, positions.clone()));
        }
        if vertex_format.normal {
            attributes.normal = Some(GeometryAttribute::floats(3, &normals));
        }
        if vertex_format.tangent {
            attributes.tangent = Some(GeometryAttribute::floats(3, &tangents));
        }
        if vertex_format.bitangent {
            attributes.bitangent = Some(GeometryAttribute::floats(3, &bitangents));
        }
        if vertex_format.st {
            let rad = js_max(top_radius, bottom_radius);
            let st: Vec<f64> = positions
                .chunks_exact(3)
                .flat_map(|p| [(p[0] + rad) / (2.0 * rad), (p[1] + rad) / (2.0 * rad)])
                .collect();
            attributes.st = Some(GeometryAttribute::floats(2, &st));
        }
        if let Some(offset) = self.offset_attribute {
            attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
        }
        let mut geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(bounding_sphere(length, top_radius, bottom_radius));
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }

    /// A position-only cylinder of length and radii 1.
    pub fn get_unit_cylinder() -> Geometry {
        Self::new(1.0, 1.0, 1.0, 128, VertexFormat::POSITION_ONLY, None)
            .and_then(|c| c.create_geometry())
            .expect("the unit cylinder is valid")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CylinderOutlineGeometry {
    pub length: f64,
    pub top_radius: f64,
    pub bottom_radius: f64,
    pub slices: usize,
    pub number_of_vertical_lines: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl CylinderOutlineGeometry {
    pub const PACKED_LENGTH: usize = 6;

    /// Cesium defaults to 128 slices and 16 vertical lines. `None` for fewer
    /// than three slices or a [`GeometryOffsetAttribute::Top`] offset (Cesium
    /// checks both in debug builds).
    pub fn new(
        length: f64,
        top_radius: f64,
        bottom_radius: f64,
        slices: usize,
        number_of_vertical_lines: f64,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Option<Self> {
        if slices < 3 || offset_attribute == Some(GeometryOffsetAttribute::Top) {
            return None;
        }
        Some(Self {
            length,
            top_radius,
            bottom_radius,
            slices,
            number_of_vertical_lines: js_max(number_of_vertical_lines, 0.0),
            offset_attribute,
        })
    }

    pub fn pack(&self, array: &mut [f64], i: usize) {
        let values = [
            self.length,
            self.top_radius,
            self.bottom_radius,
            self.slices as f64,
            self.number_of_vertical_lines,
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ];
        array[i..i + 6].copy_from_slice(&values);
    }

    pub fn unpack(array: &[f64], i: usize) -> Option<Self> {
        let a = &array[i..i + 6];
        Self::new(a[0], a[1], a[2], a[3] as usize, a[4], GeometryOffsetAttribute::unpack(a[5]))
    }

    /// The lines; `None` in the same cases as [`CylinderGeometry::create_geometry`].
    pub fn create_geometry(&self) -> Option<Geometry> {
        let (length, top_radius, bottom_radius) = (self.length, self.top_radius, self.bottom_radius);
        let slices = self.slices;
        if is_degenerate(length, top_radius, bottom_radius) {
            return None;
        }
        let num_vertices = slices * 2;
        let positions = compute_positions(length, top_radius, bottom_radius, slices, false);
        let mut num_indices = slices as f64 * 2.0;
        let mut num_side = 0.0;
        if self.number_of_vertical_lines > 0.0 {
            let num_side_lines = self.number_of_vertical_lines.min(slices as f64);
            num_side = js_round(slices as f64 / num_side_lines);
            num_indices += num_side_lines;
        }
        let s = slices as u32;
        let mut indices: Vec<u32> = Vec::new();
        for i in 0..s.saturating_sub(1) {
            indices.extend([i, i + 1, i + s, i + 1 + s]);
        }
        indices.extend([s.wrapping_sub(1), 0, (s + s).wrapping_sub(1), s]);
        if self.number_of_vertical_lines > 0.0 {
            let mut i = 0.0;
            while i < slices as f64 {
                indices.extend([i as u32, (i + slices as f64) as u32]);
                i += num_side;
            }
        }
        // Cesium's typed array has this length (fractional lengths truncate).
        indices.resize((num_indices * 2.0) as usize, 0);
        let indices = IndexDatatype::create_typed_array(num_vertices, &indices);

        let mut attributes = GeometryAttributes::default();
        if let Some(offset) = self.offset_attribute {
            attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
        }
        attributes.position = Some(GeometryAttribute::doubles(3, positions));
        let mut geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere(length, top_radius, bottom_radius));
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
