//! CesiumJS `TerrainMesh`: owned terrain buffers, metadata, mode-dependent
//! transforms and accelerated picking. Explicit invalidation mirrors Cesium;
//! call update_exaggeration/update_scene_mode after changing public data.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::map_projection::MapProjection;
use super::matrix4::Matrix4;
use super::oriented_bounding_box::OrientedBoundingBox;
use super::ray::Ray;
use super::rectangle::Rectangle;
use super::terrain_encoding::TerrainEncoding;
use super::terrain_picker::{TerrainPickContext, TerrainPickMesh, TerrainPicker, TerrainSceneMode};
use super::terrain_provider::TerrainEdgeIndices;
use super::transforms::Transforms;
use super::typed_array::TypedArray;
use super::vertical_exaggeration;

#[derive(Clone, Debug)]
pub struct TerrainMeshOptions {
    pub center: Cartesian3,
    pub vertices: Vec<f32>,
    pub indices: TypedArray,
    pub index_count_without_skirts: usize,
    pub vertex_count_without_skirts: usize,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub rectangle: Rectangle,
    pub bounding_sphere_3d: BoundingSphere,
    pub occludee_point_in_scaled_space: Option<Cartesian3>,
    pub vertex_stride: Option<usize>,
    pub oriented_bounding_box: Option<OrientedBoundingBox>,
    pub encoding: TerrainEncoding,
    pub edges: TerrainEdgeIndices,
}

#[derive(Clone, Debug)]
pub struct TerrainMesh {
    pub center: Cartesian3,
    pub vertices: Vec<f32>,
    pub stride: usize,
    pub indices: TypedArray,
    pub index_count_without_skirts: usize,
    pub vertex_count_without_skirts: usize,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub rectangle: Rectangle,
    pub bounding_sphere_3d: BoundingSphere,
    pub occludee_point_in_scaled_space: Option<Cartesian3>,
    pub oriented_bounding_box: Option<OrientedBoundingBox>,
    pub encoding: TerrainEncoding,
    pub edges: TerrainEdgeIndices,
    transform: Matrix4,
    last_pick_scene_mode: Option<TerrainSceneMode>,
    picker: TerrainPicker,
}

impl TerrainMesh {
    pub fn new(o: TerrainMeshOptions) -> Self {
        Self {
            center: o.center,
            vertices: o.vertices,
            stride: o.vertex_stride.unwrap_or(6),
            indices: o.indices,
            index_count_without_skirts: o.index_count_without_skirts,
            vertex_count_without_skirts: o.vertex_count_without_skirts,
            minimum_height: o.minimum_height,
            maximum_height: o.maximum_height,
            rectangle: o.rectangle,
            bounding_sphere_3d: o.bounding_sphere_3d,
            occludee_point_in_scaled_space: o.occludee_point_in_scaled_space,
            oriented_bounding_box: o.oriented_bounding_box,
            encoding: o.encoding,
            edges: o.edges,
            transform: Matrix4::ZERO,
            last_pick_scene_mode: None,
            picker: TerrainPicker::new(),
        }
    }

    pub fn get_transform(&mut self, mode: Option<TerrainSceneMode>, projection: &dyn MapProjection) -> Option<Matrix4> {
        // Includes the upstream initial undefined-mode cache hit (zero matrix).
        if self.last_pick_scene_mode == mode {
            return Some(self.transform);
        }
        self.picker.needs_rebuild = true;
        let min = vertical_exaggeration::get_height(
            self.minimum_height,
            self.encoding.exaggeration,
            self.encoding.exaggeration_relative_height,
        );
        let max = vertical_exaggeration::get_height(
            self.maximum_height,
            self.encoding.exaggeration,
            self.encoding.exaggeration_relative_height,
        );
        self.transform = if mode.is_none() || mode == Some(TerrainSceneMode::Scene3D) {
            // Cesium uses Ellipsoid.default, independently of the map projection.
            let obb = OrientedBoundingBox::from_rectangle(&self.rectangle, min, max, &Ellipsoid::WGS84)?;
            if self.oriented_bounding_box.is_some() {
                self.oriented_bounding_box = Some(obb);
            }
            obb.compute_transformation()
        } else {
            let sw = projection.project(&Cartographic::new(self.rectangle.west, self.rectangle.south, 0.0));
            let ne = projection.project(&Cartographic::new(self.rectangle.east, self.rectangle.north, 0.0));
            let range = max - min;
            let scale = Cartesian3::new(ne.x - sw.x, ne.y - sw.y, if range > 0.0 { range } else { 1.0 });
            let center = Cartesian3::new(sw.x + scale.x * 0.5, sw.y + scale.y * 0.5, min + scale.z * 0.5);
            Transforms::SWIZZLE_3D_TO_2D_MATRIX.multiply(&Matrix4::from_translation(center).set_scale(scale))
        };
        Some(self.transform)
    }

    pub fn pick(
        &mut self,
        ray: &Ray,
        cull_back_faces: bool,
        mode: Option<TerrainSceneMode>,
        projection: &dyn MapProjection,
    ) -> Option<Cartesian3> {
        let transform = self.get_transform(mode, projection)?;
        let result = self.picker.ray_intersect(
            TerrainPickMesh { vertices: &self.vertices, indices: &self.indices, encoding: &self.encoding },
            TerrainPickContext { ray, tile_transform: &transform, cull_back_faces, mode, projection },
        );
        self.last_pick_scene_mode = mode;
        result
    }

    /// Encoding values are already updated by the caller, as in Cesium.
    /// The source's two numeric arguments are unused and omitted in Rust.
    pub fn update_exaggeration(&mut self) {
        self.picker.needs_rebuild = true;
        self.last_pick_scene_mode = None;
    }

    /// The source's scene-mode argument is unused; the next pick supplies it.
    pub fn update_scene_mode(&mut self) {
        self.update_exaggeration();
    }

    pub fn picker(&self) -> &TerrainPicker {
        &self.picker
    }
}
