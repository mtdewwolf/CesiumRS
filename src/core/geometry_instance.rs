//! CesiumJS `GeometryInstance`: a geometry placed with a model matrix, plus
//! per-instance attributes and an id for picking.

use super::geometry::Geometry;
use super::geometry_instance_attribute::GeometryInstanceAttribute;
use super::matrix4::Matrix4;

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryInstance {
    /// `None` once `GeometryPipeline::split_longitude` has split the
    /// geometry into the two hemisphere geometries.
    pub geometry: Option<Geometry>,
    pub model_matrix: Matrix4,
    /// Cesium accepts any value; the port keeps a string.
    pub id: Option<String>,
    /// Attributes by name, in insertion order.
    pub attributes: Vec<(String, GeometryInstanceAttribute)>,
    /// Set by `GeometryPipeline.splitLongitude` when the geometry crosses
    /// the antimeridian.
    pub west_hemisphere_geometry: Option<Geometry>,
    pub east_hemisphere_geometry: Option<Geometry>,
}

impl GeometryInstance {
    /// `model_matrix` defaults to the identity.
    pub fn new(geometry: Geometry, model_matrix: Option<Matrix4>) -> Self {
        Self {
            geometry: Some(geometry),
            model_matrix: model_matrix.unwrap_or(Matrix4::IDENTITY),
            id: None,
            attributes: Vec::new(),
            west_hemisphere_geometry: None,
            east_hemisphere_geometry: None,
        }
    }
}
