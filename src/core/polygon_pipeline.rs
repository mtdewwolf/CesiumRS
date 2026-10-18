//! CesiumJS `PolygonPipeline`: 2D area and winding order, earcut
//! triangulation, subdivision of triangles to a granularity on the
//! ellipsoid, and scaling positions to a geodetic height.
//!
//! The subdivision functions return `None` where Cesium would read past its
//! arrays or throw: an index out of range, missing texture coordinates, an
//! index count that is not a multiple of three, or (for rhumb lines) a
//! position at the ellipsoid's center.

use std::collections::HashMap;

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::component_datatype::ComponentDatatype;
use super::earcut::earcut;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_rhumb_line::EllipsoidRhumbLine;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::math::{chord_length, js_max, RADIANS_PER_DEGREE};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;
use super::winding_order::WindingOrder;

/// The signed area of a 2D polygon; positive when counter-clockwise.
pub fn compute_area_2d(positions: &[Cartesian2]) -> f64 {
    let length = positions.len();
    let mut area = 0.0;
    let mut i0 = length.wrapping_sub(1);
    for i1 in 0..length {
        let (v0, v1) = (positions[i0], positions[i1]);
        area += v0.x * v1.y - v1.x * v0.y;
        i0 = i1;
    }
    area * 0.5
}

pub fn compute_winding_order_2d(positions: &[Cartesian2]) -> WindingOrder {
    if compute_area_2d(positions) > 0.0 {
        WindingOrder::CounterClockwise
    } else {
        WindingOrder::Clockwise
    }
}

/// Triangulates a polygon whose holes start at the given position indices.
pub fn triangulate(positions: &[Cartesian2], holes: &[usize]) -> Vec<usize> {
    earcut(&Cartesian2::pack_array(positions), holes, 2)
}

/// Subdivides triangles until every edge's chord on a sphere of the
/// ellipsoid's maximum radius is within `granularity` (default one degree).
pub fn compute_subdivision(
    ellipsoid: &Ellipsoid,
    positions: &[Cartesian3],
    indices: &[usize],
    texcoords: Option<&[Cartesian2]>,
    granularity: Option<f64>,
) -> Option<Geometry> {
    let radius = ellipsoid.maximum_radius;
    let min_distance = chord_length(granularity.unwrap_or(RADIANS_PER_DEGREE), radius);
    let min_distance_sqrd = min_distance * min_distance;
    subdivide(positions, indices, texcoords, |v: [Cartesian3; 3]| {
        let s = v.map(|v| v.normalize().multiply_by_scalar(radius));
        let g = [
            s[0].subtract(s[1]).magnitude_squared(),
            s[1].subtract(s[2]).magnitude_squared(),
            s[2].subtract(s[0]).magnitude_squared(),
        ];
        let split = js_max(js_max(g[0], g[1]), g[2]) > min_distance_sqrd;
        Some((g, split, move |e: usize| {
            let (a, b) = (v[e], v[(e + 1) % 3]);
            a.add(b).multiply_by_scalar(0.5)
        }))
    })
}

/// Like [`compute_subdivision`], but measures and splits edges along rhumb
/// lines, keeping the average height of each edge's endpoints.
pub fn compute_rhumb_line_subdivision(
    ellipsoid: &Ellipsoid,
    positions: &[Cartesian3],
    indices: &[usize],
    texcoords: Option<&[Cartesian2]>,
    granularity: Option<f64>,
) -> Option<Geometry> {
    let min_distance = chord_length(granularity.unwrap_or(RADIANS_PER_DEGREE), ellipsoid.maximum_radius);
    subdivide(positions, indices, texcoords, |v: [Cartesian3; 3]| {
        let c: [Cartographic; 3] = [
            ellipsoid.cartesian_to_cartographic(v[0])?,
            ellipsoid.cartesian_to_cartographic(v[1])?,
            ellipsoid.cartesian_to_cartographic(v[2])?,
        ];
        let rhumbs = [0, 1, 2].map(|e| EllipsoidRhumbLine::new(&c[e], &c[(e + 1) % 3], ellipsoid));
        let g = [rhumbs[0].surface_distance, rhumbs[1].surface_distance, rhumbs[2].surface_distance];
        let split = js_max(js_max(g[0], g[1]), g[2]) > min_distance;
        Some((g, split, move |e: usize| {
            let mid = rhumbs[e].interpolate_using_fraction(0.5);
            let height = (c[e].height + c[(e + 1) % 3].height) * 0.5;
            Cartesian3::from_radians(mid.longitude, mid.latitude, height, ellipsoid)
        }))
    })
}

/// The loop both subdivisions share. `measure` returns each edge's length
/// (edges 0→1, 1→2, 2→0), whether to split the longest, and the midpoint of
/// an edge.
fn subdivide<M, F>(
    positions: &[Cartesian3],
    indices: &[usize],
    texcoords: Option<&[Cartesian2]>,
    measure: M,
) -> Option<Geometry>
where
    M: Fn([Cartesian3; 3]) -> Option<([f64; 3], bool, F)>,
    F: Fn(usize) -> Cartesian3,
{
    if !indices.len().is_multiple_of(3) {
        return None;
    }
    if texcoords.is_some_and(|t| t.len() < positions.len()) {
        return None;
    }
    let mut triangles = indices.to_vec();
    let mut subdivided_positions = Cartesian3::pack_array(positions);
    let mut subdivided_texcoords = texcoords.map(Cartesian2::pack_array).unwrap_or_default();
    let mut subdivided_indices = Vec::new();
    // Shared edges are split once.
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();

    while let (Some(i2), Some(i1), Some(i0)) = (triangles.pop(), triangles.pop(), triangles.pop()) {
        let ids = [i0, i1, i2];
        if ids.iter().any(|&i| i * 3 >= subdivided_positions.len()) {
            return None;
        }
        let v = ids.map(|i| Cartesian3::from_array(&subdivided_positions, i * 3));
        let (g, split, midpoint) = measure(v)?;
        if !split {
            subdivided_indices.extend(ids);
            continue;
        }
        let max = js_max(js_max(g[0], g[1]), g[2]);
        // The first edge (in 0→1, 1→2, 2→0 order) with the maximum length.
        let Some(e) = (0..3).find(|&e| g[e] == max) else { continue };
        let (a, b, opposite) = (ids[e], ids[(e + 1) % 3], ids[(e + 2) % 3]);
        let key = (a.min(b), a.max(b));
        let i = match edges.get(&key) {
            Some(&i) => i,
            None => {
                let mid = midpoint(e);
                subdivided_positions.extend([mid.x, mid.y, mid.z]);
                let i = subdivided_positions.len() / 3 - 1;
                edges.insert(key, i);
                if texcoords.is_some() {
                    let ta = Cartesian2::from_array(&subdivided_texcoords, a * 2);
                    let tb = Cartesian2::from_array(&subdivided_texcoords, b * 2);
                    let t = ta.add(tb).multiply_by_scalar(0.5);
                    subdivided_texcoords.extend([t.x, t.y]);
                }
                i
            }
        };
        triangles.extend([a, i, opposite, i, b, opposite]);
    }

    let mut attributes = GeometryAttributes {
        position: Some(GeometryAttribute::doubles(3, subdivided_positions)),
        ..Default::default()
    };
    if texcoords.is_some() {
        // Cesium stores a plain array here, so the values keep full precision.
        attributes.st = Some(GeometryAttribute::new(
            ComponentDatatype::Float,
            2,
            TypedArray::Float64(subdivided_texcoords),
        ));
    }
    let indices = TypedArray::Uint32(subdivided_indices.into_iter().map(|i| i as u32).collect());
    Some(Geometry::new(attributes, Some(indices), PrimitiveType::Triangles))
}

/// Scales flat `[x, y, z, ...]` positions in place to `height` above the
/// ellipsoid, first projecting them onto the surface when `scale_to_surface`
/// (default true). Returns `None`, as Cesium throws, if a position cannot be
/// projected (it is at the center); earlier positions are already scaled.
pub fn scale_to_geodetic_height(
    positions: &mut [f64],
    height: Option<f64>,
    ellipsoid: &Ellipsoid,
    scale_to_surface: Option<bool>,
) -> Option<()> {
    let height = height.unwrap_or(0.0);
    let scale_to_surface = scale_to_surface.unwrap_or(true);
    let mut i = 0;
    while i < positions.len() {
        let mut p = Cartesian3::new(
            positions[i],
            positions.get(i + 1).copied().unwrap_or(f64::NAN),
            positions.get(i + 2).copied().unwrap_or(f64::NAN),
        );
        if scale_to_surface {
            p = ellipsoid.scale_to_geodetic_surface(p)?;
        }
        if height != 0.0 {
            let n = ellipsoid.geodetic_surface_normal(p)?.multiply_by_scalar(height);
            p = p.add(n);
        }
        for (k, value) in [p.x, p.y, p.z].into_iter().enumerate() {
            if let Some(slot) = positions.get_mut(i + k) {
                *slot = value;
            }
        }
        i += 3;
    }
    Some(())
}
