//! CesiumJS `ManagedArray`: an array whose length can shrink without freeing
//! its storage. Slots past the length are `None`.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ManagedArray<T> {
    array: Vec<Option<T>>,
    length: usize,
}

impl<T> ManagedArray<T> {
    pub fn new(length: usize) -> Self {
        let mut array = Vec::with_capacity(length);
        array.resize_with(length, || None);
        Self { array, length }
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Shrinking clears the dropped slots; growing extends the storage.
    pub fn set_length(&mut self, length: usize) {
        if length < self.length {
            for slot in &mut self.array[length..self.length] {
                *slot = None;
            }
        } else if length > self.array.len() {
            self.array.resize_with(length, || None);
        }
        self.length = length;
    }

    /// The storage, which may be longer than the length.
    pub fn values(&self) -> &[Option<T>] {
        &self.array
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.array.get(index).and_then(|v| v.as_ref())
    }

    pub fn set(&mut self, index: usize, element: T) {
        if index >= self.length {
            self.set_length(index + 1);
        }
        self.array[index] = Some(element);
    }

    pub fn peek(&self) -> Option<&T> {
        self.length.checked_sub(1).and_then(|i| self.get(i))
    }

    pub fn push(&mut self, element: T) {
        let index = self.length;
        self.set(index, element);
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.length == 0 {
            return None;
        }
        let element = self.array[self.length - 1].take();
        self.set_length(self.length - 1);
        element
    }

    pub fn reserve(&mut self, length: usize) {
        if length > self.array.len() {
            self.array.resize_with(length, || None);
        }
    }

    pub fn resize(&mut self, length: usize) {
        self.set_length(length);
    }

    /// Cuts the storage to `length` (default: the current length).
    pub fn trim(&mut self, length: Option<usize>) {
        let length = length.unwrap_or(self.length);
        self.array.resize_with(length, || None);
    }
}
