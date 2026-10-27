//! Cesium `PostProcessStageComposite`: a collection of stages executing logically together.
use crate::{
    core::{create_guid::create_guid, RuntimeError},
    renderer::*,
    scene::post_process_stage::{
        PostProcessStage, PostProcessUniformValue, SelectedFeature,
    },
};
use std::{collections::HashMap, rc::Rc};

/// An item contained in a [`PostProcessStageComposite`] or [`PostProcessStageCollection`].
#[allow(clippy::large_enum_variant)]
pub enum PostProcessStageItem<G: Gl> {
    Stage(PostProcessStage<G>),
    Composite(PostProcessStageComposite<G>),
}

impl<G: Gl> From<PostProcessStage<G>> for PostProcessStageItem<G> {
    fn from(s: PostProcessStage<G>) -> Self {
        Self::Stage(s)
    }
}

impl<G: Gl> From<PostProcessStageComposite<G>> for PostProcessStageItem<G> {
    fn from(c: PostProcessStageComposite<G>) -> Self {
        Self::Composite(c)
    }
}

impl<G: Gl> PostProcessStageItem<G> {
    pub fn name(&self) -> &str {
        match self {
            Self::Stage(s) => s.name(),
            Self::Composite(c) => c.name(),
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Stage(s) => s.enabled(),
            Self::Composite(c) => c.enabled(),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::Stage(s) => s.set_enabled(enabled),
            Self::Composite(c) => c.set_enabled(enabled),
        }
    }

    pub fn ready(&self) -> bool {
        match self {
            Self::Stage(s) => s.ready(),
            Self::Composite(c) => c.ready(),
        }
    }

    pub fn is_supported(&self, context: &Context<G>) -> bool {
        match self {
            Self::Stage(s) => s.is_supported(context),
            Self::Composite(c) => c.is_supported(context),
        }
    }

    pub fn output_texture(&self) -> Option<Rc<Texture<G>>> {
        match self {
            Self::Stage(s) => s.output_texture(),
            Self::Composite(c) => c.output_texture(),
        }
    }

    pub fn update(&mut self, context: &Context<G>, use_log_depth: bool) -> Result<(), RuntimeError> {
        match self {
            Self::Stage(s) => s.update(context, use_log_depth),
            Self::Composite(c) => c.update(context, use_log_depth),
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn execute(
        &mut self,
        context: &Context<G>,
        color_texture: &Rc<Texture<G>>,
        depth_texture: Option<&Rc<Texture<G>>>,
        id_texture: Option<&Rc<Texture<G>>>,
        resolver: Option<&dyn Fn(&str) -> Option<Rc<Texture<G>>>>,
    ) -> Result<(), RuntimeError> {
        match self {
            Self::Stage(s) => s.execute(context, color_texture, depth_texture, id_texture, resolver),
            Self::Composite(c) => c.execute(context, color_texture, depth_texture, id_texture, resolver),
        }
    }

    pub fn set_parent_selected(&mut self, parent_selected: Option<Vec<SelectedFeature>>) {
        match self {
            Self::Stage(s) => s.set_parent_selected(parent_selected),
            Self::Composite(c) => c.set_parent_selected(parent_selected),
        }
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        match self {
            Self::Stage(s) => s.destroy(),
            Self::Composite(c) => c.destroy(),
        }
    }

    pub fn is_destroyed(&self) -> bool {
        match self {
            Self::Stage(s) => s.is_destroyed(),
            Self::Composite(c) => c.is_destroyed(),
        }
    }

    pub fn as_stage(&self) -> Option<&PostProcessStage<G>> {
        match self {
            Self::Stage(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_stage_mut(&mut self) -> Option<&mut PostProcessStage<G>> {
        match self {
            Self::Stage(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_composite(&self) -> Option<&PostProcessStageComposite<G>> {
        match self {
            Self::Composite(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_composite_mut(&mut self) -> Option<&mut PostProcessStageComposite<G>> {
        match self {
            Self::Composite(c) => Some(c),
            _ => None,
        }
    }
}

/// Options to construct a [`PostProcessStageComposite`].
pub struct PostProcessStageCompositeOptions<G: Gl> {
    pub name: Option<String>,
    pub stages: Vec<PostProcessStageItem<G>>,
    pub input_previous_stage_texture: Option<bool>,
    pub uniforms: HashMap<String, PostProcessUniformValue<G>>,
}

impl<G: Gl> PostProcessStageCompositeOptions<G> {
    pub fn new(stages: Vec<PostProcessStageItem<G>>) -> Self {
        Self {
            name: None,
            stages,
            input_previous_stage_texture: None,
            uniforms: HashMap::new(),
        }
    }
}

/// A collection of [`PostProcessStage`]s or composites that execute together logically.
pub struct PostProcessStageComposite<G: Gl> {
    name: String,
    stages: Vec<PostProcessStageItem<G>>,
    input_previous_stage_texture: bool,
    uniforms: HashMap<String, PostProcessUniformValue<G>>,
    selected: Option<Vec<SelectedFeature>>,
    parent_selected: Option<Vec<SelectedFeature>>,
    selected_dirty: bool,
    destroyed: bool,
}

impl<G: Gl> PostProcessStageComposite<G> {
    pub fn new(options: PostProcessStageCompositeOptions<G>) -> Result<Self, RuntimeError> {
        if options.stages.is_empty() {
            return Err(RuntimeError::new("options.stages.len() must be greater than 0"));
        }
        let name = options.name.unwrap_or_else(create_guid);
        let input_previous_stage_texture = options.input_previous_stage_texture.unwrap_or(true);

        Ok(Self {
            name,
            stages: options.stages,
            input_previous_stage_texture,
            uniforms: options.uniforms,
            selected: None,
            parent_selected: None,
            selected_dirty: true,
            destroyed: false,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
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

    pub fn stages(&self) -> &[PostProcessStageItem<G>] {
        &self.stages
    }

    pub fn stages_mut(&mut self) -> &mut [PostProcessStageItem<G>] {
        &mut self.stages
    }

    pub fn input_previous_stage_texture(&self) -> bool {
        self.input_previous_stage_texture
    }

    pub fn ready(&self) -> bool {
        self.stages.iter().all(|s| s.ready())
    }

    pub fn enabled(&self) -> bool {
        self.stages.first().map(|s| s.enabled()).unwrap_or(false)
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        for s in &mut self.stages {
            s.set_enabled(enabled);
        }
    }

    pub fn selected(&self) -> Option<&[SelectedFeature]> {
        self.selected.as_deref()
    }

    pub fn set_selected(&mut self, selected: Option<Vec<SelectedFeature>>) {
        self.selected = selected;
        self.selected_dirty = true;
    }

    pub fn parent_selected(&self) -> Option<&[SelectedFeature]> {
        self.parent_selected.as_deref()
    }

    pub fn set_parent_selected(&mut self, parent_selected: Option<Vec<SelectedFeature>>) {
        self.parent_selected = parent_selected;
        self.selected_dirty = true;
    }

    pub fn uniforms(&self) -> &HashMap<String, PostProcessUniformValue<G>> {
        &self.uniforms
    }

    pub fn uniforms_mut(&mut self) -> &mut HashMap<String, PostProcessUniformValue<G>> {
        &mut self.uniforms
    }

    pub fn set_uniform(&mut self, name: impl Into<String>, value: impl Into<PostProcessUniformValue<G>>) {
        let name_str = name.into();
        let val = value.into();
        self.uniforms.insert(name_str.clone(), val.clone());
        // Forward to child stages if they contain a matching uniform name
        for s in &mut self.stages {
            match s {
                PostProcessStageItem::Stage(stage) => {
                    if stage.uniforms().contains_key(&name_str) {
                        stage.set_uniform(name_str.clone(), val.clone());
                    }
                }
                PostProcessStageItem::Composite(comp) => {
                    comp.set_uniform(name_str.clone(), val.clone());
                }
            }
        }
    }

    pub fn output_texture(&self) -> Option<Rc<Texture<G>>> {
        self.stages.last().and_then(|s| s.output_texture())
    }

    pub fn is_supported(&self, context: &Context<G>) -> bool {
        self.stages.iter().all(|s| s.is_supported(context))
    }

    fn combine_selected(&self) -> Option<Vec<SelectedFeature>> {
        let mut combined = Vec::new();
        if let Some(sel) = &self.selected {
            combined.extend(sel.iter().cloned());
        }
        if let Some(parent) = &self.parent_selected {
            combined.extend(parent.iter().cloned());
        }
        if combined.is_empty() {
            None
        } else {
            Some(combined)
        }
    }

    pub fn update(&mut self, context: &Context<G>, use_log_depth: bool) -> Result<(), RuntimeError> {
        let combined = if self.selected_dirty {
            self.combine_selected()
        } else {
            None
        };

        for stage in &mut self.stages {
            if self.selected_dirty {
                stage.set_parent_selected(combined.clone());
            }
            stage.update(context, use_log_depth)?;
        }

        self.selected_dirty = false;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    pub fn execute(
        &mut self,
        context: &Context<G>,
        color_texture: &Rc<Texture<G>>,
        depth_texture: Option<&Rc<Texture<G>>>,
        id_texture: Option<&Rc<Texture<G>>>,
        resolver: Option<&dyn Fn(&str) -> Option<Rc<Texture<G>>>>,
    ) -> Result<(), RuntimeError> {
        if !self.enabled() || !self.ready() {
            return Ok(());
        }

        if self.input_previous_stage_texture {
            let mut current_input = color_texture.clone();
            for stage in &mut self.stages {
                stage.execute(context, &current_input, depth_texture, id_texture, resolver)?;
                if let Some(out) = stage.output_texture() {
                    current_input = out;
                }
            }
        } else {
            for stage in &mut self.stages {
                stage.execute(context, color_texture, depth_texture, id_texture, resolver)?;
            }
        }

        Ok(())
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        for stage in &mut self.stages {
            stage.destroy()?;
        }
        self.destroyed = true;
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
