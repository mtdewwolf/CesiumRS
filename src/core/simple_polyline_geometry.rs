//! CesiumJS `SimplePolylineGeometry`: a polyline as one-pixel lines,
//! optionally densified along geodesics or rhumb lines.

use super::arc_type::ArcType;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::color::Color;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::index_datatype::IndexDatatype;
use super::math::{chord_length, RADIANS_PER_DEGREE};
use super::polyline_pipeline::{extract_heights, generate_arc, generate_rhumb_arc, number_of_points, ArcOptions, PolylineHeight};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct SimplePolylineGeometryOptions {
    pub positions: Vec<Cartesian3>,
    pub colors: Option<Vec<Color>>,
    pub colors_per_vertex: bool,
    pub arc_type: ArcType,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
}

impl SimplePolylineGeometryOptions {
    pub fn new(positions: Vec<Cartesian3>) -> Self {
        Self {
            positions,
            colors: None,
            colors_per_vertex: false,
            arc_type: ArcType::Geodesic,
            granularity: RADIANS_PER_DEGREE,
            ellipsoid: Ellipsoid::WGS84,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimplePolylineGeometry {
    pub positions: Vec<Cartesian3>,
    pub colors: Option<Vec<Color>>,
    pub colors_per_vertex: bool,
    pub arc_type: ArcType,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    pub packed_length: usize,
}

fn bytes(color: &Color) -> [f64; 4] {
    [color.red, color.green, color.blue, color.alpha].map(|v| f64::from(Color::float_to_byte(v)))
}

/// Writes into a fixed-length array, ignoring out-of-range writes as a
/// typed array does.
fn put(values: &mut [f64], index: &mut usize, v: f64) {
    if let Some(slot) = values.get_mut(*index) {
        *slot = v;
    }
    *index += 1;
}

/// Colors from `color0` toward `color1`, one per point Cesium counts along
/// the straight segment (also for rhumb lines).
fn interpolate_colors(p0: Cartesian3, p1: Cartesian3, color0: &Color, color1: &Color, min_distance: f64, array: &mut [f64], mut offset: usize) -> usize {
    let num_points = number_of_points(p0, p1, min_distance);
    let count = if num_points > 0.0 { num_points as usize } else { 0 };
    if color0 == color1 {
        for _ in 0..count {
            for v in bytes(color0) {
                put(array, &mut offset, v);
            }
        }
        return offset;
    }
    let per_vertex = [
        (color1.red - color0.red) / num_points,
        (color1.green - color0.green) / num_points,
        (color1.blue - color0.blue) / num_points,
        (color1.alpha - color0.alpha) / num_points,
    ];
    let start = [color0.red, color0.green, color0.blue, color0.alpha];
    for i in 0..count {
        for k in 0..4 {
            put(array, &mut offset, f64::from(Color::float_to_byte(start[k] + i as f64 * per_vertex[k])));
        }
    }
    offset
}

impl SimplePolylineGeometry {
    pub fn new(options: SimplePolylineGeometryOptions) -> Self {
        let colors_length = options.colors.as_ref().map_or(1, |c| 1 + c.len() * 4);
        let packed_length = 1 + options.positions.len() * 3 + colors_length + Ellipsoid::PACKED_LENGTH + 3;
        Self {
            positions: options.positions,
            colors: options.colors,
            colors_per_vertex: options.colors_per_vertex,
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
        array[i] = if self.colors_per_vertex { 1.0 } else { 0.0 };
        array[i + 1] = self.arc_type as u32 as f64;
        array[i + 2] = self.granularity;
    }

    pub fn unpack(array: &[f64], mut i: usize) -> Self {
        let n = array[i] as usize;
        i += 1;
        let positions = (0..n).map(|k| Cartesian3::unpack(array, i + k * 3)).collect();
        i += n * 3;
        let c = array[i] as usize;
        i += 1;
        let colors: Vec<Color> = (0..c)
            .map(|k| {
                let o = i + k * 4;
                Color::new(array[o], array[o + 1], array[o + 2], array[o + 3])
            })
            .collect();
        i += c * 4;
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        Self::new(SimplePolylineGeometryOptions {
            positions,
            colors: (c > 0).then_some(colors),
            colors_per_vertex: array[i] == 1.0,
            arc_type: match array[i + 1] as i32 {
                0 => ArcType::None,
                2 => ArcType::Rhumb,
                _ => ArcType::Geodesic,
            },
            granularity: array[i + 2],
            ellipsoid,
        })
    }

    /// The lines. `None` where Cesium throws: rhumb lines with per-segment
    /// colors (its point count reads cartographic fields off `Cartesian3`s),
    /// or a position at the ellipsoid's center.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let positions = &self.positions;
        let ellipsoid = &self.ellipsoid;
        let min_distance = chord_length(self.granularity, ellipsoid.maximum_radius);
        let per_segment_colors = self.colors.is_some() && !self.colors_per_vertex;
        let length = positions.len();
        let mut position_values: Vec<f64>;
        let mut color_values: Option<Vec<f64>> = None;

        if matches!(self.arc_type, ArcType::Geodesic | ArcType::Rhumb) {
            let geodesic = self.arc_type == ArcType::Geodesic;
            let heights = extract_heights(positions, ellipsoid);
            let arc = |ps: Vec<Cartesian3>, hs: Vec<f64>| {
                let mut options = ArcOptions::new(ps);
                options.ellipsoid = *ellipsoid;
                options.height = PolylineHeight::PerPosition(hs);
                if geodesic {
                    options.min_distance = Some(min_distance);
                    generate_arc(&options)
                } else {
                    options.granularity = Some(self.granularity);
                    generate_rhumb_arc(&options)
                }
            };
            if per_segment_colors {
                if !geodesic {
                    return None;
                }
                let colors = self.colors.as_ref()?;
                let position_count: usize =
                    (0..length.saturating_sub(1)).map(|i| number_of_points(positions[i], positions[i + 1], min_distance) as usize + 1).sum();
                position_values = vec![0.0; position_count * 3];
                let mut cv = vec![0.0; position_count * 4];
                let (mut offset, mut ci) = (0, 0);
                for i in 0..length.saturating_sub(1) {
                    let pos = arc(vec![positions[i], positions[i + 1]], vec![heights[i], heights[i + 1]])?;
                    let color = colors.get(i)?;
                    for _ in 0..pos.len() / 3 {
                        for v in bytes(color) {
                            put(&mut cv, &mut ci, v);
                        }
                    }
                    // Float64Array.set throws past the end.
                    position_values.get_mut(offset..offset + pos.len())?.copy_from_slice(&pos);
                    offset += pos.len();
                }
                color_values = Some(cv);
            } else {
                position_values = arc(positions.clone(), heights)?;
                if let Some(colors) = self.colors.as_ref() {
                    let mut cv = vec![0.0; (position_values.len() / 3) * 4];
                    let mut offset = 0;
                    for i in 0..length.saturating_sub(1) {
                        offset = interpolate_colors(positions[i], positions[i + 1], colors.get(i)?, colors.get(i + 1)?, min_distance, &mut cv, offset);
                    }
                    for v in bytes(colors.get(length - 1)?) {
                        put(&mut cv, &mut offset, v);
                    }
                    color_values = Some(cv);
                }
            }
        } else {
            let number_of_positions = if per_segment_colors { (length * 2).saturating_sub(2) } else { length };
            position_values = vec![0.0; number_of_positions * 3];
            let mut cv = self.colors.as_ref().map(|_| vec![0.0; number_of_positions * 4]);
            let (mut pi, mut ci) = (0, 0);
            for (i, p) in positions.iter().enumerate() {
                if per_segment_colors && i > 0 {
                    for v in [p.x, p.y, p.z] {
                        put(&mut position_values, &mut pi, v);
                    }
                    let color = self.colors.as_ref()?.get(i - 1)?;
                    for v in bytes(color) {
                        put(cv.as_mut()?, &mut ci, v);
                    }
                }
                if per_segment_colors && i == length - 1 {
                    break;
                }
                for v in [p.x, p.y, p.z] {
                    put(&mut position_values, &mut pi, v);
                }
                if let Some(colors) = self.colors.as_ref() {
                    for v in bytes(colors.get(i)?) {
                        put(cv.as_mut()?, &mut ci, v);
                    }
                }
            }
            color_values = cv;
        }

        let number_of_positions = position_values.len() / 3;
        let mut attributes = GeometryAttributes { position: Some(GeometryAttribute::doubles(3, position_values)), ..Default::default() };
        if let Some(cv) = color_values {
            let mut color = GeometryAttribute::new(
                ComponentDatatype::UnsignedByte,
                4,
                TypedArray::from_values(ComponentDatatype::UnsignedByte, &cv),
            );
            color.normalize = true;
            attributes.color = Some(color);
        }
        let indices: Vec<u32> = (0..number_of_positions.saturating_sub(1)).flat_map(|i| [i as u32, i as u32 + 1]).collect();
        let mut geometry =
            Geometry::new(attributes, Some(IndexDatatype::create_typed_array(number_of_positions, &indices)), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(BoundingSphere::from_points(positions));
        Some(geometry)
    }
}
