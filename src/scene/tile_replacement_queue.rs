//! CesiumJS `TileReplacementQueue`: the globe's least-recently-rendered
//! list of quadtree tiles. Rendered tiles move to the head; trimming walks
//! back from the tail, unloading tiles not rendered this frame until the
//! queue is small enough.
//!
//! CesiumJS links the tiles themselves through `replacementPrevious` and
//! `replacementNext`. Here the queue keeps the links, keyed by a tile id,
//! and the caller unloads tiles through a callback.

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Links<K> {
    previous: Option<K>,
    next: Option<K>,
}

impl<K> Default for Links<K> {
    fn default() -> Self {
        Self { previous: None, next: None }
    }
}

#[derive(Clone, Debug)]
pub struct TileReplacementQueue<K: Copy + Eq + Hash> {
    head: Option<K>,
    tail: Option<K>,
    count: usize,
    last_before_start_of_frame: Option<K>,
    links: HashMap<K, Links<K>>,
}

impl<K: Copy + Eq + Hash> Default for TileReplacementQueue<K> {
    fn default() -> Self {
        Self { head: None, tail: None, count: 0, last_before_start_of_frame: None, links: HashMap::new() }
    }
}

impl<K: Copy + Eq + Hash> TileReplacementQueue<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The most recently rendered tile.
    pub fn head(&self) -> Option<K> {
        self.head
    }

    /// The least recently rendered tile.
    pub fn tail(&self) -> Option<K> {
        self.tail
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn replacement_previous(&self, tile: K) -> Option<K> {
        self.links.get(&tile).and_then(|l| l.previous)
    }

    pub fn replacement_next(&self, tile: K) -> Option<K> {
        self.links.get(&tile).and_then(|l| l.next)
    }

    /// The last tile not rendered this frame, where trimming stops.
    pub fn last_before_start_of_frame(&self) -> Option<K> {
        self.last_before_start_of_frame
    }

    /// The tiles from most to least recently rendered.
    pub fn iter(&self) -> impl Iterator<Item = K> + '_ {
        std::iter::successors(self.head, move |&tile| self.replacement_next(tile))
    }

    /// Marks every tile in the queue as not yet rendered this frame.
    pub fn mark_start_of_render_frame(&mut self) {
        self.last_before_start_of_frame = self.head;
    }

    /// Unloads tiles from the tail until at most `maximum_tiles` remain,
    /// stopping at the tiles rendered this frame. `unload` frees a tile's
    /// resources and returns `true` when it is eligible for unloading; only
    /// those tiles leave the queue.
    pub fn trim_tiles(&mut self, maximum_tiles: usize, mut unload: impl FnMut(K) -> bool) {
        let mut tile_to_trim = self.tail;
        let mut keep_trimming = true;
        while keep_trimming && self.last_before_start_of_frame.is_some() && self.count > maximum_tiles {
            let Some(tile) = tile_to_trim else { break };
            // Stop after the last tile not rendered this frame.
            keep_trimming = Some(tile) != self.last_before_start_of_frame;
            let previous = self.replacement_previous(tile);
            if unload(tile) {
                self.remove(tile);
            }
            tile_to_trim = previous;
        }
    }

    fn remove(&mut self, item: K) {
        let Links { previous, next } = self.links.get(&item).copied().unwrap_or_default();
        if Some(item) == self.last_before_start_of_frame {
            self.last_before_start_of_frame = next;
        }
        if Some(item) == self.head {
            self.head = next;
        } else if let Some(previous) = previous {
            self.links.entry(previous).or_default().next = next;
        }
        if Some(item) == self.tail {
            self.tail = previous;
        } else if let Some(next) = next {
            self.links.entry(next).or_default().previous = previous;
        }
        self.links.remove(&item);
        self.count -= 1;
    }

    /// Moves `item` to the head, adding it if it is not in the queue.
    pub fn mark_tile_rendered(&mut self, item: K) {
        let head = self.head;
        if head == Some(item) {
            if Some(item) == self.last_before_start_of_frame {
                self.last_before_start_of_frame = self.replacement_next(item);
            }
            return;
        }
        self.count += 1;
        let Some(head) = head else {
            // No other tiles in the queue.
            self.links.insert(item, Links::default());
            self.head = Some(item);
            self.tail = Some(item);
            return;
        };
        let links = self.links.get(&item).copied().unwrap_or_default();
        if links.previous.is_some() || links.next.is_some() {
            // Already in the queue: unlink it from where it is.
            self.remove(item);
        }
        self.links.insert(item, Links { previous: None, next: Some(head) });
        self.links.entry(head).or_default().previous = Some(item);
        self.head = Some(item);
    }
}
