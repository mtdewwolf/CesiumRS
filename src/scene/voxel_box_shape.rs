//! CesiumJS `VoxelBoxShape` CPU bounds, UV transforms and tile/sample boxes.
//! Render-bound plane collections and shader integration remain unimplemented.
use crate::core::{BoundingSphere, Cartesian3, Matrix3, Matrix4, OrientedBoundingBox, RuntimeError};
#[derive(Clone, Debug)]
pub struct VoxelBoxShape {
    min: Cartesian3,
    max: Cartesian3,
    render_min: Cartesian3,
    render_max: Cartesian3,
    pub shape_transform: Matrix4,
    pub bound_transform: Matrix4,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub bounding_sphere: BoundingSphere,
    pub uv_scale: Cartesian3,
    pub uv_translate: Cartesian3,
    pub eye_to_local: Matrix3,
    pub shader_maximum_intersections_length: usize,
}
impl Default for VoxelBoxShape {
    fn default() -> Self {
        Self {
            min: Cartesian3::new(-1.0, -1.0, -1.0),
            max: Cartesian3::new(1.0, 1.0, 1.0),
            render_min: Cartesian3::new(-1.0, -1.0, -1.0),
            render_max: Cartesian3::new(1.0, 1.0, 1.0),
            shape_transform: Matrix4::IDENTITY,
            bound_transform: Matrix4::IDENTITY,
            oriented_bounding_box: OrientedBoundingBox::default(),
            bounding_sphere: BoundingSphere::default(),
            uv_scale: Cartesian3::ZERO,
            uv_translate: Cartesian3::ZERO,
            eye_to_local: Matrix3::ZERO,
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
    /// The six render-bound plane distances (min x/y/z, max x/y/z as negative), matching the
    /// upstream `_renderBoundPlanes` collection ordering.
    pub fn render_bound_plane_distances(&self) -> [f64; 6] {
        [
            self.render_min.x,
            self.render_min.y,
            self.render_min.z,
            -self.render_max.x,
            -self.render_max.y,
            -self.render_max.z,
        ]
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
