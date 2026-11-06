//! CesiumJS `PrimitiveCollection`: an ordered, identity-based collection of
//! primitives that is itself a primitive.
//!
//! Cesium records membership on the primitive (`primitive._external._composites`,
//! keyed by the collection's GUID) and, with `countReferences`, a reference
//! count shared by every collection that holds it. A [`PrimitiveHandle`]
//! carries that record next to the primitive, and clones of a handle share
//! both, so one primitive can sit in several collections as in Cesium.
//! Membership follows Cesium exactly: adding a primitive twice records one
//! membership, so after one `remove` the collection no longer `contains` it
//! even though a copy remains in its list.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    scene::{Result, ScenePrimitive},
    Cesium3DTilePass, FrameState,
};
use crate::core::{Event, RuntimeError};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Cesium's `primitive._external`: the collections that hold the primitive
/// and the reference count kept by `countReferences` collections.
#[derive(Debug)]
struct External {
    id: u64,
    composites: HashSet<u64>,
    reference_count: Option<i64>,
}

thread_local! {
    /// The live membership record of each wrapped primitive allocation. In
    /// Cesium the record lives on the primitive object itself, so every handle
    /// that wraps the same allocation must share it. A record is kept alive
    /// only by handles, which also keep the allocation alive, so an address
    /// cannot be reused while its record is still reachable.
    static EXTERNALS: RefCell<HashMap<usize, Weak<RefCell<External>>>> = RefCell::new(HashMap::new());
}

fn allocation(primitive: &Rc<RefCell<dyn ScenePrimitive>>) -> usize {
    Rc::as_ptr(primitive) as *const () as usize
}

/// The record for `primitive`, shared with every live handle that wraps it.
fn external_for(primitive: &Rc<RefCell<dyn ScenePrimitive>>) -> Rc<RefCell<External>> {
    EXTERNALS.with(|map| {
        let mut map = map.borrow_mut();
        let key = allocation(primitive);
        if let Some(existing) = map.get(&key).and_then(Weak::upgrade) {
            return existing;
        }
        let external =
            Rc::new(RefCell::new(External { id: next_id(), composites: HashSet::new(), reference_count: None }));
        if map.len() >= 64 {
            map.retain(|_, record| record.strong_count() > 0);
        }
        map.insert(key, Rc::downgrade(&external));
        external
    })
}

/// A shared primitive. Identity is the primitive allocation: clones, and any
/// other handle wrapping the same `Rc`, share its membership record.
#[derive(Clone)]
pub struct PrimitiveHandle {
    primitive: Rc<RefCell<dyn ScenePrimitive>>,
    external: Rc<RefCell<External>>,
}

impl std::fmt::Debug for PrimitiveHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrimitiveHandle").field("id", &self.id()).finish_non_exhaustive()
    }
}

impl PartialEq for PrimitiveHandle {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl PrimitiveHandle {
    /// Wraps a primitive.
    pub fn new(primitive: impl ScenePrimitive + 'static) -> Self {
        Self::from_rc(Rc::new(RefCell::new(primitive)))
    }

    /// Wraps a shared primitive, keeping typed access through `primitive`
    /// (for example, to a nested [`PrimitiveCollection`]). Wrapping an
    /// allocation that a live handle already wraps returns a handle with the
    /// same identity, memberships and reference count.
    pub fn from_rc(primitive: Rc<RefCell<dyn ScenePrimitive>>) -> Self {
        let external = external_for(&primitive);
        Self { primitive, external }
    }

    /// A stable identifier, usable inside `Send` event listeners.
    pub fn id(&self) -> u64 {
        self.external.borrow().id
    }

    /// Whether both handles wrap the same primitive allocation.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        allocation(&self.primitive) == allocation(&other.primitive)
    }

    pub fn primitive(&self) -> &Rc<RefCell<dyn ScenePrimitive>> {
        &self.primitive
    }

    /// The count kept by `countReferences` collections, `None` until one adds it.
    pub fn reference_count(&self) -> Option<i64> {
        self.external.borrow().reference_count
    }

    fn destroy(&self) -> Result<()> {
        self.primitive.borrow_mut().destroy()
    }
}

/// Options for creating a [`PrimitiveCollection`].
#[derive(Clone, Copy, Debug)]
pub struct PrimitiveCollectionOptions {
    pub show: Option<bool>,
    pub destroy_primitives: Option<bool>,
    /// `countReferences`: destroy a removed primitive only once no
    /// reference-counting collection holds it.
    pub count_references: Option<bool>,
}

impl Default for PrimitiveCollectionOptions {
    fn default() -> Self {
        Self { show: Some(true), destroy_primitives: Some(true), count_references: Some(false) }
    }
}

/// A collection of primitives that itself implements [`ScenePrimitive`].
pub struct PrimitiveCollection {
    pub show: bool,
    pub destroy_primitives: bool,
    count_references: bool,
    guid: u64,
    primitives: Vec<PrimitiveHandle>,
    destroyed: bool,
    pub primitive_added: Event<PrimitiveHandle>,
    pub primitive_removed: Event<PrimitiveHandle>,
}

impl Default for PrimitiveCollection {
    fn default() -> Self {
        Self::new(PrimitiveCollectionOptions::default())
    }
}

impl PrimitiveCollection {
    pub fn new(options: PrimitiveCollectionOptions) -> Self {
        Self {
            show: options.show.unwrap_or(true),
            destroy_primitives: options.destroy_primitives.unwrap_or(true),
            count_references: options.count_references.unwrap_or(false),
            guid: next_id(),
            primitives: Vec::new(),
            destroyed: false,
            primitive_added: Event::new(),
            primitive_removed: Event::new(),
        }
    }

    pub fn length(&self) -> usize {
        self.primitives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    /// Adds `primitive` at `index`, or at the end. Raises `primitiveAdded`
    /// and returns the primitive.
    pub fn add(&mut self, primitive: PrimitiveHandle, index: Option<usize>) -> Result<PrimitiveHandle> {
        if let Some(index) = index {
            if index > self.primitives.len() {
                return Err(RuntimeError::new("index must be less than or equal to the number of primitives."));
            }
        }
        {
            let mut external = primitive.external.borrow_mut();
            external.composites.insert(self.guid);
            if self.count_references {
                external.reference_count = Some(external.reference_count.map_or(1, |count| count + 1));
            }
        }
        match index {
            Some(index) => self.primitives.insert(index, primitive.clone()),
            None => self.primitives.push(primitive.clone()),
        }
        self.primitive_added.raise_event(&primitive);
        Ok(primitive)
    }

    /// Releases one membership: drops the composite entry, decrements the
    /// reference count, destroys as Cesium's policy says, and raises
    /// `primitiveRemoved`.
    fn release(&mut self, primitive: &PrimitiveHandle) -> Result<()> {
        let destroy = {
            let mut external = primitive.external.borrow_mut();
            external.composites.remove(&self.guid);
            if self.count_references {
                external.reference_count = Some(external.reference_count.unwrap_or(0) - 1);
            }
            self.destroy_primitives
                && (!self.count_references || external.reference_count.is_some_and(|count| count <= 0))
        };
        if destroy {
            primitive.destroy()?;
        }
        self.primitive_removed.raise_event(primitive);
        Ok(())
    }

    /// Removes the first copy of `primitive`. Returns `false` when the
    /// collection does not contain it.
    pub fn remove(&mut self, primitive: &PrimitiveHandle) -> Result<bool> {
        if !self.contains(primitive) {
            return Ok(false);
        }
        let Some(index) = self.primitives.iter().position(|p| p.ptr_eq(primitive)) else {
            return Ok(false);
        };
        let removed = self.primitives.remove(index);
        self.release(&removed)?;
        Ok(true)
    }

    /// Removes `primitive` and destroys it even when `destroyPrimitives` is
    /// off.
    pub fn remove_and_destroy(&mut self, primitive: &PrimitiveHandle) -> Result<bool> {
        let removed = self.remove(primitive)?;
        if removed && !self.destroy_primitives {
            primitive.destroy()?;
        }
        Ok(removed)
    }

    /// Removes every primitive, in order. As in Cesium, the list is cleared
    /// only after every primitive has been released, so an error from a
    /// primitive's `destroy` leaves the list as it was.
    pub fn remove_all(&mut self) -> Result<()> {
        for primitive in self.primitives.clone() {
            self.release(&primitive)?;
        }
        self.primitives.clear();
        Ok(())
    }

    /// Whether this collection's membership is recorded on `primitive`.
    pub fn contains(&self, primitive: &PrimitiveHandle) -> bool {
        primitive.external.borrow().composites.contains(&self.guid)
    }

    fn index_of(&self, primitive: &PrimitiveHandle) -> Result<usize> {
        if !self.contains(primitive) {
            return Err(RuntimeError::new("primitive is not in this collection."));
        }
        self.primitives
            .iter()
            .position(|p| p.ptr_eq(primitive))
            .ok_or_else(|| RuntimeError::new("primitive is not in this collection."))
    }

    pub fn raise(&mut self, primitive: &PrimitiveHandle) -> Result<()> {
        let index = self.index_of(primitive)?;
        if index != self.primitives.len() - 1 {
            self.primitives.swap(index, index + 1);
        }
        Ok(())
    }

    pub fn raise_to_top(&mut self, primitive: &PrimitiveHandle) -> Result<()> {
        let index = self.index_of(primitive)?;
        if index != self.primitives.len() - 1 {
            let p = self.primitives.remove(index);
            self.primitives.push(p);
        }
        Ok(())
    }

    pub fn lower(&mut self, primitive: &PrimitiveHandle) -> Result<()> {
        let index = self.index_of(primitive)?;
        if index != 0 {
            self.primitives.swap(index, index - 1);
        }
        Ok(())
    }

    pub fn lower_to_bottom(&mut self, primitive: &PrimitiveHandle) -> Result<()> {
        let index = self.index_of(primitive)?;
        if index != 0 {
            let p = self.primitives.remove(index);
            self.primitives.insert(0, p);
        }
        Ok(())
    }

    /// The primitive at `index`; `None` past the end, as Cesium's `undefined`.
    pub fn get(&self, index: usize) -> Option<PrimitiveHandle> {
        self.primitives.get(index).cloned()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Each call visits the primitives present at its start.
    fn each(&self, mut f: impl FnMut(&mut dyn ScenePrimitive) -> Result<()>) -> Result<()> {
        for primitive in self.primitives.clone() {
            f(&mut *primitive.primitive.borrow_mut())?;
        }
        Ok(())
    }
}

impl ScenePrimitive for PrimitiveCollection {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.each(|p| p.pre_passes_update(frame))
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if !self.show {
            return Ok(());
        }
        self.each(|p| p.update(frame))
    }

    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.each(|p| p.post_passes_update(frame))
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<()> {
        self.each(|p| p.update_for_pass(frame, pass))
    }

    /// `removeAll`, then the collection is destroyed.
    fn destroy(&mut self) -> Result<()> {
        self.remove_all()?;
        self.destroyed = true;
        Ok(())
    }
}
