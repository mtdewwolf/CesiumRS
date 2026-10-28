//! CesiumJS SkyBox's mode/pass filtering and panorama delegation.
//! The caller supplies the cube-map panorama renderer; this module does not load images.
use super::{panorama::Panorama, FrameState, SceneMode};
use crate::core::RuntimeError;
pub trait SkyBoxPanorama: Panorama {
    type Command;
    type Sources;
    fn sources(&self) -> &Self::Sources;
    fn set_sources(&mut self, sources: Self::Sources);
    fn update(&mut self, frame: &FrameState, hdr: bool) -> Result<Option<Self::Command>, RuntimeError>;
    fn destroy(&mut self) -> Result<(), RuntimeError>;
}
pub struct SkyBox<P: SkyBoxPanorama> {
    pub show: bool,
    panorama: P,
    destroyed: bool,
}
impl<P: SkyBoxPanorama> SkyBox<P> {
    pub fn new(panorama: P) -> Self {
        Self { show: true, panorama, destroyed: false }
    }
    pub fn sources(&self) -> &P::Sources {
        self.panorama.sources()
    }
    pub fn set_sources(&mut self, sources: P::Sources) {
        self.panorama.set_sources(sources);
    }
    pub fn update(&mut self, frame: &FrameState, hdr: bool) -> Result<Option<P::Command>, RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("SkyBox was destroyed"));
        }
        // Pinned source exposes show but does not consult it in update.
        if !matches!(frame.mode, SceneMode::Scene3D | SceneMode::Morphing) || !frame.passes.render {
            return Ok(None);
        }
        self.panorama.update(frame, hdr)
    }
    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        if !self.destroyed {
            self.panorama.destroy()?;
            self.destroyed = true;
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn earth_sources(base: &str) -> [String; 6] {
        ["px", "mx", "py", "my", "pz", "mz"].map(|s| format!("{base}Assets/Textures/SkyBox/tycho2t3_80_{s}.jpg"))
    }
}
