//! CesiumJS `EllipseOutlineGeometry` and `CircleOutlineGeometry`: the
//! boundary of an ellipse on the ellipsoid, or of the volume it extrudes.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipse_geometry_library::{compute_ellipse_positions, raise_positions_to_height, EllipseOptions};
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::math::{clamp, equals_epsilon, js_max, js_min, js_round, EPSILON2, RADIANS_PER_DEGREE};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseOutlineGeometry {
    pub center: Cartesian3,
    pub semi_major_axis: f64,
    pub semi_minor_axis: f64,
    pub ellipsoid: Ellipsoid,
    pub rotation: f64,
    /// The higher of the two heights.
    pub height: f64,
    pub granularity: f64,
    /// The lower of the two heights.
    pub extruded_height: f64,
    pub number_of_vertical_lines: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

/// Construction options, with Cesium's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseOutlineOptions {
    pub center: Cartesian3,
    pub semi_major_axis: f64,
    pub semi_minor_axis: f64,
    pub ellipsoid: Ellipsoid,
    pub rotation: f64,
    pub height: f64,
    /// Defaults to `height`.
    pub extruded_height: Option<f64>,
    pub granularity: f64,
    pub number_of_vertical_lines: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl EllipseOutlineOptions {
    pub fn new(center: Cartesian3, semi_major_axis: f64, semi_minor_axis: f64) -> Self {
        Self {
            center,
            semi_major_axis,
            semi_minor_axis,
            ellipsoid: Ellipsoid::WGS84,
            rotation: 0.0,
            height: 0.0,
            extruded_height: None,
            granularity: RADIANS_PER_DEGREE,
            number_of_vertical_lines: 16.0,
            offset_attribute: None,
        }
    }
}

impl EllipseOutlineGeometry {
    pub const PACKED_LENGTH: usize = 3 + Ellipsoid::PACKED_LENGTH + 8;

    /// `None` if the semi-major axis is shorter than the semi-minor axis or
    /// the granularity is not positive (Cesium throws).
    pub fn new(options: EllipseOutlineOptions) -> Option<Self> {
        if options.semi_major_axis < options.semi_minor_axis || options.granularity <= 0.0 {
            return None;
        }
        let height = options.height;
        let extruded_height = options.extruded_height.unwrap_or(height);
        Some(Self {
            center: options.center,
            semi_major_axis: options.semi_major_axis,
            semi_minor_axis: options.semi_minor_axis,
            ellipsoid: options.ellipsoid,
            rotation: options.rotation,
            height: js_max(extruded_height, height),
            granularity: options.granularity,
            extruded_height: js_min(extruded_height, height),
            number_of_vertical_lines: js_max(options.number_of_vertical_lines, 0.0),
            offset_attribute: options.offset_attribute,
        })
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        self.center.pack(array, i);
        i += 3;
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let values = [
            self.semi_major_axis,
            self.semi_minor_axis,
            self.rotation,
            self.height,
            self.granularity,
            self.extruded_height,
            self.number_of_vertical_lines,
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ];
        array[i..i + 8].copy_from_slice(&values);
    }

    pub fn unpack(array: &[f64], i: usize) -> Option<Self> {
        let center = Cartesian3::unpack(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i + 3);
        let a = &array[i + 3 + Ellipsoid::PACKED_LENGTH..i + 3 + Ellipsoid::PACKED_LENGTH + 8];
        Self::new(EllipseOutlineOptions {
            center,
            semi_major_axis: a[0],
            semi_minor_axis: a[1],
            ellipsoid,
            rotation: a[2],
            height: a[3],
            extruded_height: Some(a[5]),
            granularity: a[4],
            number_of_vertical_lines: a[6],
            offset_attribute: GeometryOffsetAttribute::unpack(a[7]),
        })
    }

    /// The outline; `None` for a non-positive axis or a center at the
    /// ellipsoid's center.
    pub fn create_geometry(&self) -> Option<Geometry> {
        if self.semi_major_axis <= 0.0 || self.semi_minor_axis <= 0.0 {
            return None;
        }
        let height = self.height;
        let extruded_height = self.extruded_height;
        let extrude = !equals_epsilon(height, extruded_height, 0.0, EPSILON2);
        let center = self.ellipsoid.scale_to_geodetic_surface(self.center)?;
        let ellipse = EllipseOptions {
            center,
            semi_major_axis: self.semi_major_axis,
            semi_minor_axis: self.semi_minor_axis,
            rotation: self.rotation,
            granularity: self.granularity,
        };
        let normal = self.ellipsoid.geodetic_surface_normal(center).unwrap_or_default();
        let outer = compute_ellipse_positions(&ellipse, false, true).outer_positions.unwrap_or_default();
        let mut attributes = GeometryAttributes::default();

        let (indices, bounding_sphere) = if extrude {
            let top = BoundingSphere::new(center.add(normal.multiply_by_scalar(height)), self.semi_major_axis);
            let bottom =
                BoundingSphere::new(center.add(normal.multiply_by_scalar(extruded_height)), self.semi_major_axis);
            let positions = raise_positions_to_height(&outer, &self.ellipsoid, height, extruded_height, true);
            let mut length = positions.len() / 3;
            if let Some(offset) = self.offset_attribute {
                let values = match offset {
                    GeometryOffsetAttribute::Top => {
                        let mut v = vec![0u8; length];
                        v[..length / 2].fill(1);
                        v
                    }
                    GeometryOffsetAttribute::None => vec![0; length],
                    GeometryOffsetAttribute::All => vec![1; length],
                };
                attributes.set_other(
                    "applyOffset",
                    GeometryAttribute::new(
                        super::component_datatype::ComponentDatatype::UnsignedByte,
                        1,
                        TypedArray::Uint8(values),
                    ),
                );
            }
            attributes.position = Some(GeometryAttribute::doubles(3, positions));

            let number_of_vertical_lines = clamp(self.number_of_vertical_lines, 0.0, length as f64 / 2.0);
            let index_length = (length as f64 * 2.0 + number_of_vertical_lines * 2.0) as usize;
            let total = length;
            length /= 2;
            let mut indices: Vec<u32> = Vec::new();
            for i in 0..length {
                indices.extend([i, (i + 1) % length, i + length, ((i + 1) % length) + length].map(|v| v as u32));
            }
            if number_of_vertical_lines > 0.0 {
                let num_side_lines = number_of_vertical_lines.min(length as f64);
                let num_side = js_round(length as f64 / num_side_lines);
                let max_i = (num_side * number_of_vertical_lines).min(length as f64);
                let mut i = 0.0;
                while i < max_i {
                    indices.extend([i as u32, (i + length as f64) as u32]);
                    i += num_side;
                }
            }
            indices.resize(index_length, 0);
            (IndexDatatype::create_typed_array(total, &indices), top.union(&bottom))
        } else {
            let bounding_sphere =
                BoundingSphere::new(center.add(normal.multiply_by_scalar(height)), self.semi_major_axis);
            let positions = raise_positions_to_height(&outer, &self.ellipsoid, height, height, false);
            let length = outer.len() / 3;
            let indices: Vec<u32> = (0..length).flat_map(|i| [i as u32, ((i + 1) % length) as u32]).collect();
            if let Some(offset) = self.offset_attribute {
                attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
            }
            attributes.position = Some(GeometryAttribute::doubles(3, positions));
            (IndexDatatype::create_typed_array(length, &indices), bounding_sphere)
        };

        let mut geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere);
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}

/// A circle is an ellipse with equal axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleOutlineGeometry {
    pub ellipse_geometry: EllipseOutlineGeometry,
}

impl CircleOutlineGeometry {
    pub const PACKED_LENGTH: usize = EllipseOutlineGeometry::PACKED_LENGTH;

    /// `options.semi_major_axis` and `semi_minor_axis` are replaced by `radius`;
    /// like Cesium, the rotation and offset attribute are not passed on.
    pub fn new(radius: f64, options: EllipseOutlineOptions) -> Option<Self> {
        let options = EllipseOutlineOptions {
            semi_major_axis: radius,
            semi_minor_axis: radius,
            rotation: 0.0,
            offset_attribute: None,
            ..options
        };
        EllipseOutlineGeometry::new(options).map(|ellipse_geometry| Self { ellipse_geometry })
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.ellipse_geometry.pack(array, starting_index);
    }

    /// Rebuilds the circle from the packed ellipse's major axis, as Cesium does.
    pub fn unpack(array: &[f64], starting_index: usize) -> Option<Self> {
        let e = EllipseOutlineGeometry::unpack(array, starting_index)?;
        Self::new(
            e.semi_major_axis,
            EllipseOutlineOptions {
                center: e.center,
                ellipsoid: e.ellipsoid,
                height: e.height,
                extruded_height: Some(e.extruded_height),
                granularity: e.granularity,
                number_of_vertical_lines: e.number_of_vertical_lines,
                ..EllipseOutlineOptions::new(e.center, e.semi_major_axis, e.semi_major_axis)
            },
        )
    }

    pub fn create_geometry(&self) -> Option<Geometry> {
        self.ellipse_geometry.create_geometry()
    }
}
