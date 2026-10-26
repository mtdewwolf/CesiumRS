//! CesiumJS `Cesium3DTilePassState`: the per-pass state for a 3D Tiles traversal pass.

use super::{camera::Camera, cesium_3d_tile_pass::Cesium3DTilePass, scene::SceneCommand};
use crate::core::CullingVolume;
use std::cell::RefCell;
use std::rc::Rc;

/// Construction options for [`Cesium3DTilePassState`].
#[derive(Clone)]
pub struct Cesium3DTilePassStateOptions {
    pub pass: Cesium3DTilePass,
    pub command_list: Option<Vec<SceneCommand>>,
    pub camera: Option<Rc<RefCell<Camera>>>,
    pub culling_volume: Option<CullingVolume>,
}

impl Default for Cesium3DTilePassStateOptions {
    fn default() -> Self {
        Self { pass: Cesium3DTilePass::Render, command_list: None, camera: None, culling_volume: None }
    }
}

/// The state for a 3D Tiles update pass.
#[derive(Clone)]
pub struct Cesium3DTilePassState {
    /// The pass.
    pub pass: Cesium3DTilePass,

    /// An optional custom command list to receive draw commands for this pass.
    pub command_list: Option<Vec<SceneCommand>>,

    /// A camera override for this pass.
    pub camera: Option<Rc<RefCell<Camera>>>,

    /// A culling volume override for this pass.
    pub culling_volume: Option<CullingVolume>,

    /// Indicates whether the pass is ready (i.e. all tiles needed by the pass are loaded).
    pub ready: bool,
}

impl std::fmt::Debug for Cesium3DTilePassState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cesium3DTilePassState")
            .field("pass", &self.pass)
            .field("command_list", &self.command_list.as_ref().map(|l| l.len()))
            .field("has_camera", &self.camera.is_some())
            .field("culling_volume", &self.culling_volume)
            .field("ready", &self.ready)
            .finish()
    }
}

impl Cesium3DTilePassState {
    pub fn new(options: Cesium3DTilePassStateOptions) -> Self {
        Self {
            pass: options.pass,
            command_list: options.command_list,
            camera: options.camera,
            culling_volume: options.culling_volume,
            ready: false,
        }
    }

    pub fn for_pass(pass: Cesium3DTilePass) -> Self {
        Self::new(Cesium3DTilePassStateOptions { pass, command_list: None, camera: None, culling_volume: None })
    }
}
