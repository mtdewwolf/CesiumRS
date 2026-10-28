//! CesiumJS `VoxelContent` metadata-array path. glTF loading is not implemented.
use super::FrameState;
use crate::core::{RuntimeError, TypedArray};
#[derive(Clone, Debug)]
pub struct VoxelContent {
    metadata: Vec<TypedArray>,
    ready: bool,
    destroyed: bool,
}
impl VoxelContent {
    pub fn from_metadata_array(metadata: Vec<TypedArray>) -> Self {
        Self { metadata, ready: false, destroyed: false }
    }
    pub fn metadata(&self) -> &[TypedArray] {
        &self.metadata
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("VoxelContent was destroyed"));
        }
        if !self.ready {
            frame.after_render.push(Box::new(|| Ok(true)));
            self.ready = true;
        }
        Ok(())
    }
    pub fn destroy(&mut self) {
        self.metadata.clear();
        self.destroyed = true;
        self.ready = false;
    }
}
