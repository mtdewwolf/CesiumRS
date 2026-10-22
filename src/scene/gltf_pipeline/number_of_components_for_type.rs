//! CesiumJS `numberOfComponentsForType`: components per accessor element.

/// `None` for an unknown type, where Cesium returns `undefined`.
pub fn number_of_components_for_type(accessor_type: &str) -> Option<usize> {
    Some(match accessor_type {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        _ => return None,
    })
}
