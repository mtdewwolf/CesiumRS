//! CesiumJS `subdivideArray`: splits an array into nearly equal parts.

/// `None` if `number_of_arrays` is zero (Cesium throws).
pub fn subdivide_array<T: Clone>(array: &[T], number_of_arrays: usize) -> Option<Vec<Vec<T>>> {
    if number_of_arrays < 1 {
        return None;
    }
    let mut result = Vec::new();
    let len = array.len();
    let mut remaining = number_of_arrays;
    let mut i = 0;
    while i < len {
        let size = (len - i).div_ceil(remaining.max(1));
        remaining = remaining.saturating_sub(1);
        result.push(array[i..(i + size).min(len)].to_vec());
        i += size;
    }
    Some(result)
}
