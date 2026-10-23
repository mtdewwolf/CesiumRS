//! CesiumJS `ImplicitSubtreeCache`: the loaded subtrees of an implicit
//! tileset, evicting the oldest leaf subtrees past a maximum count.
//!
//! Ancestors sort above their descendants, so a subtree is never evicted
//! before the subtrees below it; otherwise the oldest goes first.

use crate::core::DoubleEndedPriorityQueue;

use super::implicit_subtree::ImplicitSubtree;
use super::implicit_tile_coordinates::ImplicitTileCoordinates;
use super::metadata_value::{developer, MetadataError};

/// What the cache needs from a subtree.
pub trait CachedSubtree {
    fn implicit_coordinates(&self) -> &ImplicitTileCoordinates;
}

impl CachedSubtree for ImplicitSubtree {
    fn implicit_coordinates(&self) -> &ImplicitTileCoordinates {
        ImplicitSubtree::implicit_coordinates(self)
    }
}

impl<T: CachedSubtree> CachedSubtree for std::sync::Arc<T> {
    fn implicit_coordinates(&self) -> &ImplicitTileCoordinates {
        (**self).implicit_coordinates()
    }
}

impl<T: CachedSubtree> CachedSubtree for std::rc::Rc<T> {
    fn implicit_coordinates(&self) -> &ImplicitTileCoordinates {
        (**self).implicit_coordinates()
    }
}

/// A subtree and the order it was added in.
pub struct ImplicitSubtreeCacheNode<S> {
    pub subtree: S,
    pub stamp: u64,
}

type Comparator<S> = fn(&ImplicitSubtreeCacheNode<S>, &ImplicitSubtreeCacheNode<S>) -> f64;

pub struct ImplicitSubtreeCache<S: CachedSubtree> {
    maximum_subtree_count: usize,
    subtree_request_counter: u64,
    queue: DoubleEndedPriorityQueue<ImplicitSubtreeCacheNode<S>, Comparator<S>>,
}

impl<S: CachedSubtree> ImplicitSubtreeCache<S> {
    /// `maximum_subtree_count` 0 (the default) keeps every subtree.
    pub fn new(maximum_subtree_count: Option<usize>) -> Self {
        Self {
            maximum_subtree_count: maximum_subtree_count.unwrap_or(0),
            subtree_request_counter: 0,
            queue: DoubleEndedPriorityQueue::new(Self::comparator as Comparator<S>, None),
        }
    }

    /// Adds a subtree, then evicts the lowest-priority subtrees past the
    /// maximum, never the one just added. A subtree below the root needs its
    /// parent in the cache: Cesium throws a `DeveloperError` after adding it.
    pub fn add_subtree(&mut self, subtree: S) -> Result<(), MetadataError> {
        let stamp = self.subtree_request_counter;
        self.subtree_request_counter += 1;
        let coordinates = *subtree.implicit_coordinates();
        self.queue.insert(ImplicitSubtreeCacheNode { subtree, stamp });

        if coordinates.level > 0 {
            let parent = coordinates.get_parent_subtree_coordinates()?;
            if self.find(&parent).is_none() {
                return Err(developer("parent node needs to exist"));
            }
        }

        if self.maximum_subtree_count > 0 {
            while self.queue.len() > self.maximum_subtree_count {
                if self.queue.get_minimum().is_some_and(|node| node.stamp == stamp) {
                    break;
                }
                self.queue.remove_minimum();
            }
        }
        Ok(())
    }

    /// The cached subtree at these coordinates.
    pub fn find(&self, coordinates: &ImplicitTileCoordinates) -> Option<&S> {
        self.queue
            .internal_array()
            .iter()
            .find(|node| coordinates.is_equal(node.subtree.implicit_coordinates()))
            .map(|node| &node.subtree)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// The nodes in the queue's internal order.
    pub fn nodes(&self) -> &[ImplicitSubtreeCacheNode<S>] {
        self.queue.internal_array()
    }

    /// Ancestors rank higher; otherwise older subtrees rank lower.
    pub fn comparator(a: &ImplicitSubtreeCacheNode<S>, b: &ImplicitSubtreeCacheNode<S>) -> f64 {
        let (a_coord, b_coord) = (a.subtree.implicit_coordinates(), b.subtree.implicit_coordinates());
        if a_coord.is_ancestor(b_coord).unwrap_or(false) {
            return 1.0;
        }
        if b_coord.is_ancestor(a_coord).unwrap_or(false) {
            return -1.0;
        }
        a.stamp as f64 - b.stamp as f64
    }
}
