//! CesiumJS `binarySearch`.

/// Index of `item` in the sorted `array`, or the bitwise complement of the
/// index where it would be inserted. `comparator` returns a negative,
/// zero, or positive number like a JS comparator.
pub fn binary_search<T, U>(array: &[T], item: &U, comparator: impl Fn(&T, &U) -> f64) -> isize {
    let mut low: isize = 0;
    let mut high: isize = array.len() as isize - 1;
    while low <= high {
        let i = (low + high) / 2;
        let comparison = comparator(&array[i as usize], item);
        if comparison < 0.0 {
            low = i + 1;
            continue;
        }
        if comparison > 0.0 {
            high = i - 1;
            continue;
        }
        return i;
    }
    !(high + 1)
}
