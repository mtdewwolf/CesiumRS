//! CesiumJS `getMetadataClassProperty`: a class property of a schema by
//! class and property name.

use super::{MetadataClassProperty, MetadataSchema};
use std::sync::Arc;

/// `None` when there is no schema, the schema id differs from a given
/// `schema_id`, or the class or property is missing.
pub fn get_metadata_class_property(
    schema: Option<&MetadataSchema>,
    schema_id: Option<&str>,
    class_name: &str,
    property_name: &str,
) -> Option<Arc<MetadataClassProperty>> {
    let schema = schema?;
    if schema_id.is_some_and(|id| schema.id.as_deref() != Some(id)) {
        return None;
    }
    schema.classes.get(class_name)?.properties.get(property_name).cloned()
}
