//! CesiumJS `Queue`: a first-in, first-out queue.

use std::collections::VecDeque;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Queue<T> {
    items: VecDeque<T>,
}

impl<T> Queue<T> {
    pub fn new() -> Self {
        Self { items: VecDeque::new() }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn enqueue(&mut self, item: T) {
        self.items.push_back(item);
    }

    pub fn dequeue(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    pub fn peek(&self) -> Option<&T> {
        self.items.front()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Sorts in place; stable, like `Array.prototype.sort`.
    pub fn sort(&mut self, compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.items.make_contiguous().sort_by(compare);
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

impl<T: PartialEq> Queue<T> {
    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }
}
