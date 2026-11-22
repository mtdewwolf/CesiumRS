//! Property descriptors whose values evaluate into named fields of a result object.
use super::{graphics::GraphicsProperties, BagValues, DynamicValue, PropertyInput};
use crate::core::{JulianDate, RuntimeError};

pub(crate) struct RecordEvaluation<'a> {
    properties: &'a GraphicsProperties,
    time: &'a JulianDate,
    fields: BagValues,
}

impl<'a> RecordEvaluation<'a> {
    pub(crate) fn new(properties: &'a GraphicsProperties, time: &'a JulianDate, result: Option<&DynamicValue>) -> Self {
        let fields = match result {
            Some(DynamicValue::Record(fields)) => fields.clone(),
            _ => Vec::new(),
        };
        Self { properties, time, fields }
    }
    pub(crate) fn read(&self, name: &str) -> Result<Option<DynamicValue>, RuntimeError> {
        self.evaluate(name, None)
    }
    fn evaluate(&self, name: &str, scratch: Option<&mut DynamicValue>) -> Result<Option<DynamicValue>, RuntimeError> {
        match self.properties.get(name) {
            Some(PropertyInput::Component(component)) => component.get_value(self.time, scratch),
            Some(PropertyInput::Value(value)) => Ok(Some(value)),
            None => Ok(None),
        }
    }
    pub(crate) fn set(&mut self, name: &str, value: Option<DynamicValue>) {
        if let Some((_, field)) = self.fields.iter_mut().find(|(key, _)| key == name) {
            *field = value;
        } else {
            self.fields.push((name.to_owned(), value));
        }
    }
    /// Read with the caller's existing output field as the child property result.
    pub(crate) fn field_as(
        &mut self,
        name: &str,
        output: &str,
        default: impl Into<DynamicValue>,
    ) -> Result<(), RuntimeError> {
        let mut scratch = self.fields.iter().find(|(key, _)| key == output).and_then(|(_, v)| v.clone());
        let value = self.evaluate(name, scratch.as_mut())?.filter(|v| !matches!(v, DynamicValue::Null));
        self.set(output, value.or_else(|| Some(default.into())));
        Ok(())
    }
    pub(crate) fn field(&mut self, name: &str, default: impl Into<DynamicValue>) -> Result<(), RuntimeError> {
        self.field_as(name, name, default)
    }
    /// Read without supplying a child result, as in Property.getValueOrDefault.
    pub(crate) fn field_without_result(
        &mut self,
        name: &str,
        default: impl Into<DynamicValue>,
    ) -> Result<(), RuntimeError> {
        let value = self.read(name)?.filter(|v| !matches!(v, DynamicValue::Null));
        self.set(name, value.or_else(|| Some(default.into())));
        Ok(())
    }
    pub(crate) fn get_mut(&mut self, name: &str) -> Option<&mut DynamicValue> {
        self.fields.iter_mut().find(|(key, _)| key == name)?.1.as_mut()
    }
    pub(crate) fn finish(self, result: Option<&mut DynamicValue>) -> Option<DynamicValue> {
        super::property::write_result(Some(DynamicValue::Record(self.fields)), result)
    }
}

macro_rules! record_property {
    ($ty:ident;
     fields: [$($name:literal => $get:ident, $set:ident);+ $(;)?];
     $(constant: [$($constant:literal),*];)?
     $(material: $material:literal;)?
     evaluate: |$out:ident| $body:block) => {
        use super::{graphics::GraphicsProperties, ChangeEvent, DefinitionChanged, DynamicValue, EntityComponent,
                    PropertyChange, PropertyInput};
        use crate::core::{JulianDate, RuntimeError};

        pub struct $ty {
            properties: GraphicsProperties,
        }
        impl Default for $ty {
            fn default() -> Self {
                Self { properties: GraphicsProperties::new(&[$($name),+]) }
            }
        }
        impl $ty {
            pub fn with_options(options: impl IntoIterator<Item = (String, PropertyInput)>) -> Result<Self, RuntimeError> {
                let property = Self::default();
                property.properties.merge_values(&options.into_iter().collect::<Vec<_>>(), &[$($name),+], &[])?;
                Ok(property)
            }
            pub fn get_value<'a>(
                &self,
                time: impl Into<Option<&'a JulianDate>>,
                result: Option<&mut DynamicValue>,
            ) -> Result<Option<DynamicValue>, RuntimeError> {
                let time = time.into().copied().unwrap_or_else(|| (super::property::system_clock())());
                EntityComponent::get_value(self, &time, result)
            }
            pub fn definition_changes(&self) -> &ChangeEvent<PropertyChange> {
                self.properties.changes()
            }
            $(
                pub fn $get(&self) -> Option<PropertyInput> {
                    self.properties.get($name)
                }
                pub fn $set(&self, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
                    self.properties.set($name, value)
                }
            )+
        }
        impl EntityComponent for $ty {
            fn as_any(&self) -> &dyn std::any::Any { self }
            fn definition_changed(&self) -> &DefinitionChanged { self.properties.definition_changed() }
            fn is_constant(&self) -> bool {
                record_property!(@constant self $(, [$($constant),*])?)
            }
            fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
                other.as_any().downcast_ref::<Self>().map_or(Ok(false), |o| self.properties.equals(&o.properties))
            }
            fn get_property(&self, name: &str) -> Option<PropertyInput> { self.properties.get(name) }
            $(fn get_type(&self, _: &JulianDate) -> Result<Option<String>, RuntimeError> {
                Ok(Some($material.to_owned()))
            })?
            fn get_value(&self, time: &JulianDate, result: Option<&mut DynamicValue>) -> Result<Option<DynamicValue>, RuntimeError> {
                let mut $out = super::record_property::RecordEvaluation::new(&self.properties, time, result.as_deref());
                $body
                Ok($out.finish(result))
            }
        }
    };
    (@constant $s:ident) => { $s.properties.is_constant() };
    (@constant $s:ident, [$($field:literal),*]) => {
        true $(&& $s.properties.get($field).and_then(|p| p.component()).is_none_or(|p| p.is_constant()))*
    };
}
pub(crate) use record_property;
