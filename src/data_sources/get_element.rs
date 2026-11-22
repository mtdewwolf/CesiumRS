//! CesiumJS `getElement`: resolves an element either directly or by DOM element ID.
use crate::core::RuntimeError;

/// An element passed directly or referenced by string ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementInput<T> {
    Id(String),
    Element(T),
}

impl<T> From<String> for ElementInput<T> {
    fn from(id: String) -> Self {
        Self::Id(id)
    }
}

impl<T> From<&str> for ElementInput<T> {
    fn from(id: &str) -> Self {
        Self::Id(id.to_owned())
    }
}

/// If `element` is an ID string, resolves it through `lookup`; otherwise returns
/// the element directly.
pub fn get_element<T, F>(element: ElementInput<T>, lookup: F) -> Result<T, RuntimeError>
where
    F: FnOnce(&str) -> Option<T>,
{
    match element {
        ElementInput::Element(el) => Ok(el),
        ElementInput::Id(id) => lookup(&id)
            .ok_or_else(|| RuntimeError::new(format!("Element with id \"{id}\" does not exist in the document."))),
    }
}
