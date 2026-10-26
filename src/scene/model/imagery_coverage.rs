//! CesiumJS `Scene/Model/ImageryCoverage`: computes imagery tile coverage
//! and texture coordinate bounds for primitives draped with imagery.

use super::CartesianRectangle;
use crate::core::{Rectangle, TilingScheme};

/// Information about a piece of imagery covered by a primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageryCoverage {
    pub x: i32,
    pub y: i32,
    pub level: i32,
    pub texture_coordinate_rectangle: CartesianRectangle,
    pub imagery_id: Option<usize>,
}

impl ImageryCoverage {
    pub fn new(
        x: i32,
        y: i32,
        level: i32,
        texture_coordinate_rectangle: CartesianRectangle,
        imagery_id: Option<usize>,
    ) -> Self {
        Self { x, y, level, texture_coordinate_rectangle, imagery_id }
    }

    /// Validates the given imagery level against minimum and maximum bounds.
    pub fn clamp_imagery_level(minimum_level: Option<i32>, maximum_level: Option<i32>, imagery_level: i32) -> i32 {
        let min_lvl = minimum_level.unwrap_or(0);
        let max_lvl = maximum_level.unwrap_or(i32::MAX);
        let clamped = imagery_level.clamp(min_lvl, max_lvl.saturating_sub(1));
        clamped.max(0)
    }

    /// Computes the integer tile coordinates range overlapped by the input rectangle.
    pub fn compute_imagery_range(
        input_rectangle: &Rectangle,
        imagery_bounds: &Rectangle,
        imagery_tiling_scheme: &dyn TilingScheme,
        imagery_level: i32,
    ) -> CartesianRectangle {
        let overlapped_rectangle = Self::compute_overlapped_rectangle(input_rectangle, imagery_bounds);
        let nw_coords = imagery_tiling_scheme
            .position_to_tile_xy(&overlapped_rectangle.northwest(), imagery_level as u32)
            .unwrap_or_default();
        let se_coords = imagery_tiling_scheme
            .position_to_tile_xy(&overlapped_rectangle.southeast(), imagery_level as u32)
            .unwrap_or_default();

        let mut min_x = nw_coords.x;
        let mut min_y = nw_coords.y;
        let mut max_x = se_coords.x;
        let mut max_y = se_coords.y;

        let very_close_x = input_rectangle.width() / 512.0;
        let very_close_y = input_rectangle.height() / 512.0;

        let nw_tile_rect = imagery_tiling_scheme.tile_xy_to_rectangle(min_x, min_y, imagery_level as u32);
        let delta_north = (nw_tile_rect.north - input_rectangle.north).abs();
        if delta_north < very_close_y && min_y < max_y {
            min_y += 1.0;
        }
        let delta_west = (nw_tile_rect.east - input_rectangle.west).abs();
        if delta_west < very_close_x && min_x < max_x {
            min_x += 1.0;
        }

        let se_tile_rect = imagery_tiling_scheme.tile_xy_to_rectangle(max_x, max_y, imagery_level as u32);
        let delta_south = (se_tile_rect.north - input_rectangle.south).abs();
        if delta_south < very_close_y && max_y > min_y {
            max_y -= 1.0;
        }
        let delta_east = (se_tile_rect.west - input_rectangle.east).abs();
        if delta_east < very_close_x && max_x > min_x {
            max_x -= 1.0;
        }

        CartesianRectangle::new(min_x, min_y, max_x, max_y)
    }

    /// Clamps `input` rectangle into `clamp` rectangle.
    pub fn clamp_rectangle(input: &Rectangle, clamp: &Rectangle) -> Rectangle {
        let (south, north) = if input.south >= clamp.north {
            (clamp.north, clamp.north)
        } else if input.north <= clamp.south {
            (clamp.south, clamp.south)
        } else {
            (input.south.max(clamp.south), input.north.min(clamp.north))
        };

        let (west, east) = if input.west >= clamp.east {
            (clamp.east, clamp.east)
        } else if input.east <= clamp.west {
            (clamp.west, clamp.west)
        } else {
            (input.west.max(clamp.west), input.east.min(clamp.east))
        };

        Rectangle::new(west, south, east, north)
    }

    /// Computes overlap between input rectangle and imagery bounds.
    pub fn compute_overlapped_rectangle(input_rectangle: &Rectangle, imagery_bounds: &Rectangle) -> Rectangle {
        if let Some(overlapped) = input_rectangle.intersection(imagery_bounds) {
            overlapped
        } else {
            Self::clamp_rectangle(input_rectangle, imagery_bounds)
        }
    }

    /// Computes the normalized coordinates of rectangle A relative to rectangle B.
    pub fn localize_to_cartesian_rectangle(rectangle_a: &Rectangle, rectangle_b: &Rectangle) -> CartesianRectangle {
        let inv_x = 1.0 / rectangle_b.width();
        let inv_y = 1.0 / rectangle_b.height();
        CartesianRectangle::new(
            (rectangle_a.west - rectangle_b.west) * inv_x,
            (rectangle_a.south - rectangle_b.south) * inv_y,
            (rectangle_a.east - rectangle_b.west) * inv_x,
            (rectangle_a.north - rectangle_b.south) * inv_y,
        )
    }

    /// Computes imagery coverages given input bounds, layer bounds, tiling scheme, and level.
    pub fn create_coverages_for_scheme(
        input_rectangle: &Rectangle,
        layer_bounds: &Rectangle,
        provider_bounds: &Rectangle,
        tiling_scheme: &dyn TilingScheme,
        input_imagery_level: i32,
        minimum_level: Option<i32>,
        maximum_level: Option<i32>,
    ) -> Vec<Self> {
        let imagery_level = Self::clamp_imagery_level(minimum_level, maximum_level, input_imagery_level);

        let imagery_bounds = match provider_bounds.intersection(layer_bounds) {
            Some(b) => b,
            None => return Vec::new(),
        };

        let imagery_range = Self::compute_imagery_range(input_rectangle, &imagery_bounds, tiling_scheme, imagery_level);

        let native_input = tiling_scheme.rectangle_to_native_rectangle(input_rectangle);

        let mut coverages = Vec::new();
        let min_x = imagery_range.min_x as i32;
        let max_x = imagery_range.max_x as i32;
        let min_y = imagery_range.min_y as i32;
        let max_y = imagery_range.max_y as i32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                let tile_rect = tiling_scheme.tile_xy_to_rectangle(x as f64, y as f64, imagery_level as u32);
                let clipped_rect = match tile_rect.intersection(&imagery_bounds) {
                    Some(r) => r,
                    None => continue,
                };

                let native_clipped = tiling_scheme.rectangle_to_native_rectangle(&clipped_rect);
                let tex_coord_rect = Self::localize_to_cartesian_rectangle(&native_clipped, &native_input);

                coverages.push(Self::new(x, y, imagery_level, tex_coord_rect, None));
            }
        }

        coverages
    }
}
