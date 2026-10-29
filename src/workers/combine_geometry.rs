//! CesiumJS `combineGeometry`: `PrimitivePipeline.combineGeometry` plus the
//! pack/unpack the worker applies around it.
//!
//! Instances arrive as a `Float64Array` of model matrices and offset slots.
//! A zero offset slot is still defined, so every unpacked instance carries an
//! `offset` attribute and `offsetInstanceExtend` is allocated. Geometries
//! arrive through `packCreateGeometryResults` and are assigned in order.
//! Bounding spheres in the reply are `Float32Array`s, matching
//! `packBoundingSpheres`.

use super::PackedGeometryResults;
use crate::core::geometry_pipeline::{self, combine_instances, create_attribute_locations};
use crate::core::*;

/// One `postMessage` body for `Workers/combineGeometry`, already copied out of
/// the transferable arrays.
#[derive(Clone, Debug, PartialEq)]
pub struct PackedCombineGeometry {
    pub create_geometry_results: Vec<PackedGeometryResults>,
    pub packed_instances: Vec<f64>,
    pub ellipsoid_radii: Cartesian3,
    pub is_geographic: bool,
    pub element_index_uint_supported: bool,
    pub scene_3d_only: bool,
    pub vertex_cache_optimize: bool,
    pub compress_vertices: bool,
    pub model_matrix: Matrix4,
    pub create_pick_offsets: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickOffset {
    pub index: usize,
    pub offset: usize,
    pub count: usize,
}

/// The object `packCombineGeometryResults` posts. Bounding-sphere buffers use
/// `f32`, including the leading count.
#[derive(Clone, Debug, PartialEq)]
pub struct CombineGeometryResult {
    pub geometries: Option<Vec<Geometry>>,
    pub model_matrix: Matrix4,
    pub attribute_locations: Option<Vec<(String, u32)>>,
    pub pick_offsets: Option<Vec<PickOffset>>,
    pub offset_instance_extend: Option<Vec<Option<GeometryOffsetAttribute>>>,
    pub bounding_spheres: Vec<f32>,
    pub bounding_spheres_cv: Vec<f32>,
}

pub fn combine_geometry(packed: PackedCombineGeometry) -> Result<CombineGeometryResult, RuntimeError> {
    let ellipsoid = Ellipsoid::from_cartesian3(packed.ellipsoid_radii);
    let mut model_matrix = packed.model_matrix;
    let mut instances = unpack_instances(&packed.packed_instances)?;
    let mut geometry_index = 0;
    for results in &packed.create_geometry_results {
        for geometry in results.unpack()? {
            let instance = instances
                .get_mut(geometry_index)
                .ok_or_else(|| invalid("combineGeometry is missing an instance for a geometry"))?;
            instance.geometry = geometry;
            geometry_index += 1;
        }
    }

    let length = instances.len();
    let has_offset =
        instances.first().is_some_and(|instance| instance.attributes.iter().any(|(name, _)| name == "offset"));
    let mut geometries = None;
    let mut attribute_locations = None;
    let mut pick_offsets = None;
    if length > 0 {
        let combined = geometry_pipeline(&packed, &mut instances, &mut model_matrix, &ellipsoid)?;
        if !combined.is_empty() {
            attribute_locations = Some(create_attribute_locations(&combined[0]));
            if packed.create_pick_offsets {
                pick_offsets = Some(instance_pick_offsets(&instances, &combined)?);
            }
        }
        geometries = Some(combined);
    }

    let mut extend = has_offset.then(|| vec![None; length]);
    let mut spheres = Vec::with_capacity(length);
    let mut spheres_cv = Vec::with_capacity(length);
    for (i, instance) in instances.iter().enumerate() {
        let mut sphere = None;
        let mut sphere_cv = None;
        if let Some(geometry) = &instance.geometry {
            sphere = geometry.bounding_sphere;
            sphere_cv = geometry.bounding_sphere_cv;
            if let Some(slots) = extend.as_mut() {
                slots[i] = geometry.offset_attribute;
            }
        }
        if let (Some(east), Some(west)) = (&instance.east_hemisphere_geometry, &instance.west_hemisphere_geometry) {
            if let (Some(east_sphere), Some(west_sphere)) = (east.bounding_sphere, west.bounding_sphere) {
                sphere = Some(east_sphere.union(&west_sphere));
            }
            if let (Some(east_sphere), Some(west_sphere)) = (east.bounding_sphere_cv, west.bounding_sphere_cv) {
                sphere_cv = Some(east_sphere.union(&west_sphere));
            }
        }
        spheres.push(sphere);
        spheres_cv.push(sphere_cv);
    }

    Ok(CombineGeometryResult {
        geometries,
        model_matrix,
        attribute_locations,
        pick_offsets,
        offset_instance_extend: extend,
        bounding_spheres: pack_bounding_spheres(&spheres),
        bounding_spheres_cv: pack_bounding_spheres(&spheres_cv),
    })
}

pub(crate) fn geometry_pipeline(
    packed: &PackedCombineGeometry,
    instances: &mut [GeometryInstance],
    model_matrix: &mut Matrix4,
    ellipsoid: &Ellipsoid,
) -> Result<Vec<Geometry>, RuntimeError> {
    transform_to_world(instances, model_matrix, packed.scene_3d_only)?;
    if !packed.scene_3d_only {
        for instance in instances.iter_mut() {
            if instance.geometry.is_some() {
                geometry_pipeline::split_longitude(instance)
                    .ok_or_else(|| invalid("combineGeometry could not split an instance at the antimeridian"))?;
            }
        }
    }
    add_batch_ids(instances)?;
    if packed.vertex_cache_optimize {
        for instance in instances.iter_mut() {
            reorder_instance(instance)?;
        }
    }
    // Cesium's debug build throws DeveloperError
    // "All instance geometries must have the same primitiveType."
    // Release builds strip that check. This port keeps the first geometry's
    // primitive type and concatenates every instance's indices under it.
    let mut geometries =
        combine_instances(instances).ok_or_else(|| invalid("combineGeometry could not combine instances"))?;
    for geometry in &mut geometries {
        encode_positions(geometry, ellipsoid, packed.is_geographic, packed.scene_3d_only)?;
        if packed.compress_vertices {
            geometry_pipeline::compress_vertices(geometry)
                .ok_or_else(|| invalid("combineGeometry could not compress vertices"))?;
        }
    }
    if !packed.element_index_uint_supported {
        let mut split = Vec::new();
        for geometry in geometries {
            split.extend(
                geometry_pipeline::fit_to_unsigned_short_indices(&geometry)
                    .ok_or_else(|| invalid("combineGeometry could not fit indices into unsigned shorts"))?,
            );
        }
        geometries = split;
    }
    Ok(geometries)
}

fn transform_to_world(
    instances: &mut [GeometryInstance],
    model_matrix: &mut Matrix4,
    scene_3d_only: bool,
) -> Result<(), RuntimeError> {
    let mut to_world = !scene_3d_only;
    if !to_world && instances.len() > 1 {
        let first = instances[0].model_matrix;
        to_world = instances[1..].iter().any(|instance| instance.model_matrix != first);
    }
    if to_world {
        for instance in instances.iter_mut() {
            if instance.geometry.is_some() {
                geometry_pipeline::transform_to_world_coordinates(instance)
                    .ok_or_else(|| invalid("combineGeometry could not transform an instance to world coordinates"))?;
            }
        }
    } else if let Some(instance) = instances.first() {
        *model_matrix = model_matrix.multiply_transformation(&instance.model_matrix);
    }
    Ok(())
}

fn add_batch_ids(instances: &mut [GeometryInstance]) -> Result<(), RuntimeError> {
    for (i, instance) in instances.iter_mut().enumerate() {
        let batch_id = i as f32;
        if let Some(geometry) = instance.geometry.as_mut() {
            add_geometry_batch_id(geometry, batch_id)?;
        } else if instance.west_hemisphere_geometry.is_some() && instance.east_hemisphere_geometry.is_some() {
            if let Some(geometry) = instance.west_hemisphere_geometry.as_mut() {
                add_geometry_batch_id(geometry, batch_id)?;
            }
            if let Some(geometry) = instance.east_hemisphere_geometry.as_mut() {
                add_geometry_batch_id(geometry, batch_id)?;
            }
        }
    }
    Ok(())
}

fn add_geometry_batch_id(geometry: &mut Geometry, batch_id: f32) -> Result<(), RuntimeError> {
    let position = geometry
        .attributes
        .position
        .as_ref()
        .ok_or_else(|| invalid("combineGeometry geometry is missing a position attribute"))?;
    let components = usize::from(position.components_per_attribute);
    if components == 0 {
        return Err(invalid("combineGeometry position attribute has no components"));
    }
    let count = position.values.len() / components;
    let values = vec![f64::from(batch_id); count];
    geometry.attributes.set("batchId", GeometryAttribute::floats(1, &values));
    Ok(())
}

fn reorder_instance(instance: &mut GeometryInstance) -> Result<(), RuntimeError> {
    if let Some(geometry) = instance.geometry.as_mut() {
        return reorder_geometry(geometry);
    }
    if instance.west_hemisphere_geometry.is_some() && instance.east_hemisphere_geometry.is_some() {
        if let Some(geometry) = instance.west_hemisphere_geometry.as_mut() {
            reorder_geometry(geometry)?;
        }
        if let Some(geometry) = instance.east_hemisphere_geometry.as_mut() {
            reorder_geometry(geometry)?;
        }
    }
    Ok(())
}

fn reorder_geometry(geometry: &mut Geometry) -> Result<(), RuntimeError> {
    geometry_pipeline::reorder_for_post_vertex_cache(geometry, None);
    geometry_pipeline::reorder_for_pre_vertex_cache(geometry)
        .ok_or_else(|| invalid("combineGeometry could not reorder a geometry for the vertex cache"))
}

fn encode_positions(
    geometry: &mut Geometry,
    ellipsoid: &Ellipsoid,
    is_geographic: bool,
    scene_3d_only: bool,
) -> Result<(), RuntimeError> {
    let doubles: Vec<String> = geometry
        .attributes
        .iter()
        .filter(|(_, attribute)| attribute.component_datatype == ComponentDatatype::Double)
        .map(|(name, _)| name.to_string())
        .collect();
    for name in doubles {
        if scene_3d_only {
            geometry_pipeline::encode_attribute(geometry, &name, &format!("{name}3DHigh"), &format!("{name}3DLow"))
                .ok_or_else(|| invalid("combineGeometry could not encode a 3D position attribute"))?;
            continue;
        }
        let name_3d = format!("{name}3D");
        let name_2d = format!("{name}2D");
        if is_geographic {
            geometry_pipeline::project_to_2d(
                geometry,
                &name,
                &name_3d,
                &name_2d,
                Some(&GeographicProjection::new(*ellipsoid)),
            )
            .ok_or_else(|| invalid("combineGeometry could not project positions to geographic 2D"))?;
        } else {
            geometry_pipeline::project_to_2d(
                geometry,
                &name,
                &name_3d,
                &name_2d,
                Some(&WebMercatorProjection::new(*ellipsoid)),
            )
            .ok_or_else(|| invalid("combineGeometry could not project positions to Web Mercator"))?;
        }
        if geometry.bounding_sphere.is_some() && name == "position" {
            let values = geometry
                .attributes
                .get("position2D")
                .ok_or_else(|| invalid("combineGeometry is missing position2D after projection"))?
                .values
                .to_f64_vec();
            geometry.bounding_sphere_cv = Some(BoundingSphere::from_vertices(&values, Cartesian3::ZERO, 3));
        }
        geometry_pipeline::encode_attribute(geometry, &name_3d, &format!("{name_3d}High"), &format!("{name_3d}Low"))
            .ok_or_else(|| invalid("combineGeometry could not encode a projected 3D attribute"))?;
        geometry_pipeline::encode_attribute(geometry, &name_2d, &format!("{name_2d}High"), &format!("{name_2d}Low"))
            .ok_or_else(|| invalid("combineGeometry could not encode a projected 2D attribute"))?;
    }
    Ok(())
}

pub(crate) fn instance_pick_offsets(
    instances: &[GeometryInstance],
    geometries: &[Geometry],
) -> Result<Vec<PickOffset>, RuntimeError> {
    let mut pick_offsets = Vec::new();
    append_pick_offsets(instances, PickSource::Geometry, geometries, &mut pick_offsets)?;
    append_pick_offsets(instances, PickSource::West, geometries, &mut pick_offsets)?;
    append_pick_offsets(instances, PickSource::East, geometries, &mut pick_offsets)?;
    Ok(pick_offsets)
}

enum PickSource {
    Geometry,
    West,
    East,
}

fn source_geometry<'a>(instance: &'a GeometryInstance, source: &PickSource) -> Option<&'a Geometry> {
    match source {
        PickSource::Geometry => instance.geometry.as_ref(),
        PickSource::West => instance.west_hemisphere_geometry.as_ref(),
        PickSource::East => instance.east_hemisphere_geometry.as_ref(),
    }
}

fn append_pick_offsets(
    instances: &[GeometryInstance],
    source: PickSource,
    geometries: &[Geometry],
    pick_offsets: &mut Vec<PickOffset>,
) -> Result<(), RuntimeError> {
    let (mut offset, mut geometry_index, mut index_count) = if let Some(last) = pick_offsets.last() {
        let index_count = geometries
            .get(last.index)
            .and_then(|geometry| geometry.indices.as_ref())
            .ok_or_else(|| invalid("combineGeometry pick offsets refer to a geometry without indices"))?
            .len();
        (last.offset + last.count, last.index, index_count)
    } else {
        let index_count = geometries
            .first()
            .and_then(|geometry| geometry.indices.as_ref())
            .ok_or_else(|| invalid("combineGeometry pick offsets require indexed geometries"))?
            .len();
        (0, 0, index_count)
    };
    for instance in instances {
        let Some(geometry) = source_geometry(instance, &source) else { continue };
        let count = geometry
            .indices
            .as_ref()
            .ok_or_else(|| invalid("combineGeometry instance geometry is missing indices"))?
            .len();
        if offset + count > index_count {
            offset = 0;
            geometry_index += 1;
            index_count = geometries
                .get(geometry_index)
                .and_then(|geometry| geometry.indices.as_ref())
                .ok_or_else(|| invalid("combineGeometry pick offset rolled past the last geometry"))?
                .len();
        }
        pick_offsets.push(PickOffset { index: geometry_index, offset, count });
        offset += count;
    }
    Ok(())
}

/// `Float64Array` layout: count, then 16 matrix components and 3 offset
/// components per instance. Missing offsets are stored as zeros, and `defined(0)`
/// is true, so each instance receives an offset attribute.
fn unpack_instances(data: &[f64]) -> Result<Vec<GeometryInstance>, RuntimeError> {
    if data.is_empty() {
        return Err(invalid("combineGeometry packed instances are empty"));
    }
    let mut i = 1;
    let mut instances = Vec::new();
    while i < data.len() {
        let matrix =
            data.get(i..i + 16).ok_or_else(|| invalid("combineGeometry packed instance matrix is truncated"))?;
        i += 16;
        let offset =
            data.get(i..i + 3).ok_or_else(|| invalid("combineGeometry packed instance offset is truncated"))?;
        i += 3;
        let attribute =
            OffsetGeometryInstanceAttribute { value: [offset[0] as f32, offset[1] as f32, offset[2] as f32] };
        instances.push(GeometryInstance {
            geometry: None,
            model_matrix: Matrix4(
                matrix.try_into().map_err(|_| invalid("combineGeometry packed instance matrix is truncated"))?,
            ),
            id: None,
            attributes: vec![("offset".to_string(), attribute.into())],
            west_hemisphere_geometry: None,
            east_hemisphere_geometry: None,
        });
    }
    Ok(instances)
}

fn pack_bounding_spheres(spheres: &[Option<BoundingSphere>]) -> Vec<f32> {
    let mut buffer = Vec::with_capacity(1 + spheres.len() * 5);
    buffer.push(spheres.len() as f32);
    for sphere in spheres {
        match sphere {
            None => buffer.extend([0.0; 5]),
            Some(sphere) => buffer.extend([
                1.0,
                sphere.center.x as f32,
                sphere.center.y as f32,
                sphere.center.z as f32,
                sphere.radius as f32,
            ]),
        }
    }
    buffer
}

fn invalid(detail: &str) -> RuntimeError {
    RuntimeError::new(detail)
}
