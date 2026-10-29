//! CesiumJS `PrimitivePipeline`: geometry combination, batch IDs, attribute
//! compression and pick offsets across batched instances.

use crate::core::{
    BoundingSphere, Cartesian3, Ellipsoid, Geometry, GeometryInstance, GeometryOffsetAttribute, Matrix4, RuntimeError,
};

/// Parameters for [`PrimitivePipeline::combine_geometry`].
#[derive(Clone, Debug)]
pub struct CombineGeometryParameters {
    pub instances: Vec<GeometryInstance>,
    pub ellipsoid_radii: Cartesian3,
    pub is_geographic: bool,
    pub uint_index_support: bool,
    pub scene_3d_only: bool,
    pub vertex_cache_optimize: bool,
    pub compress_vertices: bool,
    pub model_matrix: Matrix4,
    pub create_pick_offsets: bool,
}

impl Default for CombineGeometryParameters {
    fn default() -> Self {
        Self {
            instances: Vec::new(),
            ellipsoid_radii: Ellipsoid::WGS84.radii,
            is_geographic: true,
            uint_index_support: true,
            scene_3d_only: false,
            vertex_cache_optimize: false,
            compress_vertices: true,
            model_matrix: Matrix4::IDENTITY,
            create_pick_offsets: true,
        }
    }
}

/// A sub-range within a combined geometry corresponding to one instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickOffset {
    pub index: usize,
    pub offset: usize,
    pub count: usize,
}

/// The result of [`PrimitivePipeline::combine_geometry`].
#[derive(Clone, Debug)]
pub struct CombineGeometryResults {
    pub geometries: Option<Vec<Geometry>>,
    pub model_matrix: Matrix4,
    pub attribute_locations: Option<Vec<(String, u32)>>,
    pub pick_offsets: Option<Vec<PickOffset>>,
    pub offset_instance_extend: Option<Vec<Option<GeometryOffsetAttribute>>>,
    pub bounding_spheres: Vec<Option<BoundingSphere>>,
    pub bounding_spheres_cv: Vec<Option<BoundingSphere>>,
}

/// Scene entry point for the shared, reference-validated geometry kernel.
pub struct PrimitivePipeline;
impl PrimitivePipeline {
    pub fn combine_geometry(mut parameters: CombineGeometryParameters) -> Result<CombineGeometryResults, RuntimeError> {
        let first_type =
            parameters.instances.iter().filter_map(|i| i.geometry.as_ref()).map(|g| g.primitive_type).next();
        if parameters.instances.iter().filter_map(|i| i.geometry.as_ref()).any(|g| Some(g.primitive_type) != first_type)
        {
            return Err(RuntimeError::new("All instance geometries must have the same primitiveType."));
        }
        let options = crate::workers::PackedCombineGeometry {
            create_geometry_results: vec![],
            packed_instances: vec![],
            ellipsoid_radii: parameters.ellipsoid_radii,
            is_geographic: parameters.is_geographic,
            element_index_uint_supported: parameters.uint_index_support,
            scene_3d_only: parameters.scene_3d_only,
            vertex_cache_optimize: parameters.vertex_cache_optimize,
            compress_vertices: parameters.compress_vertices,
            model_matrix: parameters.model_matrix,
            create_pick_offsets: parameters.create_pick_offsets,
        };
        let has_offset =
            parameters.instances.first().is_some_and(|i| i.attributes.iter().any(|(name, _)| name == "offset"));
        let mut model_matrix = parameters.model_matrix;
        let geometries = if parameters.instances.is_empty() {
            None
        } else {
            Some(crate::workers::combine_geometry::geometry_pipeline(
                &options,
                &mut parameters.instances,
                &mut model_matrix,
                &Ellipsoid::from_cartesian3(parameters.ellipsoid_radii),
            )?)
        };
        let nonempty = geometries.as_ref().filter(|g| !g.is_empty());
        let attribute_locations = nonempty.map(|g| crate::core::geometry_pipeline::create_attribute_locations(&g[0]));
        let pick_offsets = if parameters.create_pick_offsets {
            nonempty
                .map(|g| crate::workers::combine_geometry::instance_pick_offsets(&parameters.instances, g))
                .transpose()?
                .map(|v| {
                    v.into_iter().map(|p| PickOffset { index: p.index, offset: p.offset, count: p.count }).collect()
                })
        } else {
            None
        };
        let mut bounding_spheres = Vec::new();
        let mut bounding_spheres_cv = Vec::new();
        let mut offset_instance_extend = has_offset.then(Vec::new);
        for instance in &parameters.instances {
            let mut sphere = instance.geometry.as_ref().and_then(|g| g.bounding_sphere);
            let mut cv = instance.geometry.as_ref().and_then(|g| g.bounding_sphere_cv);
            if let Some(slots) = &mut offset_instance_extend {
                slots.push(instance.geometry.as_ref().and_then(|g| g.offset_attribute));
            }
            if let (Some(east), Some(west)) = (&instance.east_hemisphere_geometry, &instance.west_hemisphere_geometry) {
                if let (Some(e), Some(w)) = (east.bounding_sphere, west.bounding_sphere) {
                    sphere = Some(e.union(&w));
                }
                if let (Some(e), Some(w)) = (east.bounding_sphere_cv, west.bounding_sphere_cv) {
                    cv = Some(e.union(&w));
                }
            }
            bounding_spheres.push(sphere);
            bounding_spheres_cv.push(cv);
        }
        Ok(CombineGeometryResults {
            geometries,
            model_matrix,
            attribute_locations,
            pick_offsets,
            offset_instance_extend,
            bounding_spheres,
            bounding_spheres_cv,
        })
    }
}
