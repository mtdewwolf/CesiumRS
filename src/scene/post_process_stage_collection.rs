//! Cesium `PostProcessStageCollection`: owns and executes user and built-in post-process stages.
use crate::{
    core::{PixelFormat, RuntimeError},
    renderer::*,
    scene::{
        auto_exposure::AutoExposure,
        post_process_stage::{PostProcessStage, PostProcessUniformValue},
        post_process_stage_composite::{PostProcessStageComposite, PostProcessStageItem},
        post_process_stage_library::PostProcessStageLibrary,
        post_process_stage_texture_cache::{
            get_composite_dependencies, get_stage_dependencies, PostProcessStageTextureCache,
        },
        tonemapper::Tonemapper,
    },
};
use std::{collections::HashMap, rc::Rc};

/// A collection of [`PostProcessStage`]s and [`PostProcessStageComposite`]s.
pub struct PostProcessStageCollection<G: Gl> {
    stages: Vec<PostProcessStageItem<G>>,
    active_stages: Vec<usize>,

    fxaa: PostProcessStage<G>,
    ao: PostProcessStageComposite<G>,
    bloom: PostProcessStageComposite<G>,
    auto_exposure: AutoExposure<G>,
    tonemapping: PostProcessStage<G>,

    tonemapper: Tonemapper,
    exposure: f64,
    auto_exposure_enabled: bool,

    texture_cache: PostProcessStageTextureCache<G>,
    random_texture: Option<Rc<Texture<G>>>,
    copy_command: Option<ViewportCommand<G>>,

    ao_enabled: bool,
    bloom_enabled: bool,
    tonemapping_enabled: bool,
    fxaa_enabled: bool,
    texture_cache_dirty: bool,
    destroyed: bool,
}

impl<G: Gl> PostProcessStageCollection<G> {
    pub fn new(_context: &Context<G>) -> Result<Self, RuntimeError> {
        let mut fxaa = PostProcessStageLibrary::create_fxaa_stage()?;
        let mut ao = PostProcessStageLibrary::create_ambient_occlusion_stage()?;
        let mut bloom = PostProcessStageLibrary::create_bloom_stage()?;
        let auto_exposure = PostProcessStageLibrary::create_auto_exposure_stage();
        let mut tonemapping = PostProcessStageLibrary::create_pbr_neutral_tonemapping_stage(false)?;

        fxaa.set_enabled(false);
        ao.set_enabled(false);
        bloom.set_enabled(false);
        tonemapping.set_enabled(false);

        Ok(Self {
            stages: Vec::new(),
            active_stages: Vec::new(),
            fxaa,
            ao,
            bloom,
            auto_exposure,
            tonemapping,
            tonemapper: Tonemapper::PbrNeutral,
            exposure: 1.0,
            auto_exposure_enabled: false,
            texture_cache: PostProcessStageTextureCache::new(),
            random_texture: None,
            copy_command: None,
            ao_enabled: false,
            bloom_enabled: false,
            tonemapping_enabled: false,
            fxaa_enabled: false,
            texture_cache_dirty: true,
            destroyed: false,
        })
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&PostProcessStageItem<G>> {
        self.stages.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut PostProcessStageItem<G>> {
        self.stages.get_mut(index)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get_stage_by_name(name).is_some()
    }

    pub fn get_stage_by_name(&self, name: &str) -> Option<&PostProcessStageItem<G>> {
        if self.fxaa.name() == name {
            return None;
        }
        for item in &self.stages {
            if item.name() == name {
                return Some(item);
            }
            if let PostProcessStageItem::Composite(c) = item {
                for sub in c.stages() {
                    if sub.name() == name {
                        return Some(sub);
                    }
                }
            }
        }
        None
    }

    pub fn get_stage_by_name_mut(&mut self, name: &str) -> Option<&mut PostProcessStageItem<G>> {
        for item in &mut self.stages {
            if item.name() == name {
                return Some(item);
            }
            if let PostProcessStageItem::Composite(c) = item {
                for sub in c.stages_mut() {
                    if sub.name() == name {
                        return Some(sub);
                    }
                }
            }
        }
        None
    }

    pub fn add(&mut self, stage: impl Into<PostProcessStageItem<G>>) -> Result<&PostProcessStageItem<G>, RuntimeError> {
        let item = stage.into();
        let name = item.name().to_string();
        if self.contains(&name) {
            return Err(RuntimeError::new(format!(
                "Stage with name {name} has already been added to collection"
            )));
        }

        self.stages.push(item);
        self.texture_cache_dirty = true;
        Ok(self.stages.last().unwrap())
    }

    pub fn remove(&mut self, index: usize) -> Result<PostProcessStageItem<G>, RuntimeError> {
        if index >= self.stages.len() {
            return Err(RuntimeError::new("Index out of bounds"));
        }
        let mut removed = self.stages.remove(index);
        removed.destroy()?;
        self.texture_cache_dirty = true;
        Ok(removed)
    }

    pub fn remove_by_name(&mut self, name: &str) -> Result<bool, RuntimeError> {
        if let Some(pos) = self.stages.iter().position(|s| s.name() == name) {
            self.remove(pos)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn remove_all(&mut self) -> Result<(), RuntimeError> {
        for mut stage in self.stages.drain(..) {
            stage.destroy()?;
        }
        self.texture_cache_dirty = true;
        Ok(())
    }

    pub fn tonemapper(&self) -> Tonemapper {
        self.tonemapper
    }

    pub fn set_tonemapper(&mut self, tonemapper: Tonemapper) -> Result<(), RuntimeError> {
        if self.tonemapper == tonemapper {
            return Ok(());
        }
        self.tonemapping.destroy()?;
        let use_auto = self.auto_exposure_enabled;
        self.tonemapping = match tonemapper {
            Tonemapper::Reinhard => PostProcessStageLibrary::create_reinhard_tonemapping_stage(use_auto)?,
            Tonemapper::ModifiedReinhard => {
                PostProcessStageLibrary::create_modified_reinhard_tonemapping_stage(use_auto)?
            }
            Tonemapper::Filmic => PostProcessStageLibrary::create_filmic_tonemapping_stage(use_auto)?,
            Tonemapper::PbrNeutral => PostProcessStageLibrary::create_pbr_neutral_tonemapping_stage(use_auto)?,
            Tonemapper::Aces => PostProcessStageLibrary::create_aces_tonemapping_stage(use_auto)?,
        };
        self.tonemapping.set_uniform("exposure", self.exposure);
        self.tonemapper = tonemapper;
        self.texture_cache_dirty = true;
        Ok(())
    }

    pub fn exposure(&self) -> f64 {
        self.exposure
    }

    pub fn set_exposure(&mut self, exposure: f64) {
        self.exposure = exposure;
        self.tonemapping.set_uniform("exposure", exposure);
    }

    pub fn auto_exposure_enabled(&self) -> bool {
        self.auto_exposure_enabled
    }

    pub fn set_auto_exposure_enabled(&mut self, enabled: bool) -> Result<(), RuntimeError> {
        self.auto_exposure_enabled = enabled;
        // recreate tonemapping stage with new auto exposure setting
        self.set_tonemapper(self.tonemapper)
    }

    pub fn fxaa(&self) -> &PostProcessStage<G> {
        &self.fxaa
    }

    pub fn fxaa_mut(&mut self) -> &mut PostProcessStage<G> {
        &mut self.fxaa
    }

    pub fn ambient_occlusion(&self) -> &PostProcessStageComposite<G> {
        &self.ao
    }

    pub fn ambient_occlusion_mut(&mut self) -> &mut PostProcessStageComposite<G> {
        &mut self.ao
    }

    pub fn bloom(&self) -> &PostProcessStageComposite<G> {
        &self.bloom
    }

    pub fn bloom_mut(&mut self) -> &mut PostProcessStageComposite<G> {
        &mut self.bloom
    }

    pub fn auto_exposure(&self) -> &AutoExposure<G> {
        &self.auto_exposure
    }

    pub fn auto_exposure_mut(&mut self) -> &mut AutoExposure<G> {
        &mut self.auto_exposure
    }

    pub fn tonemapping(&self) -> &PostProcessStage<G> {
        &self.tonemapping
    }

    pub fn tonemapping_mut(&mut self) -> &mut PostProcessStage<G> {
        &mut self.tonemapping
    }

    pub fn texture_cache(&self) -> &PostProcessStageTextureCache<G> {
        &self.texture_cache
    }

    pub fn random_texture(&self) -> Option<Rc<Texture<G>>> {
        self.random_texture.clone()
    }

    pub fn ready(&self) -> bool {
        let mut ready_and_enabled = false;
        for s in self.stages.iter().rev() {
            ready_and_enabled = ready_and_enabled || (s.ready() && s.enabled());
        }
        ready_and_enabled = ready_and_enabled || (self.fxaa.ready() && self.fxaa.enabled());
        ready_and_enabled = ready_and_enabled || (self.ao.ready() && self.ao.enabled());
        ready_and_enabled = ready_and_enabled || (self.bloom.ready() && self.bloom.enabled());
        ready_and_enabled = ready_and_enabled || (self.tonemapping.ready() && self.tonemapping.enabled());
        ready_and_enabled
    }

    pub fn has_selected(&self) -> bool {
        for s in &self.stages {
            match s {
                PostProcessStageItem::Stage(stage) => {
                    if stage.selected().is_some() {
                        return true;
                    }
                }
                PostProcessStageItem::Composite(comp) => {
                    if comp.selected().is_some() {
                        return true;
                    }
                    for sub in comp.stages() {
                        if sub.as_stage().and_then(|st| st.selected()).is_some() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    pub fn output_texture(&self) -> Option<Rc<Texture<G>>> {
        if self.fxaa.enabled() && self.fxaa.ready() {
            return self.fxaa.output_texture();
        }
        for s in self.stages.iter().rev() {
            if s.enabled() && s.ready() {
                if let Some(tex) = s.output_texture() {
                    return Some(tex);
                }
            }
        }
        if self.tonemapping.enabled() && self.tonemapping.ready() {
            return self.tonemapping.output_texture();
        }
        if self.bloom.enabled() && self.bloom.ready() {
            return self.bloom.output_texture();
        }
        if self.ao.enabled() && self.ao.ready() {
            return self.ao.output_texture();
        }
        None
    }

    pub fn update(
        &mut self,
        context: &Context<G>,
        use_log_depth: bool,
        use_hdr: bool,
    ) -> Result<(), RuntimeError> {
        self.tonemapping.set_enabled(use_hdr);

        let ao_enabled = self.ao.enabled() && self.ao.is_supported(context);
        let bloom_enabled = self.bloom.enabled() && self.bloom.is_supported(context);
        let tonemapping_enabled = self.tonemapping.enabled() && self.tonemapping.is_supported(context);
        let fxaa_enabled = self.fxaa.enabled() && self.fxaa.is_supported(context);

        let mut active = Vec::new();
        for (i, s) in self.stages.iter().enumerate() {
            if s.enabled() && s.is_supported(context) {
                active.push(i);
            }
        }
        let active_changed = active != self.active_stages;
        self.active_stages = active;

        if active_changed
            || self.texture_cache_dirty
            || ao_enabled != self.ao_enabled
            || bloom_enabled != self.bloom_enabled
            || tonemapping_enabled != self.tonemapping_enabled
            || fxaa_enabled != self.fxaa_enabled
        {
            self.texture_cache.update_dependencies();
            self.ao_enabled = ao_enabled;
            self.bloom_enabled = bloom_enabled;
            self.tonemapping_enabled = tonemapping_enabled;
            self.fxaa_enabled = fxaa_enabled;
            self.texture_cache_dirty = false;
        }

        // Random texture for AO
        if self.random_texture.is_some() && !ao_enabled {
            if let Some(tex) = self.random_texture.take() {
                tex.destroy()?;
            }
        }
        if self.random_texture.is_none() && ao_enabled {
            let length = 256 * 256 * 3;
            let mut random_bytes = vec![0u8; length];
            for (i, b) in random_bytes.iter_mut().enumerate() {
                *b = ((i * 1103515245 + 12345) & 0xFF) as u8;
            }
            let mut opts = TextureOptions::rgba(256, 256);
            opts.pixel_format = PixelFormat::Rgb;
            opts.sampler = Sampler {
                wrap_s: TextureWrap::Repeat,
                wrap_t: TextureWrap::Repeat,
                minification_filter: TextureMinificationFilter::Nearest,
                magnification_filter: TextureMagnificationFilter::Nearest,
                ..Default::default()
            };
            let tex = Texture::new(context, opts, Some(&random_bytes))?;
            let rc_tex = Rc::new(tex);
            self.ao.set_uniform("randomTexture", PostProcessUniformValue::Texture(rc_tex.clone()));
            self.random_texture = Some(rc_tex);
        }

        // Calculate dependencies
        let mut dependencies = HashMap::new();
        let mut prev_name = None;
        if ao_enabled {
            prev_name = get_composite_dependencies(&mut dependencies, &self.ao, prev_name.as_deref());
        }
        if bloom_enabled {
            prev_name = get_composite_dependencies(&mut dependencies, &self.bloom, prev_name.as_deref());
        }
        if tonemapping_enabled {
            prev_name = get_stage_dependencies(&mut dependencies, &self.tonemapping, prev_name.as_deref());
        }
        for &idx in &self.active_stages {
            match &self.stages[idx] {
                PostProcessStageItem::Stage(s) => {
                    prev_name = get_stage_dependencies(&mut dependencies, s, prev_name.as_deref());
                }
                PostProcessStageItem::Composite(c) => {
                    prev_name = get_composite_dependencies(&mut dependencies, c, prev_name.as_deref());
                }
            }
        }
        if fxaa_enabled {
            get_stage_dependencies(&mut dependencies, &self.fxaa, prev_name.as_deref());
        }

        // Flatten all active leaf PostProcessStages to update texture cache
        let mut all_leaf_stages: Vec<&PostProcessStage<G>> = Vec::new();
        if ao_enabled {
            for sub in self.ao.stages() {
                if let Some(s) = sub.as_stage() {
                    all_leaf_stages.push(s);
                } else if let Some(comp) = sub.as_composite() {
                    for leaf in comp.stages() {
                        if let Some(s) = leaf.as_stage() {
                            all_leaf_stages.push(s);
                        }
                    }
                }
            }
        }
        if bloom_enabled {
            for sub in self.bloom.stages() {
                if let Some(s) = sub.as_stage() {
                    all_leaf_stages.push(s);
                } else if let Some(comp) = sub.as_composite() {
                    for leaf in comp.stages() {
                        if let Some(s) = leaf.as_stage() {
                            all_leaf_stages.push(s);
                        }
                    }
                }
            }
        }
        if tonemapping_enabled {
            all_leaf_stages.push(&self.tonemapping);
        }
        for &idx in &self.active_stages {
            match &self.stages[idx] {
                PostProcessStageItem::Stage(s) => all_leaf_stages.push(s),
                PostProcessStageItem::Composite(c) => {
                    for sub in c.stages() {
                        if let Some(s) = sub.as_stage() {
                            all_leaf_stages.push(s);
                        }
                    }
                }
            }
        }
        if fxaa_enabled {
            all_leaf_stages.push(&self.fxaa);
        }

        self.texture_cache.update(context, &all_leaf_stages, &dependencies)?;

        // Connect framebuffers to stages
        Self::wire_stage_framebuffers(&mut self.ao, &self.texture_cache);
        Self::wire_stage_framebuffers(&mut self.bloom, &self.texture_cache);
        self.tonemapping.set_framebuffer(
            self.texture_cache.get_framebuffer(self.tonemapping.name()),
            self.texture_cache.get_output_texture(self.tonemapping.name()),
        );
        for stage in &mut self.stages {
            match stage {
                PostProcessStageItem::Stage(s) => {
                    s.set_framebuffer(
                        self.texture_cache.get_framebuffer(s.name()),
                        self.texture_cache.get_output_texture(s.name()),
                    );
                }
                PostProcessStageItem::Composite(c) => {
                    Self::wire_stage_framebuffers(c, &self.texture_cache);
                }
            }
        }
        self.fxaa.set_framebuffer(
            self.texture_cache.get_framebuffer(self.fxaa.name()),
            self.texture_cache.get_output_texture(self.fxaa.name()),
        );

        // Update shaders and viewport commands
        self.fxaa.update(context, use_log_depth)?;
        self.ao.update(context, use_log_depth)?;
        self.bloom.update(context, use_log_depth)?;
        self.tonemapping.update(context, use_log_depth)?;

        if self.auto_exposure_enabled {
            self.auto_exposure.update(context)?;
            let auto_tex = self.auto_exposure.output_texture();
            if let Some(tex) = auto_tex {
                self.tonemapping.set_uniform("autoExposure", PostProcessUniformValue::Texture(tex));
            }
        }

        for &idx in &self.active_stages {
            self.stages[idx].update(context, use_log_depth)?;
        }

        Ok(())
    }

    fn wire_stage_framebuffers(
        composite: &mut PostProcessStageComposite<G>,
        cache: &PostProcessStageTextureCache<G>,
    ) {
        for sub in composite.stages_mut() {
            match sub {
                PostProcessStageItem::Stage(s) => {
                    s.set_framebuffer(cache.get_framebuffer(s.name()), cache.get_output_texture(s.name()));
                }
                PostProcessStageItem::Composite(c) => {
                    Self::wire_stage_framebuffers(c, cache);
                }
            }
        }
    }

    pub fn execute(
        &mut self,
        context: &Context<G>,
        color_texture: &Rc<Texture<G>>,
        depth_texture: Option<&Rc<Texture<G>>>,
        id_texture: Option<&Rc<Texture<G>>>,
    ) -> Result<(), RuntimeError> {
        let ao_enabled = self.ao.enabled() && self.ao.is_supported(context);
        let bloom_enabled = self.bloom.enabled() && self.bloom.is_supported(context);
        let auto_exposure_enabled = self.auto_exposure_enabled;
        let tonemapping_enabled = self.tonemapping.enabled() && self.tonemapping.is_supported(context);
        let fxaa_enabled = self.fxaa.enabled() && self.fxaa.is_supported(context);

        if !fxaa_enabled
            && !ao_enabled
            && !bloom_enabled
            && !tonemapping_enabled
            && self.active_stages.is_empty()
        {
            return Ok(());
        }

        let resolver = |name: &str| -> Option<Rc<Texture<G>>> {
            self.texture_cache.get_output_texture(name)
        };

        let mut current_color = color_texture.clone();

        if ao_enabled && self.ao.ready() {
            self.ao.execute(context, &current_color, depth_texture, id_texture, Some(&resolver))?;
            if let Some(out) = self.ao.output_texture() {
                current_color = out;
            }
        }

        if bloom_enabled && self.bloom.ready() {
            self.bloom.execute(context, &current_color, depth_texture, id_texture, Some(&resolver))?;
            if let Some(out) = self.bloom.output_texture() {
                current_color = out;
            }
        }

        if auto_exposure_enabled && self.auto_exposure.ready() {
            self.auto_exposure.execute(context, &current_color)?;
        }

        if tonemapping_enabled && self.tonemapping.ready() {
            self.tonemapping.execute(context, &current_color, depth_texture, id_texture, Some(&resolver))?;
            if let Some(out) = self.tonemapping.output_texture() {
                current_color = out;
            }
        }

        for &idx in &self.active_stages {
            let stage = &mut self.stages[idx];
            stage.execute(context, &current_color, depth_texture, id_texture, Some(&resolver))?;
            if let Some(out) = stage.output_texture() {
                current_color = out;
            }
        }

        if fxaa_enabled && self.fxaa.ready() {
            self.fxaa.execute(context, &current_color, depth_texture, id_texture, Some(&resolver))?;
        }

        Ok(())
    }

    pub fn copy(
        &mut self,
        context: &Context<G>,
        target: Option<Rc<Framebuffer<G>>>,
        pass: &PassState<G>,
    ) -> Result<(), RuntimeError> {
        let output = match self.output_texture() {
            Some(t) => t,
            None => return Ok(()),
        };

        if self.copy_command.is_none() {
            self.copy_command = Some(context.create_viewport_quad_command(
                ShaderSource::new(crate::shaders::post_process_stages::PASS_THROUGH.source),
                ViewportCommandOptions::default(),
            )?);
        }

        let cmd = self.copy_command.as_mut().expect("created above");
        cmd.options.framebuffer = target;
        cmd.textures = vec![("colorTexture".into(), TextureBinding::Texture(output))];
        cmd.execute(context, pass)?;
        Ok(())
    }

    pub fn clear(&self, context: &Context<G>) -> Result<(), RuntimeError> {
        self.texture_cache.clear(context)?;
        if self.auto_exposure_enabled {
            self.auto_exposure.clear(context)?;
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.fxaa.destroy()?;
        self.ao.destroy()?;
        self.bloom.destroy()?;
        self.auto_exposure.destroy()?;
        self.tonemapping.destroy()?;
        for s in &mut self.stages {
            s.destroy()?;
        }
        self.texture_cache.destroy()?;
        if let Some(tex) = self.random_texture.take() {
            tex.destroy()?;
        }
        if let Some(cmd) = self.copy_command.take() {
            cmd.shader_program.destroy()?;
        }
        self.destroyed = true;
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
