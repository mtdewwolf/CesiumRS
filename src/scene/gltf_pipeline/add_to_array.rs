//! CesiumJS `addToArray`: appends to a glTF array and returns the index.

use super::js;
use serde_json::Value;

/// Pushes `element` and returns its index. With `check_duplicates`, an
/// element already in the array (by JavaScript `===`, so only equal
/// primitives match) returns its existing index instead.
pub fn add_to_array(array: &mut Vec<Value>, element: Value, check_duplicates: bool) -> usize {
    if check_duplicates {
        if let Some(index) = js::index_of(array, Some(&element)) {
            return index;
        }
    }
    array.push(element);
    array.len() - 1
}
