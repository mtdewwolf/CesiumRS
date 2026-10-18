//! CesiumJS `PolygonGeometryLibrary`: polygon hierarchies (packing,
//! flattening into triangulatable rings, splitting on the equator), edge
//! subdivision, and the top and wall geometries the polygon geometries
//! build on.
//!
//! Cesium's in-place quirks are kept: flattening a hierarchy scales its
//! positions to the surface in place, and Cesium's equator split is called
//! with one array as both input and output, which repeats the ring.

use super::arc_type::ArcType;
use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_rectangle::BoundingRectangle;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_rhumb_line::EllipsoidRhumbLine;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::geometry_pipeline::compute_normal;
use super::index_datatype::IndexDatatype;
use super::intersection_tests::line_segment_plane;
use super::math::{chord_length, js_max, js_min, log2, sign, EPSILON10, PI};
use super::matrix3::Matrix3;
use super::plane::Plane;
use super::polygon_hierarchy::PolygonHierarchy;
use super::polygon_pipeline::{compute_rhumb_line_subdivision, compute_subdivision, compute_winding_order_2d, triangulate};
use super::primitive_type::PrimitiveType;
use super::quaternion::Quaternion;
use super::typed_array::TypedArray;
use super::winding_order::WindingOrder;

/// A hierarchy of texture-coordinate rings, mirroring the position
/// hierarchy (Cesium uses a `PolygonHierarchy` of `Cartesian2`s).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureCoordinateHierarchy {
    pub positions: Vec<Cartesian2>,
    pub holes: Vec<TextureCoordinateHierarchy>,
}

/// A packable ring point: `Cartesian3` or `Cartesian2`.
pub trait HierarchyPoint: Copy {
    const PACKED_LENGTH: usize;
    fn pack_into(&self, array: &mut [f64], index: usize);
    fn unpack_from(array: &[f64], index: usize) -> Self;
}

impl HierarchyPoint for Cartesian3 {
    const PACKED_LENGTH: usize = 3;
    fn pack_into(&self, array: &mut [f64], index: usize) {
        self.pack(array, index);
    }
    fn unpack_from(array: &[f64], index: usize) -> Self {
        Cartesian3::unpack(array, index)
    }
}

impl HierarchyPoint for Cartesian2 {
    const PACKED_LENGTH: usize = 2;
    fn pack_into(&self, array: &mut [f64], index: usize) {
        self.pack(array, index);
    }
    fn unpack_from(array: &[f64], index: usize) -> Self {
        Cartesian2::unpack(array, index)
    }
}

/// A ring hierarchy over either point type.
pub trait Hierarchy: Sized {
    type Point: HierarchyPoint;
    fn positions(&self) -> &[Self::Point];
    fn holes(&self) -> &[Self];
    fn from_parts(positions: Vec<Self::Point>, holes: Vec<Self>) -> Self;
}

impl Hierarchy for PolygonHierarchy {
    type Point = Cartesian3;
    fn positions(&self) -> &[Cartesian3] {
        &self.positions
    }
    fn holes(&self) -> &[Self] {
        &self.holes
    }
    fn from_parts(positions: Vec<Cartesian3>, holes: Vec<Self>) -> Self {
        Self { positions, holes }
    }
}

impl Hierarchy for TextureCoordinateHierarchy {
    type Point = Cartesian2;
    fn positions(&self) -> &[Cartesian2] {
        &self.positions
    }
    fn holes(&self) -> &[Self] {
        &self.holes
    }
    fn from_parts(positions: Vec<Cartesian2>, holes: Vec<Self>) -> Self {
        Self { positions, holes }
    }
}

/// Numbers needed to pack a hierarchy: two counts per ring plus its points.
pub fn compute_hierarchy_packed_length<H: Hierarchy>(hierarchy: &H) -> usize {
    let mut n = 0;
    let mut stack = vec![hierarchy];
    while let Some(h) = stack.pop() {
        n += 2 + h.positions().len() * H::Point::PACKED_LENGTH;
        stack.extend(h.holes());
    }
    n
}

/// Packs depth first from a stack, so sibling holes come out in reverse
/// order (and unpack reversed), as in Cesium. Returns the next index.
pub fn pack_polygon_hierarchy<H: Hierarchy>(hierarchy: &H, array: &mut [f64], mut index: usize) -> usize {
    let mut stack = vec![hierarchy];
    while let Some(h) = stack.pop() {
        array[index] = h.positions().len() as f64;
        array[index + 1] = h.holes().len() as f64;
        index += 2;
        for p in h.positions() {
            p.pack_into(array, index);
            index += H::Point::PACKED_LENGTH;
        }
        stack.extend(h.holes());
    }
    index
}

/// Unpacks a hierarchy and returns it with the next index.
pub fn unpack_polygon_hierarchy<H: Hierarchy>(array: &[f64], mut index: usize) -> (H, usize) {
    let positions_length = array[index] as usize;
    let holes_length = array[index + 1] as usize;
    index += 2;
    let mut positions = Vec::with_capacity(positions_length);
    for _ in 0..positions_length {
        positions.push(H::Point::unpack_from(array, index));
        index += H::Point::PACKED_LENGTH;
    }
    let mut holes = Vec::with_capacity(holes_length);
    for _ in 0..holes_length {
        let (hole, next) = unpack_polygon_hierarchy::<H>(array, index);
        holes.push(hole);
        index = next;
    }
    (H::from_parts(positions, holes), index)
}

/// `2^ceil(log2(n))`, at least 1: Cesium's subdivision count for `n` lengths.
fn power_of_two_count(n: f64) -> usize {
    let count_divide = js_max(0.0, log2(n).ceil());
    2f64.powf(count_divide) as usize
}

/// Points to place along a straight segment so each piece is at most
/// `min_distance` long.
pub fn subdivide_line_count(p0: Cartesian3, p1: Cartesian3, min_distance: f64) -> usize {
    power_of_two_count(p0.distance(p1) / min_distance)
}

fn rhumb_line(ellipsoid: &Ellipsoid, p0: Cartesian3, p1: Cartesian3) -> Option<EllipsoidRhumbLine> {
    let c0 = ellipsoid.cartesian_to_cartographic(p0)?;
    let c1 = ellipsoid.cartesian_to_cartographic(p1)?;
    Some(EllipsoidRhumbLine::new(&c0, &c1, ellipsoid))
}

/// Like [`subdivide_line_count`], along the rhumb line.
pub fn subdivide_rhumb_line_count(ellipsoid: &Ellipsoid, p0: Cartesian3, p1: Cartesian3, min_distance: f64) -> Option<usize> {
    Some(power_of_two_count(rhumb_line(ellipsoid, p0, p1)?.surface_distance / min_distance))
}

fn texcoords_along(t0: Cartesian2, t1: Cartesian2, subdivisions: usize) -> Vec<f64> {
    let length_2d = t0.distance(t1);
    let step = length_2d / subdivisions as f64;
    (0..subdivisions)
        .flat_map(|i| {
            let t = t0.add(t1.subtract(t0).multiply_by_scalar(i as f64 * step / length_2d));
            [t.x, t.y]
        })
        .collect()
}

/// Texture coordinates along a segment, one per subdivided position.
pub fn subdivide_texcoord_line(t0: Cartesian2, t1: Cartesian2, p0: Cartesian3, p1: Cartesian3, min_distance: f64) -> Vec<f64> {
    texcoords_along(t0, t1, subdivide_line_count(p0, p1, min_distance))
}

/// Points along a straight segment from `p0` (excluding `p1`), flattened.
pub fn subdivide_line(p0: Cartesian3, p1: Cartesian3, min_distance: f64) -> Vec<f64> {
    let num_vertices = subdivide_line_count(p0, p1, min_distance);
    let length = p0.distance(p1);
    let step = length / num_vertices as f64;
    (0..num_vertices)
        .flat_map(|i| {
            let p = p0.add(p1.subtract(p0).multiply_by_scalar(i as f64 * step / length));
            [p.x, p.y, p.z]
        })
        .collect()
}

/// Texture coordinates along a rhumb-line segment.
pub fn subdivide_texcoord_rhumb_line(
    t0: Cartesian2,
    t1: Cartesian2,
    ellipsoid: &Ellipsoid,
    p0: Cartesian3,
    p1: Cartesian3,
    min_distance: f64,
) -> Option<Vec<f64>> {
    Some(texcoords_along(t0, t1, subdivide_rhumb_line_count(ellipsoid, p0, p1, min_distance)?))
}

/// Points along the rhumb line from `p0` (excluding `p1`), flattened; just
/// `p0` when the line is shorter than `min_distance`.
pub fn subdivide_rhumb_line(ellipsoid: &Ellipsoid, p0: Cartesian3, p1: Cartesian3, min_distance: f64) -> Option<Vec<f64>> {
    let rhumb = rhumb_line(ellipsoid, p0, p1)?;
    if rhumb.surface_distance <= min_distance {
        return Some(vec![p0.x, p0.y, p0.z]);
    }
    let num_vertices = power_of_two_count(rhumb.surface_distance / min_distance);
    let step = rhumb.surface_distance / num_vertices as f64;
    Some(
        (0..num_vertices)
            .flat_map(|i| {
                let c = rhumb.interpolate_using_surface_distance(i as f64 * step);
                let p = ellipsoid.cartographic_to_cartesian(&c);
                [p.x, p.y, p.z]
            })
            .collect(),
    )
}

/// For a geometry whose positions are a top half and a bottom half, moves
/// the bottom half to `min_height` and the top half to `max_height` (from
/// the original positions when `per_position_height`), in place.
pub fn scale_to_geodetic_height_extruded(
    geometry: &mut Geometry,
    max_height: f64,
    min_height: f64,
    ellipsoid: &Ellipsoid,
    per_position_height: bool,
) -> Option<()> {
    let Some(position) = geometry.attributes.position.as_mut() else { return Some(()) };
    let values = &mut position.values;
    let length = values.len() / 2;
    for i in (0..length).step_by(3) {
        let p = Cartesian3::new(values.get(i)?, values.get(i + 1)?, values.get(i + 2)?);
        let n1 = ellipsoid.geodetic_surface_normal(p)?;
        let mut p2 = ellipsoid.scale_to_geodetic_surface(p)?;
        let bottom = p2.add(n1.multiply_by_scalar(min_height));
        values.set(i + length, bottom.x);
        values.set(i + 1 + length, bottom.y);
        values.set(i + 2 + length, bottom.z);
        if per_position_height {
            p2 = p;
        }
        let top = p2.add(n1.multiply_by_scalar(max_height));
        values.set(i, top.x);
        values.set(i + 1, top.y);
        values.set(i + 2, top.z);
    }
    Some(())
}

fn scale_ring(ring: &mut [Cartesian3], ellipsoid: &Ellipsoid) {
    for p in ring.iter_mut() {
        // Cesium writes the result into the same object; a point at the
        // center leaves it as it was.
        if let Some(s) = ellipsoid.scale_to_geodetic_surface(*p) {
            *p = s;
        }
    }
}

fn remove_duplicates(ring: &[Cartesian3]) -> Vec<Cartesian3> {
    array_remove_duplicates(ring, |a, b, e| a.equals_epsilon(*b, e), true, None)
}

/// Every ring of the hierarchy (holes before their outer ring), for
/// outlines. With `scale_to_ellipsoid_surface` the hierarchy's positions
/// are moved to the surface in place, as in Cesium.
pub fn polygon_outlines_from_hierarchy(
    hierarchy: &mut PolygonHierarchy,
    scale_to_ellipsoid_surface: bool,
    ellipsoid: &Ellipsoid,
) -> Vec<Vec<Cartesian3>> {
    let mut polygons = Vec::new();
    let mut queue: std::collections::VecDeque<&mut PolygonHierarchy> = std::collections::VecDeque::new();
    queue.push_back(hierarchy);
    while let Some(outer_node) = queue.pop_front() {
        if scale_to_ellipsoid_surface {
            scale_ring(&mut outer_node.positions, ellipsoid);
        }
        let outer_ring = remove_duplicates(&outer_node.positions);
        if outer_ring.len() < 3 {
            continue;
        }
        for hole in outer_node.holes.iter_mut() {
            if scale_to_ellipsoid_surface {
                scale_ring(&mut hole.positions, ellipsoid);
            }
            let hole_positions = remove_duplicates(&hole.positions);
            if hole_positions.len() < 3 {
                continue;
            }
            polygons.push(hole_positions);
            for grandchild in hole.holes.iter_mut() {
                queue.push_back(grandchild);
            }
        }
        polygons.push(outer_ring);
    }
    polygons
}

fn compute_equator_intersection_rhumb(start: Cartesian3, end: Cartesian3, ellipsoid: &Ellipsoid) -> Option<Cartesian3> {
    let c0 = ellipsoid.cartesian_to_cartographic(start)?;
    let c1 = ellipsoid.cartesian_to_cartographic(end)?;
    // Math.sign, so -0 and 0 count as different signs.
    if sign(c0.latitude).to_bits() == sign(c1.latitude).to_bits() {
        return None;
    }
    let rhumb = EllipsoidRhumbLine::new(&c0, &c1, ellipsoid);
    let intersection = rhumb.find_intersection_with_latitude(0.0)?;
    let mut min_longitude = js_min(c0.longitude, c1.longitude);
    let mut max_longitude = js_max(c0.longitude, c1.longitude);
    if (max_longitude - min_longitude).abs() > PI {
        std::mem::swap(&mut min_longitude, &mut max_longitude);
    }
    if intersection.longitude < min_longitude || intersection.longitude > max_longitude {
        return None;
    }
    Some(ellipsoid.cartographic_to_cartesian(&intersection))
}

fn compute_equator_intersection(start: Cartesian3, end: Cartesian3, ellipsoid: &Ellipsoid, arc_type: ArcType) -> Option<Cartesian3> {
    if arc_type == ArcType::Rhumb {
        return compute_equator_intersection_rhumb(start, end, ellipsoid);
    }
    let intersection = line_segment_plane(start, end, &Plane::ORIGIN_XY_PLANE)?;
    ellipsoid.scale_to_geodetic_surface(intersection)
}

#[derive(Clone, Copy, Debug)]
struct EdgeOnPlane {
    position: usize,
    kind: f64,
    visited: bool,
    next: f64,
    theta: f64,
}

fn longitude(ellipsoid: &Ellipsoid, p: Cartesian3) -> f64 {
    ellipsoid.cartesian_to_cartographic(p).map_or(f64::NAN, |c| c.longitude)
}

/// Inserts the equator crossings into `positions` and lists the points on
/// the equator.
fn compute_edges_on_plane(positions: &mut Vec<Cartesian3>, ellipsoid: &Ellipsoid, arc_type: ArcType) -> Vec<EdgeOnPlane> {
    let mut edges = Vec::new();
    let mut i = 0;
    while i < positions.len() {
        let start = positions[i];
        let end = positions[(i + 1) % positions.len()];
        let kind = sign(start.z);
        let next = sign(end.z);
        if kind == 0.0 {
            edges.push(EdgeOnPlane { position: i, kind, visited: false, next, theta: longitude(ellipsoid, start) });
        } else if next != 0.0 {
            let intersection = compute_equator_intersection(start, end, ellipsoid, arc_type);
            i += 1;
            let Some(intersection) = intersection else { continue };
            positions.insert(i, intersection);
            edges.push(EdgeOnPlane { position: i, kind, visited: false, next, theta: longitude(ellipsoid, intersection) });
        }
        i += 1;
    }
    edges
}

#[allow(clippy::too_many_arguments)]
fn wire_polygon(
    polygons: &mut Vec<Vec<Cartesian3>>,
    mut polygon_index: usize,
    positions: &[Cartesian3],
    edges: &mut [EdgeOnPlane],
    to_delete: usize,
    start_index: usize,
    above_plane: bool,
) -> usize {
    let mut polygon = Vec::new();
    let mut i = start_index as isize;
    let mut polygons_to_wire = Vec::new();
    loop {
        'body: {
            polygon.push(positions[i as usize]);
            let Some(edge_index) = edges.iter().position(|e| e.position as isize == i) else {
                i += 1;
                break 'body;
            };
            let EdgeOnPlane { visited, kind, next, .. } = edges[edge_index];
            edges[edge_index].visited = true;
            if kind == 0.0 {
                if next == 0.0 {
                    let previous = edge_index as isize - if above_plane { 1 } else { -1 };
                    match usize::try_from(previous).ok().and_then(|p| edges.get(p).map(|e| (p, e.position))) {
                        Some((p, position)) if position as isize == i + 1 => edges[p].visited = true,
                        _ => {
                            i += 1;
                            break 'body;
                        }
                    }
                }
                if (!visited && above_plane && next > 0.0) || (start_index as isize == i && !above_plane && next < 0.0) {
                    i += 1;
                    break 'body;
                }
            }
            let follow_edge = if above_plane { kind >= 0.0 } else { kind <= 0.0 };
            if !follow_edge {
                i += 1;
                break 'body;
            }
            if !visited {
                polygons_to_wire.push(i as usize);
            }
            let next_edge_index = edge_index as isize + if above_plane { 1 } else { -1 };
            match usize::try_from(next_edge_index).ok().and_then(|n| edges.get(n)) {
                Some(e) => i = e.position as isize,
                None => i += 1,
            }
        }
        if !(i < positions.len() as isize && i >= 0 && i != start_index as isize && polygon.len() < positions.len()) {
            break;
        }
    }
    // Array.prototype.splice clamps the start index.
    let at = polygon_index.min(polygons.len());
    let end = (at + to_delete).min(polygons.len());
    polygons.splice(at..end, [polygon]);
    for index in polygons_to_wire {
        polygon_index = wire_polygon(polygons, polygon_index + 1, positions, edges, 0, index, !above_plane);
    }
    polygon_index
}

/// Cesium's `splitPolygonsOnEquator` on `result`, which already holds the
/// rings to split.
fn split_on_equator_in_place(result: &mut Vec<Vec<Cartesian3>>, ellipsoid: &Ellipsoid, arc_type: ArcType) {
    let mut current = 0;
    while current < result.len() {
        let outer_ring = result[current].clone();
        let mut positions = outer_ring.clone();
        if outer_ring.len() < 3 {
            result[current] = positions;
            current += 1;
            continue;
        }
        let mut edges = compute_edges_on_plane(&mut positions, ellipsoid, arc_type);
        if positions.len() == outer_ring.len() || edges.len() <= 1 {
            result[current] = positions;
            current += 1;
            continue;
        }
        // Array.prototype.sort is stable; NaN compares equal.
        edges.sort_by(|a, b| a.theta.partial_cmp(&b.theta).unwrap_or(std::cmp::Ordering::Equal));
        let north = positions[0].z >= 0.0;
        current = wire_polygon(result, current, &positions, &mut edges, 1, 0, north);
    }
}

/// Splits rings that cross the equator into rings on each side.
pub fn split_polygons_on_equator(outer_rings: &[Vec<Cartesian3>], ellipsoid: &Ellipsoid, arc_type: ArcType) -> Vec<Vec<Cartesian3>> {
    let mut result = outer_rings.to_vec();
    split_on_equator_in_place(&mut result, ellipsoid, arc_type);
    result
}

/// What `polygonsFromHierarchy` produces for one polygon.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolygonPositions {
    /// The outer ring followed by each hole.
    pub positions: Vec<Cartesian3>,
    pub positions_2d: Vec<Cartesian2>,
    /// Where each hole starts in `positions`.
    pub holes: Vec<usize>,
}

/// The rings of one polygon after winding correction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolygonRings {
    pub outer_ring: Vec<Cartesian3>,
    pub holes: Vec<Vec<Cartesian3>>,
}

/// How `polygons_from_hierarchy` splits a ring: Cesium passes the same
/// array as input and result, so the ring is split twice over.
pub enum SplitPolygons {
    None,
    OnEquator { ellipsoid: Ellipsoid, arc_type: ArcType },
}

/// Flattens a hierarchy into counter-clockwise outer rings with clockwise
/// holes, ready for triangulation. `project_points_to_2d` maps a ring to 2D
/// (`None` skips the ring). With `scale_to_ellipsoid_surface` the
/// hierarchy's positions are scaled in place, as in Cesium.
pub fn polygons_from_hierarchy(
    polygon_hierarchy: &mut PolygonHierarchy,
    keep_duplicates: bool,
    project_points_to_2d: &dyn Fn(&[Cartesian3]) -> Option<Vec<Cartesian2>>,
    scale_to_ellipsoid_surface: bool,
    ellipsoid: &Ellipsoid,
    split_polygons: SplitPolygons,
) -> (Vec<PolygonRings>, Vec<PolygonPositions>) {
    let mut hierarchy = Vec::new();
    let mut polygons = Vec::new();
    // Split rings are new hierarchies sharing the original holes; the queue
    // holds owned copies so scaling still reaches the caller's positions for
    // the first pass.
    enum Node<'a> {
        Borrowed(&'a mut PolygonHierarchy),
        Owned(PolygonHierarchy),
    }
    let mut queue: std::collections::VecDeque<Node> = std::collections::VecDeque::new();
    queue.push_back(Node::Borrowed(polygon_hierarchy));
    let mut split = matches!(split_polygons, SplitPolygons::OnEquator { .. });
    while let Some(node) = queue.pop_front() {
        let mut owned;
        let outer_node: &mut PolygonHierarchy = match node {
            Node::Borrowed(h) => h,
            Node::Owned(h) => {
                owned = h;
                &mut owned
            }
        };
        if scale_to_ellipsoid_surface {
            scale_ring(&mut outer_node.positions, ellipsoid);
        }
        let mut outer_ring = if keep_duplicates { outer_node.positions.clone() } else { remove_duplicates(&outer_node.positions) };
        if outer_ring.len() < 3 {
            continue;
        }
        let Some(mut positions_2d) = project_points_to_2d(&outer_ring) else { continue };
        let mut hole_indices = Vec::new();
        if compute_winding_order_2d(&positions_2d) == WindingOrder::Clockwise {
            positions_2d.reverse();
            outer_ring.reverse();
        }
        if split {
            split = false;
            if let SplitPolygons::OnEquator { ellipsoid, arc_type } = &split_polygons {
                let mut rings = vec![outer_ring.clone(), outer_ring.clone()];
                split_on_equator_in_place(&mut rings, ellipsoid, *arc_type);
                if rings.len() > 1 {
                    for positions in rings {
                        queue.push_back(Node::Owned(PolygonHierarchy::new(positions, outer_node.holes.clone())));
                    }
                    continue;
                }
            }
        }
        let mut positions = outer_ring.clone();
        let mut polygon_holes = Vec::new();
        for hole in outer_node.holes.iter_mut() {
            if scale_to_ellipsoid_surface {
                scale_ring(&mut hole.positions, ellipsoid);
            }
            let mut hole_positions = if keep_duplicates { hole.positions.clone() } else { remove_duplicates(&hole.positions) };
            if hole_positions.len() < 3 {
                continue;
            }
            let Some(mut hole_2d) = project_points_to_2d(&hole_positions) else { continue };
            if compute_winding_order_2d(&hole_2d) == WindingOrder::Clockwise {
                hole_2d.reverse();
                hole_positions.reverse();
            }
            hole_indices.push(positions.len());
            positions.extend_from_slice(&hole_positions);
            positions_2d.extend_from_slice(&hole_2d);
            polygon_holes.push(hole_positions);
            for grandchild in &hole.holes {
                queue.push_back(Node::Owned(grandchild.clone()));
            }
        }
        hierarchy.push(PolygonRings { outer_ring, holes: polygon_holes });
        polygons.push(PolygonPositions { positions, positions_2d, holes: hole_indices });
    }
    (hierarchy, polygons)
}

/// `polygonsFromHierarchy` on texture coordinates: duplicates kept, no
/// projection or scaling. Returns each polygon's coordinates, holes appended.
pub fn texture_polygons_from_hierarchy(hierarchy: &TextureCoordinateHierarchy) -> Vec<Vec<Cartesian2>> {
    let mut polygons = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(hierarchy);
    while let Some(outer_node) = queue.pop_front() {
        let mut outer_ring = outer_node.positions.clone();
        if outer_ring.len() < 3 {
            continue;
        }
        if compute_winding_order_2d(&outer_ring) == WindingOrder::Clockwise {
            outer_ring.reverse();
        }
        let mut positions = outer_ring;
        for hole in &outer_node.holes {
            let mut hole_positions = hole.positions.clone();
            if hole_positions.len() < 3 {
                continue;
            }
            if compute_winding_order_2d(&hole_positions) == WindingOrder::Clockwise {
                hole_positions.reverse();
            }
            positions.extend_from_slice(&hole_positions);
            queue.extend(&hole.holes);
        }
        polygons.push(positions);
    }
    polygons
}

/// The 2D bounds of `positions` rotated by `angle` about `plane_normal`
/// and projected.
pub fn compute_bounding_rectangle(
    plane_normal: Cartesian3,
    project_point_to_2d: &dyn Fn(Cartesian3) -> Option<Cartesian2>,
    positions: &[Cartesian3],
    angle: f64,
) -> BoundingRectangle {
    let texture_matrix = Matrix3::from_quaternion(&Quaternion::from_axis_angle(plane_normal, angle));
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for p in positions {
        if let Some(st) = project_point_to_2d(texture_matrix.multiply_by_vector(*p)) {
            min_x = js_min(min_x, st.x);
            max_x = js_max(max_x, st.x);
            min_y = js_min(min_y, st.y);
            max_y = js_max(max_y, st.y);
        }
    }
    BoundingRectangle::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Plain-array attribute values, as Cesium builds them.
fn plain(datatype: ComponentDatatype, components: u8, values: Vec<f64>) -> GeometryAttribute {
    GeometryAttribute::new(datatype, components, TypedArray::Float64(values))
}

/// The triangulated (and subdivided) top of a polygon. `None` for an arc
/// type of `None`, where Cesium returns nothing.
pub fn create_geometry_from_positions(
    ellipsoid: &Ellipsoid,
    polygon: &PolygonPositions,
    texture_coordinates: Option<&[Cartesian2]>,
    granularity: f64,
    per_position_height: bool,
    compute_normals: bool,
    arc_type: ArcType,
) -> Option<Geometry> {
    let mut indices = triangulate(&polygon.positions_2d, &polygon.holes);
    // A completely unrenderable polygon uses its first three vertices.
    if indices.len() < 3 {
        indices = vec![0, 1, 2];
    }
    let positions = &polygon.positions;
    if per_position_height {
        let mut attributes =
            GeometryAttributes { position: Some(plain(ComponentDatatype::Double, 3, Cartesian3::pack_array(positions))), ..Default::default() };
        if let Some(t) = texture_coordinates {
            attributes.st = Some(plain(ComponentDatatype::Float, 2, Cartesian2::pack_array(t)));
        }
        let indices = TypedArray::Uint32(indices.iter().map(|&i| i as u32).collect());
        let mut geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Triangles);
        if compute_normals {
            compute_normal(&mut geometry)?;
        }
        return Some(geometry);
    }
    match arc_type {
        ArcType::Geodesic => compute_subdivision(ellipsoid, positions, &indices, texture_coordinates, Some(granularity)),
        ArcType::Rhumb => compute_rhumb_line_subdivision(ellipsoid, positions, &indices, texture_coordinates, Some(granularity)),
        ArcType::None => None,
    }
}

/// The side walls along a ring: subdivided top and bottom edges (equal for
/// now; heights are applied later) and their triangles.
pub fn compute_wall_geometry(
    positions: &[Cartesian3],
    texture_coordinates: Option<&[Cartesian2]>,
    ellipsoid: &Ellipsoid,
    granularity: f64,
    per_position_height: bool,
    arc_type: ArcType,
) -> Option<Geometry> {
    let length = positions.len();
    let mut edge_positions: Vec<f64>;
    let mut edge_texcoords: Option<Vec<f64>> = None;
    let mut index = 0;
    let mut texture_index = 0;
    if !per_position_height {
        let min_distance = chord_length(granularity, ellipsoid.maximum_radius);
        let mut num_vertices = 0;
        for i in 0..length {
            let (p1, p2) = (positions[i], positions[(i + 1) % length]);
            num_vertices += match arc_type {
                ArcType::Geodesic => subdivide_line_count(p1, p2, min_distance),
                ArcType::Rhumb => subdivide_rhumb_line_count(ellipsoid, p1, p2, min_distance)?,
                ArcType::None => 0,
            };
        }
        let top_edge_length = (num_vertices + length) * 3;
        edge_positions = vec![f64::NAN; top_edge_length * 2];
        let top_edge_texcoord_length = (num_vertices + length) * 2;
        if texture_coordinates.is_some() {
            edge_texcoords = Some(vec![f64::NAN; top_edge_texcoord_length * 2]);
        }
        for i in 0..length {
            let p1 = positions[i];
            let p2 = positions[(i + 1) % length];
            let texcoords = texture_coordinates.map(|t| (t[i], t[(i + 1) % length]));
            let (temp_positions, temp_texcoords) = match arc_type {
                ArcType::Geodesic => (
                    subdivide_line(p1, p2, min_distance),
                    texcoords.map(|(t1, t2)| subdivide_texcoord_line(t1, t2, p1, p2, min_distance)),
                ),
                ArcType::Rhumb => (
                    subdivide_rhumb_line(ellipsoid, p1, p2, min_distance)?,
                    match texcoords {
                        Some((t1, t2)) => Some(subdivide_texcoord_rhumb_line(t1, t2, ellipsoid, p1, p2, min_distance)?),
                        None => None,
                    },
                ),
                // Cesium reads an undefined array here.
                ArcType::None => return None,
            };
            for v in temp_positions.into_iter().chain([p2.x, p2.y, p2.z]) {
                edge_positions[index] = v;
                edge_positions[index + top_edge_length] = v;
                index += 1;
            }
            if let (Some(edge), Some(temp), Some((_, t2))) = (edge_texcoords.as_mut(), temp_texcoords, texcoords) {
                for v in temp.into_iter().chain([t2.x, t2.y]) {
                    edge[texture_index] = v;
                    edge[texture_index + top_edge_texcoord_length] = v;
                    texture_index += 1;
                }
            }
        }
    } else {
        let top_edge_length = length * 3 * 2;
        edge_positions = vec![f64::NAN; top_edge_length * 2];
        let top_edge_texcoord_length = length * 2 * 2;
        if texture_coordinates.is_some() {
            edge_texcoords = Some(vec![f64::NAN; top_edge_texcoord_length * 2]);
        }
        for i in 0..length {
            let p1 = positions[i];
            let p2 = positions[(i + 1) % length];
            for v in [p1.x, p1.y, p1.z, p2.x, p2.y, p2.z] {
                edge_positions[index] = v;
                edge_positions[index + top_edge_length] = v;
                index += 1;
            }
            if let (Some(edge), Some(t)) = (edge_texcoords.as_mut(), texture_coordinates) {
                let (t1, t2) = (t[i], t[(i + 1) % length]);
                for v in [t1.x, t1.y, t2.x, t2.y] {
                    edge[texture_index] = v;
                    edge[texture_index + top_edge_texcoord_length] = v;
                    texture_index += 1;
                }
            }
        }
    }

    let total = edge_positions.len();
    let mut indices = vec![0u32; total.saturating_sub(positions.len() * 6)];
    let mut edge_index = 0;
    let half = total / 6;
    for i in 0..half {
        let ul = i;
        let ur = ul + 1;
        let ll = ul + half;
        let lr = ll + 1;
        let p1 = Cartesian3::from_array(&edge_positions, ul * 3);
        let p2 = Cartesian3::from_array(&edge_positions, ur * 3);
        if p1.equals_epsilon_with(p2, EPSILON10, EPSILON10) {
            continue;
        }
        for v in [ul, ll, ur, ur, ll, lr] {
            if let Some(slot) = indices.get_mut(edge_index) {
                *slot = v as u32;
            }
            edge_index += 1;
        }
    }
    let mut attributes = GeometryAttributes { position: Some(plain(ComponentDatatype::Double, 3, edge_positions)), ..Default::default() };
    if let Some(t) = edge_texcoords {
        attributes.st = Some(plain(ComponentDatatype::Float, 2, t));
    }
    Some(Geometry::new(attributes, Some(IndexDatatype::create_typed_array(total / 3, &indices)), PrimitiveType::Triangles))
}
