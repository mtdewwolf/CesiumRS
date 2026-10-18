//! CesiumJS `WallGeometryLibrary`: the top and bottom edges of a wall,
//! densified along geodesics.

use super::array_remove_duplicates::array_remove_duplicates;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::math::{chord_length, equals_epsilon, EPSILON10};
use super::polyline_pipeline::{generate_arc, number_of_points, ArcOptions, PolylineHeight};

/// A wall's edges, flattened, and the number of interior corners.
#[derive(Clone, Debug, PartialEq)]
pub struct WallPositions {
    pub bottom_positions: Vec<f64>,
    pub top_positions: Vec<f64>,
    pub num_corners: usize,
}

fn lat_lon_equals(c0: &Cartographic, c1: &Cartographic) -> bool {
    equals_epsilon(c0.latitude, c1.latitude, EPSILON10, EPSILON10) && equals_epsilon(c0.longitude, c1.longitude, EPSILON10, EPSILON10)
}

struct Cleaned {
    positions: Vec<Cartesian3>,
    top_heights: Vec<f64>,
    bottom_heights: Vec<f64>,
}

/// Drops positions at the same latitude and longitude, keeping the higher
/// top. Like Cesium, the heights are read by the index after duplicate
/// Cartesian positions are removed. `None` for a wall with no area.
fn remove_duplicates(
    ellipsoid: &Ellipsoid,
    positions: &[Cartesian3],
    top_heights: Option<&[f64]>,
    bottom_heights: Option<&[f64]>,
) -> Option<Cleaned> {
    let positions = array_remove_duplicates(positions, |a, b, e| a.equals_epsilon(*b, e), false, None);
    let length = positions.len();
    if length < 2 {
        return None;
    }
    let height_at = |h: Option<&[f64]>, i: usize| h.map(|h| h.get(i).copied().unwrap_or(f64::NAN));
    let mut cleaned = Cleaned { positions: vec![positions[0]], top_heights: Vec::new(), bottom_heights: Vec::new() };
    let mut c0 = ellipsoid.cartesian_to_cartographic(positions[0])?;
    if let Some(h) = height_at(top_heights, 0) {
        c0.height = h;
    }
    cleaned.top_heights.push(c0.height);
    cleaned.bottom_heights.push(height_at(bottom_heights, 0).unwrap_or(0.0));
    let mut has_all_same_heights = cleaned.top_heights[0] == cleaned.bottom_heights[0];
    for (i, &v1) in positions.iter().enumerate().skip(1) {
        let mut c1 = ellipsoid.cartesian_to_cartographic(v1)?;
        if let Some(h) = height_at(top_heights, i) {
            c1.height = h;
        }
        has_all_same_heights = has_all_same_heights && c1.height == 0.0;
        if !lat_lon_equals(&c0, &c1) {
            cleaned.positions.push(v1);
            cleaned.top_heights.push(c1.height);
            let bottom = height_at(bottom_heights, i).unwrap_or(0.0);
            cleaned.bottom_heights.push(bottom);
            has_all_same_heights = has_all_same_heights && c1.height == bottom;
            c0 = c1;
        } else if c0.height < c1.height {
            *cleaned.top_heights.last_mut()? = c1.height;
        }
    }
    if has_all_same_heights || cleaned.positions.len() < 2 {
        return None;
    }
    Some(cleaned)
}

/// The wall's top and bottom edges. With `duplicate_corners` each segment
/// is densified on its own, so corner positions appear twice.
pub fn compute_positions(
    ellipsoid: &Ellipsoid,
    wall_positions: &[Cartesian3],
    maximum_heights: Option<&[f64]>,
    minimum_heights: Option<&[f64]>,
    granularity: f64,
    duplicate_corners: bool,
) -> Option<WallPositions> {
    let o = remove_duplicates(ellipsoid, wall_positions, maximum_heights, minimum_heights)?;
    let length = o.positions.len();
    let num_corners = length - 2;
    let min_distance = chord_length(granularity, ellipsoid.maximum_radius);
    let arc = |positions: Vec<Cartesian3>, heights: Vec<f64>| {
        let mut options = ArcOptions::new(positions);
        options.ellipsoid = *ellipsoid;
        options.min_distance = Some(min_distance);
        options.height = PolylineHeight::PerPosition(heights);
        generate_arc(&options)
    };
    let (top_positions, bottom_positions) = if duplicate_corners {
        let count: usize =
            (0..length - 1).map(|i| number_of_points(o.positions[i], o.positions[i + 1], min_distance) as usize + 1).sum();
        let mut top = vec![0.0; count * 3];
        let mut bottom = vec![0.0; count * 3];
        let mut offset = 0;
        for i in 0..length - 1 {
            let ends = vec![o.positions[i], o.positions[i + 1]];
            let pos = arc(ends.clone(), vec![o.top_heights[i], o.top_heights[i + 1]])?;
            let low = arc(ends, vec![o.bottom_heights[i], o.bottom_heights[i + 1]])?;
            // Float64Array.set throws past the end.
            top.get_mut(offset..offset + pos.len())?.copy_from_slice(&pos);
            bottom.get_mut(offset..offset + low.len())?.copy_from_slice(&low);
            offset += pos.len();
        }
        (top, bottom)
    } else {
        (arc(o.positions.clone(), o.top_heights)?, arc(o.positions, o.bottom_heights)?)
    };
    Some(WallPositions { bottom_positions, top_positions, num_corners })
}
