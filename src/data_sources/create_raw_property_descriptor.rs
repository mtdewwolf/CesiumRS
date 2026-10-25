use super::{ChangeEvent, PropertyChange, PropertyDescriptor};
pub fn create_raw_property_descriptor(
    name: impl Into<String>,
    configurable: bool,
    event: &ChangeEvent<PropertyChange>,
) -> PropertyDescriptor {
    PropertyDescriptor::new(name, configurable, None, event)
}
