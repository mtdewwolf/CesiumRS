//! Legacy batch properties, hierarchy and per-feature color, visibility and pick textures.
use super::{
    batch_table_hierarchy::BinaryColumn,
    json_metadata_table::JsonColumn,
    metadata_table_property::ByteView,
    metadata_value::{json_entries, JsMap},
    scene::Result,
    *,
};
use crate::{
    core::{Color, RuntimeError},
    renderer::{Context, Gl},
};
use serde_json::Value;
use std::{
    cell::{Cell, Ref, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub(super) fn metadata_error(e: MetadataError) -> RuntimeError {
    RuntimeError::new(e.to_string())
}

enum Column {
    Json(JsonColumn),
    Binary(BinaryColumn),
}

pub struct Cesium3DTileBatchTable<G: Gl> {
    count: usize,
    columns: RefCell<JsMap<Column>>,
    hierarchy: RefCell<Option<BatchTableHierarchy>>,
    property_bytes: usize,
    batch: RefCell<BatchTexture<G>>,
    features: RefCell<HashMap<usize, Rc<Cesium3DTileFeature<G>>>>,
    destroyed: Cell<bool>,
    pub feature_properties_dirty: Cell<bool>,
    pub metadata: RefCell<FeatureMetadata>,
}
impl<G: Gl + 'static> Cesium3DTileBatchTable<G> {
    pub fn new(count: usize, json: Option<&Value>, binary: Option<&[u8]>, max_texture_size: u32) -> Result<Rc<Self>> {
        let batch = BatchTexture::new(count, max_texture_size)?;
        let body = binary.map(|buffer| ByteView { buffer, byte_offset: 0 });
        let hierarchy = json
            .and_then(|j| {
                j.get("HIERARCHY")
                    .filter(|v| !v.is_null())
                    .or_else(|| j.pointer("/extensions/3DTILES_batch_table_hierarchy"))
            })
            .filter(|v| !v.is_null())
            .map(|v| BatchTableHierarchy::new(v, body))
            .transpose()
            .map_err(metadata_error)?;
        let mut property_bytes = hierarchy.as_ref().map_or(0, BatchTableHierarchy::byte_length);
        let mut columns = JsMap::new();
        for (name, value) in json_entries(json) {
            if matches!(name.as_str(), "HIERARCHY" | "extensions" | "extras") {
                continue;
            }
            let col = if value.get("byteOffset").is_some_and(|v| !v.is_null()) {
                let binary = BinaryColumn::read(&name, value, body.as_ref(), count as f64).map_err(metadata_error)?;
                property_bytes += binary.byte_length();
                Column::Binary(binary)
            } else {
                Column::Json(JsonColumn::from_json(value))
            };
            columns.insert(name, col);
        }
        Ok(Rc::new(Self {
            count,
            columns: RefCell::new(columns),
            hierarchy: RefCell::new(hierarchy),
            property_bytes,
            batch: RefCell::new(batch),
            features: Default::default(),
            destroyed: Cell::new(false),
            feature_properties_dirty: Cell::new(false),
            metadata: Default::default(),
        }))
    }
    fn check(&self, id: usize) -> Result<()> {
        if self.destroyed.get() {
            Err(RuntimeError::new("Batch table was destroyed"))
        } else if id >= self.count {
            Err(RuntimeError::new("Feature ID out of range"))
        } else {
            Ok(())
        }
    }
    pub fn features_length(&self) -> usize {
        self.count
    }
    pub fn byte_length(&self) -> usize {
        self.property_bytes + self.batch.borrow().byte_length()
    }
    pub fn batch_texture(&self) -> Ref<'_, BatchTexture<G>> {
        self.batch.borrow()
    }
    pub fn get_feature(self: &Rc<Self>, id: usize) -> Result<Rc<Cesium3DTileFeature<G>>> {
        self.check(id)?;
        Ok(self
            .features
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| Rc::new(Cesium3DTileFeature::new(Rc::downgrade(self), id)))
            .clone())
    }
    pub fn has_property(&self, id: usize, name: &str) -> Result<bool> {
        self.check(id)?;
        if self.columns.borrow().get(name).is_some_and(|c| !matches!(c, Column::Json(JsonColumn::Null))) {
            return Ok(true);
        }
        self.hierarchy.borrow().as_ref().map_or(Ok(false), |h| h.has_property(id, name)).map_err(metadata_error)
    }
    pub fn get_property(&self, id: usize, name: &str) -> Result<Option<MetadataValue>> {
        self.check(id)?;
        match self.columns.borrow().get(name) {
            Some(Column::Binary(c)) => c.get(id),
            Some(Column::Json(c)) if c.is_defined() => Ok(c.get(id)),
            _ => self.hierarchy.borrow().as_ref().map_or(Ok(None), |h| h.get_property(id, name)),
        }
        .map_err(metadata_error)
    }
    pub fn set_property(&self, id: usize, name: &str, value: MetadataValue) -> Result<()> {
        self.check(id)?;
        let mut columns = self.columns.borrow_mut();
        match columns.get_mut(name) {
            Some(Column::Binary(c)) => c.set(id, &value).map_err(metadata_error)?,
            Some(Column::Json(c)) if c.is_defined() => c.set(id, value).map_err(metadata_error)?,
            _ => {
                if !self
                    .hierarchy
                    .borrow_mut()
                    .as_mut()
                    .map_or(Ok(false), |h| h.set_property(id, name, value.clone()))
                    .map_err(metadata_error)?
                {
                    columns.insert(name, Column::Json(JsonColumn::Array(Vec::new())));
                    if let Some(Column::Json(c)) = columns.get_mut(name) {
                        c.set(id, value).map_err(metadata_error)?;
                    }
                }
            }
        }
        self.feature_properties_dirty.set(true);
        Ok(())
    }
    pub fn get_property_ids(&self, id: usize) -> Result<Vec<String>> {
        self.check(id)?;
        let mut ids: Vec<_> = self.columns.borrow().keys().map(str::to_owned).collect();
        if let Some(h) = self.hierarchy.borrow().as_ref() {
            ids.extend(h.get_property_ids(id).map_err(metadata_error)?);
        }
        Ok(ids)
    }
    pub fn is_class(&self, id: usize, name: &str) -> Result<bool> {
        self.check(id)?;
        self.hierarchy.borrow().as_ref().map_or(Ok(false), |h| h.is_class(id, name)).map_err(metadata_error)
    }
    pub fn is_exact_class(&self, id: usize, name: &str) -> Result<bool> {
        Ok(self.get_exact_class_name(id)?.as_deref() == Some(name))
    }
    pub fn get_exact_class_name(&self, id: usize) -> Result<Option<String>> {
        self.check(id)?;
        self.hierarchy
            .borrow()
            .as_ref()
            .map_or(Ok(None), |h| h.get_class_name(id).map(|v| v.and_then(Value::as_str).map(str::to_owned)))
            .map_err(metadata_error)
    }
    pub fn get_pick_color(&self, id: usize) -> Result<Option<Color>> {
        self.check(id)?;
        Ok(self.batch.borrow().get_pick_color(id)?.map(|p| p.color))
    }
    pub fn get_show(&self, id: usize) -> Result<bool> {
        self.batch.borrow().get_show(id)
    }
    pub fn set_show(&self, id: usize, v: bool) -> Result<()> {
        self.batch.borrow_mut().set_show(id, v)
    }
    pub fn set_all_show(&self, v: bool) -> Result<()> {
        self.batch.borrow_mut().set_all_show(v)
    }
    pub fn get_color(&self, id: usize) -> Result<Color> {
        self.batch.borrow().get_color(id)
    }
    pub fn set_color(&self, id: usize, v: Color) -> Result<()> {
        self.batch.borrow_mut().set_color(id, v)
    }
    pub fn set_all_color(&self, v: Color) -> Result<()> {
        self.batch.borrow_mut().set_all_color(v)
    }
    pub fn apply_style(self: &Rc<Self>, style: Option<&Cesium3DTileStyle>) -> Result<()> {
        let Some(style) = style else {
            self.set_all_color(Color::WHITE)?;
            return self.set_all_show(true);
        };
        for id in 0..self.count {
            let f = self.get_feature(id)?;
            let color = style
                .color()
                .map(|s| s.evaluate_color(Some(f.as_ref())))
                .transpose()?
                .flatten()
                .unwrap_or(Color::WHITE);
            let show = style
                .show()
                .map(|s| s.evaluate(Some(f.as_ref())))
                .transpose()?
                .is_none_or(|v| matches!(v, StyleValue::Undefined | StyleValue::Null) || v.to_boolean());
            self.set_color(id, color)?;
            self.set_show(id, show)?;
        }
        Ok(())
    }
    pub fn update(self: &Rc<Self>, context: &Context<G>, pick: bool) -> Result<()> {
        self.batch.borrow_mut().update(context, pick, |id| self.get_feature(id))
    }
    pub fn destroy(&self) -> Result<()> {
        self.batch.borrow_mut().destroy()?;
        self.features.borrow_mut().clear();
        self.destroyed.set(true);
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::GlValue;

    struct DummyGl;
    impl Gl for DummyGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
            Ok([1, 1])
        }
        fn invoke(&self, _: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>> {
            Ok(GlValue::Null)
        }
    }

    #[test]
    fn set_property_updates_existing_json_column_before_hierarchy() {
        let json = serde_json::json!({
            "height": [10.0],
            "HIERARCHY": {
                "classes": [
                    {
                        "name": "Building",
                        "length": 1,
                        "instances": { "height": [99.0] }
                    }
                ],
                "instancesLength": 1,
                "classIds": [0]
            }
        });
        let table = Cesium3DTileBatchTable::<DummyGl>::new(1, Some(&json), None, 64).unwrap();
        assert_eq!(table.get_property(0, "height").unwrap(), Some(MetadataValue::Number(10.0)));
        table.set_property(0, "height", MetadataValue::Number(42.0)).unwrap();
        assert_eq!(table.get_property(0, "height").unwrap(), Some(MetadataValue::Number(42.0)));
    }
}
