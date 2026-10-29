//! Shape-independent CPU voxel geometry contract, implemented by VoxelBoxShape.
//! Shader defines/uniforms and clipping plane rendering are not yet part of this contract.
use crate::core::{BoundingSphere, Cartesian3, Matrix3, Matrix4, OrientedBoundingBox, RuntimeError};
pub trait VoxelShape {
    fn default_min_bounds() -> Cartesian3
    where
        Self: Sized;
    fn default_max_bounds() -> Cartesian3
    where
        Self: Sized;
    fn oriented_bounding_box(&self) -> &OrientedBoundingBox;
    fn bounding_sphere(&self) -> &BoundingSphere;
    fn bound_transform(&self) -> &Matrix4;
    fn shape_transform(&self) -> &Matrix4;
    fn update(
        &mut self,
        model: Matrix4,
        min: Cartesian3,
        max: Cartesian3,
        clip_min: Option<Cartesian3>,
        clip_max: Option<Cartesian3>,
    ) -> bool;
    fn update_view_transforms(&mut self, inverse_view_rotation: Matrix3) -> Result<(), RuntimeError>;
    fn convert_local_to_shape_uv_space(&self, p: Cartesian3) -> Cartesian3;
    fn tile_obb(&self, level: u32, coords: [u32; 3]) -> OrientedBoundingBox;
    fn sample_obb(&self, level: u32, coords: [u32; 3], dimensions: Cartesian3, uv: Cartesian3) -> OrientedBoundingBox;
    fn min_bounds(&self) -> Cartesian3;
    fn max_bounds(&self) -> Cartesian3;
    fn render_min_bounds(&self) -> Cartesian3;
    fn render_max_bounds(&self) -> Cartesian3;
    fn shader_maximum_intersections_length(&self) -> usize;
}
impl VoxelShape for super::voxel_box_shape::VoxelBoxShape {
    fn default_min_bounds() -> Cartesian3 {
        Cartesian3::new(-1., -1., -1.)
    }
    fn default_max_bounds() -> Cartesian3 {
        Cartesian3::new(1., 1., 1.)
    }
    fn oriented_bounding_box(&self) -> &OrientedBoundingBox {
        &self.oriented_bounding_box
    }
    fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }
    fn bound_transform(&self) -> &Matrix4 {
        &self.bound_transform
    }
    fn shape_transform(&self) -> &Matrix4 {
        &self.shape_transform
    }
    fn update(
        &mut self,
        model: Matrix4,
        min: Cartesian3,
        max: Cartesian3,
        cmin: Option<Cartesian3>,
        cmax: Option<Cartesian3>,
    ) -> bool {
        self.update(model, min, max, cmin, cmax)
    }
    fn update_view_transforms(&mut self, v: Matrix3) -> Result<(), RuntimeError> {
        self.update_view_transforms(v)
    }
    fn convert_local_to_shape_uv_space(&self, p: Cartesian3) -> Cartesian3 {
        self.convert_local_to_shape_uv_space(p)
    }
    fn tile_obb(&self, l: u32, c: [u32; 3]) -> OrientedBoundingBox {
        self.tile_obb(l, c)
    }
    fn sample_obb(&self, l: u32, c: [u32; 3], d: Cartesian3, uv: Cartesian3) -> OrientedBoundingBox {
        self.sample_obb(l, c, d, uv)
    }
    fn min_bounds(&self) -> Cartesian3 {
        self.min_bounds()
    }
    fn max_bounds(&self) -> Cartesian3 {
        self.max_bounds()
    }
    fn render_min_bounds(&self) -> Cartesian3 {
        self.render_min_bounds()
    }
    fn render_max_bounds(&self) -> Cartesian3 {
        self.render_max_bounds()
    }
    fn shader_maximum_intersections_length(&self) -> usize {
        self.shader_maximum_intersections_length
    }
}

#[path = "voxel_cylinder_shape.rs"]
mod voxel_cylinder_shape;
#[path = "voxel_ellipsoid_shape.rs"]
mod voxel_ellipsoid_shape;

pub use voxel_cylinder_shape::VoxelCylinderShape;
pub use voxel_ellipsoid_shape::VoxelEllipsoidShape;

impl VoxelShape for voxel_cylinder_shape::VoxelCylinderShape {
    fn default_min_bounds() -> Cartesian3 { Self::default_min_bounds() }
    fn default_max_bounds() -> Cartesian3 { Self::default_max_bounds() }
    fn oriented_bounding_box(&self) -> &OrientedBoundingBox { &self.oriented_bounding_box }
    fn bounding_sphere(&self) -> &BoundingSphere { &self.bounding_sphere }
    fn bound_transform(&self) -> &Matrix4 { &self.bound_transform }
    fn shape_transform(&self) -> &Matrix4 { &self.shape_transform }
    fn update(&mut self, model: Matrix4, min: Cartesian3, max: Cartesian3, cmin: Option<Cartesian3>, cmax: Option<Cartesian3>) -> bool {
        self.update(model, min, max, cmin, cmax)
    }
    fn update_view_transforms(&mut self, v: Matrix3) -> Result<(), RuntimeError> { self.update_view_transforms(v) }
    fn convert_local_to_shape_uv_space(&self, p: Cartesian3) -> Cartesian3 { self.convert_local_to_shape_uv_space(p) }
    fn tile_obb(&self, l: u32, c: [u32; 3]) -> OrientedBoundingBox { self.tile_obb(l, c) }
    fn sample_obb(&self, l: u32, c: [u32; 3], d: Cartesian3, uv: Cartesian3) -> OrientedBoundingBox { self.sample_obb(l, c, d, uv) }
    fn min_bounds(&self) -> Cartesian3 { self.min_bounds() }
    fn max_bounds(&self) -> Cartesian3 { self.max_bounds() }
    fn render_min_bounds(&self) -> Cartesian3 { self.render_min_bounds() }
    fn render_max_bounds(&self) -> Cartesian3 { self.render_max_bounds() }
    fn shader_maximum_intersections_length(&self) -> usize { self.shader_maximum_intersections_length }
}

impl VoxelShape for voxel_ellipsoid_shape::VoxelEllipsoidShape {
    fn default_min_bounds() -> Cartesian3 { Self::default_min_bounds() }
    fn default_max_bounds() -> Cartesian3 { Self::default_max_bounds() }
    fn oriented_bounding_box(&self) -> &OrientedBoundingBox { &self.oriented_bounding_box }
    fn bounding_sphere(&self) -> &BoundingSphere { &self.bounding_sphere }
    fn bound_transform(&self) -> &Matrix4 { &self.bound_transform }
    fn shape_transform(&self) -> &Matrix4 { &self.shape_transform }
    fn update(&mut self, model: Matrix4, min: Cartesian3, max: Cartesian3, cmin: Option<Cartesian3>, cmax: Option<Cartesian3>) -> bool {
        self.update(model, min, max, cmin, cmax)
    }
    fn update_view_transforms(&mut self, v: Matrix3) -> Result<(), RuntimeError> { self.update_view_transforms(v) }
    fn convert_local_to_shape_uv_space(&self, p: Cartesian3) -> Cartesian3 { self.convert_local_to_shape_uv_space(p) }
    fn tile_obb(&self, l: u32, c: [u32; 3]) -> OrientedBoundingBox { self.tile_obb(l, c) }
    fn sample_obb(&self, l: u32, c: [u32; 3], d: Cartesian3, uv: Cartesian3) -> OrientedBoundingBox { self.sample_obb(l, c, d, uv) }
    fn min_bounds(&self) -> Cartesian3 { self.min_bounds() }
    fn max_bounds(&self) -> Cartesian3 { self.max_bounds() }
    fn render_min_bounds(&self) -> Cartesian3 { self.render_min_bounds() }
    fn render_max_bounds(&self) -> Cartesian3 { self.render_max_bounds() }
    fn shader_maximum_intersections_length(&self) -> usize { self.shader_maximum_intersections_length }
}
