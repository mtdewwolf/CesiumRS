//! CesiumJS `BoundingRectangle`: an axis-aligned 2D box.

use super::cartesian2::Cartesian2;
use super::geographic_projection::GeographicProjection;
use super::intersect::Intersect;
use super::map_projection::MapProjection;
use super::math::{js_max, js_min};
use super::rectangle::Rectangle;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoundingRectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl BoundingRectangle {
    pub const PACKED_LENGTH: usize = 4;

    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index..starting_index + 4].copy_from_slice(&[self.x, self.y, self.width, self.height]);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let a = &array[starting_index..starting_index + 4];
        Self::new(a[0], a[1], a[2], a[3])
    }

    /// Smallest box containing every point; empty for no points.
    pub fn from_points(positions: &[Cartesian2]) -> Self {
        let Some(first) = positions.first() else {
            return Self::default();
        };
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
        for p in &positions[1..] {
            min_x = js_min(p.x, min_x);
            max_x = js_max(p.x, max_x);
            min_y = js_min(p.y, min_y);
            max_y = js_max(p.y, max_y);
        }
        Self::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// Projected bounds of a cartographic rectangle; geographic projection
    /// on WGS84 when `projection` is `None`.
    pub fn from_rectangle(rectangle: &Rectangle, projection: Option<&dyn MapProjection>) -> Self {
        let default = GeographicProjection::default();
        let projection = projection.unwrap_or(&default);
        let lower_left = projection.project(&rectangle.southwest());
        let upper_right = projection.project(&rectangle.northeast());
        Self::new(
            lower_left.x,
            lower_left.y,
            upper_right.x - lower_left.x,
            upper_right.y - lower_left.y,
        )
    }

    pub fn union(&self, right: &Self) -> Self {
        let lower_left_x = js_min(self.x, right.x);
        let lower_left_y = js_min(self.y, right.y);
        let upper_right_x = js_max(self.x + self.width, right.x + right.width);
        let upper_right_y = js_max(self.y + self.height, right.y + right.height);
        Self::new(lower_left_x, lower_left_y, upper_right_x - lower_left_x, upper_right_y - lower_left_y)
    }

    /// Grows the box to include `point`.
    pub fn expand(&self, point: Cartesian2) -> Self {
        let mut r = *self;
        let width = point.x - r.x;
        let height = point.y - r.y;
        if width > r.width {
            r.width = width;
        } else if width < 0.0 {
            r.width -= width;
            r.x = point.x;
        }
        if height > r.height {
            r.height = height;
        } else if height < 0.0 {
            r.height -= height;
            r.y = point.y;
        }
        r
    }

    pub fn intersect(&self, right: &Self) -> Intersect {
        if !(self.x > right.x + right.width
            || self.x + self.width < right.x
            || self.y + self.height < right.y
            || self.y > right.y + right.height)
        {
            Intersect::Intersecting
        } else {
            Intersect::Outside
        }
    }

    pub fn equals(&self, right: &Self) -> bool {
        self == right
    }
}
