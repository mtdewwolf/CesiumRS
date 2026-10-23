//! A mutable clipping plane with collection dirty notifications.
use crate::core::{Cartesian3, Plane};
use std::{cell::RefCell, rc::Rc};
pub type PlaneChangeCallback = Rc<RefCell<dyn FnMut(isize, &ClippingPlane)>>;
pub type ClippingPlaneHandle = Rc<RefCell<ClippingPlane>>;
pub struct ClippingPlane {
    normal: Cartesian3,
    distance: f64,
    pub index: isize,
    pub on_change_callback: Option<PlaneChangeCallback>,
}
impl ClippingPlane {
    pub fn new(normal: Cartesian3, distance: f64) -> Self {
        Self { normal, distance, index: -1, on_change_callback: None }
    }
    pub fn handle(normal: Cartesian3, distance: f64) -> ClippingPlaneHandle {
        Rc::new(RefCell::new(Self::new(normal, distance)))
    }
    pub fn from_plane(plane: &Plane) -> Self {
        Self::new(plane.normal, plane.distance)
    }
    pub fn normal(&self) -> Cartesian3 {
        self.normal
    }
    pub fn distance(&self) -> f64 {
        self.distance
    }
    fn changed(&self) {
        if let Some(callback) = &self.on_change_callback {
            (callback.borrow_mut())(self.index, self);
        }
    }
    pub fn set_distance(&mut self, value: f64) {
        if value != self.distance {
            self.changed();
        }
        self.distance = value;
    }
    pub fn set_normal(&mut self, value: Cartesian3) {
        if self.normal != value {
            self.changed();
        }
        self.normal = value;
    }
    pub fn set_normal_component(&mut self, index: usize, value: f64) {
        let previous = [self.normal.x, self.normal.y, self.normal.z][index];
        if value != previous {
            self.changed();
        }
        match index {
            0 => self.normal.x = value,
            1 => self.normal.y = value,
            2 => self.normal.z = value,
            _ => unreachable!(),
        }
    }
    pub fn clone_into(&self, result: &mut Self) {
        result.set_normal(self.normal);
        result.set_distance(self.distance);
    }
    pub fn as_plane(&self) -> Plane {
        Plane { normal: self.normal, distance: self.distance }
    }
}
/// Clones parameters without ownership or a notification callback.
impl Clone for ClippingPlane {
    fn clone(&self) -> Self {
        Self::new(self.normal, self.distance)
    }
}
impl std::fmt::Debug for ClippingPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClippingPlane")
            .field("normal", &self.normal)
            .field("distance", &self.distance)
            .field("index", &self.index)
            .finish()
    }
}
