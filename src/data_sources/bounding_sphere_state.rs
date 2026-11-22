//! Availability of a visualizer's bounding sphere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BoundingSphereState {
    Done = 0,
    Pending = 1,
    Failed = 2,
}
