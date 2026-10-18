//! CesiumJS `SphereGeometry` and `SphereOutlineGeometry`: ellipsoid
//! geometries with equal radii.

use super::cartesian3::Cartesian3;
use super::ellipsoid_geometry::EllipsoidGeometry;
use super::ellipsoid_outline_geometry::EllipsoidOutlineGeometry;
use super::geometry::Geometry;
use super::math::{PI, TWO_PI};
use super::vertex_format::VertexFormat;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SphereGeometry {
    pub ellipsoid_geometry: EllipsoidGeometry,
}

impl SphereGeometry {
    pub const PACKED_LENGTH: usize = EllipsoidGeometry::PACKED_LENGTH;

    /// Cesium's defaults are radius 1 and 64 × 64 partitions.
    pub fn new(radius: f64, stack_partitions: f64, slice_partitions: f64, vertex_format: VertexFormat) -> Option<Self> {
        let radii = Cartesian3::new(radius, radius, radius);
        EllipsoidGeometry::new(
            radii,
            None,
            0.0,
            TWO_PI,
            0.0,
            PI,
            stack_partitions,
            slice_partitions,
            vertex_format,
            None,
        )
        .map(|ellipsoid_geometry| Self { ellipsoid_geometry })
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.ellipsoid_geometry.pack(array, starting_index);
    }

    /// Like Cesium, keeps only the x radius, the partitions, and the vertex format.
    pub fn unpack(array: &[f64], starting_index: usize) -> Option<Self> {
        let e = EllipsoidGeometry::unpack(array, starting_index)?;
        Self::new(e.radii.x, e.stack_partitions, e.slice_partitions, e.vertex_format)
    }

    pub fn create_geometry(&self) -> Option<Geometry> {
        self.ellipsoid_geometry.create_geometry()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SphereOutlineGeometry {
    pub ellipsoid_geometry: EllipsoidOutlineGeometry,
}

impl SphereOutlineGeometry {
    pub const PACKED_LENGTH: usize = EllipsoidOutlineGeometry::PACKED_LENGTH;

    /// Cesium's defaults are radius 1, 10 stacks, 8 slices, and 128 subdivisions.
    pub fn new(radius: f64, stack_partitions: f64, slice_partitions: f64, subdivisions: f64) -> Option<Self> {
        let radii = Cartesian3::new(radius, radius, radius);
        EllipsoidOutlineGeometry::new(
            radii,
            None,
            0.0,
            TWO_PI,
            0.0,
            PI,
            stack_partitions,
            slice_partitions,
            subdivisions,
            None,
        )
        .map(|ellipsoid_geometry| Self { ellipsoid_geometry })
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.ellipsoid_geometry.pack(array, starting_index);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Option<Self> {
        let e = EllipsoidOutlineGeometry::unpack(array, starting_index)?;
        Self::new(e.radii.x, e.stack_partitions, e.slice_partitions, e.subdivisions)
    }

    pub fn create_geometry(&self) -> Option<Geometry> {
        self.ellipsoid_geometry.create_geometry()
    }
}
