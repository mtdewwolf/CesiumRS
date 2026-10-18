//! CesiumJS `Heap`: an array-backed binary heap ordered by a comparator,
//! with an optional maximum length.

use std::cmp::Ordering;

pub struct Heap<T, F: Fn(&T, &T) -> Ordering> {
    array: Vec<T>,
    comparator: F,
    maximum_length: Option<usize>,
}

impl<T, F: Fn(&T, &T) -> Ordering> Heap<T, F> {
    /// The element that compares least is on top.
    pub fn new(comparator: F) -> Self {
        Self { array: Vec::new(), comparator, maximum_length: None }
    }

    pub fn len(&self) -> usize {
        self.array.len()
    }

    pub fn is_empty(&self) -> bool {
        self.array.is_empty()
    }

    /// The heap in its internal order.
    pub fn internal_array(&self) -> &[T] {
        &self.array
    }

    pub fn internal_array_mut(&mut self) -> &mut [T] {
        &mut self.array
    }

    pub fn maximum_length(&self) -> Option<usize> {
        self.maximum_length
    }

    /// Lowering the maximum drops elements from the end of the array.
    pub fn set_maximum_length(&mut self, value: Option<usize>) {
        if let Some(max) = value {
            self.array.truncate(max);
        }
        self.maximum_length = value;
    }

    pub fn reserve(&mut self, length: usize) {
        self.array.reserve(length.saturating_sub(self.array.len()));
    }

    fn less(&self, a: usize, b: usize) -> bool {
        (self.comparator)(&self.array[a], &self.array[b]) == Ordering::Less
    }

    /// Restores the heap property below `index`.
    pub fn heapify(&mut self, mut index: usize) {
        let length = self.array.len();
        loop {
            let right = 2 * (index + 1);
            let left = right - 1;
            let mut candidate = if left < length && self.less(left, index) { left } else { index };
            if right < length && self.less(right, candidate) {
                candidate = right;
            }
            if candidate == index {
                break;
            }
            self.array.swap(candidate, index);
            index = candidate;
        }
    }

    /// Re-establishes the order after elements were changed in place.
    pub fn resort(&mut self) {
        let length = self.array.len();
        for i in (0..=length.div_ceil(2)).rev() {
            self.heapify(i);
        }
    }

    /// Adds an element. Past the maximum length, the element that falls off
    /// the end of the array is removed and returned.
    pub fn insert(&mut self, element: T) -> Option<T> {
        self.array.push(element);
        let mut index = self.array.len() - 1;
        while index != 0 {
            let parent = (index - 1) / 2;
            if self.less(index, parent) {
                self.array.swap(index, parent);
                index = parent;
            } else {
                break;
            }
        }
        match self.maximum_length {
            Some(max) if self.array.len() > max => self.array.pop(),
            _ => None,
        }
    }

    /// Removes the element at `index` (the top for 0).
    pub fn pop(&mut self, index: usize) -> Option<T> {
        if index >= self.array.len() {
            return None;
        }
        let last = self.array.len() - 1;
        self.array.swap(index, last);
        let root = self.array.pop();
        self.heapify(index);
        root
    }
}
