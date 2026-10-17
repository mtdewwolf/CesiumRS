//! Port of the CesiumJS `Core` package (`@cesium/engine` 23.0.0, CesiumJS 1.139).
//!
//! One Rust module per Cesium module, named in snake_case. Cesium's
//! `result` parameters become return values, and functions that can fail
//! return `Option` or `Result` where Cesium returns `undefined` or throws.

// Constants are copied digit for digit from CesiumJS.
#![allow(clippy::excessive_precision)]

pub(crate) mod cartesian;
pub(crate) mod matrix;

pub mod bounding_rectangle;
pub mod bounding_sphere;
pub mod cartesian2;
pub mod cartesian3;
pub mod cartesian4;
pub mod cartographic;
pub mod color;
pub mod ellipsoid;
pub mod geographic_projection;
pub mod heading_pitch_range;
pub mod heading_pitch_roll;
pub mod intersect;
pub mod interval;
pub mod map_projection;
pub mod math;
pub mod matrix2;
pub mod matrix3;
pub mod matrix4;
pub mod plane;
pub mod quaternion;
pub mod rectangle;
pub mod runtime_error;
pub mod scale_to_geodetic_surface;
pub mod spherical;
pub mod transforms;
pub mod translation_rotation_scale;
pub mod web_mercator_projection;

pub use bounding_rectangle::BoundingRectangle;
pub use bounding_sphere::BoundingSphere;
pub use cartesian2::Cartesian2;
pub use cartesian3::Cartesian3;
pub use cartesian4::Cartesian4;
pub use cartographic::Cartographic;
pub use color::Color;
pub use ellipsoid::Ellipsoid;
pub use geographic_projection::GeographicProjection;
pub use heading_pitch_range::HeadingPitchRange;
pub use heading_pitch_roll::HeadingPitchRoll;
pub use intersect::Intersect;
pub use interval::Interval;
pub use map_projection::MapProjection;
pub use matrix2::Matrix2;
pub use matrix3::Matrix3;
pub use matrix4::Matrix4;
pub use plane::Plane;
pub use quaternion::Quaternion;
pub use rectangle::Rectangle;
pub use runtime_error::RuntimeError;
pub use spherical::Spherical;
pub use transforms::{LocalAxis, LocalFrame, Transforms};
pub use translation_rotation_scale::TranslationRotationScale;
pub use web_mercator_projection::WebMercatorProjection;
