//! CesiumJS `AssociativeArray`: a map that also keeps its values in an array.

use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct AssociativeArray<T> {
    array: Vec<T>,
    hash: HashMap<String, T>,
}

impl<T: Clone + PartialEq> AssociativeArray<T> {
    pub fn new() -> Self {
        Self { array: Vec::new(), hash: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.array.len()
    }

    pub fn is_empty(&self) -> bool {
        self.array.is_empty()
    }

    /// The values in insertion order (a re-set value moves to the end).
    pub fn values(&self) -> &[T] {
        &self.array
    }

    pub fn contains(&self, key: &str) -> bool {
        self.hash.contains_key(key)
    }

    pub fn set(&mut self, key: &str, value: T) {
        if self.hash.get(key) != Some(&value) {
            self.remove(key);
            self.hash.insert(key.to_string(), value.clone());
            self.array.push(value);
        }
    }

    pub fn get(&self, key: &str) -> Option<&T> {
        self.hash.get(key)
    }

    pub fn remove(&mut self, key: &str) -> bool {
        match self.hash.remove(key) {
            Some(value) => {
                if let Some(i) = self.array.iter().position(|v| *v == value) {
                    self.array.remove(i);
                }
                true
            }
            None => false,
        }
    }

    pub fn remove_all(&mut self) {
        self.array.clear();
        self.hash.clear();
    }
}
