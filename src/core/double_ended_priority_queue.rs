//! CesiumJS `DoubleEndedPriorityQueue`: a min-max heap with fast access to
//! both the least and the greatest element, and an optional maximum length.
//!
//! One deliberate difference: the trickle-down after a removal stops where a
//! min-max heap must, fixing a CesiumJS bug that can misplace the maximum.

use super::math::log2;

/// `comparator(a, b)` is negative when `a` is less, as in Cesium.
pub struct DoubleEndedPriorityQueue<T, F: Fn(&T, &T) -> f64> {
    array: Vec<T>,
    comparator: F,
    maximum_length: Option<usize>,
}

impl<T: Clone, F: Fn(&T, &T) -> f64 + Clone> Clone for DoubleEndedPriorityQueue<T, F> {
    fn clone(&self) -> Self {
        Self { array: self.array.clone(), comparator: self.comparator.clone(), maximum_length: self.maximum_length }
    }
}

impl<T, F: Fn(&T, &T) -> f64> DoubleEndedPriorityQueue<T, F> {
    pub fn new(comparator: F, maximum_length: Option<usize>) -> Self {
        Self { array: Vec::new(), comparator, maximum_length }
    }

    pub fn len(&self) -> usize {
        self.array.len()
    }

    pub fn is_empty(&self) -> bool {
        self.array.is_empty()
    }

    pub fn internal_array(&self) -> &[T] {
        &self.array
    }

    pub fn internal_array_mut(&mut self) -> &mut [T] {
        &mut self.array
    }

    pub fn maximum_length(&self) -> Option<usize> {
        self.maximum_length
    }

    /// Lowering the maximum removes minimum elements until the queue fits.
    pub fn set_maximum_length(&mut self, value: Option<usize>) {
        if let Some(max) = value {
            while self.array.len() > max {
                self.remove_minimum();
            }
        }
        self.maximum_length = value;
    }

    pub fn reset(&mut self) {
        self.array.clear();
    }

    /// Re-establishes the order after elements were changed in place.
    pub fn resort(&mut self) {
        for i in 0..self.array.len() {
            self.push_up(i);
        }
    }

    /// Adds an element. When the queue is full, the minimum is removed and
    /// returned, or `element` itself if it is not greater than the minimum.
    pub fn insert(&mut self, element: T) -> Option<T> {
        let mut removed = None;
        if let Some(max) = self.maximum_length {
            if max == 0 {
                return None;
            } else if self.array.len() == max {
                if (self.comparator)(&element, &self.array[0]) <= 0.0 {
                    return Some(element);
                }
                removed = self.remove_minimum();
            }
        }
        self.array.push(element);
        self.push_up(self.array.len() - 1);
        removed
    }

    pub fn remove_minimum(&mut self) -> Option<T> {
        let length = self.array.len();
        if length == 0 {
            return None;
        }
        let minimum = self.array.swap_remove(0);
        if length >= 2 {
            self.push_down(0);
        }
        Some(minimum)
    }

    pub fn remove_maximum(&mut self) -> Option<T> {
        let length = self.array.len();
        if length == 0 {
            return None;
        }
        if length <= 2 {
            return self.array.pop();
        }
        let maximum_index = if self.greater_than(1, 2) { 1 } else { 2 };
        let maximum = self.array.swap_remove(maximum_index);
        if length >= 4 {
            self.push_down(maximum_index);
        }
        Some(maximum)
    }

    pub fn get_minimum(&self) -> Option<&T> {
        self.array.first()
    }

    pub fn get_maximum(&self) -> Option<&T> {
        let length = self.array.len();
        if length == 0 {
            return None;
        }
        if length <= 2 {
            return self.array.last();
        }
        Some(&self.array[if self.greater_than(1, 2) { 1 } else { 2 }])
    }

    fn less_than(&self, a: usize, b: usize) -> bool {
        (self.comparator)(&self.array[a], &self.array[b]) < 0.0
    }

    fn greater_than(&self, a: usize, b: usize) -> bool {
        (self.comparator)(&self.array[a], &self.array[b]) > 0.0
    }

    fn on_min_level(index: usize) -> bool {
        (log2((index + 1) as f64).floor() as u64).is_multiple_of(2)
    }

    fn push_up(&mut self, mut index: usize) {
        if index == 0 {
            return;
        }
        let on_min_level = Self::on_min_level(index);
        let parent_index = (index - 1) / 2;
        let less_than_parent = self.less_than(index, parent_index);
        if less_than_parent != on_min_level {
            self.array.swap(index, parent_index);
            index = parent_index;
        }
        while index >= 3 {
            let grandparent_index = (index - 3) / 4;
            if self.less_than(index, grandparent_index) != less_than_parent {
                break;
            }
            self.array.swap(index, grandparent_index);
            index = grandparent_index;
        }
    }

    fn push_down(&mut self, mut index: usize) {
        let length = self.array.len();
        let on_min_level = Self::on_min_level(index);
        loop {
            let left_child_index = 2 * index + 1;
            if left_child_index >= length {
                break;
            }
            let mut target = left_child_index;
            let right_child_index = left_child_index + 1;
            if right_child_index < length {
                if self.less_than(right_child_index, target) == on_min_level {
                    target = right_child_index;
                }
                let grand_child_start = 2 * left_child_index + 1;
                let grand_child_count = length.saturating_sub(grand_child_start).min(4);
                for i in 0..grand_child_count {
                    let grand_child_index = grand_child_start + i;
                    if self.less_than(grand_child_index, target) == on_min_level {
                        target = grand_child_index;
                    }
                }
            }
            // Cesium keeps descending from `target` whether or not it swapped,
            // and also after swapping with a direct child, which is on the
            // other level: later removals can then report the wrong maximum
            // (CesiumJS 1.139 does). Only a swapped grandchild continues.
            if self.less_than(target, index) != on_min_level {
                break;
            }
            self.array.swap(target, index);
            if target == left_child_index || target == right_child_index {
                break;
            }
            let parent_of_grandchild_index = (target - 1) / 2;
            if self.greater_than(target, parent_of_grandchild_index) == on_min_level {
                self.array.swap(target, parent_of_grandchild_index);
            }
            index = target;
        }
    }
}
