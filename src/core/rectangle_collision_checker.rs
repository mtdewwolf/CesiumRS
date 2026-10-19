//! CesiumJS `RectangleCollisionChecker`.
//!
//! Stores ordinary Cartesian bounding boxes: antimeridian rectangles are not
//! split. Edge contact is a collision. A linear index replaces rbush; query
//! results are identical for unique IDs and valid rectangles.

use super::rectangle::Rectangle;

#[derive(Clone, Debug, Default)]
pub struct RectangleCollisionChecker {
    rectangles: Vec<(String, Rectangle)>,
}

impl RectangleCollisionChecker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: impl Into<String>, rectangle: Rectangle) {
        self.rectangles.push((id.into(), rectangle));
    }

    pub fn remove(&mut self, id: &str, rectangle: &Rectangle) {
        // rbush searches by the supplied box before applying ID equality.
        if let Some(index) = self.rectangles.iter().position(|(key, r)| {
            key == id
                && r.west >= rectangle.west
                && r.south >= rectangle.south
                && r.east <= rectangle.east
                && r.north <= rectangle.north
        }) {
            self.rectangles.remove(index);
        }
    }

    pub fn collides(&self, rectangle: &Rectangle) -> bool {
        self.rectangles.iter().any(|(_, r)| {
            rectangle.west <= r.east
                && rectangle.south <= r.north
                && rectangle.east >= r.west
                && rectangle.north >= r.south
        })
    }
}
