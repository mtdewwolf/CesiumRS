//! CesiumJS `TerrainPicker`. Quadtree refinement runs synchronously in native
//! Rust instead of transferring buffers to a browser worker. Each pick refines
//! only the leaves it tested, preserving Cesium's depth limit and traversal.
//! Mesh buffers are borrowed per pick so TerrainMesh needs no duplicate buffers.

use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::cartesian3::Cartesian3;
use super::index_datatype::IndexDatatype;
use super::intersection_tests;
use super::map_projection::MapProjection;
use super::math::{clamp, js_round, TWO_PI};
use super::matrix4::Matrix4;
use super::ray::Ray;
use super::terrain_encoding::TerrainEncoding;
use super::typed_array::TypedArray;

/// Numeric values match SceneMode; the terrain code needs no renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TerrainSceneMode {
    Morphing = 0,
    ColumbusView = 1,
    Scene2D = 2,
    Scene3D = 3,
}

#[derive(Clone, Copy)]
pub struct TerrainPickMesh<'a> {
    pub vertices: &'a [f32],
    pub indices: &'a TypedArray,
    pub encoding: &'a TerrainEncoding,
}

#[derive(Clone, Copy)]
pub struct TerrainPickContext<'a> {
    pub ray: &'a Ray,
    pub tile_transform: &'a Matrix4,
    pub cull_back_faces: bool,
    pub mode: Option<TerrainSceneMode>,
    pub projection: &'a dyn MapProjection,
}

#[derive(Clone, Debug)]
struct Node {
    x: u32,
    y: u32,
    level: u32,
    aabb: AxisAlignedBoundingBox,
    triangles: Vec<u32>,
    children: Vec<Node>,
}

impl Node {
    fn new(x: u32, y: u32, level: u32) -> Self {
        let size = 1.0 / (1u32 << level) as f64;
        Self {
            x,
            y,
            level,
            aabb: AxisAlignedBoundingBox::from_corners(
                Cartesian3::new(x as f64 * size - 0.5, y as f64 * size - 0.5, -0.5),
                Cartesian3::new((x + 1) as f64 * size - 0.5, (y + 1) as f64 * size - 0.5, 0.5),
            ),
            triangles: Vec::new(),
            children: Vec::new(),
        }
    }

    fn collect(&self, ray: &Ray, path: &mut Vec<usize>, out: &mut Vec<(f64, Vec<usize>)>) {
        let Some(interval) = intersection_tests::ray_axis_aligned_bounding_box(ray, &self.aabb) else {
            return;
        };
        if self.children.is_empty() {
            out.push((interval.start, path.clone()));
        } else {
            for (i, child) in self.children.iter().enumerate() {
                path.push(i);
                child.collect(ray, path, out);
                path.pop();
            }
        }
    }

    fn intersect(
        &mut self,
        mesh: TerrainPickMesh<'_>,
        context: TerrainPickContext<'_>,
        inverse: &Matrix4,
    ) -> Option<f64> {
        let mut closest = f64::MAX;
        let refine = self.level < 3;
        let mut positions = Vec::new();
        for &triangle in &self.triangles {
            let mut points = [Cartesian3::ZERO; 3];
            for (i, point) in points.iter_mut().enumerate() {
                let index = mesh.indices.get((triangle as usize).checked_mul(3)?.checked_add(i)?)? as usize;
                *point = vertex_position(mesh, context, index)?;
            }
            if let Some(t) = intersection_tests::ray_triangle_parametric(
                context.ray,
                points[0],
                points[1],
                points[2],
                context.cull_back_faces,
            ) {
                if t < closest && t >= 0.0 {
                    closest = t;
                }
            }
            if refine {
                positions.push(points);
            }
        }
        if refine {
            self.children =
                (0..4).map(|i| Node::new(self.x * 2 + (i & 1), self.y * 2 + ((i >> 1) & 1), self.level + 1)).collect();
            let boxes: Vec<_> = self.children.iter().map(|n| n.aabb).collect();
            let groups = distribute_triangles(&boxes, inverse, &self.triangles, &positions)?;
            for (child, triangles) in self.children.iter_mut().zip(groups) {
                child.triangles = triangles;
            }
            self.triangles.clear();
        }
        Some(closest)
    }
}

/// Worker calculation from `incrementallyBuildTerrainPicker`: transform each
/// triangle's AABB, clamp it to the unit tile cube, and distribute inclusively.
/// A native function replaces worker serialization and buffer transfers.
pub fn distribute_triangles(
    boxes: &[AxisAlignedBoundingBox],
    inverse: &Matrix4,
    indices: &[u32],
    positions: &[[Cartesian3; 3]],
) -> Option<Vec<Vec<u32>>> {
    if indices.len() != positions.len() {
        return None;
    }
    let mut groups = vec![Vec::new(); boxes.len()];
    for (&index, points) in indices.iter().zip(positions) {
        let points = points.map(|p| inverse.multiply_by_point(p));
        let mut bounds = AxisAlignedBoundingBox::from_points(&points);
        let constrain =
            |p: Cartesian3| Cartesian3::new(clamp(p.x, -0.5, 0.5), clamp(p.y, -0.5, 0.5), clamp(p.z, -0.5, 0.5));
        bounds.minimum = constrain(bounds.minimum);
        bounds.maximum = constrain(bounds.maximum);
        for (aabb, group) in boxes.iter().zip(&mut groups) {
            if aabb.intersect_axis_aligned_bounding_box(&bounds) {
                group.push(index);
            }
        }
    }
    Some(groups)
}

fn vertex_position(mesh: TerrainPickMesh<'_>, context: TerrainPickContext<'_>, index: usize) -> Option<Cartesian3> {
    let mut position = mesh.encoding.get_exaggerated_position(mesh.vertices, index)?;
    if context.mode == Some(TerrainSceneMode::Scene3D) {
        return Some(position);
    }
    let cartographic = context.projection.ellipsoid().cartesian_to_cartographic(position)?;
    let p = context.projection.project(&cartographic);
    position = Cartesian3::new(p.z, p.x, p.y);
    let width = TWO_PI * context.projection.ellipsoid().maximum_radius;
    position.y += js_round((context.ray.origin.y - position.y) / width) * width;
    Some(position)
}

#[derive(Clone, Debug)]
pub struct TerrainPicker {
    pub needs_rebuild: bool,
    inverse_transform: Matrix4,
    root: Node,
}

impl Default for TerrainPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl TerrainPicker {
    pub fn new() -> Self {
        Self { needs_rebuild: true, inverse_transform: Matrix4::ZERO, root: Node::new(0, 0, 0) }
    }

    /// None means a miss or invalid buffers/noninvertible transform. Set
    /// needs_rebuild after changing the borrowed mesh or its world transform.
    pub fn ray_intersect(&mut self, mesh: TerrainPickMesh<'_>, context: TerrainPickContext<'_>) -> Option<Cartesian3> {
        IndexDatatype::from_typed_array(mesh.indices)?;
        if self.needs_rebuild {
            self.inverse_transform = context.tile_transform.inverse().ok()?;
            self.root.triangles = (0..u32::try_from(mesh.indices.len() / 3).ok()?).collect();
            self.root.children.clear();
            self.needs_rebuild = false;
        }
        // Do not normalize: interval parameters must retain the world ray scale.
        let ray = Ray {
            origin: self.inverse_transform.multiply_by_point(context.ray.origin),
            direction: self.inverse_transform.multiply_by_point_as_vector(context.ray.direction),
        };
        let mut intersections = Vec::new();
        self.root.collect(&ray, &mut Vec::new(), &mut intersections);
        intersections.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, path) in intersections {
            let mut node = &mut self.root;
            for i in path {
                node = &mut node.children[i];
            }
            let closest = node.intersect(mesh, context, &self.inverse_transform)?;
            // Cesium stops at the first intersecting leaf that has a hit.
            if closest != f64::MAX {
                return Some(context.ray.get_point(closest));
            }
        }
        None
    }

    /// Diagnostic tree shape, useful to verify incremental native refinement.
    pub fn node_count(&self) -> usize {
        fn count(node: &Node) -> usize {
            1 + node.children.iter().map(count).sum::<usize>()
        }
        count(&self.root)
    }
}
