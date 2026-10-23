//! CesiumJS `TileBoundingS2Cell`: a tile bounding volume specified as an S2
//! cell token with a minimum and maximum height (`3DTILES_bounding_volume_S2`).
//!
//! The volume is a convex polyhedron of six planes: top, bottom and the four
//! sides, with outward normals.

use crate::core::{BoundingSphere, Cartesian3, Color, CoplanarPolygonOutlineGeometry, Ellipsoid, Intersect};
use crate::core::{Matrix3, Matrix4, Plane, S2Cell};

use super::tile_bounding_volume::{DebugVolume, TileBoundingVolume, TileBoundingVolumeFrame};

#[derive(Clone, Debug, PartialEq)]
pub struct TileBoundingS2Cell {
    pub s2_cell: S2Cell,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub ellipsoid: Ellipsoid,
    /// The cell center at the middle height.
    pub center: Cartesian3,
    bounding_planes: [Plane; 6],
    /// Top vertices 0–3 then bottom vertices 4–7, counter-clockwise.
    vertices: [Cartesian3; 8],
    edge_normals: [[Cartesian3; 4]; 6],
    plane_vertices: [[Cartesian3; 4]; 6],
    bounding_sphere: BoundingSphere,
}

/// `new TileBoundingS2Cell(options)`.
#[derive(Clone, Debug)]
pub struct TileBoundingS2CellOptions {
    pub token: String,
    /// Defaults to 0.
    pub minimum_height: Option<f64>,
    /// Defaults to 0.
    pub maximum_height: Option<f64>,
    /// Defaults to WGS84.
    pub ellipsoid: Option<Ellipsoid>,
}

/// Cesium's NaN when the ellipsoid has no answer at its center.
const NAN3: Cartesian3 = Cartesian3 { x: f64::NAN, y: f64::NAN, z: f64::NAN };

impl TileBoundingS2Cell {
    /// `None` for an invalid token, where `S2Cell.fromToken` throws.
    pub fn new(options: TileBoundingS2CellOptions) -> Option<Self> {
        let s2_cell = S2Cell::from_token(&options.token)?;
        let minimum_height = options.minimum_height.unwrap_or(0.0);
        let maximum_height = options.maximum_height.unwrap_or(0.0);
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);

        let bounding_planes = compute_bounding_planes(&s2_cell, minimum_height, maximum_height, &ellipsoid)?;
        let vertices = compute_vertices(&bounding_planes);

        let mut edge_normals = [[Cartesian3::ZERO; 4]; 6];
        // The edge normals point away from each face, except for the top
        // plane, whose normals are negated.
        edge_normals[0] = compute_edge_normals(&bounding_planes[0], &first_four(&vertices, 0)).map(|n| n.negate());
        edge_normals[1] = compute_edge_normals(&bounding_planes[1], &first_four(&vertices, 4));
        let mut plane_vertices = [[Cartesian3::ZERO; 4]; 6];
        plane_vertices[0] = first_four(&vertices, 0);
        plane_vertices[1] = first_four(&vertices, 4);
        for i in 0..4 {
            // Each side plane's vertices in counter-clockwise order.
            let side = [vertices[i % 4], vertices[(i + 1) % 4], vertices[4 + (i + 1) % 4], vertices[4 + i]];
            edge_normals[2 + i] = compute_edge_normals(&bounding_planes[2 + i], &side);
            plane_vertices[2 + i] = side;
        }

        let center = s2_cell.get_center(&ellipsoid);
        let center = match ellipsoid.cartesian_to_cartographic(center) {
            Some(mut cartographic) => {
                cartographic.height = (maximum_height + minimum_height) / 2.0;
                ellipsoid.cartographic_to_cartesian(&cartographic)
            }
            None => NAN3,
        };
        let bounding_sphere = BoundingSphere::from_points(&vertices);
        Some(Self {
            s2_cell,
            minimum_height,
            maximum_height,
            ellipsoid,
            center,
            bounding_planes,
            vertices,
            edge_normals,
            plane_vertices,
            bounding_sphere,
        })
    }

    /// The underlying volume, this cell.
    pub fn bounding_volume(&self) -> &Self {
        self
    }

    /// Top, bottom, then the four side planes.
    pub fn bounding_planes(&self) -> &[Plane; 6] {
        &self.bounding_planes
    }

    pub fn vertices(&self) -> &[Cartesian3; 8] {
        &self.vertices
    }
}

fn first_four(vertices: &[Cartesian3; 8], start: usize) -> [Cartesian3; 4] {
    [vertices[start], vertices[start + 1], vertices[start + 2], vertices[start + 3]]
}

fn compute_bounding_planes(
    s2_cell: &S2Cell,
    minimum_height: f64,
    maximum_height: f64,
    ellipsoid: &Ellipsoid,
) -> Option<[Plane; 6]> {
    let center_point = s2_cell.get_center(ellipsoid);

    // The top plane: the surface normal at the cell center, through the
    // center at the maximum height.
    let center_surface_normal = ellipsoid.geodetic_surface_normal(center_point).unwrap_or(NAN3);
    let top = match ellipsoid.cartesian_to_cartographic(center_point) {
        Some(mut top_cartographic) => {
            top_cartographic.height = maximum_height;
            ellipsoid.cartographic_to_cartesian(&top_cartographic)
        }
        None => NAN3,
    };
    let top_plane = Plane::from_point_normal(top, center_surface_normal);

    // The bottom plane: the top plane moved past the lowest vertex at the
    // minimum height, facing the other way.
    let mut max_distance: f64 = 0.0;
    let mut vertices = [Cartesian3::ZERO; 4];
    for (i, vertex) in vertices.iter_mut().enumerate() {
        *vertex = s2_cell.get_vertex(i as u8, ellipsoid)?;
        let lowered = match ellipsoid.cartesian_to_cartographic(*vertex) {
            Some(mut cartographic) => {
                cartographic.height = minimum_height;
                ellipsoid.cartographic_to_cartesian(&cartographic)
            }
            None => NAN3,
        };
        let distance = top_plane.get_point_distance(lowered);
        if distance < max_distance {
            max_distance = distance;
        }
    }
    let bottom_plane = Plane::new(top_plane.normal.negate(), -top_plane.distance + max_distance);

    // The side planes, through each pair of adjacent vertices and the
    // surface normal at the first.
    let side = |i: usize| {
        let vertex = vertices[i];
        let adjacent = vertices[(i + 1) % 4];
        let geodetic_normal = ellipsoid.geodetic_surface_normal(vertex).unwrap_or(NAN3);
        let side_normal = adjacent.subtract(vertex).cross(geodetic_normal).normalize();
        Plane::from_point_normal(vertex, side_normal)
    };
    Some([top_plane, bottom_plane, side(0), side(1), side(2), side(3)])
}

/// The point where three planes meet, by Cramer's rule.
fn compute_intersection(p0: &Plane, p1: &Plane, p2: &Plane) -> Cartesian3 {
    let (n0, n1, n2) = (p0.normal, p1.normal, p2.normal);
    let x0 = n0.multiply_by_scalar(-p0.distance);
    let x1 = n1.multiply_by_scalar(-p1.distance);
    let x2 = n2.multiply_by_scalar(-p2.distance);
    let f0 = n1.cross(n2).multiply_by_scalar(x0.dot(n0));
    let f1 = n2.cross(n0).multiply_by_scalar(x1.dot(n1));
    let f2 = n0.cross(n1).multiply_by_scalar(x2.dot(n2));
    let matrix = Matrix3([n0.x, n1.x, n2.x, n0.y, n1.y, n2.y, n0.z, n1.z, n2.z]);
    let determinant = matrix.determinant();
    let s = f0.add(f1).add(f2);
    Cartesian3::new(s.x / determinant, s.y / determinant, s.z / determinant)
}

fn compute_vertices(planes: &[Plane; 6]) -> [Cartesian3; 8] {
    let mut vertices = [Cartesian3::ZERO; 8];
    for i in 0..4 {
        let (before, after) = (&planes[2 + (i + 3) % 4], &planes[2 + i % 4]);
        vertices[i] = compute_intersection(&planes[0], before, after);
        vertices[i + 4] = compute_intersection(&planes[1], before, after);
    }
    vertices
}

fn compute_edge_normals(plane: &Plane, vertices: &[Cartesian3; 4]) -> [Cartesian3; 4] {
    std::array::from_fn(|i| plane.normal.cross(vertices[(i + 1) % 4].subtract(vertices[i])).normalize())
}

fn closest_point_line_segment(p: Cartesian3, l0: Cartesian3, l1: Cartesian3) -> Cartesian3 {
    let d = l1.subtract(l0);
    let t = d.dot(p.subtract(l0));
    if t <= 0.0 {
        return l0;
    }
    let d_mag = d.dot(d);
    if t >= d_mag {
        return l1;
    }
    let t = t / d_mag;
    Cartesian3::new((1.0 - t) * l0.x + t * l1.x, (1.0 - t) * l0.y + t * l1.y, (1.0 - t) * l0.z + t * l1.z)
}

fn closest_point_polygon(p: Cartesian3, vertices: &[Cartesian3; 4], edge_normals: &[Cartesian3; 4]) -> Cartesian3 {
    let mut min_distance = f64::MAX;
    let mut closest = None;
    for i in 0..vertices.len() {
        let edge_plane = Plane::from_point_normal(vertices[i], edge_normals[i]);
        // Skip edges whose plane faces away from the point.
        if edge_plane.get_point_distance(p) < 0.0 {
            continue;
        }
        let on_edge = closest_point_line_segment(p, vertices[i], vertices[(i + 1) % 4]);
        let distance = p.distance(on_edge);
        if distance < min_distance {
            min_distance = distance;
            closest = Some(on_edge);
        }
    }
    closest.unwrap_or(p)
}

impl TileBoundingVolume for TileBoundingS2Cell {
    fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    /// The distance from the camera to the nearest face, edge or vertex of
    /// the planes it is outside of.
    fn distance_to_camera(&self, frame: &TileBoundingVolumeFrame<'_>) -> f64 {
        let point = frame.camera_position_wc;
        let planes = &self.bounding_planes;
        let mut selected = Vec::with_capacity(5);
        let mut vertices = Vec::with_capacity(5);
        let mut edge_normals = None;

        if planes[0].get_point_distance(point) > 0.0 {
            selected.push(0);
            vertices.push(&self.plane_vertices[0]);
            edge_normals = Some(&self.edge_normals[0]);
        } else if planes[1].get_point_distance(point) > 0.0 {
            selected.push(1);
            vertices.push(&self.plane_vertices[1]);
            edge_normals = Some(&self.edge_normals[1]);
        }
        for (side, plane) in planes.iter().enumerate().skip(2) {
            if plane.get_point_distance(point) > 0.0 {
                selected.push(side);
                vertices.push(&self.plane_vertices[side]);
                edge_normals = Some(&self.edge_normals[side]);
            }
        }

        // Inside every plane.
        if selected.is_empty() {
            return 0.0;
        }

        if selected.len() == 1 {
            // Case I: one face.
            let plane = &planes[selected[0]];
            let face_point = closest_point_polygon(
                plane.project_point_onto_plane(point),
                vertices[0],
                edge_normals.expect("set with the plane"),
            );
            return face_point.distance(point);
        }
        if selected.len() == 2 {
            // Case II: an edge. The dihedral angle between the top plane and
            // a side plane is acute on the ellipsoid, so the edge itself is
            // the nearest feature.
            if selected[0] == 0 {
                let edge0 = self.vertices[4 * selected[0] + (selected[1] - 2)];
                let edge1 = self.vertices[4 * selected[0] + (selected[1] - 2 + 1) % 4];
                return closest_point_line_segment(point, edge0, edge1).distance(point);
            }
            let mut minimum_distance = f64::MAX;
            for i in 0..2 {
                let plane = &planes[selected[i]];
                let face_point = closest_point_polygon(
                    plane.project_point_onto_plane(point),
                    vertices[i],
                    &self.edge_normals[selected[i]],
                );
                let distance = face_point.distance_squared(point);
                if distance < minimum_distance {
                    minimum_distance = distance;
                }
            }
            return minimum_distance.sqrt();
        }
        if selected.len() > 3 {
            // Case IV: below the bottom face.
            let face_point = closest_point_polygon(
                planes[1].project_point_onto_plane(point),
                &self.plane_vertices[1],
                &self.edge_normals[1],
            );
            return face_point.distance(point);
        }

        // Case III: a vertex. `skip` handles the non-consecutive sides 2 and 5.
        let skip = if selected[1] == 2 && selected[2] == 5 { 0 } else { 1 };
        if selected[0] == 0 {
            return point.distance(self.vertices[(selected[1] - 2 + skip) % 4]);
        }
        point.distance(self.vertices[4 + (selected[1] - 2 + skip) % 4])
    }

    /// Inside when every vertex is on or in front of the plane.
    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        let mut plus_count = 0;
        let mut neg_count = 0;
        for vertex in &self.vertices {
            if plane.normal.dot(*vertex) + plane.distance < 0.0 {
                neg_count += 1;
            } else {
                plus_count += 1;
            }
        }
        if plus_count == self.vertices.len() {
            Intersect::Inside
        } else if neg_count == self.vertices.len() {
            Intersect::Outside
        } else {
            Intersect::Intersecting
        }
    }

    /// The four sides, then the bottom and top faces, as polygon outlines.
    fn create_debug_volume(&self, color: &Color) -> DebugVolume {
        let outline = |positions: &[Cartesian3; 4]| {
            CoplanarPolygonOutlineGeometry::from_positions(positions.to_vec()).create_geometry()
        };
        let pv = &self.plane_vertices;
        let geometries =
            vec![outline(&pv[2]), outline(&pv[3]), outline(&pv[4]), outline(&pv[5]), outline(&pv[1]), outline(&pv[0])];
        DebugVolume::outlines(geometries, Matrix4::IDENTITY, color)
    }
}
