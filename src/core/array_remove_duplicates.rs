//! CesiumJS `arrayRemoveDuplicates`: drops consecutive values that are equal
//! within `EPSILON10`.

use super::math::EPSILON10;

/// Removes consecutive near-duplicates (and, with `wrap_around`, a last value
/// equal to the first). `removed_indices` receives the indices dropped, in
/// Cesium's order.
pub fn array_remove_duplicates<T: Clone>(
    values: &[T],
    equals_epsilon: impl Fn(&T, &T, f64) -> bool,
    wrap_around: bool,
    mut removed_indices: Option<&mut Vec<usize>>,
) -> Vec<T> {
    let length = values.len();
    if length < 2 {
        return values.to_vec();
    }
    let mut v0 = &values[0];
    let mut cleaned: Option<Vec<T>> = None;
    let mut last_clean_index = 0;
    let mut removed_index_lci = 0;
    for (i, v1) in values.iter().enumerate().skip(1) {
        if equals_epsilon(v0, v1, EPSILON10) {
            if cleaned.is_none() {
                cleaned = Some(values[..i].to_vec());
                last_clean_index = i - 1;
                removed_index_lci = 0;
            }
            if let Some(r) = removed_indices.as_deref_mut() {
                r.push(i);
            }
        } else {
            if let Some(c) = cleaned.as_mut() {
                c.push(v1.clone());
                last_clean_index = i;
                if let Some(r) = removed_indices.as_deref() {
                    removed_index_lci = r.len();
                }
            }
            v0 = v1;
        }
    }
    if wrap_around && equals_epsilon(&values[0], &values[length - 1], EPSILON10) {
        if let Some(r) = removed_indices {
            if cleaned.is_some() {
                r.insert(removed_index_lci, last_clean_index);
            } else {
                r.push(length - 1);
            }
        }
        match cleaned.as_mut() {
            Some(c) => {
                c.pop();
            }
            None => cleaned = Some(values[..length - 1].to_vec()),
        }
    }
    cleaned.unwrap_or_else(|| values.to_vec())
}
