//! CesiumJS `NodeTransformationProperty`.
use super::record_property::record_property;
use crate::core::TranslationRotationScale;
record_property! {
    NodeTransformationProperty;
    fields: [
        "translation" => translation, set_translation;
        "rotation" => rotation, set_rotation;
        "scale" => scale, set_scale;
    ];
    evaluate: |out| {
        let defaults = TranslationRotationScale::default();
        out.field("translation", defaults.translation)?;
        out.field("rotation", defaults.rotation)?;
        out.field("scale", defaults.scale)?;
    }
}
impl NodeTransformationProperty {
    pub fn new() -> Self {
        Self::default()
    }
}

pub(crate) fn node_factory(value: DynamicValue) -> Result<super::ComponentRef, RuntimeError> {
    let options = ["translation", "rotation", "scale"]
        .into_iter()
        .filter_map(|name| value.get(name).map(|value| (name.into(), PropertyInput::Value(value))));
    Ok(std::rc::Rc::new(NodeTransformationProperty::with_options(options)?))
}
