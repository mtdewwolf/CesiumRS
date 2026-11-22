//! CesiumJS `VoxelBoxShape`: CPU bounds, UV transforms, render-bound planes,
//! tile/sample boxes, and shader uniforms/defines.
use super::{
    clipping_plane::{ClippingPlane, ClippingPlaneHandle},
    voxel_bounds_collection::{VoxelBoundsCollection, VoxelBoundsCollectionOptions},
};
use crate::{
    core::{BoundingSphere, Cartesian3, Matrix3, Matrix4, OrientedBoundingBox, RuntimeError},
    renderer::{Gl, UniformValue},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShaderUniforms {
    pub box_ec_to_xyz: Matrix3,
    pub box_local_to_shape_uv_scale: Cartesian3,
    pub box_local_to_shape_uv_translate: Cartesian3,
}

impl Default for BoxShaderUniforms {
    fn default() -> Self {
        Self {
            box_ec_to_xyz: Matrix3::ZERO,
            box_local_to_shape_uv_scale: Cartesian3::ZERO,
            box_local_to_shape_uv_translate: Cartesian3::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BoxShaderDefines {
    pub box_intersection_index: Option<usize>,
}

fn default_box_planes() -> [ClippingPlaneHandle; 6] {
    let min = Cartesian3::new(-1.0, -1.0, -1.0);
    let max = Cartesian3::new(1.0, 1.0, 1.0);
    [
        ClippingPlane::handle(Cartesian3::UNIT_X.negate(), min.x),
        ClippingPlane::handle(Cartesian3::UNIT_Y.negate(), min.y),
        ClippingPlane::handle(Cartesian3::UNIT_Z.negate(), min.z),
        ClippingPlane::handle(Cartesian3::UNIT_X, -max.x),
        ClippingPlane::handle(Cartesian3::UNIT_Y, -max.y),
        ClippingPlane::handle(Cartesian3::UNIT_Z, -max.z),
    ]
}

#[derive(Clone, Debug)]
pub struct VoxelBoxShape {
    min: Cartesian3,
    max: Cartesian3,
    render_min: Cartesian3,
    render_max: Cartesian3,
    bound_planes: [ClippingPlaneHandle; 6],
    pub shape_transform: Matrix4,
    pub bound_transform: Matrix4,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub bounding_sphere: BoundingSphere,
    pub uv_scale: Cartesian3,
    pub uv_translate: Cartesian3,
    pub eye_to_local: Matrix3,
    pub shader_uniforms: BoxShaderUniforms,
    pub shader_defines: BoxShaderDefines,
    pub shader_maximum_intersections_length: usize,
}

impl Default for VoxelBoxShape {
    fn default() -> Self {
        Self {
            min: Cartesian3::new(-1.0, -1.0, -1.0),
            max: Cartesian3::new(1.0, 1.0, 1.0),
            render_min: Cartesian3::new(-1.0, -1.0, -1.0),
            render_max: Cartesian3::new(1.0, 1.0, 1.0),
            bound_planes: default_box_planes(),
            shape_transform: Matrix4::IDENTITY,
            bound_transform: Matrix4::IDENTITY,
            oriented_bounding_box: OrientedBoundingBox::default(),
            bounding_sphere: BoundingSphere::default(),
            uv_scale: Cartesian3::ZERO,
            uv_translate: Cartesian3::ZERO,
            eye_to_local: Matrix3::ZERO,
            shader_uniforms: BoxShaderUniforms::default(),
            shader_defines: BoxShaderDefines::default(),
            shader_maximum_intersections_length: 0,
        }
    }
}

fn clamp(v: Cartesian3, lo: Cartesian3, hi: Cartesian3) -> Cartesian3 {
    Cartesian3::new(v.x.max(lo.x).min(hi.x), v.y.max(lo.y).min(hi.y), v.z.max(lo.z).min(hi.z))
}

fn lerp(a: Cartesian3, b: Cartesian3, t: Cartesian3) -> Cartesian3 {
    Cartesian3::new((1.0 - t.x) * a.x + t.x * b.x, (1.0 - t.y) * a.y + t.y * b.y, (1.0 - t.z) * a.z + t.z * b.z)
}

impl VoxelBoxShape {
    pub fn default_min_bounds() -> Cartesian3 {
        Cartesian3::new(-1.0, -1.0, -1.0)
    }

    pub fn default_max_bounds() -> Cartesian3 {
        Cartesian3::new(1.0, 1.0, 1.0)
    }

    pub fn update(
        &mut self,
        model: Matrix4,
        min: Cartesian3,
        max: Cartesian3,
        clip_min: Option<Cartesian3>,
        clip_max: Option<Cartesian3>,
    ) -> bool {
        self.min = min;
        self.max = max;
        let lo = clamp(min, clip_min.unwrap_or(min), clip_max.unwrap_or(max));
        let hi = clamp(max, clip_min.unwrap_or(min), clip_max.unwrap_or(max));
        self.render_min = lo;
        self.render_max = hi;
        let s = model.get_scale();
        if lo.x > hi.x
            || lo.y > hi.y
            || lo.z > hi.z
            || (lo.x == hi.x) as u8 + (lo.y == hi.y) as u8 + (lo.z == hi.z) as u8 >= 2
            || s.x == 0.0
            || s.y == 0.0
            || s.z == 0.0
        {
            return false;
        }
        // Update the six render-bound clipping planes matching upstream VoxelBoxShape.js.
        self.bound_planes[0].borrow_mut().set_distance(lo.x);
        self.bound_planes[1].borrow_mut().set_distance(lo.y);
        self.bound_planes[2].borrow_mut().set_distance(lo.z);
        self.bound_planes[3].borrow_mut().set_distance(-hi.x);
        self.bound_planes[4].borrow_mut().set_distance(-hi.y);
        self.bound_planes[5].borrow_mut().set_distance(-hi.z);

        self.shape_transform = model;
        self.oriented_bounding_box = self.chunk(lo, hi);
        let obb = self.oriented_bounding_box;
        self.bound_transform = Matrix4::from_rotation_translation(&obb.half_axes, obb.center);
        self.bounding_sphere = BoundingSphere::from_oriented_bounding_box(&obb);
        fn scale(a: f64, b: f64) -> f64 {
            if (a - b).abs() <= 1e-7_f64.max(1e-7 * a.abs().max(b.abs())) {
                1.0
            } else {
                1.0 / (b - a)
            }
        }
        self.uv_scale = Cartesian3::new(scale(min.x, max.x), scale(min.y, max.y), scale(min.z, max.z));
        self.uv_translate = self.uv_scale.multiply_components(min).negate();
        self.shader_uniforms.box_local_to_shape_uv_scale = self.uv_scale;
        self.shader_uniforms.box_local_to_shape_uv_translate = self.uv_translate;
        self.shader_defines = BoxShaderDefines { box_intersection_index: Some(0) };
        self.shader_maximum_intersections_length = 1;
        true
    }

    fn chunk(&self, min: Cartesian3, max: Cartesian3) -> OrientedBoundingBox {
        if min == Cartesian3::new(-1.0, -1.0, -1.0) && max == Cartesian3::new(1.0, 1.0, 1.0) {
            return OrientedBoundingBox::new(
                self.shape_transform.get_translation(),
                self.shape_transform.get_matrix3(),
            );
        }
        let scale = self.shape_transform.get_scale().multiply_components(max.subtract(min).multiply_by_scalar(0.5));
        OrientedBoundingBox::new(
            self.shape_transform.multiply_by_point(min.add(max).multiply_by_scalar(0.5)),
            self.shape_transform.get_rotation().set_scale(scale),
        )
    }

    pub fn update_view_transforms(&mut self, inverse_view_rotation: Matrix3) -> Result<(), RuntimeError> {
        self.eye_to_local = self.shape_transform.inverse()?.get_matrix3().multiply(&inverse_view_rotation);
        self.shader_uniforms.box_ec_to_xyz = self.eye_to_local;
        Ok(())
    }

    pub fn convert_local_to_shape_uv_space(&self, p: Cartesian3) -> Cartesian3 {
        p.multiply_components(self.uv_scale).add(self.uv_translate)
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

    /// Returns the six clipping plane handles representing the box render bounds.
    pub fn render_bound_planes(&self) -> &[ClippingPlaneHandle; 6] {
        &self.bound_planes
    }

    /// Creates a GPU-backed [`VoxelBoundsCollection`] sharing this shape's six render-bound planes.
    pub fn render_bound_planes_collection<G: Gl>(&self) -> VoxelBoundsCollection<G> {
        VoxelBoundsCollection::new(VoxelBoundsCollectionOptions {
            planes: self.bound_planes.to_vec(),
            ..Default::default()
        })
    }

    /// The six render-bound plane distances (min x/y/z, max x/y/z as negative), matching the
    /// upstream `_renderBoundPlanes` collection ordering.
    pub fn render_bound_plane_distances(&self) -> [f64; 6] {
        [
            self.bound_planes[0].borrow().distance(),
            self.bound_planes[1].borrow().distance(),
            self.bound_planes[2].borrow().distance(),
            self.bound_planes[3].borrow().distance(),
            self.bound_planes[4].borrow().distance(),
            self.bound_planes[5].borrow().distance(),
        ]
    }

    /// Shader uniform map matching CesiumJS `VoxelBoxShape#shaderUniforms`.
    pub fn shader_uniforms_map(&self) -> BTreeMap<String, UniformValue> {
        let mut map = BTreeMap::new();
        map.insert(
            "boxEcToXyz".into(),
            UniformValue::Matrix {
                dimension: 3,
                values: self.shader_uniforms.box_ec_to_xyz.0.iter().map(|&v| v as f32).collect(),
            },
        );
        let s = self.shader_uniforms.box_local_to_shape_uv_scale;
        map.insert("boxLocalToShapeUvScale".into(), UniformValue::Float(vec![s.x as f32, s.y as f32, s.z as f32]));
        let t = self.shader_uniforms.box_local_to_shape_uv_translate;
        map.insert("boxLocalToShapeUvTranslate".into(), UniformValue::Float(vec![t.x as f32, t.y as f32, t.z as f32]));
        map
    }

    /// Shader defines map matching CesiumJS `VoxelBoxShape#shaderDefines`.
    pub fn shader_defines_map(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        if let Some(index) = self.shader_defines.box_intersection_index {
            map.insert("BOX_INTERSECTION_INDEX".into(), index.to_string());
        }
        map
    }

    pub fn tile_obb(&self, level: u32, coords: [u32; 3]) -> OrientedBoundingBox {
        let s = 2.0_f64.powf(-(level as f64));
        let lo = Cartesian3::new(coords[0] as f64 * s, coords[1] as f64 * s, coords[2] as f64 * s);
        let hi = lo.add(Cartesian3::new(s, s, s));
        self.chunk(lerp(self.min, self.max, lo), lerp(self.min, self.max, hi))
    }

    pub fn sample_obb(
        &self,
        level: u32,
        coords: [u32; 3],
        dimensions: Cartesian3,
        uv: Cartesian3,
    ) -> OrientedBoundingBox {
        let s = 2.0_f64.powf(-(level as f64));
        let lo = Cartesian3::new(coords[0] as f64 + uv.x, coords[1] as f64 + uv.y, coords[2] as f64 + uv.z)
            .multiply_by_scalar(s);
        let hi = lo.add(Cartesian3::new(s / dimensions.x, s / dimensions.y, s / dimensions.z));
        self.chunk(lerp(self.min, self.max, lo), lerp(self.min, self.max, hi))
    }
}
