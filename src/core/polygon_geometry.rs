//! CesiumJS `PolygonGeometry`: a polygon with holes on the ellipsoid,
//! optionally extruded, with per-position heights, texture coordinates and
//! normal frames.
//!
//! Cesium scales the hierarchy's positions to the surface in place when it
//! builds the geometry; `create_geometry` works on a copy, so the stored
//! hierarchy is unchanged.

use super::arc_type::ArcType;
use super::bounding_rectangle::BoundingRectangle;
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_tangent_plane::EllipsoidTangentPlane;
use super::geometry::{Geometry, GeometryAttribute, GeometryOffsetAttribute};
use super::geometry_instance::GeometryInstance;
use super::geometry_pipeline::combine_instances;
use super::index_datatype::IndexDatatype;
use super::math::{clamp, equals_epsilon, js_max, js_min, sign, EPSILON10, EPSILON2, PI, PI_OVER_THREE, PI_OVER_TWO, RADIANS_PER_DEGREE, TWO_PI};
use super::matrix3::Matrix3;
use super::polygon_geometry_library::{
    compute_bounding_rectangle, compute_hierarchy_packed_length, compute_wall_geometry, create_geometry_from_positions,
    pack_polygon_hierarchy, polygons_from_hierarchy, scale_to_geodetic_height_extruded, texture_polygons_from_hierarchy,
    unpack_polygon_hierarchy, PolygonPositions, PolygonRings, SplitPolygons, TextureCoordinateHierarchy,
};
use super::polygon_hierarchy::PolygonHierarchy;
use super::polygon_pipeline::{compute_winding_order_2d, scale_to_geodetic_height};
use super::quaternion::Quaternion;
use super::rectangle::Rectangle;
use super::stereographic::Stereographic;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;
use super::winding_order::WindingOrder;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct PolygonGeometryOptions {
    pub polygon_hierarchy: PolygonHierarchy,
    pub height: f64,
    /// Defaults to `height`.
    pub extruded_height: Option<f64>,
    pub vertex_format: VertexFormat,
    pub st_rotation: f64,
    pub ellipsoid: Ellipsoid,
    pub granularity: f64,
    pub per_position_height: bool,
    pub close_top: bool,
    pub close_bottom: bool,
    pub arc_type: ArcType,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub texture_coordinates: Option<TextureCoordinateHierarchy>,
}

impl PolygonGeometryOptions {
    pub fn new(polygon_hierarchy: PolygonHierarchy) -> Self {
        Self {
            polygon_hierarchy,
            height: 0.0,
            extruded_height: None,
            vertex_format: VertexFormat::DEFAULT,
            st_rotation: 0.0,
            ellipsoid: Ellipsoid::WGS84,
            granularity: RADIANS_PER_DEGREE,
            per_position_height: false,
            close_top: true,
            close_bottom: true,
            arc_type: ArcType::Geodesic,
            shadow_volume: false,
            offset_attribute: None,
            texture_coordinates: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolygonGeometry {
    pub polygon_hierarchy: PolygonHierarchy,
    pub vertex_format: VertexFormat,
    pub ellipsoid: Ellipsoid,
    pub granularity: f64,
    pub st_rotation: f64,
    pub height: f64,
    pub extruded_height: f64,
    pub close_top: bool,
    pub close_bottom: bool,
    pub per_position_height: bool,
    pub per_position_height_extrude: bool,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub arc_type: ArcType,
    pub texture_coordinates: Option<TextureCoordinateHierarchy>,
    pub packed_length: usize,
}

fn arc_type_from(value: f64) -> ArcType {
    match value as i32 {
        0 => ArcType::None,
        2 => ArcType::Rhumb,
        _ => ArcType::Geodesic,
    }
}

/// Settings for `computeAttributes`.
struct AttributeOptions<'a> {
    vertex_format: VertexFormat,
    shadow_volume: bool,
    bounding_rectangle: BoundingRectangle,
    rotation_axis: Cartesian3,
    project_to_2d: &'a dyn Fn(Cartesian3) -> Option<Cartesian2>,
    ellipsoid: Ellipsoid,
    st_rotation: f64,
    per_position_height: bool,
    top: bool,
    bottom: bool,
    wall: bool,
    extrude: bool,
    offset_attribute: Option<GeometryOffsetAttribute>,
}

fn c3(values: &[f64], i: usize) -> Cartesian3 {
    let at = |k: usize| values.get(k).copied().unwrap_or(f64::NAN);
    Cartesian3::new(at(i), at(i + 1), at(i + 2))
}

fn put3(values: &mut [f32], i: usize, v: Cartesian3) {
    for (k, x) in [v.x, v.y, v.z].into_iter().enumerate() {
        if let Some(slot) = values.get_mut(i + k) {
            *slot = x as f32;
        }
    }
}

fn adjust_pos_heights_for_normal(position: Cartesian3, p1: &mut Cartesian3, p2: &mut Cartesian3, ellipsoid: &Ellipsoid) -> Option<()> {
    let height = ellipsoid.cartesian_to_cartographic(position)?.height;
    let mut c1 = ellipsoid.cartesian_to_cartographic(*p1)?;
    c1.height = height;
    *p1 = ellipsoid.cartographic_to_cartesian(&c1);
    let mut c2 = ellipsoid.cartesian_to_cartographic(*p2)?;
    c2.height = height - 100.0;
    *p2 = ellipsoid.cartographic_to_cartesian(&c2);
    Some(())
}

fn floats(components: u8, values: Vec<f32>) -> GeometryAttribute {
    GeometryAttribute::new(ComponentDatatype::Float, components, TypedArray::Float32(values))
}

/// The per-position tangent frame Cesium keeps in module scratch. Walls
/// never recompute it, so their tops reuse the last value the top surface
/// left there. Cesium's value also carries over between calls; here it
/// starts at zero for each `create_geometry`.
#[derive(Default)]
struct PerPositionFrame {
    tangent: Cartesian3,
    bitangent: Cartesian3,
}

/// Cesium's `computeAttributes`: texture coordinates, normals, tangents,
/// bitangents, extrusion directions and offsets for a top, bottom or wall.
fn compute_attributes(o: &AttributeOptions, geometry: &mut Geometry, frame: &mut PerPositionFrame) -> Option<()> {
    let vf = o.vertex_format;
    let ellipsoid = &o.ellipsoid;
    let flat_positions = geometry.attributes.position.as_ref()?.values.to_f64_vec();
    let has_texcoords = geometry.attributes.st.is_some();
    let mut length = flat_positions.len();
    let wall = o.wall;
    let top = o.top || wall;
    let bottom = o.bottom || wall;

    if vf.st || vf.normal || vf.tangent || vf.bitangent || o.shadow_volume {
        let br = o.bounding_rectangle;
        let origin = Cartesian2::new(br.x, br.y);
        let mut texture_coordinates = vf.st.then(|| vec![0f32; 2 * (length / 3)]);
        let mut normals: Option<Vec<f32>> = if vf.normal {
            if o.per_position_height && top && !wall {
                let n = geometry.attributes.normal.as_ref()?.values.to_f64_vec();
                Some(n.into_iter().map(|v| v as f32).collect())
            } else {
                Some(vec![0f32; length])
            }
        } else {
            None
        };
        let mut tangents = vf.tangent.then(|| vec![0f32; length]);
        let mut bitangents = vf.bitangent.then(|| vec![0f32; length]);
        let mut extrude_normals = o.shadow_volume.then(|| vec![0f32; length]);

        let (texture_matrix, tangent_rotation) = if o.st_rotation != 0.0 {
            (
                Matrix3::from_quaternion(&Quaternion::from_axis_angle(o.rotation_axis, o.st_rotation)),
                Matrix3::from_quaternion(&Quaternion::from_axis_angle(o.rotation_axis, -o.st_rotation)),
            )
        } else {
            (Matrix3::IDENTITY, Matrix3::IDENTITY)
        };
        let (mut bottom_offset, mut bottom_offset2) = (0, 0);
        if top && bottom {
            bottom_offset = length / 2;
            bottom_offset2 = length / 3;
            length /= 2;
        }
        let (mut normal, mut tangent, mut bitangent) = (Cartesian3::ZERO, Cartesian3::ZERO, Cartesian3::ZERO);
        let mut recompute_normal = true;
        let mut tex_index = 0;
        let mut attr_index = 0;
        for i in (0..length).step_by(3) {
            let position = c3(&flat_positions, i);
            if let (Some(st), false) = (texture_coordinates.as_mut(), has_texcoords) {
                let p = ellipsoid.scale_to_geodetic_surface(texture_matrix.multiply_by_vector(position))?;
                let s = (o.project_to_2d)(p)?.subtract(origin);
                let stx = clamp(s.x / br.width, 0.0, 1.0) as f32;
                let sty = clamp(s.y / br.height, 0.0, 1.0) as f32;
                if bottom {
                    st[tex_index + bottom_offset2] = stx;
                    st[tex_index + 1 + bottom_offset2] = sty;
                }
                if top {
                    st[tex_index] = stx;
                    st[tex_index + 1] = sty;
                }
                tex_index += 2;
            }
            if !(vf.normal || vf.tangent || vf.bitangent || o.shadow_volume) {
                continue;
            }
            if wall {
                if i + 3 < length {
                    let mut p1 = c3(&flat_positions, i + 3);
                    if recompute_normal {
                        let mut p2 = c3(&flat_positions, i + length);
                        if o.per_position_height {
                            adjust_pos_heights_for_normal(position, &mut p1, &mut p2, ellipsoid)?;
                        }
                        p1 = p1.subtract(position);
                        p2 = p2.subtract(position);
                        normal = p2.cross(p1).normalize();
                        recompute_normal = false;
                    }
                    // Cesium compares the edge vector after a recompute,
                    // and the next position otherwise.
                    if p1.equals_epsilon(position, EPSILON10) {
                        recompute_normal = true;
                    }
                }
                if vf.tangent || vf.bitangent {
                    bitangent = ellipsoid.geodetic_surface_normal(position)?;
                    if vf.tangent {
                        tangent = bitangent.cross(normal).normalize();
                    }
                }
            } else {
                normal = ellipsoid.geodetic_surface_normal(position)?;
                if vf.tangent || vf.bitangent {
                    if o.per_position_height {
                        // Cesium reads per-position normals it may not have.
                        let n = normals.as_ref()?;
                        let pp_normal = Cartesian3::new(
                            f64::from(n[attr_index]),
                            f64::from(n[attr_index + 1]),
                            f64::from(n[attr_index + 2]),
                        );
                        frame.tangent = tangent_rotation.multiply_by_vector(Cartesian3::UNIT_Z.cross(pp_normal)).normalize();
                        if vf.bitangent {
                            frame.bitangent = pp_normal.cross(frame.tangent).normalize();
                        }
                    }
                    tangent = tangent_rotation.multiply_by_vector(Cartesian3::UNIT_Z.cross(normal)).normalize();
                    if vf.bitangent {
                        bitangent = normal.cross(tangent).normalize();
                    }
                }
            }
            if let Some(n) = normals.as_mut() {
                if o.wall {
                    put3(n, attr_index + bottom_offset, normal);
                } else if bottom {
                    put3(n, attr_index + bottom_offset, normal.negate());
                }
                if (top && !o.per_position_height) || wall {
                    put3(n, attr_index, normal);
                }
            }
            if let Some(e) = extrude_normals.as_mut() {
                if wall {
                    normal = ellipsoid.geodetic_surface_normal(position)?;
                }
                put3(e, attr_index + bottom_offset, normal.negate());
            }
            if let Some(t) = tangents.as_mut() {
                if o.wall {
                    put3(t, attr_index + bottom_offset, tangent);
                } else if bottom {
                    put3(t, attr_index + bottom_offset, tangent.negate());
                }
                if top {
                    put3(t, attr_index, if o.per_position_height { frame.tangent } else { tangent });
                }
            }
            if let Some(b) = bitangents.as_mut() {
                if bottom {
                    put3(b, attr_index + bottom_offset, bitangent);
                }
                if top {
                    put3(b, attr_index, if o.per_position_height { frame.bitangent } else { bitangent });
                }
            }
            attr_index += 3;
        }
        let attributes = &mut geometry.attributes;
        if let (Some(st), false) = (texture_coordinates, has_texcoords) {
            attributes.st = Some(floats(2, st));
        }
        if let Some(n) = normals {
            attributes.normal = Some(floats(3, n));
        }
        if let Some(t) = tangents {
            attributes.tangent = Some(floats(3, t));
        }
        if let Some(b) = bitangents {
            attributes.bitangent = Some(floats(3, b));
        }
        if let Some(e) = extrude_normals {
            attributes.set_other("extrudeDirection", floats(3, e));
        }
    }
    if let (true, Some(offset)) = (o.extrude, o.offset_attribute) {
        let size = flat_positions.len() / 3;
        let mut values = vec![0u8; size];
        match offset {
            GeometryOffsetAttribute::Top => {
                if (top && bottom) || wall {
                    values[..size / 2].fill(1);
                } else if top {
                    values.fill(1);
                }
            }
            GeometryOffsetAttribute::None => {}
            GeometryOffsetAttribute::All => values.fill(1),
        }
        geometry.attributes.set_other(
            "applyOffset",
            GeometryAttribute::new(ComponentDatatype::UnsignedByte, 1, TypedArray::Uint8(values)),
        );
    }
    Some(())
}

/// Moves a geometry's positions to `height` with `PolygonPipeline`.
fn scale_positions(geometry: &mut Geometry, height: f64, ellipsoid: &Ellipsoid, to_surface: bool) -> Option<()> {
    let position = geometry.attributes.position.as_mut()?;
    let mut values = position.values.to_f64_vec();
    scale_to_geodetic_height(&mut values, Some(height), ellipsoid, Some(to_surface))?;
    position.values = TypedArray::Float64(values);
    Some(())
}

fn reversed_triangles(indices: &[f64]) -> Vec<u32> {
    indices.chunks(3).flat_map(|t| [t.get(2), t.get(1), t.first()].map(|v| v.map_or(0, |&v| v as u32))).collect()
}

/// The (unscaled) top and bottom, and the walls, of one extruded polygon.
#[allow(clippy::too_many_arguments)]
fn create_geometry_from_positions_extruded(
    ellipsoid: &Ellipsoid,
    polygon: &PolygonPositions,
    texture_coordinates: Option<&[Cartesian2]>,
    granularity: f64,
    hierarchy: &PolygonRings,
    per_position_height: bool,
    close_top: bool,
    close_bottom: bool,
    vertex_format: VertexFormat,
    arc_type: ArcType,
) -> Option<(Option<Geometry>, Vec<Geometry>)> {
    let mut top_and_bottom = None;
    if close_top || close_bottom {
        let mut top = create_geometry_from_positions(
            ellipsoid,
            polygon,
            texture_coordinates,
            granularity,
            per_position_height,
            vertex_format.normal,
            arc_type,
        )?;
        let edge_points = top.attributes.position.as_ref()?.values.to_f64_vec();
        let indices = top.indices.as_ref()?.to_f64_vec();
        if close_top && close_bottom {
            let top_bottom = [edge_points.as_slice(), &edge_points].concat();
            let num_positions = top_bottom.len() / 3;
            let half = (num_positions / 2) as u32;
            let mut new_indices: Vec<u32> = indices.iter().map(|&i| i as u32).collect();
            new_indices.extend(reversed_triangles(&indices).into_iter().map(|i| i + half));
            top.attributes.position.as_mut()?.values = TypedArray::Float64(top_bottom);
            if per_position_height && vertex_format.normal {
                let normals = top.attributes.normal.as_ref()?.values.to_f64_vec();
                let mut doubled = vec![0f32; num_positions * 3];
                for (slot, v) in doubled.iter_mut().zip(normals) {
                    *slot = v as f32;
                }
                top.attributes.normal.as_mut()?.values = TypedArray::Float32(doubled);
            }
            if vertex_format.st && texture_coordinates.is_some() {
                let st = top.attributes.st.as_mut()?;
                let values = st.values.to_f64_vec();
                st.values = TypedArray::Float64([values.as_slice(), &values].concat());
            }
            top.indices = Some(IndexDatatype::create_typed_array(num_positions, &new_indices));
        } else if close_bottom {
            let num_positions = edge_points.len() / 3;
            top.indices = Some(IndexDatatype::create_typed_array(num_positions, &reversed_triangles(&indices)));
        }
        top_and_bottom = Some(top);
    }

    let tangent_plane = EllipsoidTangentPlane::from_points(&hierarchy.outer_ring, ellipsoid)?;
    let mut walls = Vec::new();
    let mut outer_ring = hierarchy.outer_ring.clone();
    if compute_winding_order_2d(&tangent_plane.project_points_onto_plane(&outer_ring)) == WindingOrder::Clockwise {
        outer_ring.reverse();
    }
    walls.push(compute_wall_geometry(&outer_ring, texture_coordinates, ellipsoid, granularity, per_position_height, arc_type)?);
    for hole in &hierarchy.holes {
        let mut hole = hole.clone();
        if compute_winding_order_2d(&tangent_plane.project_points_onto_plane(&hole)) == WindingOrder::CounterClockwise {
            hole.reverse();
        }
        walls.push(compute_wall_geometry(&hole, texture_coordinates, ellipsoid, granularity, per_position_height, arc_type)?);
    }
    Some((top_and_bottom, walls))
}

/// Cesium's `expandRectangle` state.
struct PolarBounds {
    north_angle: f64,
    south_angle: f64,
    west_over_idl: f64,
    east_over_idl: f64,
}

fn expand_rectangle(
    polar: &Stereographic,
    last_polar: &Stereographic,
    ellipsoid: &Ellipsoid,
    arc_type: ArcType,
    polygon: &mut PolarBounds,
    result: &mut Rectangle,
) {
    let longitude = polar.longitude();
    let lon_adjusted = if longitude >= 0.0 { longitude } else { longitude + TWO_PI };
    polygon.west_over_idl = js_min(polygon.west_over_idl, lon_adjusted);
    polygon.east_over_idl = js_max(polygon.east_over_idl, lon_adjusted);
    result.west = js_min(result.west, longitude);
    result.east = js_max(result.east, longitude);
    let latitude = polar.get_latitude(ellipsoid).unwrap_or(f64::NAN);
    let mut segment_latitude = latitude;
    result.south = js_min(result.south, latitude);
    result.north = js_max(result.north, latitude);
    if arc_type != ArcType::Rhumb {
        let segment = last_polar.position.subtract(polar.position);
        let t = last_polar.position.dot(segment) / segment.dot(segment);
        if t > 0.0 && t < 1.0 {
            let projected = last_polar.position.add(segment.multiply_by_scalar(-t));
            let closest = Stereographic { position: projected, ..*last_polar };
            let adjusted = closest.get_latitude(ellipsoid).unwrap_or(f64::NAN);
            result.south = js_min(result.south, adjusted);
            result.north = js_max(result.north, adjusted);
            if latitude.abs() > adjusted.abs() {
                segment_latitude = adjusted;
            }
        }
    }
    let direction = last_polar.x() * polar.y() - polar.x() * last_polar.y();
    let mut angle = sign(direction);
    if angle != 0.0 {
        angle *= last_polar.position.angle_between(polar.position);
    }
    if segment_latitude >= 0.0 {
        polygon.north_angle += angle;
    }
    if segment_latitude <= 0.0 {
        polygon.south_angle += angle;
    }
}

fn is_large(rectangle: &Rectangle) -> bool {
    rectangle.height() >= PI || rectangle.width() >= PI
}

/// Equirectangular coordinates for polygons spanning the equator and more
/// than half the globe.
fn cylindrical(ellipsoid: &Ellipsoid, p: Cartesian3) -> Cartesian2 {
    let c = ellipsoid.cartesian_to_cartographic(p).unwrap_or(super::cartographic::Cartographic::new(f64::NAN, f64::NAN, f64::NAN));
    Cartesian2::new(c.longitude / PI, c.latitude / PI_OVER_TWO)
}

impl PolygonGeometry {
    pub fn new(options: PolygonGeometryOptions) -> Self {
        let per_position_height_extrude = options.per_position_height && options.extruded_height.is_some();
        let mut height = options.height;
        let mut extruded_height = options.extruded_height.unwrap_or(height);
        if !per_position_height_extrude {
            let h = js_max(height, extruded_height);
            extruded_height = js_min(height, extruded_height);
            height = h;
        }
        let packed_length = compute_hierarchy_packed_length(&options.polygon_hierarchy)
            + Ellipsoid::PACKED_LENGTH
            + VertexFormat::PACKED_LENGTH
            + options.texture_coordinates.as_ref().map_or(1, compute_hierarchy_packed_length)
            + 12;
        Self {
            polygon_hierarchy: options.polygon_hierarchy,
            vertex_format: options.vertex_format,
            ellipsoid: options.ellipsoid,
            granularity: options.granularity,
            st_rotation: options.st_rotation,
            height,
            extruded_height,
            close_top: options.close_top,
            close_bottom: options.close_bottom,
            per_position_height: options.per_position_height,
            per_position_height_extrude,
            shadow_volume: options.shadow_volume,
            offset_attribute: options.offset_attribute,
            arc_type: options.arc_type,
            texture_coordinates: options.texture_coordinates,
            packed_length,
        }
    }

    /// A polygon without holes.
    pub fn from_positions(positions: Vec<Cartesian3>, options: PolygonGeometryOptions) -> Self {
        Self::new(PolygonGeometryOptions { polygon_hierarchy: PolygonHierarchy::new(positions, Vec::new()), ..options })
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        let mut i = pack_polygon_hierarchy(&self.polygon_hierarchy, array, starting_index);
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let values = [
            self.height,
            self.extruded_height,
            self.granularity,
            self.st_rotation,
            flag(self.per_position_height_extrude),
            flag(self.per_position_height),
            flag(self.close_top),
            flag(self.close_bottom),
            flag(self.shadow_volume),
            GeometryOffsetAttribute::pack(self.offset_attribute),
            self.arc_type as u32 as f64,
        ];
        array[i..i + 11].copy_from_slice(&values);
        i += 11;
        match &self.texture_coordinates {
            Some(t) => i = pack_polygon_hierarchy(t, array, i),
            None => {
                array[i] = -1.0;
                i += 1;
            }
        }
        array[i] = self.packed_length as f64;
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let (polygon_hierarchy, mut i) = unpack_polygon_hierarchy::<PolygonHierarchy>(array, starting_index);
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        let a = &array[i..i + 11];
        i += 11;
        let texture_coordinates = if array[i] == -1.0 {
            i += 1;
            None
        } else {
            let (t, next) = unpack_polygon_hierarchy::<TextureCoordinateHierarchy>(array, i);
            i = next;
            Some(t)
        };
        Self {
            polygon_hierarchy,
            vertex_format,
            ellipsoid,
            granularity: a[2],
            st_rotation: a[3],
            height: a[0],
            extruded_height: a[1],
            close_top: a[6] == 1.0,
            close_bottom: a[7] == 1.0,
            per_position_height: a[5] == 1.0,
            per_position_height_extrude: a[4] == 1.0,
            shadow_volume: a[8] == 1.0,
            offset_attribute: GeometryOffsetAttribute::unpack(a[9]),
            arc_type: arc_type_from(a[10]),
            texture_coordinates,
            packed_length: array[i] as usize,
        }
    }

    /// The cartographic bounds of a ring, following its edges through the
    /// stereographic projection so polar and antimeridian-crossing rings are
    /// bounded correctly.
    pub fn compute_rectangle_from_positions(positions: &[Cartesian3], ellipsoid: &Ellipsoid, arc_type: ArcType) -> Rectangle {
        let mut result = Rectangle::new(0.0, 0.0, 0.0, 0.0);
        if positions.len() < 3 {
            return result;
        }
        result = Rectangle::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut polygon =
            PolarBounds { north_angle: 0.0, south_angle: 0.0, west_over_idl: f64::INFINITY, east_over_idl: f64::NEG_INFINITY };
        let polar_of = |p: Cartesian3| Stereographic::from_cartesian(p).unwrap_or_else(|| Stereographic::new(None, None));
        let mut last = polar_of(positions[0]);
        for p in &positions[1..] {
            let polar = polar_of(*p);
            expand_rectangle(&polar, &last, ellipsoid, arc_type, &mut polygon, &mut result);
            last = polar;
        }
        expand_rectangle(&polar_of(positions[0]), &last, ellipsoid, arc_type, &mut polygon, &mut result);
        if result.east - result.west > polygon.east_over_idl - polygon.west_over_idl {
            result.west = polygon.west_over_idl;
            result.east = polygon.east_over_idl;
            if result.east > PI {
                result.east -= TWO_PI;
            }
            if result.west > PI {
                result.west -= TWO_PI;
            }
        }
        if equals_epsilon(polygon.north_angle.abs(), TWO_PI, EPSILON10, EPSILON10) {
            result.north = PI_OVER_TWO;
            result.east = PI;
            result.west = -PI;
        }
        if equals_epsilon(polygon.south_angle.abs(), TWO_PI, EPSILON10, EPSILON10) {
            result.south = -PI_OVER_TWO;
            result.east = PI;
            result.west = -PI;
        }
        result
    }

    pub fn rectangle(&self) -> Rectangle {
        Self::compute_rectangle_from_positions(&self.polygon_hierarchy.positions, &self.ellipsoid, self.arc_type)
    }

    /// Three corners of the rotated texture space, as six numbers.
    pub fn texture_coordinate_rotation_points(&self) -> [f64; 6] {
        let st_rotation = -self.st_rotation;
        if st_rotation == 0.0 {
            return [0.0, 0.0, 0.0, 1.0, 1.0, 0.0];
        }
        Geometry::texture_coordinate_rotation_points(&self.polygon_hierarchy.positions, st_rotation, &self.ellipsoid, &self.rectangle())
    }

    /// A shadow volume between the heights the two functions return.
    pub fn create_shadow_volume(
        &self,
        min_height: impl Fn(f64, &Ellipsoid) -> f64,
        max_height: impl Fn(f64, &Ellipsoid) -> f64,
    ) -> Self {
        let mut options = PolygonGeometryOptions::new(self.polygon_hierarchy.clone());
        options.ellipsoid = self.ellipsoid;
        options.st_rotation = self.st_rotation;
        options.granularity = self.granularity;
        options.per_position_height = false;
        options.extruded_height = Some(min_height(self.granularity, &self.ellipsoid));
        options.height = max_height(self.granularity, &self.ellipsoid);
        options.vertex_format = VertexFormat::POSITION_ONLY;
        options.shadow_volume = true;
        options.arc_type = self.arc_type;
        Self::new(options)
    }

    /// The polygon's geometry. `None` for fewer than three positions or
    /// nothing left to triangulate, and where Cesium throws: an arc type of
    /// `None`, texture coordinates on a polygon wider or taller than half the
    /// globe, or per-position tangents without normals.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let vf = self.vertex_format;
        let ellipsoid = self.ellipsoid;
        let granularity = self.granularity;
        let per_position_height = self.per_position_height;
        let (close_top, close_bottom) = (self.close_top, self.close_bottom);
        let arc_type = self.arc_type;
        let mut hierarchy_copy = self.polygon_hierarchy.clone();
        if hierarchy_copy.positions.len() < 3 {
            return None;
        }
        let rectangle = self.rectangle();
        let large = is_large(&rectangle);

        // Cesium projects with a plane from the root ring, which by then
        // has been scaled to the surface.
        let mut scaled_root = hierarchy_copy.positions.clone();
        if !per_position_height {
            for p in scaled_root.iter_mut() {
                if let Some(s) = ellipsoid.scale_to_geodetic_surface(*p) {
                    *p = s;
                }
            }
        }
        let root_plane = if large { None } else { EllipsoidTangentPlane::from_points(&scaled_root, &ellipsoid) };
        let project_to_2d = |positions: &[Cartesian3]| -> Option<Vec<Cartesian2>> {
            if large {
                if rectangle.south < 0.0 && rectangle.north > 0.0 {
                    return Some(positions.iter().map(|p| cylindrical(&ellipsoid, *p)).collect());
                }
                return Stereographic::from_cartesian_array(positions).map(|s| s.iter().map(|s| s.position).collect());
            }
            Some(root_plane.as_ref()?.project_points_onto_plane(positions))
        };
        let split = if !per_position_height && (rectangle.height() >= PI_OVER_TWO || rectangle.width() >= 2.0 * PI_OVER_THREE) {
            SplitPolygons::OnEquator { ellipsoid, arc_type }
        } else {
            SplitPolygons::None
        };
        let has_texture_coordinates = self.texture_coordinates.is_some();
        let (hierarchy, polygons) = polygons_from_hierarchy(
            &mut hierarchy_copy,
            has_texture_coordinates,
            &project_to_2d,
            !per_position_height,
            &ellipsoid,
            split,
        );
        let texture_polygons = self.texture_coordinates.as_ref().map(texture_polygons_from_hierarchy);
        if hierarchy.is_empty() {
            return None;
        }
        let outer_ring = &hierarchy[0].outer_ring;

        // Texture coordinates for polygons this large hit a Cesium bug
        // (the projection receives an array) and throw.
        if large && vf.st && !has_texture_coordinates {
            return None;
        }
        let ring_plane = if large { None } else { Some(EllipsoidTangentPlane::from_points(outer_ring, &ellipsoid)?) };
        let bounding_rectangle = match &ring_plane {
            None => BoundingRectangle::from_rectangle(&rectangle, None),
            Some(plane) => compute_bounding_rectangle(
                plane.plane.normal,
                &|p| plane.project_point_onto_plane(p),
                outer_ring,
                self.st_rotation,
            ),
        };
        let rotation_axis = match &ring_plane {
            Some(plane) => plane.plane.normal,
            None => Stereographic::from_cartesian(outer_ring[0])?.tangent_plane.plane.normal,
        };
        let project_position = |p: Cartesian3| -> Option<Cartesian2> { ring_plane.as_ref()?.project_point_onto_plane(p) };

        let height = self.height;
        let extruded_height = self.extruded_height;
        let extrude = self.per_position_height_extrude || !equals_epsilon(height, extruded_height, 0.0, EPSILON2);
        let mut options = AttributeOptions {
            vertex_format: vf,
            shadow_volume: false,
            bounding_rectangle,
            rotation_axis,
            project_to_2d: &project_position,
            ellipsoid,
            st_rotation: self.st_rotation,
            per_position_height,
            top: true,
            bottom: false,
            wall: false,
            extrude: false,
            offset_attribute: None,
        };
        let mut geometries = Vec::new();
        let mut frame = PerPositionFrame::default();
        let texcoords_of = |i: usize| texture_polygons.as_ref().and_then(|t| t.get(i)).map(|t| t.as_slice());
        if extrude {
            options.extrude = true;
            options.top = close_top;
            options.bottom = close_bottom;
            options.shadow_volume = self.shadow_volume;
            options.offset_attribute = self.offset_attribute;
            for (i, polygon) in polygons.iter().enumerate() {
                let (top_and_bottom, walls) = create_geometry_from_positions_extruded(
                    &ellipsoid,
                    polygon,
                    texcoords_of(i),
                    granularity,
                    &hierarchy[i],
                    per_position_height,
                    close_top,
                    close_bottom,
                    vf,
                    arc_type,
                )?;
                if let Some(mut top) = top_and_bottom {
                    if close_top && close_bottom {
                        scale_to_geodetic_height_extruded(&mut top, height, extruded_height, &ellipsoid, per_position_height)?;
                    } else if close_top {
                        scale_positions(&mut top, height, &ellipsoid, !per_position_height)?;
                    } else {
                        scale_positions(&mut top, extruded_height, &ellipsoid, true)?;
                    }
                    options.wall = false;
                    compute_attributes(&options, &mut top, &mut frame)?;
                    geometries.push(GeometryInstance::new(top, None));
                }
                options.wall = true;
                for mut wall in walls {
                    scale_to_geodetic_height_extruded(&mut wall, height, extruded_height, &ellipsoid, per_position_height)?;
                    compute_attributes(&options, &mut wall, &mut frame)?;
                    geometries.push(GeometryInstance::new(wall, None));
                }
            }
        } else {
            for (i, polygon) in polygons.iter().enumerate() {
                let mut geometry = create_geometry_from_positions(
                    &ellipsoid,
                    polygon,
                    texcoords_of(i),
                    granularity,
                    per_position_height,
                    vf.normal,
                    arc_type,
                )?;
                scale_positions(&mut geometry, height, &ellipsoid, !per_position_height)?;
                compute_attributes(&options, &mut geometry, &mut frame)?;
                if let Some(offset) = self.offset_attribute {
                    let count = geometry.attributes.position.as_ref()?.values.len() / 3;
                    geometry.attributes.set_other("applyOffset", GeometryAttribute::apply_offset(count, offset));
                }
                geometries.push(GeometryInstance::new(geometry, None));
            }
        }

        let mut geometry = combine_instances(&geometries)?.into_iter().next()?;
        let positions = geometry.attributes.position.as_ref()?.values.to_f64_vec();
        let indices: Vec<u32> = geometry.indices.as_ref()?.to_f64_vec().iter().map(|&i| i as u32).collect();
        geometry.indices = Some(IndexDatatype::create_typed_array(positions.len() / 3, &indices));
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        geometry.attributes.position.as_mut()?.values = TypedArray::Float64(positions);
        if !vf.position {
            geometry.attributes.position = None;
        }
        let mut result = Geometry::new(geometry.attributes, geometry.indices, geometry.primitive_type);
        result.bounding_sphere = Some(bounding_sphere);
        result.offset_attribute = self.offset_attribute;
        Some(result)
    }
}
