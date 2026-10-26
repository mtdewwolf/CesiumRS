//! CesiumJS `FrustumCommands`: the commands that fall inside one depth range
//! of the camera's multi-frustum, grouped by render pass.
use super::SceneCommand;
use crate::renderer::Pass;

const PASS_COUNT: usize = Pass::NUMBER_OF_PASSES as usize;

#[derive(Clone)]
pub struct FrustumCommands {
    pub near: f64,
    pub far: f64,
    pub commands: [Vec<SceneCommand>; PASS_COUNT],
    /// Next free slot per pass, matching the upstream `indices` array. The
    /// vector lengths are kept in sync so callers can use either view.
    pub indices: [usize; PASS_COUNT],
}
impl Default for FrustumCommands {
    fn default() -> Self {
        Self::new(0., 0.)
    }
}
impl FrustumCommands {
    pub fn new(near: f64, far: f64) -> Self {
        Self { near, far, commands: std::array::from_fn(|_| Vec::new()), indices: [0; PASS_COUNT] }
    }
    /// Start a frame: forget the previous contents but keep the allocation.
    pub fn reset(&mut self) {
        for commands in &mut self.commands {
            commands.clear();
        }
        self.indices = [0; PASS_COUNT];
    }
    /// Append a command to its pass bin. Returns the slot index.
    pub fn insert(&mut self, pass: usize, command: SceneCommand) -> usize {
        let index = self.indices[pass];
        self.commands[pass].push(command);
        self.indices[pass] = index + 1;
        index
    }
    pub fn len(&self, pass: Pass) -> usize {
        self.indices[pass as usize]
    }
    pub fn is_empty(&self, pass: Pass) -> bool {
        self.len(pass) == 0
    }
}
