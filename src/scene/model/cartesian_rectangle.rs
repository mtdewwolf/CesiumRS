//! CesiumJS `CartesianRectangle`: an axis-aligned rectangle in a plane.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CartesianRectangle {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl CartesianRectangle {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self { min_x, min_y, max_x, max_y }
    }

    /// Minimum inclusive, maximum exclusive.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x < self.max_x && y >= self.min_y && y < self.max_y
    }

    pub fn contains_exclusive(&self, x: f64, y: f64) -> bool {
        x > self.min_x && x < self.max_x && y > self.min_y && y < self.max_y
    }

    pub fn contains_inclusive(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}
