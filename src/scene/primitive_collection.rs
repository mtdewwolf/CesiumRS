//! CesiumJS `PrimitiveCollection`: a hierarchy-capable collection of primitives.

use super::{
    scene::{Result, ScenePrimitive},
    Cesium3DTilePass, FrameState,
};

/// Options for creating a [`PrimitiveCollection`].
#[derive(Clone, Copy, Debug)]
pub struct PrimitiveCollectionOptions {
    pub show: Option<bool>,
    pub destroy_primitives: Option<bool>,
}

impl Default for PrimitiveCollectionOptions {
    fn default() -> Self {
        Self { show: Some(true), destroy_primitives: Some(true) }
    }
}

/// A collection of primitives that itself implements [`ScenePrimitive`].
pub struct PrimitiveCollection {
    pub show: bool,
    pub destroy_primitives: bool,
    primitives: Vec<Box<dyn ScenePrimitive>>,
    destroyed: bool,
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
            primitives: Vec::new(),
            destroyed: false,
        }
    }

    pub fn length(&self) -> usize {
        self.primitives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    pub fn add(&mut self, primitive: Box<dyn ScenePrimitive>) {
        self.primitives.push(primitive);
    }

    pub fn insert(&mut self, index: usize, primitive: Box<dyn ScenePrimitive>) {
        if index >= self.primitives.len() {
            self.primitives.push(primitive);
        } else {
            self.primitives.insert(index, primitive);
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<Box<dyn ScenePrimitive>> {
        if index < self.primitives.len() {
            let mut p = self.primitives.remove(index);
            if self.destroy_primitives {
                let _ = p.destroy();
            }
            Some(p)
        } else {
            None
        }
    }

    pub fn remove_all(&mut self) {
        if self.destroy_primitives {
            for mut p in self.primitives.drain(..) {
                let _ = p.destroy();
            }
        } else {
            self.primitives.clear();
        }
    }

    pub fn get(&self, index: usize) -> Option<&dyn ScenePrimitive> {
        self.primitives.get(index).map(|p| p.as_ref())
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut (dyn ScenePrimitive + '_)> {
        match self.primitives.get_mut(index) {
            Some(p) => Some(&mut **p),
            None => None,
        }
    }

    pub fn raise(&mut self, index: usize) {
        if index + 1 < self.primitives.len() {
            self.primitives.swap(index, index + 1);
        }
    }

    pub fn raise_to_top(&mut self, index: usize) {
        if index + 1 < self.primitives.len() {
            let item = self.primitives.remove(index);
            self.primitives.push(item);
        }
    }

    pub fn lower(&mut self, index: usize) {
        if index > 0 && index < self.primitives.len() {
            self.primitives.swap(index, index - 1);
        }
    }

    pub fn lower_to_bottom(&mut self, index: usize) {
        if index > 0 && index < self.primitives.len() {
            let item = self.primitives.remove(index);
            self.primitives.insert(0, item);
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

impl ScenePrimitive for PrimitiveCollection {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        for p in &mut self.primitives {
            p.pre_passes_update(frame)?;
        }
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if !self.show {
            return Ok(());
        }
        for p in &mut self.primitives {
            p.update(frame)?;
        }
        Ok(())
    }

    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        for p in &mut self.primitives {
            p.post_passes_update(frame)?;
        }
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<()> {
        for p in &mut self.primitives {
            p.update_for_pass(frame, pass)?;
        }
        Ok(())
    }

    fn destroy(&mut self) -> Result<()> {
        self.destroyed = true;
        self.remove_all();
        Ok(())
    }
}
