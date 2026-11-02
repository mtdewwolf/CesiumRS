//! CesiumJS `ShadowVolumeAppearance`: classification shader synthesis for
//! 3D Tiles and terrain clamping, and the per-instance texture-coordinate
//! attributes that ground and classification primitives attach.
//!
//! Cesium tests `appearance instanceof PerInstanceColorAppearance`. The port
//! treats an appearance without a material as per-instance color. In Cesium,
//! any other appearance without a material throws when its shader source is
//! read.

use std::cell::OnceCell;

use super::appearance::Appearance;
use crate::core::math::{self, fast_approximate_atan2, js_max, js_min, js_number_to_string, js_to_fixed};
use crate::core::{
    Cartesian3, Cartographic, ComponentDatatype, Ellipsoid, EncodedCartesian3, GeometryInstanceAttribute,
    MapProjection, Rectangle, RuntimeError, Transforms, TypedArray,
};
use crate::renderer::{PickColorQualifier, ShaderSource};
use crate::shaders::root as root_shaders;

/// Which eye-space, world-space and texture-coordinate inputs a shader needs.
/// Fields are the result of Cesium's propagating setters: setting `st` also
/// requires texture coordinates, which require world coordinates, which
/// require eye coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShaderDependencies {
    pub requires_ec: bool,
    pub requires_wc: bool,
    pub requires_texture_coordinates: bool,
    pub requires_normal_ec: bool,
    pub normal_ec: bool,
    pub position_to_eye_ec: bool,
    pub tangent_to_eye_matrix: bool,
    pub st: bool,
}

/// Instance attributes by name, in Cesium's insertion order.
pub type ShadowVolumeAttributes = Vec<(String, GeometryInstanceAttribute)>;

pub struct ShadowVolumeAppearance {
    pub extents_culling: bool,
    pub planar_extents: bool,
    pub appearance: Box<dyn Appearance>,
    pub color_dependencies: ShaderDependencies,
    pub pick_dependencies: ShaderDependencies,
    /// `_projectionExtentDefines`: computed from the projection passed to the
    /// first vertex-shader call and reused for every later call.
    projection_extent_defines: OnceCell<[String; 4]>,
}

impl ShadowVolumeAppearance {
    /// `ShadowVolumeAppearance.MAX_WIDTH_FOR_PLANAR_EXTENTS`: one degree.
    pub const MAX_WIDTH_FOR_PLANAR_EXTENTS: f64 = math::RADIANS_PER_DEGREE;

    pub fn new(extents_culling: bool, planar_extents: bool, appearance: Box<dyn Appearance>) -> Self {
        let is_flat = appearance.flat().unwrap_or(false);
        let mut color_dependencies = ShaderDependencies {
            requires_texture_coordinates: extents_culling,
            requires_ec: !is_flat,
            ..Default::default()
        };

        let mut pick_dependencies =
            ShaderDependencies { requires_texture_coordinates: extents_culling, ..Default::default() };

        if let Some(mat) = appearance.material() {
            let combined = format!("{}\n{}", mat.shader_source, appearance.fragment_shader_source());
            color_dependencies.normal_ec =
                combined.contains("materialInput.normalEC") || combined.contains("czm_getDefaultMaterial");
            color_dependencies.position_to_eye_ec = combined.contains("materialInput.positionToEyeEC");
            color_dependencies.tangent_to_eye_matrix = combined.contains("materialInput.tangentToEyeMatrix");
            color_dependencies.st = combined.contains("materialInput.st");
        } else {
            color_dependencies.requires_normal_ec = !is_flat;
        }

        // Upstream dependency setters propagate requirements transitively.
        color_dependencies.requires_normal_ec |=
            color_dependencies.normal_ec || color_dependencies.tangent_to_eye_matrix;
        color_dependencies.requires_texture_coordinates |= color_dependencies.st;
        color_dependencies.requires_wc |=
            color_dependencies.requires_texture_coordinates || color_dependencies.tangent_to_eye_matrix;
        color_dependencies.requires_ec |= color_dependencies.requires_wc
            || color_dependencies.requires_normal_ec
            || color_dependencies.position_to_eye_ec;
        pick_dependencies.requires_wc |= pick_dependencies.requires_texture_coordinates;
        pick_dependencies.requires_ec |= pick_dependencies.requires_wc;
        Self {
            extents_culling,
            planar_extents,
            appearance,
            color_dependencies,
            pick_dependencies,
            projection_extent_defines: OnceCell::new(),
        }
    }

    fn per_instance_color(&self) -> bool {
        self.appearance.material().is_none()
    }

    /// Derive the classification vertex stage with encoded projection extents.
    pub fn create_vertex_shader(
        &self,
        defines: &[String],
        source: &str,
        columbus_view_2d: bool,
        projection: &dyn MapProjection,
    ) -> ShaderSource {
        self.vertex_shader(defines, source, columbus_view_2d, projection, false)
    }

    pub fn create_pick_vertex_shader(
        &self,
        defines: &[String],
        source: &str,
        columbus_view_2d: bool,
        projection: &dyn MapProjection,
    ) -> ShaderSource {
        self.vertex_shader(defines, source, columbus_view_2d, projection, true)
    }

    fn vertex_shader(
        &self,
        defines: &[String],
        source: &str,
        cv: bool,
        projection: &dyn MapProjection,
        pick: bool,
    ) -> ShaderSource {
        let mut shader = ShaderSource::new(source);
        shader.defines = defines.to_vec();
        let extents = self.projection_extent_defines.get_or_init(|| projection_extent_defines(projection));
        if cv {
            shader.defines.extend(extents.iter().cloned());
        }
        if !pick && self.per_instance_color() {
            shader.defines.push("PER_INSTANCE_COLOR".into());
        }
        let dependencies = if pick { self.pick_dependencies } else { self.color_dependencies };
        if dependencies.requires_texture_coordinates {
            shader.defines.push("TEXTURE_COORDINATES".into());
            if !(self.planar_extents || cv) {
                shader.defines.push("SPHERICAL".into());
            }
            if cv {
                shader.defines.push("COLUMBUS_VIEW_2D".into());
            }
        }
        shader
    }

    /// `createFragmentShader`: the classification color pass, as the material
    /// source (empty for per-instance color) followed by
    /// `ShadowVolumeAppearanceFS`.
    pub fn create_fragment_shader(&self, columbus_view_2d: bool) -> ShaderSource {
        let dependencies = self.color_dependencies;
        let mut defines = Vec::new();
        if !columbus_view_2d && !self.planar_extents {
            defines.push("SPHERICAL");
        }
        if dependencies.requires_ec {
            defines.push("REQUIRES_EC");
        }
        if dependencies.requires_wc {
            defines.push("REQUIRES_WC");
        }
        if dependencies.requires_texture_coordinates {
            defines.push("TEXTURE_COORDINATES");
        }
        if self.extents_culling {
            defines.push("CULL_FRAGMENTS");
        }
        if dependencies.requires_normal_ec {
            defines.push("NORMAL_EC");
        }
        if self.per_instance_color() {
            defines.push("PER_INSTANCE_COLOR");
        }
        if dependencies.normal_ec {
            defines.push("USES_NORMAL_EC");
        }
        if dependencies.position_to_eye_ec {
            defines.push("USES_POSITION_TO_EYE_EC");
        }
        if dependencies.tangent_to_eye_matrix {
            defines.push("USES_TANGENT_TO_EYE");
        }
        if dependencies.st {
            defines.push("USES_ST");
        }
        if self.appearance.flat() == Some(true) {
            defines.push("FLAT");
        }

        let material_source = self.appearance.material().map_or_else(String::new, |m| m.shader_source.clone());
        ShaderSource {
            defines: defines.into_iter().map(String::from).collect(),
            sources: vec![material_source, root_shaders::SHADOW_VOLUME_APPEARANCE_FS.source.to_string()],
            ..Default::default()
        }
    }

    /// `createPickFragmentShader`: the pick pass reads its pick color from a
    /// varying (`pickColorQualifier: "in"`).
    pub fn create_pick_fragment_shader(&self, columbus_view_2d: bool) -> ShaderSource {
        let dependencies = self.pick_dependencies;
        let mut defines = vec!["PICK"];
        if !columbus_view_2d && !self.planar_extents {
            defines.push("SPHERICAL");
        }
        if dependencies.requires_ec {
            defines.push("REQUIRES_EC");
        }
        if dependencies.requires_wc {
            defines.push("REQUIRES_WC");
        }
        if dependencies.requires_texture_coordinates {
            defines.push("TEXTURE_COORDINATES");
        }
        if self.extents_culling {
            defines.push("CULL_FRAGMENTS");
        }
        ShaderSource {
            defines: defines.into_iter().map(String::from).collect(),
            sources: vec![root_shaders::SHADOW_VOLUME_APPEARANCE_FS.source.to_string()],
            pick_color_qualifier: Some(PickColorQualifier::In),
            ..Default::default()
        }
    }

    /// `getPlanarTextureCoordinateAttributes`: the high-precision south-west
    /// corner and the eastward/northward vectors of the rectangle's planar
    /// bounds, the texture-coordinate rotation, and the 2D/Columbus View
    /// planes. Used for small rectangles. `height` defaults to 0.
    pub fn get_planar_texture_coordinate_attributes(
        bounding_rectangle: &Rectangle,
        texture_coordinate_rotation_points: &[f64; 6],
        ellipsoid: &Ellipsoid,
        projection: &dyn MapProjection,
        height: Option<f64>,
    ) -> Result<ShadowVolumeAttributes, RuntimeError> {
        let (corner, eastward, northward) =
            compute_rectangle_bounds(bounding_rectangle, ellipsoid, height.unwrap_or(0.0))?;
        let mut attributes = Vec::new();
        add_texture_coordinate_rotation_attributes(&mut attributes, texture_coordinate_rotation_points);
        let encoded = EncodedCartesian3::from_cartesian(corner);
        let xyz = |c: Cartesian3| vec![c.x, c.y, c.z];
        attributes.push(("southWest_HIGH".into(), float_attribute(xyz(encoded.high))));
        attributes.push(("southWest_LOW".into(), float_attribute(xyz(encoded.low))));
        attributes.push(("eastward".into(), float_attribute(xyz(eastward))));
        attributes.push(("northward".into(), float_attribute(xyz(northward))));
        add_2d_texture_coordinate_attributes(bounding_rectangle, projection, &mut attributes);
        Ok(attributes)
    }

    /// `getSphericalExtentGeometryInstanceAttributes`: the rectangle's
    /// south-west corner and inverse ranges in the shader's approximate
    /// spherical coordinates, an antimeridian rotation, the
    /// texture-coordinate rotation and the 2D/Columbus View planes. Used for
    /// large rectangles.
    pub fn get_spherical_extent_geometry_instance_attributes(
        bounding_rectangle: &Rectangle,
        texture_coordinate_rotation_points: &[f64; 6],
        ellipsoid: &Ellipsoid,
        projection: &dyn MapProjection,
    ) -> Result<ShadowVolumeAttributes, RuntimeError> {
        // Rectangle coordinates are not spherical because they are on an ellipsoid.
        let (mut south, mut west) =
            lat_long_to_spherical(bounding_rectangle.south, bounding_rectangle.west, ellipsoid)?;
        let (mut north, mut east) =
            lat_long_to_spherical(bounding_rectangle.north, bounding_rectangle.east, ellipsoid)?;

        // Rotate extents that cross the antimeridian; the shader rotates too.
        let mut rotation_radians = 0.0;
        if west > east {
            rotation_radians = math::PI - west;
            west = -math::PI;
            east += rotation_radians;
        }

        // Pad slightly against floating-point error when culling fragments at edges.
        south -= math::EPSILON5;
        west -= math::EPSILON5;
        north += math::EPSILON5;
        east += math::EPSILON5;

        let longitude_range_inverse = 1.0 / (east - west);
        let latitude_range_inverse = 1.0 / (north - south);

        let mut attributes = vec![
            (
                "sphericalExtents".to_string(),
                float_attribute(vec![south, west, latitude_range_inverse, longitude_range_inverse]),
            ),
            ("longitudeRotation".to_string(), float_attribute(vec![rotation_radians])),
        ];
        add_texture_coordinate_rotation_attributes(&mut attributes, texture_coordinate_rotation_points);
        add_2d_texture_coordinate_attributes(bounding_rectangle, projection, &mut attributes);
        Ok(attributes)
    }

    /// `hasAttributesForTextureCoordinatePlanes`.
    pub fn has_attributes_for_texture_coordinate_planes(attributes: &[(String, GeometryInstanceAttribute)]) -> bool {
        has_all(
            attributes,
            &[
                "southWest_HIGH",
                "southWest_LOW",
                "northward",
                "eastward",
                "planes2D_HIGH",
                "planes2D_LOW",
                "uMaxVmax",
                "uvMinAndExtents",
            ],
        )
    }

    /// `hasAttributesForSphericalExtents`.
    pub fn has_attributes_for_spherical_extents(attributes: &[(String, GeometryInstanceAttribute)]) -> bool {
        has_all(
            attributes,
            &["sphericalExtents", "longitudeRotation", "planes2D_HIGH", "planes2D_LOW", "uMaxVmax", "uvMinAndExtents"],
        )
    }

    /// `shouldUseSphericalCoordinates`: whether the rectangle is wider or
    /// taller than [`Self::MAX_WIDTH_FOR_PLANAR_EXTENTS`].
    pub fn should_use_spherical_coordinates(rectangle: &Rectangle) -> bool {
        js_max(rectangle.width(), rectangle.height()) > Self::MAX_WIDTH_FOR_PLANAR_EXTENTS
    }
}

fn has_all(attributes: &[(String, GeometryInstanceAttribute)], names: &[&str]) -> bool {
    names.iter().all(|name| attributes.iter().any(|(n, _)| n == name))
}

/// The four `EAST/WEST_MOST_X_HIGH/LOW` defines, formatted as Cesium does
/// with `value.toFixed(String(value).length + 1)`.
fn projection_extent_defines(projection: &dyn MapProjection) -> [String; 4] {
    let mut defines = Vec::with_capacity(4);
    for (name, longitude) in [("EAST", math::PI), ("WEST", -math::PI)] {
        let x = projection.project(&Cartographic::new(longitude, 0.0, 0.0)).x;
        let encoded = EncodedCartesian3::encode(x);
        for (part, value) in [("HIGH", encoded.high), ("LOW", encoded.low)] {
            let digits = js_number_to_string(value).len() + 1;
            defines.push(format!("{name}_MOST_X_{part} {}", js_to_fixed(value, digits)));
        }
    }
    defines.try_into().expect("four projection extent defines")
}

/// A `FLOAT` instance attribute whose value is a plain JavaScript number
/// array. The doubles are kept; conversion to `f32` happens on upload.
fn float_attribute(values: Vec<f64>) -> GeometryInstanceAttribute {
    let components = u8::try_from(values.len()).expect("one to four components");
    GeometryInstanceAttribute::new(ComponentDatatype::Float, components, Some(false), TypedArray::Float64(values))
}

/// `pointLineDistance`: the distance from `point` to the line through
/// `point1` and `point2`.
fn point_line_distance(point1: (f64, f64), point2: (f64, f64), point: (f64, f64)) -> f64 {
    let (dx, dy) = (point2.0 - point1.0, point2.1 - point1.1);
    ((point2.1 - point1.1) * point.0 - (point2.0 - point1.0) * point.1 + point2.0 * point1.1 - point2.1 * point1.0)
        .abs()
        / (dx * dx + dy * dy).sqrt()
}

/// `addTextureCoordinateRotationAttributes`: the rotation points form two
/// lines in the computed UV space that remap to the geometry's own texture
/// coordinates.
fn add_texture_coordinate_rotation_attributes(attributes: &mut ShadowVolumeAttributes, points: &[f64; 6]) {
    let min_xy_corner = (points[0], points[1]);
    let max_y_corner = (points[2], points[3]);
    let max_x_corner = (points[4], points[5]);
    attributes.push((
        "uMaxVmax".into(),
        float_attribute(vec![max_y_corner.0, max_y_corner.1, max_x_corner.0, max_x_corner.1]),
    ));
    let inverse_extent_x = 1.0 / point_line_distance(min_xy_corner, max_y_corner, max_x_corner);
    let inverse_extent_y = 1.0 / point_line_distance(min_xy_corner, max_x_corner, max_y_corner);
    attributes.push((
        "uvMinAndExtents".into(),
        float_attribute(vec![min_xy_corner.0, min_xy_corner.1, inverse_extent_x, inverse_extent_y]),
    ));
}

/// `add2DTextureCoordinateAttributes`: the projected south-west x and y, the
/// north-west y and the south-east x, each split into high and low parts.
fn add_2d_texture_coordinate_attributes(
    rectangle: &Rectangle,
    projection: &dyn MapProjection,
    attributes: &mut ShadowVolumeAttributes,
) {
    let south_west = projection.project(&Cartographic::new(rectangle.west, rectangle.south, 0.0));
    let north_west = projection.project(&Cartographic::new(rectangle.west, rectangle.north, 0.0));
    let south_east = projection.project(&Cartographic::new(rectangle.east, rectangle.south, 0.0));
    let encoded = [south_west.x, south_west.y, north_west.y, south_east.x].map(EncodedCartesian3::encode);
    attributes.push(("planes2D_HIGH".into(), float_attribute(encoded.iter().map(|e| e.high).collect())));
    attributes.push(("planes2D_LOW".into(), float_attribute(encoded.iter().map(|e| e.low).collect())));
}

/// `computeRectangleBounds`: flattens the rectangle's corners and edge
/// midpoints into the plane of the local east-north-up frame at its center,
/// bounds them in 2D, and returns the world-space south-west corner and the
/// eastward and northward edge vectors.
fn compute_rectangle_bounds(
    rectangle: &Rectangle,
    ellipsoid: &Ellipsoid,
    height: f64,
) -> Result<(Cartesian3, Cartesian3, Cartesian3), RuntimeError> {
    let mut center = rectangle.center();
    center.height = height;
    let enu = Transforms::east_north_up_to_fixed_frame(center.to_cartesian(ellipsoid), ellipsoid);
    let inverse_enu = enu.inverse()?;

    let (west, east, north, south) = (rectangle.west, rectangle.east, rectangle.north, rectangle.south);
    let longitude_center = (west + east) * 0.5;
    let latitude_center = (north + south) * 0.5;
    let points = [
        (west, south),
        (west, north),
        (east, north),
        (east, south),
        (longitude_center, south),
        (longitude_center, north),
        (west, latitude_center),
        (east, latitude_center),
    ];

    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for (longitude, latitude) in points {
        let local =
            inverse_enu.multiply_by_point(Cartographic::new(longitude, latitude, height).to_cartesian(ellipsoid));
        min_x = js_min(min_x, local.x);
        max_x = js_max(max_x, local.x);
        min_y = js_min(min_y, local.y);
        max_y = js_max(max_y, local.y);
    }

    let south_west = enu.multiply_by_point(Cartesian3::new(min_x, min_y, 0.0));
    let south_east = enu.multiply_by_point(Cartesian3::new(max_x, min_y, 0.0));
    let north_west = enu.multiply_by_point(Cartesian3::new(min_x, max_y, 0.0));
    Ok((south_west, south_east - south_west, north_west - south_west))
}

/// `latLongToSpherical`: latitude and longitude on a sphere through the
/// ellipsoid point, using the shader's `atan2` approximation.
fn lat_long_to_spherical(latitude: f64, longitude: f64, ellipsoid: &Ellipsoid) -> Result<(f64, f64), RuntimeError> {
    let point = Cartographic::new(longitude, latitude, 0.0).to_cartesian(ellipsoid);
    let mag_xy = (point.x * point.x + point.y * point.y).sqrt();
    // Cesium's fastApproximateAtan2 rejects (0, 0) with a DeveloperError.
    if (mag_xy == 0.0 && point.z == 0.0) || (point.x == 0.0 && point.y == 0.0) {
        return Err(RuntimeError::new("x and y cannot both be zero"));
    }
    Ok((fast_approximate_atan2(mag_xy, point.z), fast_approximate_atan2(point.x, point.y)))
}
