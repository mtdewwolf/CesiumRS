//! CesiumJS `createTangentSpaceDebugPrimitive`: creates a [`Primitive`] to visualize
//! well-known vector vertex attributes (`normal`, `tangent`, and `bitangent`).
//!
//! Normal is red, tangent is green, and bitangent is blue. If an attribute is
//! not present on the geometry, it is omitted.

use super::per_instance_color_appearance::{PerInstanceColorAppearance, PerInstanceColorAppearanceOptions};
use super::primitive::{Primitive, PrimitiveOptions};
use crate::core::color_geometry_instance_attribute::ColorGeometryInstanceAttribute;
use crate::core::geometry_pipeline::create_line_segments_for_vectors;
use crate::core::{Color, Geometry, GeometryInstance, Matrix4};

#[derive(Clone, Debug, PartialEq)]
pub struct CreateTangentSpaceDebugPrimitiveOptions {
    pub geometry: Geometry,
    pub length: Option<f64>,
    pub model_matrix: Option<Matrix4>,
}

impl CreateTangentSpaceDebugPrimitiveOptions {
    pub fn new(geometry: Geometry) -> Self {
        Self {
            geometry,
            length: None,
            model_matrix: None,
        }
    }
}

/// Creates a [`Primitive`] with lines visualizing normal, tangent, and bitangent
/// attributes of a geometry.
pub fn create_tangent_space_debug_primitive(
    options: CreateTangentSpaceDebugPrimitiveOptions,
) -> Option<Primitive> {
    let geometry = options.geometry;
    let length = options.length.unwrap_or(10000.0);
    let model_matrix = options.model_matrix.unwrap_or(Matrix4::IDENTITY);
    let mut instances = Vec::new();

    if geometry.attributes.normal.is_some() {
        if let Some(lines) = create_line_segments_for_vectors(&geometry, Some("normal"), Some(length)) {
            let mut instance = GeometryInstance::new(lines, Some(model_matrix));
            instance.attributes.push((
                "color".into(),
                ColorGeometryInstanceAttribute::from_color(&Color::new(1.0, 0.0, 0.0, 1.0)).into(),
            ));
            instances.push(instance);
        }
    }

    if geometry.attributes.tangent.is_some() {
        if let Some(lines) = create_line_segments_for_vectors(&geometry, Some("tangent"), Some(length)) {
            let mut instance = GeometryInstance::new(lines, Some(model_matrix));
            instance.attributes.push((
                "color".into(),
                ColorGeometryInstanceAttribute::from_color(&Color::new(0.0, 1.0, 0.0, 1.0)).into(),
            ));
            instances.push(instance);
        }
    }

    if geometry.attributes.bitangent.is_some() {
        if let Some(lines) = create_line_segments_for_vectors(&geometry, Some("bitangent"), Some(length)) {
            let mut instance = GeometryInstance::new(lines, Some(model_matrix));
            instance.attributes.push((
                "color".into(),
                ColorGeometryInstanceAttribute::from_color(&Color::new(0.0, 0.0, 1.0, 1.0)).into(),
            ));
            instances.push(instance);
        }
    }

    if instances.is_empty() {
        return None;
    }

    let primitive = Primitive::new(PrimitiveOptions {
        geometry_instances: instances,
        appearance: Some(Box::new(PerInstanceColorAppearance::new(
            PerInstanceColorAppearanceOptions {
                flat: Some(true),
                translucent: Some(false),
                ..Default::default()
            },
        ))),
        asynchronous: Some(false),
        ..Default::default()
    });

    Some(primitive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::{GeometryAttribute, GeometryAttributes};
    use crate::core::PrimitiveType;

    #[test]
    fn creates_lines_for_normal_attribute() {
        let positions = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let normals = vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];

        let geom = Geometry::new(
            GeometryAttributes {
                position: Some(GeometryAttribute::doubles(3, positions)),
                normal: Some(GeometryAttribute::doubles(3, normals)),
                ..Default::default()
            },
            None,
            PrimitiveType::Triangles,
        );

        let prim = create_tangent_space_debug_primitive(
            CreateTangentSpaceDebugPrimitiveOptions::new(geom),
        );
        assert!(prim.is_some());
        let primitive = prim.unwrap();
        assert_eq!(primitive.asynchronous, false);
    }

    #[test]
    fn returns_none_when_no_tangent_space_attributes() {
        let positions = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let geom = Geometry::new(
            GeometryAttributes {
                position: Some(GeometryAttribute::doubles(3, positions)),
                ..Default::default()
            },
            None,
            PrimitiveType::Triangles,
        );

        let prim = create_tangent_space_debug_primitive(
            CreateTangentSpaceDebugPrimitiveOptions::new(geom),
        );
        assert!(prim.is_none());
    }
}
