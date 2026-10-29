//! CesiumJS `VoxelCylinderShape`: CPU bounds, UV transforms, tile/sample boxes
//! and cylinder clipping/intersection shader metadata.
use crate::core::{
    BoundingSphere, Cartesian2, Cartesian3, Matrix3, Matrix4, OrientedBoundingBox, RuntimeError,
};
use crate::core::math::{equals_epsilon, negative_pi_to_pi, EPSILON10, PI, TWO_PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CylinderShaderUniforms {
    pub camera_shape_position: Cartesian3,
    pub cylinder_ec_to_radial_tangent_up: Matrix3,
    pub cylinder_render_radius_min_max: Cartesian2,
    pub cylinder_render_angle_min_max: Cartesian2,
    pub cylinder_local_to_shape_uv_radius: Cartesian2,
    pub cylinder_local_to_shape_uv_angle: Cartesian2,
    pub cylinder_local_to_shape_uv_height: Cartesian2,
    pub cylinder_shape_uv_angle_range_origin: f64,
}

impl Default for CylinderShaderUniforms {
    fn default() -> Self {
        Self {
            camera_shape_position: Cartesian3::ZERO,
            cylinder_ec_to_radial_tangent_up: Matrix3::ZERO,
            cylinder_render_radius_min_max: Cartesian2::ZERO,
            cylinder_render_angle_min_max: Cartesian2::ZERO,
            cylinder_local_to_shape_uv_radius: Cartesian2::ZERO,
            cylinder_local_to_shape_uv_angle: Cartesian2::ZERO,
            cylinder_local_to_shape_uv_height: Cartesian2::ZERO,
            cylinder_shape_uv_angle_range_origin: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CylinderShaderDefines {
    pub has_shape_bounds_angle: bool,
    pub has_render_bounds_radius_min: bool,
    pub has_render_bounds_radius_flat: bool,
    pub has_render_bounds_angle: bool,
    pub has_render_bounds_angle_range_equal_zero: bool,
    pub has_render_bounds_angle_range_under_half: bool,
    pub has_render_bounds_angle_range_over_half: bool,
    pub intersection_index_radius_max: usize,
    pub intersection_index_radius_min: Option<usize>,
    pub intersection_index_angle: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct VoxelCylinderShape {
    pub min: Cartesian3,
    pub max: Cartesian3,
    pub render_min: Cartesian3,
    pub render_max: Cartesian3,
    pub shape_transform: Matrix4,
    pub bound_transform: Matrix4,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub bounding_sphere: BoundingSphere,
    pub uv_scale: Cartesian3,
    pub uv_translate: Cartesian3,
    pub eye_to_local: Matrix3,
    pub shader_uniforms: CylinderShaderUniforms,
    pub shader_defines: CylinderShaderDefines,
    pub shader_maximum_intersections_length: usize,
}

impl Default for VoxelCylinderShape {
    fn default() -> Self {
        Self {
            min: Cartesian3::new(0.0, -PI, -1.0),
            max: Cartesian3::new(1.0, PI, 1.0),
            render_min: Cartesian3::new(0.0, -PI, -1.0),
            render_max: Cartesian3::new(1.0, PI, 1.0),
            shape_transform: Matrix4::IDENTITY,
            bound_transform: Matrix4::IDENTITY,
            oriented_bounding_box: OrientedBoundingBox::default(),
            bounding_sphere: BoundingSphere::default(),
            uv_scale: Cartesian3::ZERO,
            uv_translate: Cartesian3::ZERO,
            eye_to_local: Matrix3::ZERO,
            shader_uniforms: CylinderShaderUniforms::default(),
            shader_defines: CylinderShaderDefines::default(),
            shader_maximum_intersections_length: 0,
        }
    }
}

impl VoxelCylinderShape {
    pub fn default_min_bounds() -> Cartesian3 {
        Cartesian3::new(0.0, -PI, -1.0)
    }
    pub fn default_max_bounds() -> Cartesian3 {
        Cartesian3::new(1.0, PI, 1.0)
    }
    pub fn update(
        &mut self,
        model: Matrix4,
        min: Cartesian3,
        max: Cartesian3,
        clip_min: Option<Cartesian3>,
        clip_max: Option<Cartesian3>,
    ) -> bool {
        let default_min = Cartesian3::new(0.0, -PI, -1.0);
        let default_max = Cartesian3::new(1.0, PI, 1.0);
        let default_angle_range = default_max.y - default_min.y;
        let default_angle_range_half = 0.5 * default_angle_range;
        let eps_zero = EPSILON10;

        let mut min_bounds = min;
        let mut max_bounds = max;
        let mut clip_min = clip_min.unwrap_or(min_bounds);
        let mut clip_max = clip_max.unwrap_or(max_bounds);

        min_bounds.x = 0.0_f64.max(min_bounds.x);
        max_bounds.x = 0.0_f64.max(max_bounds.x);
        min_bounds.y = negative_pi_to_pi(min_bounds.y);
        max_bounds.y = negative_pi_to_pi(max_bounds.y);
        clip_min.y = negative_pi_to_pi(clip_min.y);
        clip_max.y = negative_pi_to_pi(clip_max.y);

        let render_min = Cartesian3::new(
            min_bounds.x.max(clip_min.x),
            min_bounds.y.max(clip_min.y),
            min_bounds.z.max(clip_min.z),
        );
        let render_max = Cartesian3::new(
            max_bounds.x.min(clip_max.x),
            max_bounds.y.min(clip_max.y),
            max_bounds.z.min(clip_max.z),
        );

        let scale = model.get_scale();
        if render_max.x == 0.0
            || render_min.x > render_max.x
            || render_min.z > render_max.z
            || equals_epsilon(scale.x, 0.0, 0.0, eps_zero)
            || equals_epsilon(scale.y, 0.0, 0.0, eps_zero)
            || equals_epsilon(scale.z, 0.0, 0.0, eps_zero)
        {
            return false;
        }

        self.min = min_bounds;
        self.max = max_bounds;
        self.render_min = render_min;
        self.render_max = render_max;

        self.shape_transform = model;

        let obb = get_cylinder_chunk_obb(render_min, render_max, self.shape_transform);
        self.oriented_bounding_box = obb;
        self.bound_transform = Matrix4::from_rotation_translation(&obb.half_axes, obb.center);
        self.bounding_sphere = BoundingSphere::from_oriented_bounding_box(&obb);

        let shape_is_angle_reversed = max_bounds.y < min_bounds.y;
        let shape_angle_range =
            max_bounds.y - min_bounds.y + if shape_is_angle_reversed { default_angle_range } else { 0.0 };
        let render_is_angle_reversed = render_max.y < render_min.y;
        let render_angle_range =
            render_max.y - render_min.y + if render_is_angle_reversed { default_angle_range } else { 0.0 };
        let render_is_angle_regular =
            render_angle_range >= default_angle_range_half - EPSILON10 && render_angle_range < default_angle_range - EPSILON10;
        let render_is_angle_flipped =
            render_angle_range > EPSILON10 && render_angle_range < default_angle_range_half - EPSILON10;
        let render_is_angle_range_zero = render_angle_range <= EPSILON10;
        let render_has_angle = render_is_angle_regular || render_is_angle_flipped || render_is_angle_range_zero;

        self.shader_defines = CylinderShaderDefines::default();
        let mut intersection_count = 0usize;
        self.shader_defines.intersection_index_radius_max = intersection_count;
        intersection_count += 1;

        let shader_uniforms = &mut self.shader_uniforms;
        let shader_defines = &mut self.shader_defines;

        if shape_angle_range < default_angle_range - EPSILON10 {
            shader_defines.has_shape_bounds_angle = true;
        }

        if render_min.x != default_min.x {
            shader_defines.has_render_bounds_radius_min = true;
            shader_defines.intersection_index_radius_min = Some(intersection_count);
            intersection_count += 1;
        }
        shader_uniforms.cylinder_render_radius_min_max = Cartesian2::new(render_min.x, render_max.x);

        if render_min.x == render_max.x {
            shader_defines.has_render_bounds_radius_flat = true;
        }

        let radius_range = max_bounds.x - min_bounds.x;
        let (radial_scale, radial_offset) = if radius_range != 0.0 {
            (1.0 / radius_range, -min_bounds.x / radius_range)
        } else {
            (0.0, 1.0)
        };
        shader_uniforms.cylinder_local_to_shape_uv_radius = Cartesian2::new(radial_scale, radial_offset);

        let height_range = max_bounds.z - min_bounds.z;
        let (height_scale, height_offset) = if height_range != 0.0 {
            (1.0 / height_range, -min_bounds.z / height_range)
        } else {
            (0.0, 1.0)
        };
        shader_uniforms.cylinder_local_to_shape_uv_height = Cartesian2::new(height_scale, height_offset);

        if render_has_angle {
            shader_defines.has_render_bounds_angle = true;
            shader_defines.intersection_index_angle = Some(intersection_count);

            if render_is_angle_regular {
                shader_defines.has_render_bounds_angle_range_under_half = true;
                intersection_count += 1;
            } else if render_is_angle_flipped {
                shader_defines.has_render_bounds_angle_range_over_half = true;
                intersection_count += 2;
            } else if render_is_angle_range_zero {
                shader_defines.has_render_bounds_angle_range_equal_zero = true;
                intersection_count += 2;
            }

            shader_uniforms.cylinder_render_angle_min_max = Cartesian2::new(render_min.y, render_max.y);
        }

        let uv_min_angle = (min_bounds.y - default_min.y) / default_angle_range;
        let uv_max_angle = (max_bounds.y - default_min.y) / default_angle_range;
        let uv_angle_range_zero = 1.0 - shape_angle_range / default_angle_range;
        let uv_angle_range_origin = (uv_max_angle + 0.5 * uv_angle_range_zero) % 1.0;
        shader_uniforms.cylinder_shape_uv_angle_range_origin = uv_angle_range_origin;

        if shape_angle_range <= EPSILON10 {
            shader_uniforms.cylinder_local_to_shape_uv_angle = Cartesian2::new(0.0, 1.0);
        } else {
            let scale = default_angle_range / shape_angle_range;
            let shifted_min_angle = uv_min_angle - uv_angle_range_origin;
            let offset = -scale * (shifted_min_angle - shifted_min_angle.floor());
            shader_uniforms.cylinder_local_to_shape_uv_angle = Cartesian2::new(scale, offset);
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
        let transform_world_to_local = self.shape_transform.inverse()?;
        let camera_position_local = transform_world_to_local.multiply_by_point(camera_position_wc);
        let mag = (camera_position_local.x * camera_position_local.x + camera_position_local.y * camera_position_local.y).sqrt();
        self.shader_uniforms.camera_shape_position = Cartesian3::new(
            mag,
            camera_position_local.y.atan2(camera_position_local.x),
            camera_position_local.z,
        );

        let radial = if mag > 0.0 {
            Cartesian2::new(camera_position_local.x / mag, camera_position_local.y / mag)
        } else {
            Cartesian2::new(1.0, 0.0)
        };
        let rotate_local_to_rtu = Matrix3::from_row_major_array([
            radial.x, radial.y, 0.0,
            -radial.y, radial.x, 0.0,
            0.0, 0.0, 1.0,
        ]);

        let transform_view_to_local = transform_world_to_local.get_matrix3().multiply(&inverse_view_rotation);
        self.shader_uniforms.cylinder_ec_to_radial_tangent_up =
            rotate_local_to_rtu.multiply(&transform_view_to_local);
        self.eye_to_local = transform_world_to_local.get_matrix3().multiply(&inverse_view_rotation);
        Ok(())
    }

    pub fn convert_local_to_shape_uv_space(&self, p: Cartesian3) -> Cartesian3 {
        let u = &self.shader_uniforms;
        let mut radius = (p.x * p.x + p.y * p.y).sqrt();
        let mut angle = p.y.atan2(p.x);
        let mut height = p.z;

        radius = radius * u.cylinder_local_to_shape_uv_radius.x + u.cylinder_local_to_shape_uv_radius.y;

        angle = (angle + PI) / (2.0 * PI);
        angle -= u.cylinder_shape_uv_angle_range_origin;
        angle = angle - angle.floor();
        angle = angle * u.cylinder_local_to_shape_uv_angle.x + u.cylinder_local_to_shape_uv_angle.y;

        height = height * u.cylinder_local_to_shape_uv_height.x + u.cylinder_local_to_shape_uv_height.y;

        Cartesian3::new(radius, angle, height)
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

    pub fn render_bound_plane_distances(&self) -> [f64; 2] {
        [self.render_min.z, -self.render_max.z]
    }

    pub fn tile_obb(&self, level: u32, coords: [u32; 3]) -> OrientedBoundingBox {
        let min_bounds = self.min;
        let max_bounds = self.max;
        let size_at_level = 1.0 / 2.0_f64.powi(level as i32);
        let tile_min = Cartesian3::new(
            crate::core::math::lerp(min_bounds.x, max_bounds.x, coords[0] as f64 * size_at_level),
            crate::core::math::lerp(min_bounds.y, max_bounds.y, coords[1] as f64 * size_at_level),
            crate::core::math::lerp(min_bounds.z, max_bounds.z, coords[2] as f64 * size_at_level),
        );
        let tile_max = Cartesian3::new(
            crate::core::math::lerp(min_bounds.x, max_bounds.x, (coords[0] + 1) as f64 * size_at_level),
            crate::core::math::lerp(min_bounds.y, max_bounds.y, (coords[1] + 1) as f64 * size_at_level),
            crate::core::math::lerp(min_bounds.z, max_bounds.z, (coords[2] + 1) as f64 * size_at_level),
        );
        get_cylinder_chunk_obb(tile_min, tile_max, self.shape_transform)
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
        let min_bounds = self.min;
        let max_bounds = self.max;
        let sample_min = Cartesian3::new(
            crate::core::math::lerp(min_bounds.x, max_bounds.x, min_lerp.x),
            crate::core::math::lerp(min_bounds.y, max_bounds.y, min_lerp.y),
            crate::core::math::lerp(min_bounds.z, max_bounds.z, min_lerp.z),
        );
        let sample_max = Cartesian3::new(
            crate::core::math::lerp(min_bounds.x, max_bounds.x, max_lerp.x),
            crate::core::math::lerp(min_bounds.y, max_bounds.y, max_lerp.y),
            crate::core::math::lerp(min_bounds.z, max_bounds.z, max_lerp.z),
        );
        get_cylinder_chunk_obb(sample_min, sample_max, self.shape_transform)
    }
}

fn get_cylinder_chunk_obb(
    chunk_min: Cartesian3,
    chunk_max: Cartesian3,
    matrix: Matrix4,
) -> OrientedBoundingBox {
    let radius_start = chunk_min.x;
    let radius_end = chunk_max.x;
    let angle_start = chunk_min.y;
    let angle_end = if chunk_max.y < angle_start {
        chunk_max.y + TWO_PI
    } else {
        chunk_max.y
    };
    let height_start = chunk_min.z;
    let height_end = chunk_max.z;

    let angle_range = angle_end - angle_start;
    let angle_mid = angle_start + angle_range * 0.5;

    let mut test_angles = [0.0; 5];
    let mut count = 0usize;
    test_angles[count] = angle_start;
    count += 1;
    test_angles[count] = angle_end;
    count += 1;
    test_angles[count] = angle_mid;
    count += 1;
    if angle_range > PI {
        test_angles[count] = angle_mid - PI / 2.0;
        count += 1;
        test_angles[count] = angle_mid + PI / 2.0;
        count += 1;
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for i in 0..count {
        let angle = test_angles[i] - angle_mid;
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let x1 = cos_a * radius_start;
        let y1 = sin_a * radius_start;
        let x2 = cos_a * radius_end;
        let y2 = sin_a * radius_end;
        min_x = min_x.min(x1).min(x2);
        min_y = min_y.min(y1).min(y2);
        max_x = max_x.max(x1).max(x2);
        max_y = max_y.max(y1).max(y2);
    }

    let extent_x = max_x - min_x;
    let extent_y = max_y - min_y;
    let extent_z = height_end - height_start;

    let center_x = (min_x + max_x) * 0.5;
    let center_y = (min_y + max_y) * 0.5;
    let center_z = (height_start + height_end) * 0.5;

    let translation = Cartesian3::new(center_x, center_y, center_z);
    let rotation = Matrix3::from_rotation_z(angle_mid);
    let scale = Cartesian3::new(extent_x, extent_y, extent_z);

    let scale_matrix = Matrix4::from_scale(scale);
    let rotation_matrix = Matrix4::from_rotation(&rotation);
    let translation_matrix = Matrix4::from_translation(translation);

    let local_matrix = rotation_matrix.multiply_transformation(&translation_matrix.multiply_transformation(&scale_matrix));
    let global_matrix = matrix.multiply_transformation(&local_matrix);

    let col0 = global_matrix.get_column(0);
    let col1 = global_matrix.get_column(1);
    let col2 = global_matrix.get_column(2);
    let eps = crate::core::math::EPSILON4;
    let orthogonal = |a: crate::core::Cartesian4, b: crate::core::Cartesian4| -> bool {
        (a.x * b.x + a.y * b.y + a.z * b.z).abs() < eps
    };
    if !(orthogonal(col0, col1) && orthogonal(col1, col2)) {
        let mut corners = [Cartesian3::ZERO; 8];
        let pts = [
            Cartesian3::new(-0.5, -0.5, -0.5),
            Cartesian3::new(-0.5, -0.5, 0.5),
            Cartesian3::new(-0.5, 0.5, -0.5),
            Cartesian3::new(-0.5, 0.5, 0.5),
            Cartesian3::new(0.5, -0.5, -0.5),
            Cartesian3::new(0.5, -0.5, 0.5),
            Cartesian3::new(0.5, 0.5, -0.5),
            Cartesian3::new(0.5, 0.5, 0.5),
        ];
        for i in 0..8 {
            corners[i] = global_matrix.multiply_by_point(pts[i]);
        }
        return OrientedBoundingBox::from_points(&corners);
    }

    OrientedBoundingBox::from_transformation(&global_matrix)
}
