//! CesiumJS `PolylinePipeline`: densifying polylines along geodesics or
//! rhumb lines, and splitting them where they cross the antimeridian.

use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_geodesic::EllipsoidGeodesic;
use super::ellipsoid_rhumb_line::EllipsoidRhumbLine;
use super::intersection_tests::line_segment_plane;
use super::math::{chord_length, js_max, RADIANS_PER_DEGREE};
use super::matrix4::Matrix4;
use super::plane::Plane;

/// Heights for generated arcs: one for every position, or one per position.
#[derive(Clone, Debug, PartialEq)]
pub enum PolylineHeight {
    Constant(f64),
    PerPosition(Vec<f64>),
}

impl Default for PolylineHeight {
    fn default() -> Self {
        Self::Constant(0.0)
    }
}

impl PolylineHeight {
    fn at(&self, i: usize) -> f64 {
        match self {
            Self::Constant(h) => *h,
            Self::PerPosition(h) => h.get(i).copied().unwrap_or(f64::NAN),
        }
    }
}

/// Options for [`generate_arc`] and [`generate_rhumb_arc`].
#[derive(Clone, Debug, PartialEq)]
pub struct ArcOptions {
    pub positions: Vec<Cartesian3>,
    pub ellipsoid: Ellipsoid,
    pub height: PolylineHeight,
    /// Defaults to one degree.
    pub granularity: Option<f64>,
    /// For geodesic arcs; defaults to the chord of `granularity`.
    pub min_distance: Option<f64>,
}

impl ArcOptions {
    pub fn new(positions: Vec<Cartesian3>) -> Self {
        Self { positions, ellipsoid: Ellipsoid::WGS84, height: PolylineHeight::default(), granularity: None, min_distance: None }
    }
}

/// Segments of length `min_distance` between two points.
pub fn number_of_points(p0: Cartesian3, p1: Cartesian3, min_distance: f64) -> f64 {
    (p0.distance(p1) / min_distance).ceil()
}

/// Segments along a rhumb line at `granularity`, at least one.
pub fn number_of_points_rhumb_line(p0: &Cartographic, p1: &Cartographic, granularity: f64) -> f64 {
    let radians_distance_squared = (p0.longitude - p1.longitude).powi(2) + (p0.latitude - p1.latitude).powi(2);
    js_max(1.0, (radians_distance_squared / (granularity * granularity)).sqrt().ceil())
}

/// Each position's height above the ellipsoid.
pub fn extract_heights(positions: &[Cartesian3], ellipsoid: &Ellipsoid) -> Vec<f64> {
    positions.iter().map(|p| ellipsoid.cartesian_to_cartographic(*p).map_or(f64::NAN, |c| c.height)).collect()
}

fn subdivide_heights(num_points: usize, h0: f64, h1: f64) -> Vec<f64> {
    if h0 == h1 {
        return vec![h0; num_points];
    }
    let per_vertex = (h1 - h0) / num_points as f64;
    (0..num_points).map(|i| h0 + i as f64 * per_vertex).collect()
}

/// Writes into a plain array, growing it as JavaScript does.
fn pack_at(array: &mut Vec<f64>, index: usize, p: Cartesian3) {
    if array.len() < index + 3 {
        array.resize(index + 3, f64::NAN);
    }
    array[index..index + 3].copy_from_slice(&[p.x, p.y, p.z]);
}

/// Interpolates `num_points` points from `start` with heights from `h0` to
/// `h1` and packs them at `offset`; returns the next offset. A segment with
/// no points still writes its start, as Cesium does.
#[allow(clippy::too_many_arguments)]
fn write_arc(
    start: Cartographic,
    num_points: usize,
    surface_distance: f64,
    interpolate: impl Fn(f64) -> Cartographic,
    ellipsoid: &Ellipsoid,
    h0: f64,
    h1: f64,
    array: &mut Vec<f64>,
    offset: usize,
) -> usize {
    let heights = subdivide_heights(num_points, h0, h1);
    let step = surface_distance / num_points as f64;
    let mut index = offset;
    let start = Cartographic::new(start.longitude, start.latitude, h0);
    pack_at(array, index, ellipsoid.cartographic_to_cartesian(&start));
    index += 3;
    for (i, &height) in heights.iter().enumerate().skip(1) {
        let mut c = interpolate(i as f64 * step);
        c.height = height;
        pack_at(array, index, ellipsoid.cartographic_to_cartesian(&c));
        index += 3;
    }
    index
}

/// The single position, raised to its height.
fn single_point(options: &ArcOptions) -> Option<Vec<f64>> {
    let ellipsoid = &options.ellipsoid;
    let mut p = ellipsoid.scale_to_geodetic_surface(options.positions[0])?;
    let height = options.height.at(0);
    if height != 0.0 {
        p = p.add(ellipsoid.geodetic_surface_normal(p)?.multiply_by_scalar(height));
    }
    Some(vec![p.x, p.y, p.z])
}

fn finish(options: &ArcOptions, mut array: Vec<f64>, array_length: usize) -> Option<Vec<f64>> {
    let ellipsoid = &options.ellipsoid;
    let length = options.positions.len();
    let mut carto = ellipsoid.cartesian_to_cartographic(options.positions[length - 1])?;
    carto.height = options.height.at(length - 1);
    let p = ellipsoid.cartographic_to_cartesian(&carto);
    pack_at(&mut array, array_length - 3, p);
    Some(array)
}

/// Densified positions along geodesics, flattened. `None` where Cesium
/// throws: a position at the ellipsoid's center, or near-antipodal points.
pub fn generate_arc(options: &ArcOptions) -> Option<Vec<f64>> {
    let positions = &options.positions;
    let ellipsoid = &options.ellipsoid;
    let length = positions.len();
    if length < 1 {
        return Some(Vec::new());
    }
    if length == 1 {
        return single_point(options);
    }
    let min_distance = options
        .min_distance
        .unwrap_or_else(|| chord_length(options.granularity.unwrap_or(RADIANS_PER_DEGREE), ellipsoid.maximum_radius));
    let num_points: f64 = (0..length - 1).map(|i| number_of_points(positions[i], positions[i + 1], min_distance)).sum();
    let array_length = (num_points as usize + 1) * 3;
    let mut array = vec![f64::NAN; array_length];
    let mut offset = 0;
    for i in 0..length - 1 {
        let (p0, p1) = (positions[i], positions[i + 1]);
        let first = ellipsoid.scale_to_geodetic_surface(p0)?;
        let last = ellipsoid.scale_to_geodetic_surface(p1)?;
        let n = number_of_points(p0, p1, min_distance) as usize;
        let start = ellipsoid.cartesian_to_cartographic(first)?;
        let end = ellipsoid.cartesian_to_cartographic(last)?;
        // Cesium's module-level geodesic keeps the default (WGS84) ellipsoid.
        let geodesic = EllipsoidGeodesic::new(&start, &end, &Ellipsoid::WGS84)?;
        offset = write_arc(
            start,
            n,
            geodesic.surface_distance,
            |d| geodesic.interpolate_using_surface_distance(d),
            ellipsoid,
            options.height.at(i),
            options.height.at(i + 1),
            &mut array,
            offset,
        );
    }
    finish(options, array, array_length)
}

/// Densified positions along rhumb lines, flattened.
pub fn generate_rhumb_arc(options: &ArcOptions) -> Option<Vec<f64>> {
    let positions = &options.positions;
    let ellipsoid = &options.ellipsoid;
    let length = positions.len();
    if length < 1 {
        return Some(Vec::new());
    }
    if length == 1 {
        return single_point(options);
    }
    let granularity = options.granularity.unwrap_or(RADIANS_PER_DEGREE);
    let cartographics: Option<Vec<Cartographic>> = positions.iter().map(|p| ellipsoid.cartesian_to_cartographic(*p)).collect();
    let cartographics = cartographics?;
    let num_points: f64 =
        (0..length - 1).map(|i| number_of_points_rhumb_line(&cartographics[i], &cartographics[i + 1], granularity)).sum();
    let array_length = (num_points as usize + 1) * 3;
    let mut array = vec![f64::NAN; array_length];
    let mut offset = 0;
    for i in 0..length - 1 {
        let mut start = cartographics[i];
        let mut end = cartographics[i + 1];
        let n = number_of_points_rhumb_line(&start, &end, granularity) as usize;
        start.height = 0.0;
        end.height = 0.0;
        let rhumb = EllipsoidRhumbLine::new(&start, &end, ellipsoid);
        offset = write_arc(
            start,
            n,
            rhumb.surface_distance,
            |d| rhumb.interpolate_using_surface_distance(d),
            ellipsoid,
            options.height.at(i),
            options.height.at(i + 1),
            &mut array,
            offset,
        );
    }
    finish(options, array, array_length)
}

fn unflatten(values: Vec<f64>) -> Vec<Cartesian3> {
    (0..values.len() / 3).map(|i| Cartesian3::from_array(&values, i * 3)).collect()
}

/// [`generate_arc`] as `Cartesian3`s.
pub fn generate_cartesian_arc(options: &ArcOptions) -> Option<Vec<Cartesian3>> {
    generate_arc(options).map(unflatten)
}

/// [`generate_rhumb_arc`] as `Cartesian3`s.
pub fn generate_cartesian_rhumb_arc(options: &ArcOptions) -> Option<Vec<Cartesian3>> {
    generate_rhumb_arc(options).map(unflatten)
}

/// The polyline split where it crosses the antimeridian (in the model
/// frame), with points nudged to each side, and the length of each piece.
pub fn wrap_longitude(positions: &[Cartesian3], model_matrix: Option<&Matrix4>) -> (Vec<Cartesian3>, Vec<usize>) {
    let mut cartesians = Vec::new();
    let mut segments = Vec::new();
    if positions.is_empty() {
        return (cartesians, segments);
    }
    let inverse = model_matrix.unwrap_or(&Matrix4::IDENTITY).inverse_transformation();
    let origin = inverse.multiply_by_point(Cartesian3::ZERO);
    let xz_normal = inverse.multiply_by_point_as_vector(Cartesian3::UNIT_Y).normalize();
    let xz_plane = Plane::from_point_normal(origin, xz_normal);
    let yz_normal = inverse.multiply_by_point_as_vector(Cartesian3::UNIT_X).normalize();
    let yz_plane = Plane::from_point_normal(origin, yz_normal);
    let mut count = 1;
    cartesians.push(positions[0]);
    let mut prev = positions[0];
    for &cur in &positions[1..] {
        if yz_plane.get_point_distance(prev) < 0.0 || yz_plane.get_point_distance(cur) < 0.0 {
            if let Some(intersection) = line_segment_plane(prev, cur, &xz_plane) {
                let mut offset = xz_normal.multiply_by_scalar(5.0e-9);
                if xz_plane.get_point_distance(prev) < 0.0 {
                    offset = offset.negate();
                }
                cartesians.push(intersection.add(offset));
                segments.push(count + 1);
                cartesians.push(intersection.add(offset.negate()));
                count = 1;
            }
        }
        cartesians.push(cur);
        count += 1;
        prev = cur;
    }
    segments.push(count);
    (cartesians, segments)
}
