//! Cesium `PostProcessStageTextureCache`: creates and caches a minimal amount
//! of textures and framebuffers for post-process stages.
use crate::{
    core::{Color, PixelFormat, RuntimeError},
    renderer::*,
    scene::{
        post_process_stage::{PostProcessStage, PostProcessUniformValue},
        post_process_stage_composite::{PostProcessStageComposite, PostProcessStageItem},
    },
};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

/// An allocated framebuffer managed by the texture cache.
pub struct CachedFramebuffer<G: Gl> {
    pub texture_scale: f64,
    pub force_power_of_two: bool,
    pub pixel_format: PixelFormat,
    pub pixel_datatype: PixelDatatype,
    pub clear_color: Color,
    pub stages: Vec<String>,
    pub buffer: FramebufferManager<G>,
    pub clear_command: ClearCommand<G>,
}

/// Creates and manages a minimal set of framebuffers for post-process stages.
pub struct PostProcessStageTextureCache<G: Gl> {
    framebuffers: Vec<CachedFramebuffer<G>>,
    stage_to_framebuffer: HashMap<String, usize>,
    width: Option<u32>,
    height: Option<u32>,
    update_dependencies: bool,
    destroyed: bool,
}

impl<G: Gl> Default for PostProcessStageTextureCache<G> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: Gl> PostProcessStageTextureCache<G> {
    pub fn new() -> Self {
        Self {
            framebuffers: Vec::new(),
            stage_to_framebuffer: HashMap::new(),
            width: None,
            height: None,
            update_dependencies: false,
            destroyed: false,
        }
    }

    pub fn update_dependencies(&mut self) {
        self.update_dependencies = true;
    }

    pub fn framebuffer_count(&self) -> usize {
        self.framebuffers.len()
    }

    pub fn get_framebuffer(&self, name: &str) -> Option<Rc<Framebuffer<G>>> {
        let idx = *self.stage_to_framebuffer.get(name)?;
        self.framebuffers.get(idx)?.buffer.framebuffer().cloned()
    }

    pub fn get_output_texture(&self, name: &str) -> Option<Rc<Texture<G>>> {
        let idx = *self.stage_to_framebuffer.get(name)?;
        self.framebuffers.get(idx)?.buffer.color_texture(0).ok().flatten().cloned()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.release_resources()?;
        self.destroyed = true;
        Ok(())
    }

    fn release_resources(&mut self) -> Result<(), RuntimeError> {
        for fb in &mut self.framebuffers {
            fb.buffer.destroy()?;
        }
        self.framebuffers.clear();
        self.stage_to_framebuffer.clear();
        self.width = None;
        self.height = None;
        Ok(())
    }

    pub fn clear(&self, context: &Context<G>) -> Result<(), RuntimeError> {
        for fb in &self.framebuffers {
            let pass = PassState::default();
            fb.buffer.clear(context, &fb.clear_command, &pass)?;
        }
        Ok(())
    }

    /// Primary update call before stages execute.
    pub fn update(
        &mut self,
        context: &Context<G>,
        active_stages: &[&PostProcessStage<G>],
        dependencies: &HashMap<String, HashSet<String>>,
    ) -> Result<(), RuntimeError> {
        let drawing_size = context.drawing_buffer_size().unwrap_or([1, 1]);
        let width = drawing_size[0];
        let height = drawing_size[1];

        let dimensions_changed = self.width != Some(width) || self.height != Some(height);

        if self.update_dependencies {
            self.release_resources()?;
        }

        if self.framebuffers.is_empty() && !active_stages.is_empty() {
            self.create_framebuffers(active_stages, dependencies)?;
        }

        if !self.update_dependencies && !dimensions_changed {
            return Ok(());
        }

        self.width = Some(width);
        self.height = Some(height);
        self.update_dependencies = false;

        self.update_framebuffers(context, width, height)?;
        Ok(())
    }

    fn create_framebuffers(
        &mut self,
        active_stages: &[&PostProcessStage<G>],
        dependencies: &HashMap<String, HashSet<String>>,
    ) -> Result<(), RuntimeError> {
        for stage in active_stages {
            let stage_name = stage.name();
            let stage_deps = dependencies.get(stage_name);

            let mut found_index = None;
            for (idx, fb) in self.framebuffers.iter_mut().enumerate() {
                if (stage.texture_scale() - fb.texture_scale).abs() > 1e-6
                    || stage.force_power_of_two() != fb.force_power_of_two
                    || stage.pixel_format() != fb.pixel_format
                    || stage.pixel_datatype() != fb.pixel_datatype
                    || stage.clear_color() != fb.clear_color
                {
                    continue;
                }

                let conflict = match stage_deps {
                    Some(deps) => fb.stages.iter().any(|s| deps.contains(s)),
                    None => false,
                };

                if !conflict {
                    fb.stages.push(stage_name.to_string());
                    found_index = Some(idx);
                    break;
                }
            }

            let fb_idx = match found_index {
                Some(idx) => idx,
                None => {
                    let options = FramebufferManagerOptions {
                        color: true,
                        pixel_format: Some(stage.pixel_format()),
                        pixel_datatype: Some(stage.pixel_datatype()),
                        create_depth_attachments: false,
                        ..Default::default()
                    };
                    let manager = FramebufferManager::new(options)?;
                    let idx = self.framebuffers.len();
                    self.framebuffers.push(CachedFramebuffer {
                        texture_scale: stage.texture_scale(),
                        force_power_of_two: stage.force_power_of_two(),
                        pixel_format: stage.pixel_format(),
                        pixel_datatype: stage.pixel_datatype(),
                        clear_color: stage.clear_color(),
                        stages: vec![stage_name.to_string()],
                        buffer: manager,
                        clear_command: ClearCommand::default(),
                    });
                    idx
                }
            };

            self.stage_to_framebuffer.insert(stage_name.to_string(), fb_idx);
        }
        Ok(())
    }

    fn update_framebuffers(&mut self, context: &Context<G>, width: u32, height: u32) -> Result<(), RuntimeError> {
        for fb in &mut self.framebuffers {
            let scale = fb.texture_scale;
            let mut texture_width = ((width as f64) * scale).ceil().max(1.0) as u32;
            let mut texture_height = ((height as f64) * scale).ceil().max(1.0) as u32;

            if fb.force_power_of_two {
                let mut size = texture_width.min(texture_height);
                if !size.is_power_of_two() {
                    size = size.next_power_of_two();
                }
                texture_width = size;
                texture_height = size;
            }

            fb.buffer.update(
                context,
                texture_width,
                texture_height,
                Some(1),
                Some(fb.pixel_datatype),
                Some(fb.pixel_format),
            )?;

            fb.clear_command = ClearCommand {
                color: Some([
                    fb.clear_color.red,
                    fb.clear_color.green,
                    fb.clear_color.blue,
                    fb.clear_color.alpha,
                ]),
                framebuffer: fb.buffer.framebuffer().cloned(),
                ..Default::default()
            };
        }
        Ok(())
    }
}

/// Helper function to get the name of the last stage in a stage item or composite.
pub fn get_last_stage_name<G: Gl>(item: &PostProcessStageItem<G>) -> String {
    match item {
        PostProcessStageItem::Stage(s) => s.name().to_string(),
        PostProcessStageItem::Composite(c) => {
            if let Some(last) = c.stages().last() {
                get_last_stage_name(last)
            } else {
                c.name().to_string()
            }
        }
    }
}

/// Collects dependencies for a single stage.
pub fn get_stage_dependencies<G: Gl>(
    dependencies: &mut HashMap<String, HashSet<String>>,
    stage: &PostProcessStage<G>,
    previous_last_name: Option<&str>,
) -> Option<String> {
    if !stage.enabled() {
        return previous_last_name.map(|s| s.to_string());
    }

    let stage_name = stage.name().to_string();
    let entry = dependencies.entry(stage_name.clone()).or_default();
    if let Some(prev) = previous_last_name {
        entry.insert(prev.to_string());
    }

    for val in stage.uniforms().values() {
        if let PostProcessUniformValue::StageName(target_name) = val {
            entry.insert(target_name.clone());
        }
    }

    Some(stage_name)
}

/// Collects dependencies for a composite stage.
#[allow(clippy::needless_range_loop)]
pub fn get_composite_dependencies<G: Gl>(
    dependencies: &mut HashMap<String, HashSet<String>>,
    composite: &PostProcessStageComposite<G>,
    previous_last_name: Option<&str>,
) -> Option<String> {
    if !composite.enabled() || composite.is_empty() {
        return previous_last_name.map(|s| s.to_string());
    }

    let in_series = composite.input_previous_stage_texture();
    let original_prev = previous_last_name.map(|s| s.to_string());
    let mut current_last = original_prev.clone();

    let mut stage_last_names = Vec::new();
    for stage in composite.stages() {
        match stage {
            PostProcessStageItem::Stage(s) => {
                current_last = get_stage_dependencies(dependencies, s, current_last.as_deref());
                stage_last_names.push(s.name().to_string());
            }
            PostProcessStageItem::Composite(c) => {
                current_last = get_composite_dependencies(dependencies, c, current_last.as_deref());
                stage_last_names.push(get_last_stage_name(stage));
            }
        }
        if in_series {
            // In series, next stage depends on current stage
        } else {
            // Not in series, restore to original_prev for input of next
            current_last = original_prev.clone();
        }
    }

    if !in_series {
        for j in 1..stage_last_names.len() {
            let name = &stage_last_names[j];
            let entry = dependencies.entry(name.clone()).or_default();
            for k in 0..j {
                entry.insert(stage_last_names[k].clone());
            }
        }
    } else if let Some(orig) = &original_prev {
        for j in 1..stage_last_names.len() {
            let name = &stage_last_names[j];
            dependencies.entry(name.clone()).or_default().insert(orig.clone());
        }
    }

    stage_last_names.last().cloned().or(original_prev)
}
