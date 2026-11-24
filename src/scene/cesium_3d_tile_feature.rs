//! A stable feature handle, shared by style evaluation, application code and picking.
use super::{cesium_3d_tile_batch_table::metadata_error, model::model_feature::style_value, scene::Result, *};
use crate::{
    core::{Color, RuntimeError},
    renderer::Gl,
};
use std::rc::{Rc, Weak};

/// Metadata is searched from the most specific entity to the tileset.
#[derive(Clone, Default)]
pub struct FeatureMetadata {
    pub content: Option<Rc<dyn MetadataEntity>>,
    pub tile: Option<Rc<dyn MetadataEntity>>,
    pub subtree: Option<Rc<dyn MetadataEntity>>,
    pub group: Option<Rc<dyn MetadataEntity>>,
    pub tileset: Option<Rc<dyn MetadataEntity>>,
}
impl FeatureMetadata {
    pub fn get_property_inherited(&self, name: &str) -> Result<Option<MetadataValue>> {
        for entity in [&self.content, &self.tile, &self.subtree, &self.group, &self.tileset].into_iter().flatten() {
            if entity.has_property_by_semantic(name) {
                return entity.get_property_by_semantic(name).map_err(metadata_error);
            }
            if entity.has_property(name) {
                return entity.get_property(name).map_err(metadata_error);
            }
        }
        Ok(None)
    }
}
pub struct Cesium3DTileFeature<G: Gl> {
    batch: Weak<Cesium3DTileBatchTable<G>>,
    id: usize,
}
impl<G: Gl + 'static> Cesium3DTileFeature<G> {
    pub(super) fn new(batch: Weak<Cesium3DTileBatchTable<G>>, id: usize) -> Self {
        Self { batch, id }
    }
    pub fn batch_table(&self) -> Result<Rc<Cesium3DTileBatchTable<G>>> {
        self.batch
            .upgrade()
            .filter(|b| !b.is_destroyed())
            .ok_or_else(|| RuntimeError::new("Feature content was released"))
    }
    pub fn feature_id(&self) -> usize {
        self.id
    }
    pub fn show(&self) -> Result<bool> {
        self.batch_table()?.get_show(self.id)
    }
    pub fn set_show(&self, v: bool) -> Result<()> {
        self.batch_table()?.set_show(self.id, v)
    }
    pub fn color(&self) -> Result<Color> {
        self.batch_table()?.get_color(self.id)
    }
    pub fn set_color(&self, v: Color) -> Result<()> {
        self.batch_table()?.set_color(self.id, v)
    }
    pub fn pick_color(&self) -> Result<Option<Color>> {
        Ok(self.batch_table()?.batch_texture().get_pick_color(self.id)?.map(|p| p.color))
    }
    pub fn get_property(&self, name: &str) -> Result<Option<MetadataValue>> {
        self.batch_table()?.get_property(self.id, name)
    }
    pub fn has_property(&self, name: &str) -> Result<bool> {
        self.batch_table()?.has_property(self.id, name)
    }
    pub fn get_property_ids(&self) -> Result<Vec<String>> {
        self.batch_table()?.get_property_ids(self.id)
    }
    pub fn set_property(&self, name: &str, value: MetadataValue) -> Result<()> {
        self.batch_table()?.set_property(self.id, name, value)
    }
    pub fn is_class(&self, name: &str) -> Result<bool> {
        self.batch_table()?.is_class(self.id, name)
    }
    pub fn get_exact_class_name(&self) -> Result<Option<String>> {
        self.batch_table()?.get_exact_class_name(self.id)
    }
    pub fn is_exact_class(&self, name: &str) -> Result<bool> {
        Ok(self.get_exact_class_name()?.as_deref() == Some(name))
    }
    pub fn get_property_inherited(&self, name: &str) -> Result<Option<MetadataValue>> {
        let table = self.batch_table()?;
        if table.has_property(self.id, name)? {
            return table.get_property(self.id, name);
        }
        let metadata = table.metadata.borrow();
        metadata.get_property_inherited(name)
    }
}
impl<G: Gl + 'static> StyleFeature for Cesium3DTileFeature<G> {
    fn property_inherited(&self, name: &str) -> Option<StyleValue> {
        self.get_property_inherited(name).ok().flatten().map(style_value)
    }
    fn is_class(&self, name: &str) -> bool {
        self.is_class(name).unwrap_or(false)
    }
    fn is_exact_class(&self, name: &str) -> bool {
        self.is_exact_class(name).unwrap_or(false)
    }
    fn exact_class_name(&self) -> Option<String> {
        self.get_exact_class_name().ok().flatten()
    }
}

/// Feature operations available through a heterogeneous content tree.
pub trait TileFeature: StyleFeature {
    fn feature_id(&self) -> usize;
    fn show(&self) -> Result<bool>;
    fn set_show(&self, v: bool) -> Result<()>;
    fn color(&self) -> Result<Color>;
    fn set_color(&self, v: Color) -> Result<()>;
    fn has_property(&self, name: &str) -> Result<bool>;
    fn get_property(&self, name: &str) -> Result<Option<MetadataValue>>;
    fn get_property_inherited(&self, name: &str) -> Result<Option<MetadataValue>>;
    fn set_property(&self, name: &str, v: MetadataValue) -> Result<()>;
    fn get_property_ids(&self) -> Result<Vec<String>>;
}
impl<G: Gl + 'static> TileFeature for Cesium3DTileFeature<G> {
    fn feature_id(&self) -> usize {
        Cesium3DTileFeature::feature_id(self)
    }
    fn show(&self) -> Result<bool> {
        Cesium3DTileFeature::show(self)
    }
    fn set_show(&self, v: bool) -> Result<()> {
        Cesium3DTileFeature::set_show(self, v)
    }
    fn color(&self) -> Result<Color> {
        Cesium3DTileFeature::color(self)
    }
    fn set_color(&self, v: Color) -> Result<()> {
        Cesium3DTileFeature::set_color(self, v)
    }
    fn has_property(&self, name: &str) -> Result<bool> {
        Cesium3DTileFeature::has_property(self, name)
    }
    fn get_property(&self, name: &str) -> Result<Option<MetadataValue>> {
        Cesium3DTileFeature::get_property(self, name)
    }
    fn get_property_inherited(&self, name: &str) -> Result<Option<MetadataValue>> {
        Cesium3DTileFeature::get_property_inherited(self, name)
    }
    fn set_property(&self, name: &str, v: MetadataValue) -> Result<()> {
        Cesium3DTileFeature::set_property(self, name, v)
    }
    fn get_property_ids(&self) -> Result<Vec<String>> {
        Cesium3DTileFeature::get_property_ids(self)
    }
}
impl<G: Gl + 'static> TileFeature for model::ModelFeature<G> {
    fn feature_id(&self) -> usize {
        model::ModelFeature::feature_id(self)
    }
    fn show(&self) -> Result<bool> {
        model::ModelFeature::show(self)
    }
    fn set_show(&self, v: bool) -> Result<()> {
        model::ModelFeature::set_show(self, v)
    }
    fn color(&self) -> Result<Color> {
        model::ModelFeature::color(self)
    }
    fn set_color(&self, v: Color) -> Result<()> {
        model::ModelFeature::set_color(self, v)
    }
    fn has_property(&self, name: &str) -> Result<bool> {
        model::ModelFeature::has_property(self, name)
    }
    fn get_property(&self, name: &str) -> Result<Option<MetadataValue>> {
        model::ModelFeature::get_property(self, name)
    }
    fn get_property_inherited(&self, name: &str) -> Result<Option<MetadataValue>> {
        model::ModelFeature::get_property_inherited(self, name)
    }
    fn set_property(&self, name: &str, v: MetadataValue) -> Result<()> {
        model::ModelFeature::set_property(self, name, v)
    }
    fn get_property_ids(&self) -> Result<Vec<String>> {
        model::ModelFeature::get_property_ids(self)
    }
}
