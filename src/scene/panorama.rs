//! CesiumJS abstract `Panorama` contract. Implementations own their rendering resources.
use crate::core::{Credit, Matrix4};
pub trait Panorama {
    fn show(&self) -> bool;
    fn set_show(&mut self, show: bool);
    fn transform(&self) -> &Matrix4;
    fn credit(&self) -> Option<&Credit>;
}
