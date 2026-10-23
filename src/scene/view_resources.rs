//! Typed GPU resources owned by a View. Shared ownership lets the backend and
//! SceneView refer to the same resources without retaining a Scene reference.
use super::{framebuffer_helpers::*, *};
use crate::renderer::*;
pub struct ViewResources<G: Gl> {
    pub pick_framebuffer: Option<PickFramebuffer<G>>,
    pub pick_depth_framebuffer: PickDepthFramebuffer<G>,
    pub scene_framebuffer: SceneFramebuffer<G>,
    pub edge_framebuffer: EdgeFramebuffer<G>,
    pub globe_depth: Option<GlobeDepth<G>>,
    pub globe_translucency_framebuffer: GlobeTranslucencyFramebuffer<G>,
    pub oit: Option<Oit<G>>,
    pub translucent_tile_classification: TranslucentTileClassification<G>,
    pub pick_depths: Vec<PickDepth<G>>,
    destroyed: bool,
}
impl<G: Gl> ViewResources<G> {
    pub fn new(context: &Context<G>, use_oit: bool) -> Result<Self> {
        Ok(Self {
            pick_framebuffer: Some(PickFramebuffer::new(context.clone())?),
            pick_depth_framebuffer: PickDepthFramebuffer::new()?,
            scene_framebuffer: SceneFramebuffer::new()?,
            edge_framebuffer: EdgeFramebuffer::new()?,
            globe_depth: if context.capabilities()?.depth_texture { Some(GlobeDepth::new()?) } else { None },
            globe_translucency_framebuffer: GlobeTranslucencyFramebuffer::new()?,
            oit: if use_oit && context.capabilities()?.depth_texture { Some(Oit::new(context)?) } else { None },
            translucent_tile_classification: TranslucentTileClassification::new(context)?,
            pick_depths: Vec::new(),
            destroyed: false,
        })
    }
    pub fn get_pick_depth(&mut self, index: usize) -> Result<&mut PickDepth<G>> {
        alive(self.destroyed)?;
        while self.pick_depths.len() <= index {
            self.pick_depths.push(PickDepth::new()?);
        }
        Ok(&mut self.pick_depths[index])
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        if let Some(f) = &mut self.pick_framebuffer {
            f.destroy()?;
        }
        self.pick_depth_framebuffer.destroy()?;
        self.scene_framebuffer.destroy()?;
        self.edge_framebuffer.destroy()?;
        if let Some(g) = &mut self.globe_depth {
            g.destroy()?;
        }
        self.globe_translucency_framebuffer.destroy()?;
        if let Some(oit) = &mut self.oit {
            oit.destroy()?;
        }
        self.translucent_tile_classification.destroy()?;
        for depth in &mut self.pick_depths {
            depth.destroy()?;
        }
        self.pick_depths.clear();
        self.destroyed = true;
        Ok(())
    }
}
