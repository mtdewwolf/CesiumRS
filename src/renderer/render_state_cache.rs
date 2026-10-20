//! Immutable render-state interning and sparse transition application.
//! Cache ownership is explicit; Rc keeps acquired immutable states alive.
use super::{context::*, render_state::*};
use std::{collections::HashMap, ops::Deref, rc::Rc};
#[derive(Debug)]
pub struct CachedRenderState {
    id: u64,
    state: RenderState,
}
impl CachedRenderState {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn get_state(&self) -> RenderState {
        self.state.clone()
    }
}
impl Deref for CachedRenderState {
    type Target = RenderState;
    fn deref(&self) -> &RenderState {
        &self.state
    }
}
#[derive(Default)]
pub struct RenderStateCache {
    states: HashMap<String, Rc<CachedRenderState>>,
    next_id: u64,
    acquisitions: HashMap<String, usize>,
}
impl RenderStateCache {
    pub fn from_cache(&mut self, state: RenderState) -> Result<Rc<CachedRenderState>> {
        self.intern(state, true)
    }
    fn intern(&mut self, state: RenderState, acquire: bool) -> Result<Rc<CachedRenderState>> {
        state.validate()?;
        let key = serde_json::to_string(&state).map_err(|e| crate::core::RuntimeError::new(e.to_string()))?;
        if acquire {
            *self.acquisitions.entry(key.clone()).or_default() += 1;
        }
        if let Some(v) = self.states.get(&key) {
            return Ok(v.clone());
        }
        let cached = Rc::new(CachedRenderState { id: self.next_id, state });
        self.next_id += 1;
        self.states.insert(key, cached.clone());
        Ok(cached)
    }
    pub fn remove_from_cache(&mut self, state: &RenderState) {
        if let Ok(key) = serde_json::to_string(state) {
            if let Some(count) = self.acquisitions.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    self.acquisitions.remove(&key);
                    self.states.remove(&key);
                }
            }
        }
    }
    pub fn clear_cache(&mut self) {
        self.states.clear();
        self.acquisitions.clear();
    }
    pub fn len(&self) -> usize {
        self.states.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
#[derive(Clone)]
struct Applied {
    state: Rc<CachedRenderState>,
    scissor: ScissorTest,
    blending: bool,
    viewport: Viewport,
}
#[derive(Default)]
pub(crate) struct RenderStateDriver {
    cache: RenderStateCache,
    transitions: HashMap<(u64, u64), u32>,
    previous: Option<Applied>,
}
impl RenderStateDriver {
    pub fn invalidate(&mut self) {
        self.previous = None;
    }
    pub fn apply<G: Gl>(
        &mut self,
        context: &Context<G>,
        state: &RenderState,
        pass: &PassState<G>,
        clear: bool,
    ) -> Result<()> {
        let cached = self.cache.intern(state.clone(), false)?;
        let dimensions = context.host().drawing_buffer_size()?;
        let viewport = state.viewport.or(pass.viewport).unwrap_or(Viewport {
            x: 0,
            y: 0,
            width: dimensions[0],
            height: dimensions[1],
        });
        let scissor = pass.scissor_test.as_ref().unwrap_or(&state.scissor_test).clone();
        let blending = pass.blending_enabled.unwrap_or(state.blending.enabled);
        let next = Applied { state: cached.clone(), scissor, blending, viewport };
        let mask = if let Some(prev) = &self.previous {
            let mask = *self
                .transitions
                .entry((prev.state.id, cached.id))
                .or_insert_with(|| transition_mask(&prev.state, &cached));
            mask | if clear || prev.scissor != next.scissor { 1 << 11 } else { 0 }
                | if prev.blending != blending || blending && prev.state.blending != state.blending {
                    1 << 12
                } else {
                    0
                }
                | if prev.state.id != cached.id || prev.viewport != viewport { 1 << 13 } else { 0 }
        } else {
            u32::MAX
        };
        let result = state.apply_masked(context, pass, dimensions, mask);
        self.previous = if result.is_ok() { Some(next) } else { None };
        result
    }
}
pub(crate) fn transition_mask(a: &RenderState, b: &RenderState) -> u32 {
    let changed = [
        a.front_face != b.front_face,
        a.cull != b.cull,
        a.line_width != b.line_width,
        a.polygon_offset != b.polygon_offset,
        a.depth_range != b.depth_range,
        a.depth_test != b.depth_test,
        a.color_mask != b.color_mask,
        a.depth_mask != b.depth_mask,
        a.stencil_mask != b.stencil_mask,
        a.stencil_test != b.stencil_test,
        a.sample_coverage != b.sample_coverage,
    ];
    changed.iter().enumerate().fold(0, |m, (i, c)| m | if *c { 1 << i } else { 0 })
}
pub(crate) fn call_group<O>(name: &str, args: &[GlValue<O>]) -> u32 {
    use crate::core::webgl_constants as gl;
    let n = match args.first() {
        Some(GlValue::Number(n)) => *n as u32,
        _ => 0,
    };
    let group = match name {
        "frontFace" => 0,
        "cullFace" => 1,
        "lineWidth" => 2,
        "polygonOffset" => 3,
        "depthRange" => 4,
        "depthFunc" => 5,
        "colorMask" => 6,
        "depthMask" => 7,
        "stencilMask" => 8,
        "stencilFunc" | "stencilFuncSeparate" | "stencilOpSeparate" => 9,
        "sampleCoverage" => 10,
        "scissor" => 11,
        "blendColor" | "blendEquationSeparate" | "blendFuncSeparate" => 12,
        "viewport" => 13,
        "enable" | "disable" => match n {
            gl::CULL_FACE => 1,
            gl::POLYGON_OFFSET_FILL => 3,
            gl::DEPTH_TEST => 5,
            gl::STENCIL_TEST => 9,
            gl::SAMPLE_COVERAGE => 10,
            gl::SCISSOR_TEST => 11,
            gl::BLEND => 12,
            _ => 31,
        },
        _ => 31,
    };
    1 << group
}
impl<G: Gl> Context<G> {
    /// Call after external code mutates GL state through the raw host.
    pub fn invalidate_render_state(&self) {
        self.state_driver.borrow_mut().invalidate();
    }
    pub(crate) fn apply_render_state(&self, state: &RenderState, pass: &PassState<G>, clear: bool) -> Result<()> {
        self.state_driver.borrow_mut().apply(self, state, pass, clear)
    }
}
