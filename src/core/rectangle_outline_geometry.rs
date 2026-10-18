//! CesiumJS `RectangleOutlineGeometry`: the boundary of a cartographic
//! rectangle, or of the volume it extrudes.

use super::bounding_sphere::BoundingSphere;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::math::{equals_epsilon, js_max, js_min, EPSILON10, EPSILON2, RADIANS_PER_DEGREE};
use super::polygon_pipeline::scale_to_geodetic_height;
use super::primitive_type::PrimitiveType;
use super::rectangle::Rectangle;
use super::rectangle_geometry_library::{compute_options, compute_position, RectangleComputedOptions};

/// Construction options, with Cesium's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleOutlineOptions {
    pub rectangle: Rectangle,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    pub rotation: f64,
    pub height: f64,
    /// Defaults to `height`.
    pub extruded_height: Option<f64>,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl RectangleOutlineOptions {
    pub fn new(rectangle: Rectangle) -> Self {
        Self {
            rectangle,
            granularity: RADIANS_PER_DEGREE,
            ellipsoid: Ellipsoid::WGS84,
            rotation: 0.0,
            height: 0.0,
            extruded_height: None,
            offset_attribute: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleOutlineGeometry {
    pub rectangle: Rectangle,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    /// The higher of the two heights.
    pub surface_height: f64,
    pub rotation: f64,
    /// The lower of the two heights.
    pub extruded_height: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

fn put(values: &mut [u32], i: usize, v: usize) {
    if let Some(slot) = values.get_mut(i) {
        *slot = v as u32;
    }
}

/// The boundary positions, clockwise from the northwest corner.
fn construct_rectangle(ellipsoid: &Ellipsoid, o: &RectangleComputedOptions) -> Vec<f64> {
    let (height, width) = (o.height, o.width);
    let mut row_height = height as isize;
    let mut width_multiplier = 2isize;
    let mut size = 0isize;
    let mut corners = 4isize;
    for cap in [o.north_cap, o.south_cap] {
        if cap {
            width_multiplier -= 1;
            row_height -= 1;
            size += 1;
            corners -= 2;
        }
    }
    size += width_multiplier * width as isize + 2 * row_height - corners;
    let mut positions = vec![0.0; size.max(0) as usize * 3];
    let mut pos_index = 0;
    let mut add = |row: f64, col: f64| {
        let (p, _) = compute_position(o, ellipsoid, false, row, col);
        for v in [p.x, p.y, p.z] {
            if let Some(slot) = positions.get_mut(pos_index) {
                *slot = v;
            }
            pos_index += 1;
        }
    };
    if o.north_cap {
        add(0.0, 0.0);
    } else {
        for col in 0..width {
            add(0.0, col as f64);
        }
    }
    let last_col = width as f64 - 1.0;
    for row in 1..height {
        add(row as f64, last_col);
    }
    let last_row = height as f64 - 1.0;
    if !o.south_cap {
        let mut col = width as isize - 2;
        while col >= 0 {
            add(last_row, col as f64);
            col -= 1;
        }
    }
    let mut row = height as isize - 2;
    while row > 0 {
        add(row as f64, 0.0);
        row -= 1;
    }
    positions
}

fn ring_indices(count: usize) -> Vec<u32> {
    let mut indices = vec![0u32; count * 2];
    let mut index = 0;
    for i in 0..count.saturating_sub(1) {
        put(&mut indices, index, i);
        put(&mut indices, index + 1, i + 1);
        index += 2;
    }
    put(&mut indices, index, count.wrapping_sub(1));
    put(&mut indices, index + 1, 0);
    indices
}

fn extruded_indices(o: &RectangleComputedOptions, total: usize) -> Vec<u32> {
    let mut corners = 4;
    if o.north_cap {
        corners -= 1;
    }
    if o.south_cap {
        corners -= 1;
    }
    let mut indices = vec![0u32; (total + corners) * 2];
    let length = total / 2;
    let mut index = 0;
    let mut push = |indices: &mut Vec<u32>, v: usize| {
        put(indices, index, v);
        index += 1;
    };
    for i in 0..length.saturating_sub(1) {
        for v in [i, i + 1, i + length, i + length + 1] {
            push(&mut indices, v);
        }
    }
    for v in [length.wrapping_sub(1), 0, length + length.wrapping_sub(1), length, 0, length] {
        push(&mut indices, v);
    }
    let (height, width) = (o.height, o.width);
    let bottom_corner = if o.north_cap {
        height.wrapping_sub(1)
    } else {
        let top_right = width.wrapping_sub(1);
        push(&mut indices, top_right);
        push(&mut indices, top_right.wrapping_add(length));
        (width + height).wrapping_sub(2)
    };
    push(&mut indices, bottom_corner);
    push(&mut indices, bottom_corner.wrapping_add(length));
    if !o.south_cap {
        let bottom_left = (width + bottom_corner).wrapping_sub(1);
        push(&mut indices, bottom_left);
        push(&mut indices, bottom_left.wrapping_add(length));
    }
    indices
}

impl RectangleOutlineGeometry {
    pub const PACKED_LENGTH: usize = 4 + Ellipsoid::PACKED_LENGTH + 5;

    pub fn new(options: RectangleOutlineOptions) -> Self {
        let height = options.height;
        let extruded_height = options.extruded_height.unwrap_or(height);
        Self {
            rectangle: options.rectangle,
            granularity: options.granularity,
            ellipsoid: options.ellipsoid,
            surface_height: js_max(height, extruded_height),
            rotation: options.rotation,
            extruded_height: js_min(height, extruded_height),
            offset_attribute: options.offset_attribute,
        }
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        self.rectangle.pack(array, i);
        i += 4;
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let values = [
            self.granularity,
            self.surface_height,
            self.rotation,
            self.extruded_height,
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ];
        array[i..i + 5].copy_from_slice(&values);
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let rectangle = Rectangle::unpack(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i + 4);
        let o = i + 4 + Ellipsoid::PACKED_LENGTH;
        let a = &array[o..o + 5];
        Self::new(RectangleOutlineOptions {
            rectangle,
            granularity: a[0],
            ellipsoid,
            rotation: a[2],
            height: a[1],
            extruded_height: Some(a[3]),
            offset_attribute: GeometryOffsetAttribute::unpack(a[4]),
        })
    }

    /// The outline; `None` for a rectangle with no area.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let r = self.rectangle;
        let ellipsoid = &self.ellipsoid;
        let o = compute_options(&r, self.granularity, self.rotation, 0.0);
        if equals_epsilon(r.north, r.south, EPSILON10, EPSILON10) || equals_epsilon(r.east, r.west, EPSILON10, EPSILON10) {
            return None;
        }
        let extrude = !equals_epsilon(self.surface_height, self.extruded_height, 0.0, EPSILON2);
        let mut attributes = GeometryAttributes::default();
        let (indices, count, bounding_sphere) = if extrude {
            // Cesium scales the same array twice: up to the top, then from
            // there to the surface and down to the bottom.
            let mut top = construct_rectangle(ellipsoid, &o);
            scale_to_geodetic_height(&mut top, Some(self.surface_height), ellipsoid, Some(false))?;
            let mut bottom = top.clone();
            scale_to_geodetic_height(&mut bottom, Some(self.extruded_height), ellipsoid, None)?;
            let positions = [top.as_slice(), &bottom].concat();
            let total = positions.len() / 3;
            if let Some(offset) = self.offset_attribute {
                let mut values = vec![0u8; total];
                match offset {
                    GeometryOffsetAttribute::Top => values[..total / 2].fill(1),
                    GeometryOffsetAttribute::None => {}
                    GeometryOffsetAttribute::All => values.fill(1),
                }
                attributes.set_other(
                    "applyOffset",
                    GeometryAttribute::new(
                        super::component_datatype::ComponentDatatype::UnsignedByte,
                        1,
                        super::typed_array::TypedArray::Uint8(values),
                    ),
                );
            }
            attributes.position = Some(GeometryAttribute::doubles(3, positions));
            let top_bs = BoundingSphere::from_rectangle_3d(&r, ellipsoid, self.surface_height);
            let bottom_bs = BoundingSphere::from_rectangle_3d(&r, ellipsoid, self.extruded_height);
            (extruded_indices(&o, total), total, top_bs.union(&bottom_bs))
        } else {
            let mut positions = construct_rectangle(ellipsoid, &o);
            scale_to_geodetic_height(&mut positions, Some(self.surface_height), ellipsoid, Some(false))?;
            let count = positions.len() / 3;
            if let Some(offset) = self.offset_attribute {
                attributes.set_other("applyOffset", GeometryAttribute::apply_offset(count, offset));
            }
            attributes.position = Some(GeometryAttribute::doubles(3, positions));
            (ring_indices(count), count, BoundingSphere::from_rectangle_3d(&r, ellipsoid, self.surface_height))
        };
        let mut geometry =
            Geometry::new(attributes, Some(IndexDatatype::create_typed_array(count, &indices)), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere);
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
