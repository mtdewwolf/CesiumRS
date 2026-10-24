//! CesiumJS `getMetadataProperty`: finds the property texture property a
//! metadata pick reads. Property attributes are not searched, since they
//! cannot be picked through the frame buffer.

use super::property_texture_property::PropertyTextureProperty;
use super::StructuralMetadata;
use crate::renderer::Gl;

/// The first property texture of class `class_name` that has
/// `property_name`.
pub fn get_metadata_property<'a, G: Gl>(
    structural_metadata: Option<&'a StructuralMetadata<G>>,
    class_name: &str,
    property_name: &str,
) -> Option<&'a PropertyTextureProperty<G>> {
    structural_metadata?
        .property_textures()
        .iter()
        .filter(|t| t.class().id == class_name)
        .find_map(|t| t.properties().get(property_name))
}
