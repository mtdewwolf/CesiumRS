//! Typed counterparts of JavaScript picked objects and mutable show attributes.
use super::{MetadataClassProperty, MetadataValue, PickMetadataShader, PickedObject};
use crate::core::RuntimeError;
use std::{cell::Cell, collections::HashMap, rc::Rc, sync::Arc};
#[derive(Clone, Debug)]
pub struct PickedMetadataInfo {
    pub shader: Rc<PickMetadataShader>,
    pub class_property: Arc<MetadataClassProperty>,
    pub value_transform: Option<(MetadataValue, MetadataValue)>,
}
impl PickedMetadataInfo {
    pub fn new(
        schema_id: Option<String>,
        class_name: String,
        property_name: String,
        class_property: Arc<MetadataClassProperty>,
        value_transform: Option<(MetadataValue, MetadataValue)>,
    ) -> Result<Self, RuntimeError> {
        let shader = Rc::new(PickMetadataShader::new(
            schema_id,
            class_name,
            property_name,
            &class_property,
            value_transform.clone(),
        )?);
        Ok(Self { shader, class_property, value_transform })
    }
}
/// Register this object with Context.create_pick_id. Attach its show handles
/// to the corresponding SceneCommand visibility fields so drill picks can
/// hide a geometry instance, feature, or whole primitive on subsequent passes.
pub struct ScenePickObject {
    pub primitive: PickedObject,
    pub id: Option<MetadataValue>,
    pub primitive_show: Rc<Cell<bool>>,
    pub instance_show: Option<Rc<Cell<bool>>>,
    pub feature_show: Option<Rc<Cell<bool>>>,
    pub metadata: HashMap<(Option<String>, String, String), PickedMetadataInfo>,
}
impl ScenePickObject {
    pub fn new(primitive: PickedObject, primitive_show: Rc<Cell<bool>>) -> Self {
        Self { primitive, id: None, primitive_show, instance_show: None, feature_show: None, metadata: HashMap::new() }
    }
    pub fn show_handle(&self) -> Rc<Cell<bool>> {
        self.instance_show.as_ref().or(self.feature_show.as_ref()).unwrap_or(&self.primitive_show).clone()
    }
    pub fn show_handles(&self) -> Vec<Rc<Cell<bool>>> {
        let mut handles = Vec::new();
        if let Some(show) = &self.instance_show {
            handles.push(show.clone());
        }
        if let Some(show) = &self.feature_show {
            handles.push(show.clone());
        }
        if handles.is_empty() {
            handles.push(self.primitive_show.clone());
        }
        handles
    }
}
#[derive(Clone)]
pub struct RayPickResult {
    pub object: Option<PickedObject>,
    pub position: Option<crate::core::Cartesian3>,
    pub exclude: bool,
}
#[derive(Clone)]
pub enum PickExclusion {
    Object(PickedObject),
    Primitive(PickedObject),
    Id(MetadataValue),
}
impl PickExclusion {
    pub fn matches(&self, object: &PickedObject) -> bool {
        match self {
            Self::Object(excluded) if Rc::ptr_eq(excluded, object) => true,
            _ => {
                let Some(picked) = object.downcast_ref::<ScenePickObject>() else {
                    return false;
                };
                match self {
                    Self::Object(excluded) | Self::Primitive(excluded) => Rc::ptr_eq(excluded, &picked.primitive),
                    Self::Id(id) => picked.id.as_ref().is_some_and(|value| value.strict_equals(id)),
                }
            }
        }
    }
}
