//! Port of the CesiumJS `Core` package (`@cesium/engine` 23.0.0, CesiumJS 1.139).
//!
//! One Rust module per Cesium module, named in snake_case. Cesium's
//! `result` parameters become return values, and functions that can fail
//! return `Option` or `Result` where Cesium returns `undefined` or throws.

// Constants are copied digit for digit from CesiumJS.
#![allow(clippy::excessive_precision)]

pub(crate) mod cartesian;
pub(crate) mod matrix;

pub mod axis_aligned_bounding_box;
pub mod bounding_rectangle;
pub mod bounding_sphere;
pub mod cartesian2;
pub mod cartesian3;
pub mod cartesian4;
pub mod cartographic;
pub mod color;
pub mod cubic_real_polynomial;
pub mod culling_volume;
pub mod ellipsoid;
pub mod ellipsoid_geodesic;
pub mod ellipsoid_rhumb_line;
pub mod ellipsoid_tangent_plane;
pub mod ellipsoidal_occluder;
pub mod geographic_projection;
pub mod heading_pitch_range;
pub mod heading_pitch_roll;
pub mod intersect;
pub mod intersection_tests;
pub mod interval;
pub mod map_projection;
pub mod math;
pub mod matrix2;
pub mod matrix3;
pub mod matrix4;
pub mod occluder;
pub mod oriented_bounding_box;
pub mod orthographic_frustum;
pub mod orthographic_off_center_frustum;
pub mod perspective_frustum;
pub mod perspective_off_center_frustum;
pub mod plane;
pub mod quadratic_real_polynomial;
pub mod quartic_real_polynomial;
pub mod quaternion;
pub mod ray;
pub mod rectangle;
pub mod runtime_error;
pub mod scale_to_geodetic_surface;
pub mod spherical;
pub mod transforms;
pub mod translation_rotation_scale;
pub mod visibility;
pub mod web_mercator_projection;

pub use axis_aligned_bounding_box::AxisAlignedBoundingBox;
pub use bounding_rectangle::BoundingRectangle;
pub use bounding_sphere::BoundingSphere;
pub use cartesian2::Cartesian2;
pub use cartesian3::Cartesian3;
pub use cartesian4::Cartesian4;
pub use cartographic::Cartographic;
pub use color::Color;
pub use culling_volume::{BoundingVolume, CullingVolume};
pub use ellipsoid::Ellipsoid;
pub use ellipsoid_geodesic::EllipsoidGeodesic;
pub use ellipsoid_rhumb_line::EllipsoidRhumbLine;
pub use ellipsoid_tangent_plane::EllipsoidTangentPlane;
pub use ellipsoidal_occluder::EllipsoidalOccluder;
pub use geographic_projection::GeographicProjection;
pub use heading_pitch_range::HeadingPitchRange;
pub use heading_pitch_roll::HeadingPitchRoll;
pub use intersect::Intersect;
pub use interval::Interval;
pub use map_projection::MapProjection;
pub use matrix2::Matrix2;
pub use matrix3::Matrix3;
pub use matrix4::Matrix4;
pub use occluder::Occluder;
pub use oriented_bounding_box::OrientedBoundingBox;
pub use orthographic_frustum::OrthographicFrustum;
pub use orthographic_off_center_frustum::OrthographicOffCenterFrustum;
pub use perspective_frustum::PerspectiveFrustum;
pub use perspective_off_center_frustum::PerspectiveOffCenterFrustum;
pub use plane::Plane;
pub use quaternion::Quaternion;
pub use ray::Ray;
pub use rectangle::Rectangle;
pub use runtime_error::RuntimeError;
pub use spherical::Spherical;
pub use transforms::{LocalAxis, LocalFrame, Transforms};
pub use translation_rotation_scale::TranslationRotationScale;
pub use visibility::Visibility;
pub use web_mercator_projection::WebMercatorProjection;
