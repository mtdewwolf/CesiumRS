//! CesiumJS `Vector3DTileBatch`: a contiguous index range sharing a color.
use crate::core::Color;
#[derive(Clone, Debug, PartialEq)]
pub struct Vector3DTileBatch {
    pub offset: usize,
    pub count: usize,
    pub color: Color,
    pub batch_ids: Vec<u32>,
}
impl Vector3DTileBatch {
    pub fn new(offset: usize, count: usize, color: Color, batch_ids: Vec<u32>) -> Self {
        Self { offset, count, color, batch_ids }
    }
}
