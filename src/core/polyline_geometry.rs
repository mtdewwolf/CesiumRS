//! CesiumJS `PolylineGeometry`: a screen-space-width polyline as triangle
//! strips, with previous and next positions for the shader to expand.

use super::arc_type::ArcType;
use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::color::Color;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryType};
use super::index_datatype::IndexDatatype;
use super::math::{chord_length, js_max, RADIANS_PER_DEGREE};
use super::polyline_pipeline::{
    extract_heights, generate_cartesian_arc, generate_cartesian_rhumb_arc, number_of_points, ArcOptions, PolylineHeight,
};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct PolylineGeometryOptions {
    pub positions: Vec<Cartesian3>,
    pub colors: Option<Vec<Color>>,
    pub width: f64,
    pub colors_per_vertex: bool,
    pub vertex_format: VertexFormat,
    pub arc_type: ArcType,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
}

impl PolylineGeometryOptions {
    pub fn new(positions: Vec<Cartesian3>) -> Self {
        Self {
            positions,
            colors: None,
            width: 1.0,
            colors_per_vertex: false,
            vertex_format: VertexFormat::DEFAULT,
            arc_type: ArcType::Geodesic,
            granularity: RADIANS_PER_DEGREE,
            ellipsoid: Ellipsoid::WGS84,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolylineGeometry {
    pub positions: Vec<Cartesian3>,
    pub colors: Option<Vec<Color>>,
    pub width: f64,
    pub colors_per_vertex: bool,
    pub vertex_format: VertexFormat,
    pub arc_type: ArcType,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    pub packed_length: usize,
}

/// `numPoints` colors from `color0` toward `color1`.
fn interpolate_colors(color0: &Color, color1: &Color, num_points: usize) -> Vec<Color> {
    if color0 == color1 {
        return vec![*color0; num_points];
    }
    let n = num_points as f64;
    let (dr, dg, db, da) = (
        (color1.red - color0.red) / n,
        (color1.green - color0.green) / n,
        (color1.blue - color0.blue) / n,
        (color1.alpha - color0.alpha) / n,
    );
    (0..num_points)
        .map(|i| {
            let i = i as f64;
            Color::new(color0.red + i * dr, color0.green + i * dg, color0.blue + i * db, color0.alpha + i * da)
        })
        .collect()
}

/// Drops the colors of removed duplicate positions, as Cesium's filter does.
fn filter_colors(colors: &[Color], removed: &[usize], colors_per_vertex: bool) -> Vec<Color> {
    let mut removed_index = 0;
    let mut next_removed = removed.first().copied();
    colors
        .iter()
        .enumerate()
        .filter(|&(index, _)| {
            let remove = if colors_per_vertex {
                Some(index) == next_removed || (index == 0 && next_removed == Some(1))
            } else {
                Some(index + 1) == next_removed
            };
            if remove {
                removed_index += 1;
                next_removed = removed.get(removed_index).copied();
            }
            !remove
        })
        .map(|(_, c)| *c)
        .collect()
}

impl PolylineGeometry {
    pub fn new(options: PolylineGeometryOptions) -> Self {
        let colors_length = options.colors.as_ref().map_or(1, |c| 1 + c.len() * 4);
        let packed_length = 1 + options.positions.len() * 3 + colors_length + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH + 4;
        Self {
            positions: options.positions,
            colors: options.colors,
            width: options.width,
            colors_per_vertex: options.colors_per_vertex,
            vertex_format: options.vertex_format,
            arc_type: options.arc_type,
            granularity: options.granularity,
            ellipsoid: options.ellipsoid,
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
        let colors = self.colors.as_deref().unwrap_or(&[]);
        array[i] = colors.len() as f64;
        i += 1;
        for c in colors {
            array[i..i + 4].copy_from_slice(&[c.red, c.green, c.blue, c.alpha]);
            i += 4;
        }
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        array[i] = self.width;
        array[i + 1] = if self.colors_per_vertex { 1.0 } else { 0.0 };
        array[i + 2] = self.arc_type as u32 as f64;
        array[i + 3] = self.granularity;
    }

    pub fn unpack(array: &[f64], mut i: usize) -> Self {
        let n = array[i] as usize;
        i += 1;
        let positions = (0..n).map(|k| Cartesian3::unpack(array, i + k * 3)).collect();
        i += n * 3;
        let c = array[i] as usize;
        i += 1;
        let colors: Vec<Color> = (0..c).map(|k| {
            let o = i + k * 4;
            Color::new(array[o], array[o + 1], array[o + 2], array[o + 3])
        }).collect();
        i += c * 4;
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        Self::new(PolylineGeometryOptions {
            positions,
            colors: (c > 0).then_some(colors),
            width: array[i],
            colors_per_vertex: array[i + 1] == 1.0,
            vertex_format,
            arc_type: match array[i + 2] as i32 {
                0 => ArcType::None,
                2 => ArcType::Rhumb,
                _ => ArcType::Geodesic,
            },
            granularity: array[i + 3],
            ellipsoid,
        })
    }

    /// The polyline; `None` for fewer than two distinct positions or a
    /// non-positive width, and where Cesium throws: rhumb lines with colors
    /// (its point count reads cartographic fields off `Cartesian3`s).
    pub fn create_geometry(&self) -> Option<Geometry> {
        let width = self.width;
        let vf = self.vertex_format;
        let ellipsoid = &self.ellipsoid;
        let mut removed = Vec::new();
        let mut positions = array_remove_duplicates(&self.positions, |a, b, e| a.equals_epsilon(*b, e), false, Some(&mut removed));
        let mut colors = self.colors.clone();
        if let Some(c) = colors.as_mut() {
            if !removed.is_empty() {
                *c = filter_colors(c, &removed, self.colors_per_vertex);
            }
        }
        if positions.len() < 2 || width <= 0.0 {
            return None;
        }
        if matches!(self.arc_type, ArcType::Geodesic | ArcType::Rhumb) {
            let heights = extract_heights(&positions, ellipsoid);
            if let Some(c) = colors.as_ref() {
                if self.arc_type == ArcType::Rhumb {
                    return None;
                }
                let subdivision_size = chord_length(self.granularity, ellipsoid.maximum_radius);
                let mut new_colors = Vec::new();
                for i in 0..positions.len() - 1 {
                    let num = number_of_points(positions[i], positions[i + 1], subdivision_size) as usize;
                    let c0 = c.get(i)?;
                    if self.colors_per_vertex {
                        new_colors.extend(interpolate_colors(c0, c.get(i + 1)?, num));
                    } else {
                        new_colors.extend(std::iter::repeat_n(*c0, num));
                    }
                }
                new_colors.push(*c.last()?);
                colors = Some(new_colors);
            }
            let mut options = ArcOptions::new(positions);
            options.ellipsoid = *ellipsoid;
            options.height = PolylineHeight::PerPosition(heights);
            positions = if self.arc_type == ArcType::Geodesic {
                options.min_distance = Some(chord_length(self.granularity, ellipsoid.maximum_radius));
                generate_cartesian_arc(&options)?
            } else {
                options.granularity = Some(self.granularity);
                generate_cartesian_rhumb_arc(&options)?
            };
        }

        let n = positions.len();
        let size = n * 4 - 4;
        let mut final_positions = Vec::with_capacity(size * 3);
        let mut prev_positions = Vec::with_capacity(size * 3);
        let mut next_positions = Vec::with_capacity(size * 3);
        let mut expand_and_width = Vec::with_capacity(size * 2);
        let mut st = Vec::with_capacity(size * 2);
        let mut final_colors: Vec<u8> = Vec::with_capacity(size * 4);
        for j in 0..n {
            let prev = if j == 0 { positions[0].add(positions[0].subtract(positions[1])) } else { positions[j - 1] };
            let position = positions[j];
            let next = if j == n - 1 { positions[n - 1].add(positions[n - 1].subtract(positions[n - 2])) } else { positions[j + 1] };
            let (color0, color1) = match colors.as_ref() {
                Some(c) => {
                    let c0 = if j != 0 && !self.colors_per_vertex { c.get(j - 1) } else { c.get(j) };
                    (c0, if j != n - 1 { c.get(j) } else { None })
                }
                None => (None, None),
            };
            let start_k = if j == 0 { 2 } else { 0 };
            let end_k = if j == n - 1 { 2 } else { 4 };
            for k in start_k..end_k {
                final_positions.extend([position.x, position.y, position.z]);
                prev_positions.extend([prev.x, prev.y, prev.z]);
                next_positions.extend([next.x, next.y, next.z]);
                let direction = if k < 2 { -1.0 } else { 1.0 };
                let expand = (2 * (k % 2)) as f64 - 1.0;
                expand_and_width.extend([expand, direction * width]);
                if vf.st {
                    st.extend([j as f64 / (n - 1) as f64, js_max(expand, 0.0)]);
                }
                if colors.is_some() {
                    let color = if k < 2 { color0 } else { color1 }?;
                    final_colors.extend(
                        [color.red, color.green, color.blue, color.alpha].map(|v| Color::float_to_byte(v) as u8),
                    );
                }
            }
        }
        let mut attributes = GeometryAttributes { position: Some(GeometryAttribute::doubles(3, final_positions)), ..Default::default() };
        attributes.set_other("prevPosition", GeometryAttribute::doubles(3, prev_positions));
        attributes.set_other("nextPosition", GeometryAttribute::doubles(3, next_positions));
        attributes.set_other("expandAndWidth", GeometryAttribute::floats(2, &expand_and_width));
        if vf.st {
            attributes.st = Some(GeometryAttribute::floats(2, &st));
        }
        if colors.is_some() {
            let mut color = GeometryAttribute::new(ComponentDatatype::UnsignedByte, 4, TypedArray::Uint8(final_colors));
            color.normalize = true;
            attributes.color = Some(color);
        }
        let mut indices = Vec::with_capacity(n * 6 - 6);
        let mut index = 0u32;
        for _ in 0..n - 1 {
            indices.extend([index, index + 2, index + 1, index + 1, index + 2, index + 3]);
            index += 4;
        }
        let mut geometry = Geometry::new(attributes, Some(IndexDatatype::create_typed_array(size, &indices)), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(BoundingSphere::from_points(&positions));
        geometry.geometry_type = GeometryType::Polylines;
        Some(geometry)
    }
}
