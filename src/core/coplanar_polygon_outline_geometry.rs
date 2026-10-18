//! CesiumJS `CoplanarPolygonOutlineGeometry`: the rings of a polygon whose
//! points share a plane, used as given (not draped on the ellipsoid).

use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::coplanar_polygon_geometry_library::valid_outline;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::geometry_instance::GeometryInstance;
use super::geometry_pipeline::combine_instances;
use super::index_datatype::IndexDatatype;
use super::polygon_geometry_library::{
    compute_hierarchy_packed_length, pack_polygon_hierarchy, polygon_outlines_from_hierarchy, unpack_polygon_hierarchy,
};
use super::polygon_hierarchy::PolygonHierarchy;
use super::primitive_type::PrimitiveType;

#[derive(Clone, Debug, PartialEq)]
pub struct CoplanarPolygonOutlineGeometry {
    pub polygon_hierarchy: PolygonHierarchy,
    pub packed_length: usize,
}

fn ring_geometry(positions: &[Cartesian3]) -> Geometry {
    let length = positions.len();
    let indices: Vec<u32> = (0..length).flat_map(|i| [i as u32, ((i + 1) % length) as u32]).collect();
    Geometry::new(
        GeometryAttributes { position: Some(GeometryAttribute::doubles(3, Cartesian3::pack_array(positions))), ..Default::default() },
        Some(IndexDatatype::create_typed_array(length, &indices)),
        PrimitiveType::Lines,
    )
}

impl CoplanarPolygonOutlineGeometry {
    pub fn new(polygon_hierarchy: PolygonHierarchy) -> Self {
        let packed_length = compute_hierarchy_packed_length(&polygon_hierarchy) + 1;
        Self { polygon_hierarchy, packed_length }
    }

    pub fn from_positions(positions: Vec<Cartesian3>) -> Self {
        Self::new(PolygonHierarchy::new(positions, Vec::new()))
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        let i = pack_polygon_hierarchy(&self.polygon_hierarchy, array, starting_index);
        array[i] = self.packed_length as f64;
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let (polygon_hierarchy, i) = unpack_polygon_hierarchy::<PolygonHierarchy>(array, starting_index);
        Self { polygon_hierarchy, packed_length: array[i] as usize }
    }

    /// The outline; `None` for fewer than three distinct outer positions or
    /// points that do not span a line.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let outer = array_remove_duplicates(&self.polygon_hierarchy.positions, |a, b, e| a.equals_epsilon(*b, e), true, None);
        if outer.len() < 3 || !valid_outline(&outer) {
            return None;
        }
        let mut hierarchy = self.polygon_hierarchy.clone();
        let polygons = polygon_outlines_from_hierarchy(&mut hierarchy, false, &Ellipsoid::WGS84);
        if polygons.is_empty() {
            return None;
        }
        let instances: Vec<GeometryInstance> = polygons.iter().map(|p| GeometryInstance::new(ring_geometry(p), None)).collect();
        let geometry = combine_instances(&instances)?.into_iter().next()?;
        let mut result = Geometry::new(geometry.attributes, geometry.indices, geometry.primitive_type);
        result.bounding_sphere = Some(BoundingSphere::from_points(&self.polygon_hierarchy.positions));
        Some(result)
    }
}
