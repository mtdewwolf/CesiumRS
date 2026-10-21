//! Deep immutability is structural in Rust; application caches live separately.
use super::RenderState;
use std::ops::Deref;
#[derive(Clone, Debug)]
pub struct FrozenRenderState(RenderState);
pub fn freeze_render_state(state: RenderState) -> FrozenRenderState {
    FrozenRenderState(state)
}
impl FrozenRenderState {
    pub fn to_owned_state(&self) -> RenderState {
        self.0.clone()
    }
}
impl Deref for FrozenRenderState {
    type Target = RenderState;
    fn deref(&self) -> &RenderState {
        &self.0
    }
}
