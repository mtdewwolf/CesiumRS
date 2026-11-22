//! Host visualizers update an entity collection for a given simulation time.
use super::{BoundingSphereState, EntityRef};
use crate::core::{BoundingSphere, JulianDate, RuntimeError};

pub trait Visualizer {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError>;
    fn get_bounding_sphere(&self, _entity: &EntityRef, _result: &mut BoundingSphere) -> BoundingSphereState {
        BoundingSphereState::Failed
    }
    fn is_destroyed(&self) -> bool;
    fn destroy(&mut self);
}
