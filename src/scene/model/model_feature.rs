//! A stable feature handle. The feature table owns handles; handles retain a
//! weak link back to the table, so context pick registries cannot form cycles.
use super::model_feature_table::FeatureTableInner;
use crate::{
    core::{Color, RuntimeError},
    renderer::Gl,
    scene::metadata_value::MetadataValue,
};
use std::{
    any::Any,
    rc::{Rc, Weak},
};
type Result<T> = std::result::Result<T, RuntimeError>;
pub struct ModelFeature<G: Gl> {
    pub(crate) table: Weak<FeatureTableInner<G>>,
    pub(crate) id: usize,
}
impl<G: Gl> ModelFeature<G> {
    fn table(&self) -> Result<Rc<FeatureTableInner<G>>> {
        let t = self.table.upgrade().ok_or_else(|| RuntimeError::new("Feature table was released"))?;
        t.check()?;
        Ok(t)
    }
    pub fn feature_id(&self) -> usize {
        self.id
    }
    pub fn primitive(&self) -> Option<Rc<dyn Any>> {
        self.table.upgrade()?.model.as_ref()?.upgrade()
    }
    pub fn feature_table(&self) -> Option<super::ModelFeatureTable<G>> {
        Some(super::ModelFeatureTable { inner: self.table.upgrade()? })
    }
    pub fn show(&self) -> Result<bool> {
        self.table()?.batch.borrow().get_show(self.id)
    }
    pub fn set_show(&self, value: bool) -> Result<()> {
        self.table()?.batch.borrow_mut().set_show(self.id, value)
    }
    pub fn color(&self) -> Result<Color> {
        self.table()?.batch.borrow().get_color(self.id)
    }
    pub fn set_color(&self, value: Color) -> Result<()> {
        self.table()?.batch.borrow_mut().set_color(self.id, value)
    }
    pub fn has_property(&self, name: &str) -> Result<bool> {
        self.table()?.properties.borrow().has_property(self.id, name).map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn get_property(&self, name: &str) -> Result<Option<MetadataValue>> {
        self.table()?.properties.borrow_mut().get_property(self.id, name).map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn get_property_inherited(&self, name: &str) -> Result<Option<MetadataValue>> {
        let table = self.table()?;
        let mut properties = table.properties.borrow_mut();
        if properties.has_property_by_semantic(self.id, name) {
            properties.get_property_by_semantic(self.id, name)
        } else {
            properties.get_property(self.id, name)
        }
        .map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn get_property_ids(&self) -> Result<Vec<String>> {
        self.table()?.properties.borrow().get_property_ids(self.id).map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn set_property(&self, name: &str, value: MetadataValue) -> Result<()> {
        self.table()?
            .properties
            .borrow_mut()
            .set_property(self.id, name, value)
            .map_err(|e| RuntimeError::new(e.to_string()))
    }
}
