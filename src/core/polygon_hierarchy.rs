//! CesiumJS `PolygonHierarchy`: an outer ring and its holes, each of which
//! may have holes of its own.

use super::cartesian3::Cartesian3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolygonHierarchy {
    pub positions: Vec<Cartesian3>,
    pub holes: Vec<PolygonHierarchy>,
}

impl PolygonHierarchy {
    pub fn new(positions: Vec<Cartesian3>, holes: Vec<PolygonHierarchy>) -> Self {
        Self { positions, holes }
    }
}
