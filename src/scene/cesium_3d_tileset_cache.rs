//! CesiumJS `Cesium3DTilesetCache`: LRU cache using a doubly linked list with a sentinel node
//! separating selected and unselected tiles in the current frame.

use crate::core::doubly_linked_list::{DoublyLinkedList, NodeId};

/// Stores loaded tiles in LRU order with sentinel-partitioned selection tracking.
#[derive(Clone, Debug)]
pub struct Cesium3DTilesetCache<T = usize> {
    list: DoublyLinkedList<Option<T>>,
    sentinel: NodeId,
    trim_tiles: bool,
}

impl<T> Default for Cesium3DTilesetCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Cesium3DTilesetCache<T> {
    pub fn new() -> Self {
        let mut list = DoublyLinkedList::new();
        let sentinel = list.add(None);
        Self { list, sentinel, trim_tiles: false }
    }

    /// Number of items (excluding sentinel).
    pub fn len(&self) -> usize {
        self.list.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Move sentinel node to the tail so, at the start of the frame, all tiles
    /// may potentially be unloaded. Tiles are moved to the right of the sentinel
    /// when touched/selected.
    pub fn reset(&mut self) {
        if let Some(tail) = self.list.tail() {
            self.list.splice(tail, self.sentinel);
        }
    }

    /// Moves a node to the right of the sentinel (marked as selected this frame).
    pub fn touch(&mut self, node: NodeId) {
        self.list.splice(self.sentinel, node);
    }

    /// Adds a tile item to the cache and returns its node ID.
    pub fn add(&mut self, item: T) -> NodeId {
        self.list.add(Some(item))
    }

    /// Unloads a single tile node.
    pub fn unload_tile<F: FnOnce(T)>(&mut self, node: NodeId, unload_callback: F) {
        if let Some(item) = self.list.remove(node) {
            unload_callback(item);
        }
    }

    /// Traverses the LRU sublist to the left of the sentinel, unloading tiles while
    /// `should_unload(trim_tiles)` returns true.
    pub fn unload_tiles<P: FnMut(bool) -> bool, F: FnMut(T)>(&mut self, mut should_unload: P, mut unload_callback: F) {
        let trim = self.trim_tiles;
        self.trim_tiles = false;

        let mut current = self.list.head();
        while let Some(node) = current {
            if node == self.sentinel {
                break;
            }
            if !should_unload(trim) {
                break;
            }
            let next = self.list.next(node);
            if let Some(item) = self.list.remove(node) {
                unload_callback(item);
            }
            current = next;
        }
    }

    /// Requests cache trimming on next unload pass.
    pub fn trim(&mut self) {
        self.trim_tiles = true;
    }

    pub fn sentinel(&self) -> NodeId {
        self.sentinel
    }
}
