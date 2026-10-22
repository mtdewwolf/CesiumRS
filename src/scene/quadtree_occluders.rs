//! CesiumJS `QuadtreeOccluders`: the occluders quadtree selection culls
//! tiles against.

use crate::core::{Ellipsoid, EllipsoidalOccluder};

#[derive(Clone, Debug)]
pub struct QuadtreeOccluders {
    ellipsoid: EllipsoidalOccluder,
}

impl QuadtreeOccluders {
    /// An occluder for `ellipsoid`, with the camera at the origin until the
    /// first update.
    pub fn new(ellipsoid: &Ellipsoid) -> Self {
        Self { ellipsoid: EllipsoidalOccluder::new(ellipsoid, Some(crate::core::Cartesian3::ZERO)) }
    }

    pub fn ellipsoid(&self) -> &EllipsoidalOccluder {
        &self.ellipsoid
    }

    pub fn ellipsoid_mut(&mut self) -> &mut EllipsoidalOccluder {
        &mut self.ellipsoid
    }
}
