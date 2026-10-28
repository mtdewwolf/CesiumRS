//! CesiumJS `OrderedGroundPrimitiveCollection`. Stable numeric handles replace
//! JavaScript primitive identity. Removing without destruction transfers ownership.
use super::{
    scene::{Result, ScenePrimitive},
    Cesium3DTilePass, FrameState,
};
use crate::core::RuntimeError;
struct Entry {
    id: u64,
    z: f64,
    primitive: Box<dyn ScenePrimitive>,
}
pub struct OrderedGroundPrimitiveCollection {
    pub show: bool,
    entries: Vec<Entry>,
    next: u64,
    destroyed: bool,
}
impl Default for OrderedGroundPrimitiveCollection {
    fn default() -> Self {
        Self { show: true, entries: vec![], next: 0, destroyed: false }
    }
}
impl OrderedGroundPrimitiveCollection {
    fn check(&self) -> Result<()> {
        if self.destroyed {
            Err(RuntimeError::new("Collection was destroyed"))
        } else {
            Ok(())
        }
    }
    pub fn length(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn contains(&self, id: u64) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }
    pub fn add(&mut self, primitive: Box<dyn ScenePrimitive>, z: f64) -> Result<u64> {
        self.check()?;
        if !z.is_finite() {
            return Err(RuntimeError::new("z-index must be finite"));
        }
        let id = self.next;
        self.next += 1;
        let i = self.entries.partition_point(|e| e.z <= z);
        self.entries.insert(i, Entry { id, z, primitive });
        Ok(id)
    }
    pub fn set(&mut self, id: u64, z: f64) -> Result<bool> {
        self.check()?;
        if !z.is_finite() {
            return Err(RuntimeError::new("z-index must be finite"));
        }
        let Some(i) = self.entries.iter().position(|e| e.id == id) else {
            return Ok(false);
        };
        if self.entries[i].z == z {
            return Ok(true);
        }
        let mut e = self.entries.remove(i);
        e.z = z;
        let j = self.entries.partition_point(|e| e.z <= z);
        self.entries.insert(j, e);
        Ok(true)
    }
    pub fn remove(&mut self, id: u64, do_not_destroy: bool) -> Result<Option<Box<dyn ScenePrimitive>>> {
        self.check()?;
        let Some(i) = self.entries.iter().position(|e| e.id == id) else {
            return Ok(None);
        };
        let mut e = self.entries.remove(i);
        if !do_not_destroy {
            e.primitive.destroy()?;
        }
        Ok(Some(e.primitive))
    }
    pub fn remove_all(&mut self) -> Result<()> {
        self.check()?;
        let mut error = None;
        for mut e in self.entries.drain(..) {
            if let Err(e) = e.primitive.destroy() {
                error.get_or_insert(e);
            }
        }
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
impl ScenePrimitive for OrderedGroundPrimitiveCollection {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        self.check()
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        self.check()
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, _: Cesium3DTilePass) -> Result<()> {
        self.update(frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.check()?;
        if self.show {
            for e in &mut self.entries {
                e.primitive.update(frame)?;
            }
        }
        Ok(())
    }
    fn destroy(&mut self) -> Result<()> {
        let result = self.remove_all();
        self.destroyed = true;
        result
    }
}
