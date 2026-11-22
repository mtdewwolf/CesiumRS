//! Ordered data sources and synchronous membership and ordering events.
use super::{ChangeEvent, DataSourceRef};
use crate::core::RuntimeError;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
};

#[derive(Clone)]
pub struct DataSourceMembership {
    pub collection: Rc<DataSourceCollection>,
    pub(crate) member: Rc<Member>,
    pub source: DataSourceRef,
}
pub struct DataSourceMove {
    pub source: DataSourceRef,
    pub new_index: usize,
    pub old_index: usize,
}

pub(crate) struct Member {
    pub source: DataSourceRef,
    pub removing: Cell<bool>,
}

#[derive(Default)]
pub struct DataSourceCollection {
    items: RefCell<Vec<Rc<Member>>>,
    added: ChangeEvent<DataSourceMembership>,
    removed: ChangeEvent<DataSourceMembership>,
    moved: ChangeEvent<DataSourceMove>,
    destroyed: Cell<bool>,
    generation: Cell<u64>,
}
impl DataSourceCollection {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }
    pub fn data_source_added(&self) -> &ChangeEvent<DataSourceMembership> {
        &self.added
    }
    pub fn data_source_removed(&self) -> &ChangeEvent<DataSourceMembership> {
        &self.removed
    }
    pub fn data_source_moved(&self) -> &ChangeEvent<DataSourceMove> {
        &self.moved
    }
    pub fn length(&self) -> usize {
        self.items.borrow().len()
    }
    pub fn get(&self, index: usize) -> Option<DataSourceRef> {
        self.items.borrow().get(index).map(|m| m.source.clone())
    }
    pub(crate) fn members(&self) -> Vec<Rc<Member>> {
        self.items.borrow().clone()
    }
    pub fn get_by_name(&self, name: &str) -> Vec<DataSourceRef> {
        self.members().into_iter().map(|m| m.source.clone()).filter(|s| s.name().as_deref() == Some(name)).collect()
    }
    pub fn index_of(&self, source: &DataSourceRef) -> Option<usize> {
        self.items.borrow().iter().position(|s| Rc::ptr_eq(&s.source, source))
    }
    pub fn contains(&self, source: &DataSourceRef) -> bool {
        self.index_of(source).is_some()
    }
    pub fn add(self: &Rc<Self>, source: DataSourceRef) -> Result<DataSourceRef, RuntimeError> {
        if self.is_destroyed() {
            return Err(RuntimeError::new("This collection was destroyed."));
        }
        let member = Rc::new(Member { source: source.clone(), removing: Cell::new(false) });
        self.items.borrow_mut().push(member.clone());
        self.added.raise(DataSourceMembership { member, collection: self.clone(), source: source.clone() });
        Ok(source)
    }
    /// Resolve a pending source, retaining its original collection generation.
    /// A clear while it is pending suppresses insertion but still returns the source.
    pub fn add_async<'a>(
        self: &'a Rc<Self>,
        pending: impl Future<Output = Result<DataSourceRef, RuntimeError>> + 'a,
    ) -> impl Future<Output = Result<DataSourceRef, RuntimeError>> + 'a {
        let generation = self.generation.get();
        async move {
            let source = pending.await?;
            if self.generation.get() == generation {
                self.add(source)
            } else {
                Ok(source)
            }
        }
    }
    pub fn remove(self: &Rc<Self>, source: &DataSourceRef, destroy: bool) -> bool {
        let Some(index) = self.index_of(source) else {
            return false;
        };
        let member = self.items.borrow_mut().remove(index);
        if !member.removing.replace(true) {
            self.notify_removed(&member, destroy);
        }
        true
    }
    fn notify_removed(self: &Rc<Self>, member: &Rc<Member>, destroy: bool) {
        self.removed.raise(DataSourceMembership {
            member: member.clone(),
            collection: self.clone(),
            source: member.source.clone(),
        });
        if destroy {
            member.source.destroy();
        }
    }
    pub fn remove_all(self: &Rc<Self>, destroy: bool) {
        // Cesium keeps the old membership visible to removeAll listeners.
        self.generation.set(self.generation.get().wrapping_add(1));
        let batch = self.items.borrow().clone();
        for member in &batch {
            if !member.removing.replace(true) {
                self.notify_removed(member, destroy);
            }
        }
        self.items.borrow_mut().retain(|m| !m.removing.get());
    }
    fn move_to(&self, source: &DataSourceRef, target: impl FnOnce(usize, usize) -> usize) -> Result<(), RuntimeError> {
        let old_index =
            self.index_of(source).ok_or_else(|| RuntimeError::new("dataSource is not in this collection."))?;
        let new_index = target(old_index, self.length() - 1);
        if new_index != old_index {
            {
                let mut items = self.items.borrow_mut();
                let item = items.remove(old_index);
                items.insert(new_index, item);
            }
            self.moved.raise(DataSourceMove { source: source.clone(), old_index, new_index });
        }
        Ok(())
    }
    pub fn raise(&self, source: &DataSourceRef) -> Result<(), RuntimeError> {
        self.move_to(source, |i, last| (i + 1).min(last))
    }
    pub fn lower(&self, source: &DataSourceRef) -> Result<(), RuntimeError> {
        self.move_to(source, |i, _| i.saturating_sub(1))
    }
    pub fn raise_to_top(&self, source: &DataSourceRef) -> Result<(), RuntimeError> {
        self.move_to(source, |_, last| last)
    }
    pub fn lower_to_bottom(&self, source: &DataSourceRef) -> Result<(), RuntimeError> {
        self.move_to(source, |_, _| 0)
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }
    pub fn destroy(self: &Rc<Self>) {
        if !self.destroyed.replace(true) {
            self.remove_all(true);
        }
    }
}
