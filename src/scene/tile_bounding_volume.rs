//! CesiumJS `TileBoundingVolume`: the interface of a tile's bounding volume.
//!
//! A Rust trait replaces Cesium's abstract prototype. `boundingVolume` has a
//! different type for each volume, so each type exposes its own accessor
//! instead of a trait method.

use crate::core::{BoundingSphere, Intersect, MapProjection, Matrix4, Plane};
use crate::core::{Cartesian3, Cartographic, Color, ColorGeometryInstanceAttribute, Geometry, GeometryInstance};

use super::scene_mode::SceneMode;

/// The parts of a `FrameState` a tile bounding volume reads.
#[derive(Clone, Copy)]
pub struct TileBoundingVolumeFrame<'a> {
    pub mode: SceneMode,
    /// `frameState.camera.positionWC`.
    pub camera_position_wc: Cartesian3,
    /// `frameState.camera.positionCartographic`.
    pub camera_position_cartographic: Cartographic,
    pub map_projection: &'a dyn MapProjection,
}

/// What `createDebugVolume` builds: Cesium wraps these instances in a
/// `Primitive` drawn synchronously with an opaque, flat
/// `PerInstanceColorAppearance`. Each instance has the id `"outline"`, a
/// `color` attribute and its geometry already created.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugVolume {
    pub geometry_instances: Vec<GeometryInstance>,
}

impl DebugVolume {
    /// One outline instance per geometry, in order.
    pub(crate) fn outlines(geometries: Vec<Option<Geometry>>, model_matrix: Matrix4, color: &Color) -> Self {
        let color = ColorGeometryInstanceAttribute::from_color(color);
        let geometry_instances = geometries
            .into_iter()
            .map(|geometry| GeometryInstance {
                geometry,
                model_matrix,
                id: Some("outline".into()),
                attributes: vec![("color".into(), color.into())],
                west_hemisphere_geometry: None,
                east_hemisphere_geometry: None,
            })
            .collect();
        Self { geometry_instances }
    }
}

pub trait TileBoundingVolume {
    /// The underlying bounding sphere.
    fn bounding_sphere(&self) -> &BoundingSphere;

    /// The distance between the tile and the camera, in meters; 0 inside.
    fn distance_to_camera(&self, frame: &TileBoundingVolumeFrame<'_>) -> f64;

    /// Which side of the plane the volume is on.
    fn intersect_plane(&self, plane: &Plane) -> Intersect;

    /// The outline of the volume.
    fn create_debug_volume(&self, color: &Color) -> DebugVolume;
}
