//! CesiumJS `Scene/Model/MappedPositions`: cartographic positions computed for
//! one imagery layer's ellipsoid, with their bounding rectangle.
//!
//! The positions are shared with the owning `ModelPrimitiveImagery` rather than
//! copied, because the point of the class is to avoid recomputing an
//! ellipsoid-specific mapping for every layer.

use crate::core::{Cartographic, Ellipsoid, Rectangle};
use std::rc::Rc;

/// A collection of cartographic positions and their bounding rectangle for a
/// specific ellipsoid.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedPositions {
    cartographic_positions: Rc<Vec<Cartographic>>,
    num_positions: usize,
    cartographic_bounding_rectangle: Rectangle,
    ellipsoid: Ellipsoid,
}

impl MappedPositions {
    /// `cartographicPositions` must hold at least `num_positions` entries. As in
    /// CesiumJS, `num_positions` is stored separately from the collection so
    /// callers can share a larger buffer.
    pub fn new(
        cartographic_positions: Rc<Vec<Cartographic>>,
        num_positions: usize,
        cartographic_bounding_rectangle: Rectangle,
        ellipsoid: Ellipsoid,
    ) -> Self {
        Self { cartographic_positions, num_positions, cartographic_bounding_rectangle, ellipsoid }
    }

    pub fn cartographic_positions(&self) -> &[Cartographic] {
        &self.cartographic_positions
    }

    pub fn num_positions(&self) -> usize {
        self.num_positions
    }

    pub fn cartographic_bounding_rectangle(&self) -> &Rectangle {
        &self.cartographic_bounding_rectangle
    }

    pub fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }
}
