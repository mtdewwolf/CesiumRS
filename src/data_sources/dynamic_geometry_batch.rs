//! CesiumJS `DynamicGeometryBatch`: manages a collection of
//! [`DynamicGeometryUpdater`] instances for time-varying entities.
use super::{BoundingSphereState, DynamicGeometryUpdater, GeometryUpdater};
use crate::{
    core::{BoundingSphere, JulianDate, RuntimeError},
    scene::{OrderedGroundPrimitiveCollection, PrimitiveCollection},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// Port of CesiumJS `DynamicGeometryBatch`.
pub struct DynamicGeometryBatch {
    primitives: Rc<RefCell<PrimitiveCollection>>,
    ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    order: Vec<String>,
    dynamic_updaters: HashMap<String, Box<dyn DynamicGeometryUpdater>>,
}

impl DynamicGeometryBatch {
    pub fn new(
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Self {
        Self { primitives, ordered_ground_primitives, order: Vec::new(), dynamic_updaters: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.dynamic_updaters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dynamic_updaters.is_empty()
    }

    pub fn add(&mut self, _time: &JulianDate, updater: &dyn GeometryUpdater) -> Result<(), RuntimeError> {
        let id = updater.id().to_owned();
        let dynamic =
            updater.create_dynamic_updater(Rc::clone(&self.primitives), Rc::clone(&self.ordered_ground_primitives))?;
        if !self.dynamic_updaters.contains_key(&id) {
            self.order.push(id.clone());
        }
        if let Some(mut old) = self.dynamic_updaters.insert(id, dynamic) {
            old.destroy();
        }
        Ok(())
    }

    pub fn remove(&mut self, updater: &dyn GeometryUpdater) {
        let id = updater.id();
        if let Some(mut dynamic) = self.dynamic_updaters.remove(id) {
            self.order.retain(|k| k != id);
            dynamic.destroy();
        }
    }

    pub fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        for id in &self.order {
            if let Some(dynamic) = self.dynamic_updaters.get_mut(id) {
                dynamic.update(time)?;
            }
        }
        Ok(true)
    }

    pub fn remove_all_primitives(&mut self) {
        for id in self.order.drain(..) {
            if let Some(mut dynamic) = self.dynamic_updaters.remove(&id) {
                dynamic.destroy();
            }
        }
        self.dynamic_updaters.clear();
    }

    pub fn get_bounding_sphere(
        &self,
        updater: &dyn GeometryUpdater,
        result: &mut BoundingSphere,
    ) -> BoundingSphereState {
        match self.dynamic_updaters.get(updater.id()) {
            Some(dynamic) => dynamic.get_bounding_sphere(result),
            None => BoundingSphereState::Failed,
        }
    }
}
