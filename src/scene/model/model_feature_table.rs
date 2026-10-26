//! Model feature identity, property access, styling and frame-driven textures.
use super::{
    batch_texture_pipeline_stage::BatchTextureState, model_statistics::BatchTextureStatistics,
    picking_pipeline_stage::PickBatchTextureSource, ModelFeature, StyleCommandsNeeded,
};
use crate::{
    core::{Color, RuntimeError},
    renderer::{Context, Gl, PickId},
    scene::{metadata_value::MetadataValue, BatchTexture, PropertyTable},
};
use std::{
    any::Any,
    cell::{Cell, Ref, RefCell},
    rc::{Rc, Weak},
};
type Result<T> = std::result::Result<T, RuntimeError>;
pub type FeatureStyleEvaluator<'a, G, T> = &'a dyn Fn(&ModelFeature<G>) -> Result<Option<T>>;
pub struct ModelFeatureStyle<'a, G: Gl> {
    pub color: Option<FeatureStyleEvaluator<'a, G, Color>>,
    pub show: Option<FeatureStyleEvaluator<'a, G, bool>>,
}
impl<G: Gl> Default for ModelFeatureStyle<'_, G> {
    fn default() -> Self {
        Self { color: None, show: None }
    }
}
type PipelineSources<T> = RefCell<Vec<Weak<RefCell<T>>>>;
pub(crate) struct FeatureTableInner<G: Gl> {
    pub properties: RefCell<PropertyTable>,
    pub batch: RefCell<BatchTexture<G>>,
    pub model: Option<Weak<dyn Any>>,
    batch_sources: PipelineSources<BatchTextureState<G>>,
    pick_sources: PipelineSources<PickBatchTextureSource<G>>,
    statistics: BatchTextureStatistics,
    features: Vec<Rc<ModelFeature<G>>>,
    style_commands: Cell<StyleCommandsNeeded>,
    style_dirty: Cell<bool>,
    destroyed: Cell<bool>,
}
impl<G: Gl> FeatureTableInner<G> {
    pub(crate) fn check(&self) -> Result<()> {
        if self.destroyed.get() {
            Err(RuntimeError::new("ModelFeatureTable is destroyed"))
        } else {
            Ok(())
        }
    }
}
impl<G: Gl> Drop for FeatureTableInner<G> {
    fn drop(&mut self) {
        let _ = self.batch.get_mut().destroy();
        self.statistics.byte_length.set(0);
        for source in self.batch_sources.get_mut().iter().filter_map(Weak::upgrade) {
            if let Ok(mut source) = source.try_borrow_mut() {
                source.batch_texture = None;
                source.default_texture = None;
            }
        }
        for source in self.pick_sources.get_mut().iter().filter_map(Weak::upgrade) {
            if let Ok(mut source) = source.try_borrow_mut() {
                source.pick_texture = None;
                source.default_texture = None;
            }
        }
    }
}

pub struct ModelFeatureTable<G: Gl> {
    pub(crate) inner: Rc<FeatureTableInner<G>>,
}
impl<G: Gl> Clone for ModelFeatureTable<G> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}
impl<G: Gl + 'static> ModelFeatureTable<G> {
    pub fn new(properties: PropertyTable, model: Option<Weak<dyn Any>>, max_texture_size: u32) -> Result<Self> {
        let count = properties.count();
        let batch = BatchTexture::new(count, max_texture_size)?;
        let statistics = BatchTextureStatistics::new(batch.id(), 0);
        Ok(Self {
            inner: Rc::new_cyclic(|table| FeatureTableInner {
                properties: RefCell::new(properties),
                batch: RefCell::new(batch),
                model,
                batch_sources: RefCell::new(vec![]),
                pick_sources: RefCell::new(vec![]),
                statistics,
                features: (0..count).map(|id| Rc::new(ModelFeature { table: table.clone(), id })).collect(),
                style_commands: Cell::new(StyleCommandsNeeded::AllOpaque),
                style_dirty: Cell::new(false),
                destroyed: Cell::new(false),
            }),
        })
    }
    pub fn features_length(&self) -> usize {
        self.inner.features.len()
    }
    pub fn get_feature(&self, id: usize) -> Result<Rc<ModelFeature<G>>> {
        self.inner.check()?;
        self.inner.features.get(id).cloned().ok_or_else(|| RuntimeError::new("Feature ID out of range"))
    }
    pub fn batch_texture(&self) -> Ref<'_, BatchTexture<G>> {
        self.inner.batch.borrow()
    }
    pub fn property_table(&self) -> Ref<'_, PropertyTable> {
        self.inner.properties.borrow()
    }
    pub fn style_commands_needed(&self) -> StyleCommandsNeeded {
        self.inner.style_commands.get()
    }
    pub fn style_commands_needed_dirty(&self) -> bool {
        self.inner.style_dirty.get()
    }
    pub fn is_destroyed(&self) -> bool {
        self.inner.destroyed.get()
    }
    pub fn get_show(&self, id: usize) -> Result<bool> {
        self.get_feature(id)?.show()
    }
    pub fn set_show(&self, id: usize, value: bool) -> Result<()> {
        self.get_feature(id)?.set_show(value)
    }
    pub fn get_color(&self, id: usize) -> Result<Color> {
        self.get_feature(id)?.color()
    }
    pub fn set_color(&self, id: usize, value: Color) -> Result<()> {
        self.get_feature(id)?.set_color(value)
    }
    pub fn get_pick_color(&self, id: usize) -> Result<Option<Ref<'_, PickId>>> {
        self.get_feature(id)?;
        Ok(Ref::filter_map(self.inner.batch.borrow(), |batch| batch.get_pick_color(id).ok().flatten()).ok())
    }
    pub fn set_statistics_counter(&self, counter: Option<Rc<Cell<usize>>>) -> Result<()> {
        self.inner.check()?;
        self.inner.batch.borrow_mut().statistics_byte_length = counter;
        Ok(())
    }
    pub fn batch_statistics(&self) -> BatchTextureStatistics {
        self.inner.statistics.clone()
    }
    pub fn batch_pipeline_source(&self) -> Result<Rc<RefCell<BatchTextureState<G>>>> {
        self.inner.check()?;
        let batch = self.inner.batch.borrow();
        let source = Rc::new(RefCell::new(BatchTextureState {
            features_length: self.features_length(),
            batch_texture: batch.batch_texture(),
            default_texture: batch.default_texture(),
            texture_step: batch.texture_step().unwrap_or(crate::core::Cartesian4::ZERO),
            texture_dimensions: batch.texture_dimensions().unwrap_or(crate::core::Cartesian2::ZERO),
        }));
        self.inner.batch_sources.borrow_mut().push(Rc::downgrade(&source));
        Ok(source)
    }
    pub fn picking_pipeline_source(&self) -> Result<Rc<RefCell<PickBatchTextureSource<G>>>> {
        self.inner.check()?;
        let batch = self.inner.batch.borrow();
        let source = Rc::new(RefCell::new(PickBatchTextureSource {
            pick_texture: batch.pick_texture(),
            default_texture: batch.default_texture(),
            ..Default::default()
        }));
        self.inner.pick_sources.borrow_mut().push(Rc::downgrade(&source));
        Ok(source)
    }
    fn refresh_pipeline_sources(&self, batch: &BatchTexture<G>) {
        self.inner.statistics.byte_length.set(batch.byte_length());
        self.inner.batch_sources.borrow_mut().retain(|weak| {
            let Some(source) = weak.upgrade() else { return false };
            let mut source = source.borrow_mut();
            source.batch_texture = batch.batch_texture();
            source.default_texture = batch.default_texture();
            true
        });
        self.inner.pick_sources.borrow_mut().retain(|weak| {
            let Some(source) = weak.upgrade() else { return false };
            let mut source = source.borrow_mut();
            source.pick_texture = batch.pick_texture();
            source.default_texture = batch.default_texture();
            true
        });
    }
    pub fn batch_texture_byte_length(&self) -> usize {
        self.inner.batch.borrow().byte_length()
    }
    pub fn set_all_show(&self, show: bool) -> Result<()> {
        self.inner.check()?;
        self.inner.batch.borrow_mut().set_all_show(show)
    }
    pub fn set_all_color(&self, color: Color) -> Result<()> {
        self.inner.check()?;
        self.inner.batch.borrow_mut().set_all_color(color)
    }
    pub fn update(&self, context: &Context<G>, pick_or_post_process: bool) -> Result<()> {
        self.update_with_pick_objects(context, pick_or_post_process, |id| self.get_feature(id))
    }
    /// Tile content may supply its own feature objects without changing table identity or textures.
    pub fn update_with_pick_objects<T: Any>(
        &self,
        context: &Context<G>,
        pick_or_post_process: bool,
        feature: impl Fn(usize) -> Result<Rc<T>>,
    ) -> Result<()> {
        self.inner.check()?;
        self.inner.style_dirty.set(false);
        let mut batch = self.inner.batch.borrow_mut();
        batch.update(context, pick_or_post_process, feature)?;
        self.refresh_pipeline_sources(&batch);
        let commands = StyleCommandsNeeded::get_style_commands_needed(
            self.features_length() as f64,
            batch.translucent_features_length() as f64,
        );
        if commands != self.inner.style_commands.replace(commands) {
            self.inner.style_dirty.set(true);
        }
        Ok(())
    }
    pub fn apply_style(&self, style: Option<&ModelFeatureStyle<'_, G>>) -> Result<()> {
        self.inner.check()?;
        for feature in &self.inner.features {
            let color = match style.and_then(|s| s.color) {
                Some(eval) => eval(feature)?.unwrap_or(Color::WHITE),
                None => Color::WHITE,
            };
            let show = match style.and_then(|s| s.show) {
                Some(eval) => eval(feature)?.unwrap_or(true),
                None => true,
            };
            feature.set_color(color)?;
            feature.set_show(show)?;
        }
        Ok(())
    }
    pub fn has_property(&self, id: usize, name: &str) -> Result<bool> {
        self.get_feature(id)?.has_property(name)
    }
    pub fn get_property(&self, id: usize, name: &str) -> Result<Option<MetadataValue>> {
        self.get_feature(id)?.get_property(name)
    }
    pub fn get_property_by_semantic(&self, id: usize, name: &str) -> Result<Option<MetadataValue>> {
        self.get_feature(id)?;
        self.inner
            .properties
            .borrow_mut()
            .get_property_by_semantic(id, name)
            .map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn has_property_by_semantic(&self, id: usize, name: &str) -> Result<bool> {
        self.get_feature(id)?;
        Ok(self.inner.properties.borrow().has_property_by_semantic(id, name))
    }
    pub fn get_property_ids(&self) -> Result<Vec<String>> {
        self.inner.check()?;
        self.inner.properties.borrow().get_property_ids(0).map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn set_property(&self, id: usize, name: &str, value: MetadataValue) -> Result<()> {
        self.get_feature(id)?.set_property(name, value)
    }
    pub fn is_class(&self, id: usize, name: &str) -> Result<bool> {
        self.inner.check()?;
        self.inner.properties.borrow().is_class(id, name).map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn is_exact_class(&self, id: usize, name: &str) -> Result<bool> {
        self.inner.check()?;
        self.inner.properties.borrow().is_exact_class(id, name).map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn get_exact_class_name(&self, id: usize) -> Result<Option<serde_json::Value>> {
        self.inner.check()?;
        self.inner
            .properties
            .borrow()
            .get_exact_class_name(id)
            .map(|v| v.cloned())
            .map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn destroy(&self) -> Result<()> {
        let mut batch = self.inner.batch.borrow_mut();
        batch.destroy()?;
        self.refresh_pipeline_sources(&batch);
        self.inner.destroyed.set(true);
        Ok(())
    }
}
