//! CesiumJS `PolygonOutlineGeometry`: the rings of a polygon with holes, or
//! the edges of the volume it extrudes.

use super::arc_type::ArcType;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_tangent_plane::EllipsoidTangentPlane;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::geometry_instance::GeometryInstance;
use super::geometry_pipeline::combine_instances;
use super::index_datatype::IndexDatatype;
use super::math::{chord_length, equals_epsilon, js_max, js_min, EPSILON2, RADIANS_PER_DEGREE};
use super::polygon_geometry_library::{
    compute_hierarchy_packed_length, pack_polygon_hierarchy, polygon_outlines_from_hierarchy, scale_to_geodetic_height_extruded,
    subdivide_line, subdivide_line_count, subdivide_rhumb_line, subdivide_rhumb_line_count, unpack_polygon_hierarchy,
};
use super::polygon_hierarchy::PolygonHierarchy;
use super::polygon_pipeline::{compute_winding_order_2d, scale_to_geodetic_height};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;
use super::winding_order::WindingOrder;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct PolygonOutlineOptions {
    pub polygon_hierarchy: PolygonHierarchy,
    pub height: f64,
    /// Defaults to `height`.
    pub extruded_height: Option<f64>,
    pub ellipsoid: Ellipsoid,
    pub granularity: f64,
    pub per_position_height: bool,
    pub arc_type: ArcType,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl PolygonOutlineOptions {
    pub fn new(polygon_hierarchy: PolygonHierarchy) -> Self {
        Self {
            polygon_hierarchy,
            height: 0.0,
            extruded_height: None,
            ellipsoid: Ellipsoid::WGS84,
            granularity: RADIANS_PER_DEGREE,
            per_position_height: false,
            arc_type: ArcType::Geodesic,
            offset_attribute: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolygonOutlineGeometry {
    pub polygon_hierarchy: PolygonHierarchy,
    pub ellipsoid: Ellipsoid,
    pub granularity: f64,
    pub height: f64,
    pub extruded_height: f64,
    pub arc_type: ArcType,
    pub per_position_height: bool,
    pub per_position_height_extrude: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub packed_length: usize,
}

/// The ring counter-clockwise in its tangent plane.
fn counter_clockwise(positions: &[Cartesian3], ellipsoid: &Ellipsoid) -> Option<Vec<Cartesian3>> {
    let plane = EllipsoidTangentPlane::from_points(positions, ellipsoid)?;
    let mut positions = positions.to_vec();
    if compute_winding_order_2d(&plane.project_points_onto_plane(&positions)) == WindingOrder::Clockwise {
        positions.reverse();
    }
    Some(positions)
}

/// The ring's edges, subdivided, with the index of each corner.
fn subdivide_ring(
    ellipsoid: &Ellipsoid,
    positions: &[Cartesian3],
    min_distance: f64,
    per_position_height: bool,
    arc_type: ArcType,
) -> Option<(Vec<f64>, Vec<usize>)> {
    let length = positions.len();
    let mut subdivided = Vec::new();
    let mut corners = Vec::with_capacity(length);
    for i in 0..length {
        corners.push(subdivided.len() / 3);
        let (p0, p1) = (positions[i], positions[(i + 1) % length]);
        if per_position_height {
            subdivided.extend([p0.x, p0.y, p0.z, p1.x, p1.y, p1.z]);
            continue;
        }
        match arc_type {
            ArcType::Geodesic => subdivided.extend(subdivide_line(p0, p1, min_distance)),
            ArcType::Rhumb => subdivided.extend(subdivide_rhumb_line(ellipsoid, p0, p1, min_distance)?),
            // Cesium reads an undefined array here.
            ArcType::None => return None,
        }
    }
    if !per_position_height {
        // Cesium sizes the array from the counts; check they agree.
        let mut num_vertices = 0;
        for i in 0..length {
            let (p0, p1) = (positions[i], positions[(i + 1) % length]);
            num_vertices += match arc_type {
                ArcType::Geodesic => subdivide_line_count(p0, p1, min_distance),
                _ => subdivide_rhumb_line_count(ellipsoid, p0, p1, min_distance)?,
            };
        }
        subdivided.resize(num_vertices * 3, 0.0);
    }
    Some((subdivided, corners))
}

fn positions_attribute(values: Vec<f64>) -> GeometryAttributes {
    GeometryAttributes { position: Some(GeometryAttribute::doubles(3, values)), ..Default::default() }
}

fn put(indices: &mut [u32], i: usize, v: usize) {
    if let Some(slot) = indices.get_mut(i) {
        *slot = v as u32;
    }
}

fn ring_geometry(ellipsoid: &Ellipsoid, positions: &[Cartesian3], min_distance: f64, per_position_height: bool, arc_type: ArcType) -> Option<Geometry> {
    let positions = counter_clockwise(positions, ellipsoid)?;
    let (subdivided, _) = subdivide_ring(ellipsoid, &positions, min_distance, per_position_height, arc_type)?;
    let length = subdivided.len() / 3;
    let mut indices = vec![0u32; length * 2];
    let mut index = 0;
    for i in 0..length.saturating_sub(1) {
        put(&mut indices, index, i);
        put(&mut indices, index + 1, i + 1);
        index += 2;
    }
    put(&mut indices, index, length.wrapping_sub(1));
    put(&mut indices, index + 1, 0);
    Some(Geometry::new(
        positions_attribute(subdivided),
        Some(IndexDatatype::create_typed_array(length, &indices)),
        PrimitiveType::Lines,
    ))
}

fn extruded_ring_geometry(
    ellipsoid: &Ellipsoid,
    positions: &[Cartesian3],
    min_distance: f64,
    per_position_height: bool,
    arc_type: ArcType,
) -> Option<Geometry> {
    let positions = counter_clockwise(positions, ellipsoid)?;
    let (mut subdivided, corners) = subdivide_ring(ellipsoid, &positions, min_distance, per_position_height, arc_type)?;
    // The bottom half is filled in when the heights are applied.
    let top = subdivided.len();
    subdivided.resize(top * 2, 0.0);
    let length = subdivided.len() / 6;
    let mut indices = vec![0u32; (length * 2 + corners.len()) * 2];
    let mut index = 0;
    for i in 0..length {
        for v in [i, (i + 1) % length, i + length, ((i + 1) % length) + length] {
            put(&mut indices, index, v);
            index += 1;
        }
    }
    for &corner in &corners {
        put(&mut indices, index, corner);
        put(&mut indices, index + 1, corner + length);
        index += 2;
    }
    Some(Geometry::new(
        positions_attribute(subdivided),
        Some(IndexDatatype::create_typed_array(length + corners.len(), &indices)),
        PrimitiveType::Lines,
    ))
}

impl PolygonOutlineGeometry {
    pub fn new(options: PolygonOutlineOptions) -> Self {
        let per_position_height_extrude = options.per_position_height && options.extruded_height.is_some();
        let mut height = options.height;
        let mut extruded_height = options.extruded_height.unwrap_or(height);
        if !per_position_height_extrude {
            let h = js_max(height, extruded_height);
            extruded_height = js_min(height, extruded_height);
            height = h;
        }
        let packed_length = compute_hierarchy_packed_length(&options.polygon_hierarchy) + Ellipsoid::PACKED_LENGTH + 8;
        Self {
            polygon_hierarchy: options.polygon_hierarchy,
            ellipsoid: options.ellipsoid,
            granularity: options.granularity,
            height,
            extruded_height,
            arc_type: options.arc_type,
            per_position_height: options.per_position_height,
            per_position_height_extrude,
            offset_attribute: options.offset_attribute,
            packed_length,
        }
    }

    /// An outline without holes.
    pub fn from_positions(positions: Vec<Cartesian3>, options: PolygonOutlineOptions) -> Self {
        Self::new(PolygonOutlineOptions { polygon_hierarchy: PolygonHierarchy::new(positions, Vec::new()), ..options })
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        let mut i = pack_polygon_hierarchy(&self.polygon_hierarchy, array, starting_index);
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let values = [
            self.height,
            self.extruded_height,
            self.granularity,
            flag(self.per_position_height_extrude),
            flag(self.per_position_height),
            self.arc_type as u32 as f64,
            GeometryOffsetAttribute::pack(self.offset_attribute),
            self.packed_length as f64,
        ];
        array[i..i + 8].copy_from_slice(&values);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let (polygon_hierarchy, mut i) = unpack_polygon_hierarchy::<PolygonHierarchy>(array, starting_index);
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let a = &array[i..i + 8];
        Self {
            polygon_hierarchy,
            ellipsoid,
            granularity: a[2],
            height: a[0],
            extruded_height: a[1],
            arc_type: match a[5] as i32 {
                0 => ArcType::None,
                2 => ArcType::Rhumb,
                _ => ArcType::Geodesic,
            },
            per_position_height: a[4] == 1.0,
            per_position_height_extrude: a[3] == 1.0,
            offset_attribute: GeometryOffsetAttribute::unpack(a[6]),
            packed_length: a[7] as usize,
        }
    }

    /// The outline; `None` when no ring has three distinct positions, or for
    /// an arc type of `None` (Cesium throws).
    pub fn create_geometry(&self) -> Option<Geometry> {
        let ellipsoid = &self.ellipsoid;
        let per_position_height = self.per_position_height;
        let mut hierarchy = self.polygon_hierarchy.clone();
        let polygons = polygon_outlines_from_hierarchy(&mut hierarchy, !per_position_height, ellipsoid);
        if polygons.is_empty() {
            return None;
        }
        let min_distance = chord_length(self.granularity, ellipsoid.maximum_radius);
        let (height, extruded_height) = (self.height, self.extruded_height);
        let extrude = self.per_position_height_extrude || !equals_epsilon(height, extruded_height, 0.0, EPSILON2);
        let mut geometries = Vec::new();
        for ring in &polygons {
            let geometry = if extrude {
                let mut g = extruded_ring_geometry(ellipsoid, ring, min_distance, per_position_height, self.arc_type)?;
                scale_to_geodetic_height_extruded(&mut g, height, extruded_height, ellipsoid, per_position_height)?;
                if let Some(offset) = self.offset_attribute {
                    let size = g.attributes.position.as_ref()?.values.len() / 3;
                    let mut values = vec![0u8; size];
                    match offset {
                        GeometryOffsetAttribute::Top => values[..size / 2].fill(1),
                        GeometryOffsetAttribute::None => {}
                        GeometryOffsetAttribute::All => values.fill(1),
                    }
                    g.attributes.set_other(
                        "applyOffset",
                        GeometryAttribute::new(ComponentDatatype::UnsignedByte, 1, TypedArray::Uint8(values)),
                    );
                }
                g
            } else {
                let mut g = ring_geometry(ellipsoid, ring, min_distance, per_position_height, self.arc_type)?;
                let position = g.attributes.position.as_mut()?;
                let mut values = position.values.to_f64_vec();
                scale_to_geodetic_height(&mut values, Some(height), ellipsoid, Some(!per_position_height))?;
                let count = values.len() / 3;
                position.values = TypedArray::Float64(values);
                if let Some(offset) = self.offset_attribute {
                    g.attributes.set_other("applyOffset", GeometryAttribute::apply_offset(count, offset));
                }
                g
            };
            geometries.push(GeometryInstance::new(geometry, None));
        }
        let geometry = combine_instances(&geometries)?.into_iter().next()?;
        let positions = geometry.attributes.position.as_ref()?.values.to_f64_vec();
        let mut result = Geometry::new(geometry.attributes, geometry.indices, geometry.primitive_type);
        result.bounding_sphere = Some(BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3));
        result.offset_attribute = self.offset_attribute;
        Some(result)
    }
}
