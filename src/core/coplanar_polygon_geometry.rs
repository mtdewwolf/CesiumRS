//! CesiumJS `CoplanarPolygonGeometry`: a polygon whose points share a
//! plane, triangulated in that plane rather than draped on the ellipsoid.

use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_rectangle::BoundingRectangle;
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::coplanar_polygon_geometry_library::{compute_project_to_2d_arguments, project_to_2d};
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::geometry_instance::GeometryInstance;
use super::geometry_pipeline::combine_instances;
use super::index_datatype::IndexDatatype;
use super::math::{clamp, EPSILON6};
use super::matrix3::Matrix3;
use super::polygon_geometry_library::{
    compute_bounding_rectangle, compute_hierarchy_packed_length, pack_polygon_hierarchy, polygons_from_hierarchy,
    texture_polygons_from_hierarchy, unpack_polygon_hierarchy, PolygonPositions, SplitPolygons, TextureCoordinateHierarchy,
};
use super::polygon_hierarchy::PolygonHierarchy;
use super::polygon_pipeline::triangulate;
use super::primitive_type::PrimitiveType;
use super::quaternion::Quaternion;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct CoplanarPolygonGeometryOptions {
    pub polygon_hierarchy: PolygonHierarchy,
    pub vertex_format: VertexFormat,
    pub st_rotation: f64,
    pub ellipsoid: Ellipsoid,
    pub texture_coordinates: Option<TextureCoordinateHierarchy>,
}

impl CoplanarPolygonGeometryOptions {
    pub fn new(polygon_hierarchy: PolygonHierarchy) -> Self {
        Self {
            polygon_hierarchy,
            vertex_format: VertexFormat::DEFAULT,
            st_rotation: 0.0,
            ellipsoid: Ellipsoid::WGS84,
            texture_coordinates: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoplanarPolygonGeometry {
    pub polygon_hierarchy: PolygonHierarchy,
    pub vertex_format: VertexFormat,
    pub st_rotation: f64,
    pub ellipsoid: Ellipsoid,
    pub texture_coordinates: Option<TextureCoordinateHierarchy>,
    pub packed_length: usize,
}

/// The plane's frame; Cesium keeps the tangent in module scratch, so with a
/// texture rotation it turns again for each polygon of the hierarchy.
struct Frame {
    normal: Cartesian3,
    tangent: Cartesian3,
    bitangent: Cartesian3,
}

fn floats(components: u8, values: Vec<f64>) -> GeometryAttribute {
    GeometryAttribute::floats(components, &values)
}

#[allow(clippy::too_many_arguments)]
fn create_geometry_from_polygon(
    polygon: &PolygonPositions,
    vertex_format: VertexFormat,
    bounding_rectangle: &BoundingRectangle,
    st_rotation: f64,
    hardcoded_texture_coordinates: Option<&[Cartesian2]>,
    project_point: &dyn Fn(Cartesian3) -> Cartesian2,
    frame: &mut Frame,
) -> Geometry {
    let positions = &polygon.positions;
    let mut indices = triangulate(&polygon.positions_2d, &polygon.holes);
    if indices.len() < 3 {
        indices = vec![0, 1, 2];
    }
    let indices: Vec<u32> = indices.into_iter().map(|i| i as u32).collect();
    let normal = frame.normal;
    let texture_matrix = if st_rotation != 0.0 {
        if vertex_format.tangent || vertex_format.bitangent {
            let tangent_rotation = Matrix3::from_quaternion(&Quaternion::from_axis_angle(normal, -st_rotation));
            frame.tangent = tangent_rotation.multiply_by_vector(frame.tangent).normalize();
            if vertex_format.bitangent {
                frame.bitangent = normal.cross(frame.tangent).normalize();
            }
        }
        Matrix3::from_quaternion(&Quaternion::from_axis_angle(normal, st_rotation))
    } else {
        Matrix3::IDENTITY
    };
    let st_origin = Cartesian2::new(bounding_rectangle.x, bounding_rectangle.y);
    let length = positions.len();
    let mut normals = Vec::new();
    let mut tangents = Vec::new();
    let mut bitangents = Vec::new();
    let mut texture_coordinates = Vec::new();
    for (i, position) in positions.iter().enumerate() {
        if vertex_format.st {
            match hardcoded_texture_coordinates.filter(|t| t.len() == length) {
                Some(t) => texture_coordinates.extend([t[i].x, t[i].y]),
                None => {
                    let st = project_point(texture_matrix.multiply_by_vector(*position)).subtract(st_origin);
                    texture_coordinates.extend([
                        clamp(st.x / bounding_rectangle.width, 0.0, 1.0),
                        clamp(st.y / bounding_rectangle.height, 0.0, 1.0),
                    ]);
                }
            }
        }
        normals.extend([normal.x, normal.y, normal.z]);
        tangents.extend([frame.tangent.x, frame.tangent.y, frame.tangent.z]);
        bitangents.extend([frame.bitangent.x, frame.bitangent.y, frame.bitangent.z]);
    }
    let mut attributes = GeometryAttributes::default();
    if vertex_format.position {
        attributes.position = Some(GeometryAttribute::doubles(3, Cartesian3::pack_array(positions)));
    }
    if vertex_format.normal {
        attributes.normal = Some(floats(3, normals));
    }
    if vertex_format.tangent {
        attributes.tangent = Some(floats(3, tangents));
    }
    if vertex_format.bitangent {
        attributes.bitangent = Some(floats(3, bitangents));
    }
    if vertex_format.st {
        attributes.st = Some(floats(2, texture_coordinates));
    }
    Geometry::new(attributes, Some(IndexDatatype::create_typed_array(length, &indices)), PrimitiveType::Triangles)
}

impl CoplanarPolygonGeometry {
    pub fn new(options: CoplanarPolygonGeometryOptions) -> Self {
        let packed_length = compute_hierarchy_packed_length(&options.polygon_hierarchy)
            + VertexFormat::PACKED_LENGTH
            + Ellipsoid::PACKED_LENGTH
            + options.texture_coordinates.as_ref().map_or(1, compute_hierarchy_packed_length)
            + 2;
        Self {
            polygon_hierarchy: options.polygon_hierarchy,
            vertex_format: options.vertex_format,
            st_rotation: options.st_rotation,
            ellipsoid: options.ellipsoid,
            texture_coordinates: options.texture_coordinates,
            packed_length,
        }
    }

    pub fn from_positions(positions: Vec<Cartesian3>, options: CoplanarPolygonGeometryOptions) -> Self {
        Self::new(CoplanarPolygonGeometryOptions { polygon_hierarchy: PolygonHierarchy::new(positions, Vec::new()), ..options })
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        let mut i = pack_polygon_hierarchy(&self.polygon_hierarchy, array, starting_index);
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        array[i] = self.st_rotation;
        i += 1;
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
        let st_rotation = array[i];
        i += 1;
        let texture_coordinates = if array[i] == -1.0 {
            i += 1;
            None
        } else {
            let (t, next) = unpack_polygon_hierarchy::<TextureCoordinateHierarchy>(array, i);
            i = next;
            Some(t)
        };
        Self { polygon_hierarchy, vertex_format, st_rotation, ellipsoid, texture_coordinates, packed_length: array[i] as usize }
    }

    /// The polygon's geometry; `None` for fewer than three distinct outer
    /// positions or points that do not span a plane.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let vf = self.vertex_format;
        let outer = array_remove_duplicates(&self.polygon_hierarchy.positions, |a, b, e| a.equals_epsilon(*b, e), true, None);
        if outer.len() < 3 {
            return None;
        }
        let (center, mut axis1, axis2) = compute_project_to_2d_arguments(&outer)?;
        let mut normal = axis1.cross(axis2).normalize();
        if !center.equals_epsilon(Cartesian3::ZERO, EPSILON6) {
            if let Some(surface_normal) = self.ellipsoid.geodetic_surface_normal(center) {
                if normal.dot(surface_normal) < 0.0 {
                    normal = normal.negate();
                    axis1 = axis1.negate();
                }
            }
        }
        let project_points = |positions: &[Cartesian3]| -> Option<Vec<Cartesian2>> {
            Some(positions.iter().map(|p| project_to_2d(*p, center, axis1, axis2)).collect())
        };
        let project_point = |p: Cartesian3| project_to_2d(p, center, axis1, axis2);
        // Cesium's scratch tangent and bitangent start at zero unless the
        // vertex format sets them.
        let mut frame = Frame {
            normal,
            tangent: if vf.tangent { axis1 } else { Cartesian3::ZERO },
            bitangent: if vf.bitangent { axis2 } else { Cartesian3::ZERO },
        };
        let mut hierarchy_copy = self.polygon_hierarchy.clone();
        let (hierarchy, polygons) = polygons_from_hierarchy(
            &mut hierarchy_copy,
            self.texture_coordinates.is_some(),
            &project_points,
            false,
            &self.ellipsoid,
            SplitPolygons::None,
        );
        let texture_polygons = self.texture_coordinates.as_ref().map(texture_polygons_from_hierarchy);
        if hierarchy.is_empty() {
            return None;
        }
        let outer_ring = &hierarchy[0].outer_ring;
        let bounding_sphere = BoundingSphere::from_points(outer_ring);
        let bounding_rectangle = compute_bounding_rectangle(normal, &|p| Some(project_point(p)), outer_ring, self.st_rotation);
        let instances: Vec<GeometryInstance> = polygons
            .iter()
            .enumerate()
            .map(|(i, polygon)| {
                let texcoords = texture_polygons.as_ref().and_then(|t| t.get(i)).map(|t| t.as_slice());
                GeometryInstance::new(
                    create_geometry_from_polygon(polygon, vf, &bounding_rectangle, self.st_rotation, texcoords, &project_point, &mut frame),
                    None,
                )
            })
            .collect();
        let mut geometry = combine_instances(&instances)?.into_iter().next()?;
        if let Some(position) = geometry.attributes.position.as_mut() {
            position.values = TypedArray::Float64(position.values.to_f64_vec());
        }
        let count = geometry.attributes.position.as_ref().map_or(0.0, |p| p.values.len() as f64 / 3.0);
        let indices: Vec<u32> = geometry.indices.as_ref()?.to_f64_vec().iter().map(|&i| i as u32).collect();
        geometry.indices = Some(IndexDatatype::create_typed_array(count as usize, &indices));
        let mut result = Geometry::new(geometry.attributes, geometry.indices, geometry.primitive_type);
        result.bounding_sphere = Some(bounding_sphere);
        Some(result)
    }
}
