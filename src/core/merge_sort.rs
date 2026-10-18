//! CesiumJS `mergeSort`: a stable sort with a comparator returning a number.

fn merge<T: Clone, F: Fn(&T, &T) -> f64>(array: &mut [T], compare: &F, start: usize, middle: usize, end: usize) {
    let left: Vec<T> = array[start..=middle].to_vec();
    let right: Vec<T> = array[middle + 1..=end].to_vec();
    let (mut i, mut j) = (0, 0);
    for slot in &mut array[start..=end] {
        // Cesium's test: take the left element unless the right one sorts strictly first.
        if i < left.len() && (j >= right.len() || compare(&left[i], &right[j]) <= 0.0) {
            *slot = left[i].clone();
            i += 1;
        } else if j < right.len() {
            *slot = right[j].clone();
            j += 1;
        }
    }
}

fn sort<T: Clone, F: Fn(&T, &T) -> f64>(array: &mut [T], compare: &F, start: usize, end: usize) {
    if start >= end {
        return;
    }
    let middle = (start + end) / 2;
    sort(array, compare, start, middle);
    sort(array, compare, middle + 1, end);
    merge(array, compare, start, middle, end);
}

/// Sorts `array` in place; `comparator(a, b)` is negative when `a` comes first.
pub fn merge_sort<T: Clone>(array: &mut [T], comparator: impl Fn(&T, &T) -> f64) {
    if array.len() > 1 {
        let end = array.len() - 1;
        sort(array, &comparator, 0, end);
    }
}
