//! CesiumJS `CircleGeometry`: an [`EllipseGeometry`] with equal axes.

use super::cartesian3::Cartesian3;
use super::ellipse_geometry::{EllipseGeometry, EllipseGeometryOptions};
use super::ellipsoid::Ellipsoid;
use super::geometry::Geometry;
use super::rectangle::Rectangle;
use super::vertex_format::VertexFormat;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleGeometry {
    pub ellipse_geometry: EllipseGeometry,
}

impl CircleGeometry {
    pub const PACKED_LENGTH: usize = EllipseGeometry::PACKED_LENGTH;

    /// `options.semi_major_axis` and `semi_minor_axis` are replaced by
    /// `radius`; like Cesium, the rotation and offset attribute are not
    /// passed on.
    pub fn new(radius: f64, options: EllipseGeometryOptions) -> Option<Self> {
        let options = EllipseGeometryOptions {
            semi_major_axis: radius,
            semi_minor_axis: radius,
            rotation: 0.0,
            offset_attribute: None,
            ..options
        };
        EllipseGeometry::new(options).map(|ellipse_geometry| Self { ellipse_geometry })
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.ellipse_geometry.pack(array, starting_index);
    }

    /// Unpacks the ellipse and rebuilds the circle from its semi-major axis.
    pub fn unpack(array: &[f64], starting_index: usize) -> Option<Self> {
        let e = EllipseGeometry::unpack(array, starting_index)?;
        let mut options = EllipseGeometryOptions::new(e.center, e.semi_major_axis, e.semi_major_axis);
        options.ellipsoid = e.ellipsoid;
        options.height = e.height;
        options.extruded_height = Some(e.extruded_height);
        options.granularity = e.granularity;
        options.vertex_format = e.vertex_format;
        options.st_rotation = e.st_rotation;
        options.shadow_volume = e.shadow_volume;
        Self::new(e.semi_major_axis, options)
    }

    pub fn create_geometry(&self) -> Option<Geometry> {
        self.ellipse_geometry.create_geometry()
    }

    pub fn create_shadow_volume(
        &self,
        min_height: impl Fn(f64, &Ellipsoid) -> f64,
        max_height: impl Fn(f64, &Ellipsoid) -> f64,
    ) -> Option<Self> {
        let e = &self.ellipse_geometry;
        let mut options = EllipseGeometryOptions::new(e.center, e.semi_major_axis, e.semi_major_axis);
        options.ellipsoid = e.ellipsoid;
        options.st_rotation = e.st_rotation;
        options.granularity = e.granularity;
        options.extruded_height = Some(min_height(e.granularity, &e.ellipsoid));
        options.height = max_height(e.granularity, &e.ellipsoid);
        options.vertex_format = VertexFormat::POSITION_ONLY;
        options.shadow_volume = true;
        Self::new(e.semi_major_axis, options)
    }

    pub fn rectangle(&self) -> Rectangle {
        self.ellipse_geometry.rectangle()
    }

    pub fn texture_coordinate_rotation_points(&self) -> [f64; 6] {
        self.ellipse_geometry.texture_coordinate_rotation_points()
    }

    pub fn center(&self) -> Cartesian3 {
        self.ellipse_geometry.center
    }
}
