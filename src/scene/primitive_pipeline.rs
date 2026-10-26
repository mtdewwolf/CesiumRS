//! CesiumJS `PrimitivePipeline`: geometry combination, batch IDs, attribute
//! compression and pick offsets across batched instances.

use crate::core::{
    geometry_pipeline, BoundingSphere, ComponentDatatype, Geometry, GeometryAttribute, GeometryInstance, Matrix4,
    RuntimeError, TypedArray,
};

/// Parameters for [`PrimitivePipeline::combine_geometry`].
#[derive(Clone, Debug)]
pub struct CombineGeometryParameters {
    pub instances: Vec<GeometryInstance>,
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
    pub bounding_spheres: Vec<Option<BoundingSphere>>,
    pub bounding_spheres_cv: Vec<Option<BoundingSphere>>,
}

fn add_geometry_batch_id(geometry: &mut Geometry, batch_id: usize) {
    let num_vertices = if let Some(pos) = geometry.attributes.get("position") {
        let comp = pos.components_per_attribute as usize;
        pos.values.len().checked_div(comp).unwrap_or(0)
    } else {
        0
    };

    let values = vec![batch_id as f32; num_vertices];
    geometry.attributes.set(
        "batchId",
        GeometryAttribute {
            component_datatype: ComponentDatatype::Float,
            components_per_attribute: 1,
            normalize: false,
            values: TypedArray::Float32(values),
        },
    );
}

fn add_batch_ids(instances: &mut [GeometryInstance]) {
    for (i, instance) in instances.iter_mut().enumerate() {
        if let Some(geom) = &mut instance.geometry {
            add_geometry_batch_id(geom, i);
        } else {
            if let Some(geom) = &mut instance.west_hemisphere_geometry {
                add_geometry_batch_id(geom, i);
            }
            if let Some(geom) = &mut instance.east_hemisphere_geometry {
                add_geometry_batch_id(geom, i);
            }
        }
    }
}

fn transform_to_world_coordinates(
    instances: &mut [GeometryInstance],
    primitive_model_matrix: &mut Matrix4,
    scene_3d_only: bool,
) {
    let mut to_world = !scene_3d_only;
    let length = instances.len();

    if !to_world && length > 1 {
        let first_matrix = instances[0].model_matrix;
        for instance in &instances[1..] {
            if instance.model_matrix != first_matrix {
                to_world = true;
                break;
            }
        }
    }

    if to_world {
        for instance in instances.iter_mut() {
            if instance.geometry.is_some() {
                geometry_pipeline::transform_to_world_coordinates(instance);
            }
        }
    } else if length > 0 {
        *primitive_model_matrix = primitive_model_matrix.multiply(&instances[0].model_matrix);
    }
}

fn create_instance_pick_offsets(instances: &[GeometryInstance], geometries: &[Geometry]) -> Vec<PickOffset> {
    let mut pick_offsets = Vec::new();

    let mut create_offsets = |get_geom: fn(&GeometryInstance) -> Option<&Geometry>| {
        for instance in instances {
            let geom = match get_geom(instance) {
                Some(g) => g,
                None => continue,
            };
            let count = geom.indices.as_ref().map_or(0, |idx| idx.len());

            let (geom_index, offset) = if let Some(last) = pick_offsets.last() {
                let last: &PickOffset = last;
                let next_offset = last.offset + last.count;
                let max_count = geometries.get(last.index).and_then(|g| g.indices.as_ref()).map_or(0, |i| i.len());
                if next_offset + count > max_count && last.index + 1 < geometries.len() {
                    (last.index + 1, 0)
                } else {
                    (last.index, next_offset)
                }
            } else {
                (0, 0)
            };

            pick_offsets.push(PickOffset { index: geom_index, offset, count });
        }
    };

    create_offsets(|i| i.geometry.as_ref());
    create_offsets(|i| i.west_hemisphere_geometry.as_ref());
    create_offsets(|i| i.east_hemisphere_geometry.as_ref());

    pick_offsets
}

/// Pipeline operations to prepare geometries for rendering in a [`super::Primitive`].
pub struct PrimitivePipeline;

impl PrimitivePipeline {
    pub fn combine_geometry(mut parameters: CombineGeometryParameters) -> Result<CombineGeometryResults, RuntimeError> {
        let length = parameters.instances.len();
        if length == 0 {
            return Ok(CombineGeometryResults {
                geometries: None,
                model_matrix: parameters.model_matrix,
                attribute_locations: None,
                pick_offsets: None,
                bounding_spheres: Vec::new(),
                bounding_spheres_cv: Vec::new(),
            });
        }

        let mut primitive_type = None;
        for instance in &parameters.instances {
            if let Some(geom) = &instance.geometry {
                primitive_type = Some(geom.primitive_type);
                break;
            }
        }

        if let Some(prim_type) = primitive_type {
            for instance in &parameters.instances {
                if let Some(geom) = &instance.geometry {
                    if geom.primitive_type != prim_type {
                        return Err(RuntimeError::new("All instance geometries must have the same primitiveType."));
                    }
                }
            }
        }

        transform_to_world_coordinates(
            &mut parameters.instances,
            &mut parameters.model_matrix,
            parameters.scene_3d_only,
        );

        if !parameters.scene_3d_only {
            for instance in parameters.instances.iter_mut() {
                if instance.geometry.is_some() {
                    geometry_pipeline::split_longitude(instance);
                }
            }
        }

        add_batch_ids(&mut parameters.instances);

        if parameters.vertex_cache_optimize {
            for instance in parameters.instances.iter_mut() {
                if let Some(geom) = &mut instance.geometry {
                    geometry_pipeline::reorder_for_post_vertex_cache(geom, None);
                    geometry_pipeline::reorder_for_pre_vertex_cache(geom);
                } else {
                    if let Some(geom) = &mut instance.west_hemisphere_geometry {
                        geometry_pipeline::reorder_for_post_vertex_cache(geom, None);
                        geometry_pipeline::reorder_for_pre_vertex_cache(geom);
                    }
                    if let Some(geom) = &mut instance.east_hemisphere_geometry {
                        geometry_pipeline::reorder_for_post_vertex_cache(geom, None);
                        geometry_pipeline::reorder_for_pre_vertex_cache(geom);
                    }
                }
            }
        }

        let mut geometries = geometry_pipeline::combine_instances(&parameters.instances).unwrap_or_default();

        for geom in geometries.iter_mut() {
            if parameters.compress_vertices {
                geometry_pipeline::compress_vertices(geom);
            }
        }

        if !parameters.uint_index_support {
            let mut split_geometries = Vec::new();
            for geom in &geometries {
                if let Some(splits) = geometry_pipeline::fit_to_unsigned_short_indices(geom) {
                    split_geometries.extend(splits);
                } else {
                    split_geometries.push(geom.clone());
                }
            }
            geometries = split_geometries;
        }

        let attribute_locations = if !geometries.is_empty() {
            Some(geometry_pipeline::create_attribute_locations(&geometries[0]))
        } else {
            None
        };

        let pick_offsets = if parameters.create_pick_offsets && !geometries.is_empty() {
            Some(create_instance_pick_offsets(&parameters.instances, &geometries))
        } else {
            None
        };

        let mut bounding_spheres = Vec::with_capacity(length);
        let mut bounding_spheres_cv = Vec::with_capacity(length);

        for instance in &parameters.instances {
            if let Some(geom) = &instance.geometry {
                bounding_spheres.push(geom.bounding_sphere);
                bounding_spheres_cv.push(None);
            } else if let (Some(east), Some(west)) =
                (&instance.east_hemisphere_geometry, &instance.west_hemisphere_geometry)
            {
                let bs = match (east.bounding_sphere, west.bounding_sphere) {
                    (Some(e), Some(w)) => Some(e.union(&w)),
                    (Some(e), None) => Some(e),
                    (None, Some(w)) => Some(w),
                    _ => None,
                };
                bounding_spheres.push(bs);
                bounding_spheres_cv.push(None);
            } else {
                bounding_spheres.push(None);
                bounding_spheres_cv.push(None);
            }
        }

        Ok(CombineGeometryResults {
            geometries: Some(geometries),
            model_matrix: parameters.model_matrix,
            attribute_locations,
            pick_offsets,
            bounding_spheres,
            bounding_spheres_cv,
        })
    }
}
