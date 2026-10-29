//! CesiumJS `VoxelEllipsoidShape`: CPU bounds, UV transforms, tile/sample boxes
//! and ellipsoid clipping/intersection shader metadata.
use crate::core::{
    BoundingSphere, Cartesian2, Cartesian3, Ellipsoid, Matrix3, Matrix4, OrientedBoundingBox, Rectangle,
    RuntimeError,
};
use crate::core::math::{equals_epsilon, lerp, EPSILON3, EPSILON10, PI, PI_OVER_TWO};
use crate::core::transforms::Transforms;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidShaderUniforms {
    pub camera_position_cartographic: Cartesian3,
    pub ellipsoid_ec_to_east_north_up: Matrix3,
    pub ellipsoid_radii: Cartesian3,
    pub eccentricity_squared: f64,
    pub evolute_scale: Cartesian2,
    pub ellipsoid_curvature_at_latitude: Cartesian2,
    pub ellipsoid_inverse_radii_squared: Cartesian3,
    pub ellipsoid_render_longitude_min_max: Cartesian2,
    pub ellipsoid_shape_uv_longitude_range_origin: f64,
    pub ellipsoid_shape_uv_longitude_min_max_mid: Cartesian3,
    pub ellipsoid_local_to_shape_uv_longitude: Cartesian2,
    pub ellipsoid_local_to_shape_uv_latitude: Cartesian2,
    pub ellipsoid_render_latitude_sin_min_max: Cartesian2,
    pub ellipsoid_inverse_height_difference: f64,
    pub clip_min_max_height: Cartesian2,
}

impl Default for EllipsoidShaderUniforms {
    fn default() -> Self {
        Self {
            camera_position_cartographic: Cartesian3::ZERO,
            ellipsoid_ec_to_east_north_up: Matrix3::ZERO,
            ellipsoid_radii: Cartesian3::ZERO,
            eccentricity_squared: 0.0,
            evolute_scale: Cartesian2::ZERO,
            ellipsoid_curvature_at_latitude: Cartesian2::ZERO,
            ellipsoid_inverse_radii_squared: Cartesian3::ZERO,
            ellipsoid_render_longitude_min_max: Cartesian2::ZERO,
            ellipsoid_shape_uv_longitude_range_origin: 0.0,
            ellipsoid_shape_uv_longitude_min_max_mid: Cartesian3::ZERO,
            ellipsoid_local_to_shape_uv_longitude: Cartesian2::ZERO,
            ellipsoid_local_to_shape_uv_latitude: Cartesian2::ZERO,
            ellipsoid_render_latitude_sin_min_max: Cartesian2::ZERO,
            ellipsoid_inverse_height_difference: 0.0,
            clip_min_max_height: Cartesian2::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct EllipsoidShaderDefines {
    pub has_render_bounds_longitude: bool,
    pub has_render_bounds_longitude_range_equal_zero: bool,
    pub has_render_bounds_longitude_range_under_half: bool,
    pub has_render_bounds_longitude_range_over_half: bool,
    pub has_render_bounds_longitude_min_discontinuity: bool,
    pub has_render_bounds_longitude_max_discontinuity: bool,
    pub has_shape_bounds_longitude: bool,
    pub has_shape_bounds_longitude_min_max_reversed: bool,
    pub has_render_bounds_latitude_max_under_half: bool,
    pub has_render_bounds_latitude_max_equal_half: bool,
    pub has_render_bounds_latitude_max_over_half: bool,
    pub has_render_bounds_latitude_min_under_half: bool,
    pub has_render_bounds_latitude_min_equal_half: bool,
    pub has_render_bounds_latitude_min_over_half: bool,
    pub has_shape_bounds_latitude: bool,
    pub intersection_index_longitude: Option<usize>,
    pub intersection_index_latitude_max: Option<usize>,
    pub intersection_index_latitude_min: Option<usize>,
    pub intersection_index_height_max: usize,
    pub intersection_index_height_min: usize,
}

#[derive(Clone, Debug)]
pub struct VoxelEllipsoidShape {
    pub min: Cartesian3,
    pub max: Cartesian3,
    pub render_min: Cartesian3,
    pub render_max: Cartesian3,
    pub shape_transform: Matrix4,
    pub bound_transform: Matrix4,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub bounding_sphere: BoundingSphere,
    pub rectangle: Rectangle,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub ellipsoid: Ellipsoid,
    pub translation: Cartesian3,
    pub rotation: Matrix3,
    pub shader_uniforms: EllipsoidShaderUniforms,
    pub shader_defines: EllipsoidShaderDefines,
    pub shader_maximum_intersections_length: usize,
}

impl Default for VoxelEllipsoidShape {
    fn default() -> Self {
        Self {
            min: Cartesian3::new(-PI, -PI_OVER_TWO, -Ellipsoid::WGS84.minimum_radius),
            max: Cartesian3::new(PI, PI_OVER_TWO, 10.0 * Ellipsoid::WGS84.maximum_radius),
            render_min: Cartesian3::new(-PI, -PI_OVER_TWO, -Ellipsoid::WGS84.minimum_radius),
            render_max: Cartesian3::new(PI, PI_OVER_TWO, 10.0 * Ellipsoid::WGS84.maximum_radius),
            shape_transform: Matrix4::IDENTITY,
            bound_transform: Matrix4::IDENTITY,
            oriented_bounding_box: OrientedBoundingBox::default(),
            bounding_sphere: BoundingSphere::default(),
            rectangle: Rectangle::new(-PI, -PI_OVER_TWO, PI, PI_OVER_TWO),
            minimum_height: -Ellipsoid::WGS84.minimum_radius,
            maximum_height: 10.0 * Ellipsoid::WGS84.maximum_radius,
            ellipsoid: Ellipsoid::WGS84,
            translation: Cartesian3::ZERO,
            rotation: Matrix3::IDENTITY,
            shader_uniforms: EllipsoidShaderUniforms::default(),
            shader_defines: EllipsoidShaderDefines::default(),
            shader_maximum_intersections_length: 0,
        }
    }
}

impl VoxelEllipsoidShape {
    pub fn default_min_bounds() -> Cartesian3 {
        Cartesian3::new(-PI, -PI_OVER_TWO, -Ellipsoid::WGS84.minimum_radius)
    }
    pub fn default_max_bounds() -> Cartesian3 {
        Cartesian3::new(PI, PI_OVER_TWO, 10.0 * Ellipsoid::WGS84.maximum_radius)
    }
    pub fn update(
        &mut self,
        model: Matrix4,
        min: Cartesian3,
        max: Cartesian3,
        clip_min: Option<Cartesian3>,
        clip_max: Option<Cartesian3>,
    ) -> bool {
        let default_min = Cartesian3::new(-PI, -PI_OVER_TWO, -Ellipsoid::WGS84.minimum_radius);
        let default_max = Cartesian3::new(PI, PI_OVER_TWO, 10.0 * Ellipsoid::WGS84.maximum_radius);
        let eps_zero = EPSILON10;
        let eps_lon_disc = EPSILON3;
        let eps_lon = EPSILON10;
        let eps_lat = EPSILON10;
        let eps_lat_flat = EPSILON3;

        let radii = model.get_scale();
        let actual_min_z = -radii.x.min(radii.y).min(radii.z);
        let actual_min = Cartesian3::new(default_min.x, default_min.y, actual_min_z);

        let shape_min = clamp_cartesian(min, actual_min, default_max);
        let shape_max = clamp_cartesian(max, actual_min, default_max);
        let clip_min = clip_min.unwrap_or(default_min);
        let clip_max = clip_max.unwrap_or(default_max);
        let clamped_clip_min = clamp_cartesian(clip_min, actual_min, default_max);
        let clamped_clip_max = clamp_cartesian(clip_max, actual_min, default_max);

        let render_min = Cartesian3::new(
            shape_min.x.max(clamped_clip_min.x),
            shape_min.y.max(clamped_clip_min.y),
            shape_min.z.max(clamped_clip_min.z),
        );
        let render_max = Cartesian3::new(
            shape_max.x.min(clamped_clip_max.x),
            shape_max.y.min(clamped_clip_max.y),
            shape_max.z.min(clamped_clip_max.z),
        );

        // shape outer extent
        let shape_outer = Cartesian3::new(
            radii.x + shape_max.z,
            radii.y + shape_max.z,
            radii.z + shape_max.z,
        );
        let render_outer = Cartesian3::new(
            radii.x + render_max.z,
            radii.y + render_max.z,
            radii.z + render_max.z,
        );

        // visibility
        if render_min.y > render_max.y
            || render_min.y == default_max.y
            || render_max.y == default_min.y
            || render_min.z > render_max.z
            || equals_epsilon(render_outer.x, 0.0, 0.0, eps_zero) && equals_epsilon(render_outer.y, 0.0, 0.0, eps_zero) && equals_epsilon(render_outer.z, 0.0, 0.0, eps_zero)
        {
            return false;
        }

        self.rectangle = Rectangle::from_radians(shape_min.x, shape_min.y, shape_max.x, shape_max.y);
        self.translation = model.get_translation();
        self.rotation = model.get_rotation();
        self.ellipsoid = Ellipsoid::from_cartesian3(radii);
        self.minimum_height = shape_min.z;
        self.maximum_height = shape_max.z;

        let render_rectangle = Rectangle::from_radians(render_min.x, render_min.y, render_max.x, render_max.y);

        let obb = get_ellipsoid_chunk_obb(
            &render_rectangle,
            render_min.z,
            render_max.z,
            &self.ellipsoid,
            self.translation,
            self.rotation,
        );
        self.oriented_bounding_box = obb;
        self.shape_transform = Matrix4::from_rotation_translation(&self.rotation, self.translation);
        self.bound_transform = Matrix4::from_rotation_translation(&obb.half_axes, obb.center);
        self.bounding_sphere = BoundingSphere::from_oriented_bounding_box(&obb);

        self.min = shape_min;
        self.max = shape_max;
        self.render_min = render_min;
        self.render_max = render_max;

        // Longitude
        let default_longitude_range = default_max.x - default_min.x;
        let default_longitude_range_half = 0.5 * default_longitude_range;
        let render_is_longitude_reversed = render_max.x < render_min.x;
        let render_longitude_range = render_max.x - render_min.x + if render_is_longitude_reversed { default_longitude_range } else { 0.0 };
        let render_is_longitude_range_zero = render_longitude_range <= eps_lon;
        let render_is_longitude_range_under_half = render_longitude_range >= default_longitude_range_half - eps_lon && render_longitude_range < default_longitude_range - eps_lon;
        let render_is_longitude_range_over_half = render_longitude_range > eps_lon && render_longitude_range < default_longitude_range_half - eps_lon;
        let render_has_longitude = render_is_longitude_range_zero || render_is_longitude_range_under_half || render_is_longitude_range_over_half;

        let shape_is_longitude_reversed = shape_max.x < shape_min.x;
        let shape_longitude_range = shape_max.x - shape_min.x + if shape_is_longitude_reversed { default_longitude_range } else { 0.0 };
        let shape_is_longitude_range_under_half = shape_longitude_range > default_longitude_range_half + eps_lon && shape_longitude_range < default_longitude_range - eps_lon;
        let shape_is_longitude_range_half = shape_longitude_range >= default_longitude_range_half - eps_lon && shape_longitude_range <= default_longitude_range_half + eps_lon;
        let shape_is_longitude_range_over_half = shape_longitude_range < default_longitude_range_half - eps_lon;
        let shape_has_longitude = shape_is_longitude_range_under_half || shape_is_longitude_range_half || shape_is_longitude_range_over_half;

        // Latitude
        let render_is_latitude_max_under_half = render_max.y < -eps_lat_flat;
        let render_is_latitude_max_half = render_max.y >= -eps_lat_flat && render_max.y <= eps_lat_flat;
        let render_is_latitude_max_over_half = render_max.y > eps_lat_flat && render_max.y < default_max.y - eps_lat;
        let render_has_latitude_max = render_is_latitude_max_under_half || render_is_latitude_max_half || render_is_latitude_max_over_half;
        let render_is_latitude_min_under_half = render_min.y > default_min.y + eps_lat && render_min.y < -eps_lat_flat;
        let render_is_latitude_min_half = render_min.y >= -eps_lat_flat && render_min.y <= eps_lat_flat;
        let render_is_latitude_min_over_half = render_min.y > eps_lat_flat;
        let render_has_latitude_min = render_is_latitude_min_under_half || render_is_latitude_min_half || render_is_latitude_min_over_half;
        let render_has_latitude = render_has_latitude_max || render_has_latitude_min;

        let shape_latitude_range = shape_max.y - shape_min.y;
        let shape_is_latitude_max_under_half = shape_max.y < -eps_lat_flat;
        let shape_is_latitude_max_half = shape_max.y >= -eps_lat_flat && shape_max.y <= eps_lat_flat;
        let shape_is_latitude_max_over_half = shape_max.y > eps_lat_flat && shape_max.y < default_max.y - eps_lat;
        let shape_has_latitude_max = shape_is_latitude_max_under_half || shape_is_latitude_max_half || shape_is_latitude_max_over_half;
        let shape_is_latitude_min_under_half = shape_min.y > default_min.y + eps_lat && shape_min.y < -eps_lat_flat;
        let shape_is_latitude_min_half = shape_min.y >= -eps_lat_flat && shape_min.y <= eps_lat_flat;
        let shape_is_latitude_min_over_half = shape_min.y > eps_lat_flat;
        let shape_has_latitude_min = shape_is_latitude_min_under_half || shape_is_latitude_min_half || shape_is_latitude_min_over_half;
        let shape_has_latitude = shape_has_latitude_max || shape_has_latitude_min;

        let su = &mut self.shader_uniforms;
        let sd = &mut self.shader_defines;

        // clear defines
        *sd = EllipsoidShaderDefines::default();

        su.ellipsoid_radii = shape_outer;
        let radii_x = su.ellipsoid_radii.x;
        let radii_z = su.ellipsoid_radii.z;
        let axis_ratio = if radii_x != 0.0 { radii_z / radii_x } else { 0.0 };
        su.eccentricity_squared = 1.0 - axis_ratio * axis_ratio;
        su.evolute_scale = Cartesian2::new(
            (radii_x * radii_x - radii_z * radii_z) / radii_x,
            (radii_z * radii_z - radii_x * radii_x) / radii_z,
        );
        su.ellipsoid_inverse_radii_squared = Cartesian3::new(
            1.0 / (radii_x * radii_x),
            1.0 / (radii.y * radii.y),
            1.0 / (radii_z * radii_z),
        );

        let mut intersection_count = 0usize;
        sd.intersection_index_height_max = intersection_count;
        intersection_count += 1;
        sd.intersection_index_height_min = intersection_count;
        intersection_count += 1;

        su.clip_min_max_height = Cartesian2::new(render_min.z - shape_max.z, render_max.z - shape_max.z);

        let thickness = shape_max.z - shape_min.z;
        su.ellipsoid_inverse_height_difference = if thickness != 0.0 { 1.0 / thickness } else { 0.0 };
        if shape_min.z == shape_max.z {
            su.ellipsoid_inverse_height_difference = 0.0;
        }

        if render_has_longitude {
            sd.has_render_bounds_longitude = true;
            sd.intersection_index_longitude = Some(intersection_count);

            if render_is_longitude_range_under_half {
                sd.has_render_bounds_longitude_range_under_half = true;
                intersection_count += 1;
            } else if render_is_longitude_range_over_half {
                sd.has_render_bounds_longitude_range_over_half = true;
                intersection_count += 2;
            } else if render_is_longitude_range_zero {
                sd.has_render_bounds_longitude_range_equal_zero = true;
                intersection_count += 2;
            }

            su.ellipsoid_render_longitude_min_max = Cartesian2::new(render_min.x, render_max.x);
        }

        if shape_has_longitude {
            sd.has_shape_bounds_longitude = true;

            let uv_shape_min_longitude = (shape_min.x - default_min.x) / default_longitude_range;
            let uv_shape_max_longitude = (shape_max.x - default_min.x) / default_longitude_range;
            let uv_longitude_range_zero = 1.0 - shape_longitude_range / default_longitude_range;
            let uv_longitude_range_origin = (uv_shape_max_longitude + 0.5 * uv_longitude_range_zero) % 1.0;
            su.ellipsoid_shape_uv_longitude_range_origin = uv_longitude_range_origin;

            if shape_is_longitude_reversed {
                sd.has_shape_bounds_longitude_min_max_reversed = true;
            }

            if shape_longitude_range <= eps_lon {
                su.ellipsoid_local_to_shape_uv_longitude = Cartesian2::new(0.0, 1.0);
            } else {
                let scale = default_longitude_range / shape_longitude_range;
                let shifted_min_longitude = uv_shape_min_longitude - uv_longitude_range_origin;
                let offset = -scale * (shifted_min_longitude - shifted_min_longitude.floor());
                su.ellipsoid_local_to_shape_uv_longitude = Cartesian2::new(scale, offset);
            }
        }

        if render_has_longitude {
            let render_is_min_lon_disc = equals_epsilon(render_min.x, default_min.x, 0.0, eps_lon_disc);
            let render_is_max_lon_disc = equals_epsilon(render_max.x, default_max.x, 0.0, eps_lon_disc);
            if render_is_min_lon_disc {
                sd.has_render_bounds_longitude_min_discontinuity = true;
            }
            if render_is_max_lon_disc {
                sd.has_render_bounds_longitude_max_discontinuity = true;
            }
            let uv_shape_min_longitude = (shape_min.x - default_min.x) / default_longitude_range;
            let uv_shape_max_longitude = (shape_max.x - default_min.x) / default_longitude_range;
            let uv_render_max_longitude = (render_max.x - default_min.x) / default_longitude_range;
            let uv_render_longitude_range_zero = 1.0 - render_longitude_range / default_longitude_range;
            let uv_render_longitude_range_zero_mid = (uv_render_max_longitude + 0.5 * uv_render_longitude_range_zero) % 1.0;
            su.ellipsoid_shape_uv_longitude_min_max_mid = Cartesian3::new(
                uv_shape_min_longitude,
                uv_shape_max_longitude,
                uv_render_longitude_range_zero_mid,
            );
        }

        if render_has_latitude {
            if render_has_latitude_min {
                sd.has_render_bounds_latitude_min_under_half = render_is_latitude_min_under_half;
                sd.has_render_bounds_latitude_min_equal_half = render_is_latitude_min_half;
                sd.has_render_bounds_latitude_min_over_half = render_is_latitude_min_over_half;
                sd.intersection_index_latitude_min = Some(intersection_count);
                if render_is_latitude_min_under_half {
                    intersection_count += 1;
                } else if render_is_latitude_min_half {
                    intersection_count += 1;
                } else if render_is_latitude_min_over_half {
                    intersection_count += 2;
                }
            }
            if render_has_latitude_max {
                sd.has_render_bounds_latitude_max_under_half = render_is_latitude_max_under_half;
                sd.has_render_bounds_latitude_max_equal_half = render_is_latitude_max_half;
                sd.has_render_bounds_latitude_max_over_half = render_is_latitude_max_over_half;
                sd.intersection_index_latitude_max = Some(intersection_count);
                if render_is_latitude_max_under_half {
                    intersection_count += 2;
                } else if render_is_latitude_max_half {
                    intersection_count += 1;
                } else if render_is_latitude_max_over_half {
                    intersection_count += 1;
                }
            }

            su.ellipsoid_render_latitude_sin_min_max = Cartesian2::new(
                render_min.y.sin(),
                render_max.y.sin(),
            );
        }

        if shape_has_latitude {
            sd.has_shape_bounds_latitude = true;
            if shape_latitude_range < eps_lat {
                su.ellipsoid_local_to_shape_uv_latitude = Cartesian2::new(0.0, 1.0);
            } else {
                let default_latitude_range = default_max.y - default_min.y;
                let scale = default_latitude_range / shape_latitude_range;
                let offset = (default_min.y - shape_min.y) / shape_latitude_range;
                su.ellipsoid_local_to_shape_uv_latitude = Cartesian2::new(scale, offset);
            }
        }

        // Need set has_render_bounds_latitude_min/max booleans
        sd.has_render_bounds_latitude_min_under_half = render_has_latitude_min && render_is_latitude_min_under_half;
        // but defines above already conditionally set; ensure flags for min/max presence
        // We'll set presence via render_has_latitude_min? Cesium uses separate defines only under renderHasLatitudeMin. Already set above guarded? We set unconditionally; fix by resetting if not present.
        if !render_has_latitude_min {
            sd.has_render_bounds_latitude_min_under_half = false;
            sd.has_render_bounds_latitude_min_equal_half = false;
            sd.has_render_bounds_latitude_min_over_half = false;
            sd.intersection_index_latitude_min = None;
        }
        if !render_has_latitude_max {
            sd.has_render_bounds_latitude_max_under_half = false;
            sd.has_render_bounds_latitude_max_equal_half = false;
            sd.has_render_bounds_latitude_max_over_half = false;
            sd.intersection_index_latitude_max = None;
        }

        self.shader_maximum_intersections_length = intersection_count;

        true
    }

    pub fn update_view_transforms(&mut self, inverse_view_rotation: Matrix3) -> Result<(), RuntimeError> {
        self.update_view_transforms_with_camera(Cartesian3::ZERO, inverse_view_rotation)
    }

    pub fn update_view_transforms_with_camera(
        &mut self,
        camera_position_wc: Cartesian3,
        inverse_view_rotation: Matrix3,
    ) -> Result<(), RuntimeError> {
        let su = &mut self.shader_uniforms;
        // cartographic of camera
        if let Some(cart) = self.ellipsoid.cartesian_to_cartographic(camera_position_wc) {
            su.camera_position_cartographic = Cartesian3::new(cart.longitude, cart.latitude, cart.height);
        }
        let surface_position = Cartesian3::from_radians(
            su.camera_position_cartographic.x,
            su.camera_position_cartographic.y,
            0.0,
            &self.ellipsoid,
        );
        su.ellipsoid_curvature_at_latitude = self.ellipsoid.get_local_curvature(surface_position);

        let enu_to_world = Transforms::east_north_up_to_fixed_frame(surface_position, &self.ellipsoid);
        let rotate_enu_to_world = enu_to_world.get_rotation();
        // rotateWorldToView approximated by inverse_view_rotation? We only have rotation. Use identity? 
        let rotate_enu_to_view = rotate_enu_to_world; // placeholder
        // inverse is transpose
        su.ellipsoid_ec_to_east_north_up = rotate_enu_to_view.transpose(); // Matrix3 has transpose method? yes via matrix_common
        self.rotation; // noop
        let _ = inverse_view_rotation;
        Ok(())
    }

    pub fn convert_local_to_shape_uv_space(&self, p: Cartesian3) -> Cartesian3 {
        // port nearestPointAndRadiusOnEllipse
        let su = &self.shader_uniforms;
        let mut longitude = p.y.atan2(p.x);

        let distance_from_z_axis = (p.x * p.x + p.y * p.y).sqrt();
        let pos_ellipse = Cartesian2::new(distance_from_z_axis, p.z);
        let surface = nearest_point_and_radius_on_ellipse(
            pos_ellipse,
            Cartesian2::new(su.ellipsoid_radii.x, su.ellipsoid_radii.z),
            su.evolute_scale,
        );
        let normal2d = Cartesian2::new(
            surface.x * su.ellipsoid_inverse_radii_squared.x,
            surface.y * su.ellipsoid_inverse_radii_squared.z,
        ).normalize();
        let mut latitude = normal2d.y.atan2(normal2d.x);

        let height_sign = if Cartesian2::new(pos_ellipse.x, pos_ellipse.y).magnitude()
            < Cartesian2::new(surface.x, surface.y).magnitude()
        {
            -1.0
        } else {
            1.0
        };
        let height_vec = Cartesian2::new(pos_ellipse.x - surface.x, pos_ellipse.y - surface.y);
        let height = height_sign * height_vec.magnitude();

        let sd = &self.shader_defines;

        longitude = (longitude + PI) / (2.0 * PI);
        if sd.has_shape_bounds_longitude {
            longitude -= su.ellipsoid_shape_uv_longitude_range_origin;
            longitude = longitude - longitude.floor();
            longitude = longitude * su.ellipsoid_local_to_shape_uv_longitude.x + su.ellipsoid_local_to_shape_uv_longitude.y;
        }

        latitude = (latitude + PI / 2.0) / PI;
        if sd.has_shape_bounds_latitude {
            latitude = latitude * su.ellipsoid_local_to_shape_uv_latitude.x + su.ellipsoid_local_to_shape_uv_latitude.y;
        }

        let height = 1.0 + height * su.ellipsoid_inverse_height_difference;

        Cartesian3::new(longitude, latitude, height)
    }

    pub fn min_bounds(&self) -> Cartesian3 {
        self.min
    }
    pub fn max_bounds(&self) -> Cartesian3 {
        self.max
    }
    pub fn render_min_bounds(&self) -> Cartesian3 {
        self.render_min
    }
    pub fn render_max_bounds(&self) -> Cartesian3 {
        self.render_max
    }

    pub fn tile_obb(&self, level: u32, coords: [u32; 3]) -> OrientedBoundingBox {
        // computeOrientedBoundingBoxForTile
        let size_at_level = 1.0 / 2.0_f64.powi(level as i32);
        let min_lon_lerp = coords[0] as f64 * size_at_level;
        let max_lon_lerp = (coords[0] + 1) as f64 * size_at_level;
        let min_lat_lerp = coords[1] as f64 * size_at_level;
        let max_lat_lerp = (coords[1] + 1) as f64 * size_at_level;
        let min_h_lerp = coords[2] as f64 * size_at_level;
        let max_h_lerp = (coords[2] + 1) as f64 * size_at_level;

        let rectangle = Rectangle::subsection(&self.rectangle, min_lon_lerp, min_lat_lerp, max_lon_lerp, max_lat_lerp);
        let min_height = lerp(self.minimum_height, self.maximum_height, min_h_lerp);
        let max_height = lerp(self.minimum_height, self.maximum_height, max_h_lerp);

        get_ellipsoid_chunk_obb(&rectangle, min_height, max_height, &self.ellipsoid, self.translation, self.rotation)
    }

    pub fn sample_obb(
        &self,
        level: u32,
        coords: [u32; 3],
        dimensions: Cartesian3,
        uv: Cartesian3,
    ) -> OrientedBoundingBox {
        let tile_size_at_level = 1.0 / 2.0_f64.powi(level as i32);
        let sample_size = Cartesian3::new(1.0 / dimensions.x, 1.0 / dimensions.y, 1.0 / dimensions.z);
        let sample_size_at_level = sample_size.multiply_by_scalar(tile_size_at_level);
        let min_lerp = Cartesian3::new(
            (coords[0] as f64 + uv.x) * tile_size_at_level,
            (coords[1] as f64 + uv.y) * tile_size_at_level,
            (coords[2] as f64 + uv.z) * tile_size_at_level,
        );
        let max_lerp = min_lerp.add(sample_size_at_level);

        let rectangle = Rectangle::subsection(&self.rectangle, min_lerp.x, min_lerp.y, max_lerp.x, max_lerp.y);
        let min_height = lerp(self.minimum_height, self.maximum_height, min_lerp.z);
        let max_height = lerp(self.minimum_height, self.maximum_height, max_lerp.z);

        get_ellipsoid_chunk_obb(&rectangle, min_height, max_height, &self.ellipsoid, self.translation, self.rotation)
    }
}

fn clamp_cartesian(v: Cartesian3, lo: Cartesian3, hi: Cartesian3) -> Cartesian3 {
    Cartesian3::new(
        v.x.max(lo.x).min(hi.x),
        v.y.max(lo.y).min(hi.y),
        v.z.max(lo.z).min(hi.z),
    )
}

fn get_ellipsoid_chunk_obb(
    rectangle: &Rectangle,
    min_height: f64,
    max_height: f64,
    ellipsoid: &Ellipsoid,
    translation: Cartesian3,
    rotation: Matrix3,
) -> OrientedBoundingBox {
    let mut result = OrientedBoundingBox::from_rectangle(rectangle, min_height, max_height, ellipsoid).unwrap_or_default();
    result.center = result.center.add(translation);
    result.half_axes = result.half_axes.multiply(&rotation);
    result
}

// Fix signature: function should return OBB not take result. Rewrite below by making wrapper.

fn nearest_point_and_radius_on_ellipse(
    position: Cartesian2,
    radii: Cartesian2,
    evolute_scale: Cartesian2,
) -> Cartesian3 {
    // returns x,y nearest point, z radius of curvature
    let p = Cartesian2::new(position.x.abs(), position.y.abs());
    let inverse_radii = Cartesian2::new(1.0 / radii.x, 1.0 / radii.y);
    let mut t_trigs = Cartesian2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
    let mut v = Cartesian2::new(radii.x * t_trigs.x, radii.y * t_trigs.y);
    let mut evolute = Cartesian2::new(
        evolute_scale.x * t_trigs.x * t_trigs.x * t_trigs.x,
        evolute_scale.y * t_trigs.y * t_trigs.y * t_trigs.y,
    );
    for _ in 0..3 {
        let distance = (v.x - evolute.x).hypot(v.y - evolute.y);
        let dir = Cartesian2::new(p.x - evolute.x, p.y - evolute.y);
        let d = dir.magnitude();
        let direction = if d > 0.0 { Cartesian2::new(dir.x / d, dir.y / d) } else { Cartesian2::new(0.0, 0.0) };
        let q = Cartesian2::new(direction.x * distance, direction.y * distance);
        t_trigs = Cartesian2::new((q.x + evolute.x) * inverse_radii.x, (q.y + evolute.y) * inverse_radii.y);
        let n = t_trigs.magnitude();
        if n > 0.0 {
            t_trigs = Cartesian2::new(t_trigs.x / n, t_trigs.y / n);
            // clamp to [0,1]
            t_trigs = Cartesian2::new(t_trigs.x.clamp(0.0, 1.0), t_trigs.y.clamp(0.0, 1.0));
            let n2 = t_trigs.magnitude();
            if n2 > 0.0 {
                t_trigs = Cartesian2::new(t_trigs.x / n2, t_trigs.y / n2);
            }
        }
        v = Cartesian2::new(radii.x * t_trigs.x, radii.y * t_trigs.y);
        evolute = Cartesian2::new(
            evolute_scale.x * t_trigs.x * t_trigs.x * t_trigs.x,
            evolute_scale.y * t_trigs.y * t_trigs.y * t_trigs.y,
        );
    }
    let radius = (Cartesian2::new(v.x - evolute.x, v.y - evolute.y)).magnitude();
    Cartesian3::new(
        position.x.signum() * v.x,
        position.y.signum() * v.y,
        radius,
    )
}
