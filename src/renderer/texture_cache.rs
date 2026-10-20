//! CesiumJS TextureCache using owned Rust texture leases (Rc clones).
use super::{context::*, Texture};
use std::{collections::HashMap, rc::Rc};
pub struct TextureCache<G: Gl> {
    textures: HashMap<String, Rc<Texture<G>>>,
    destroyed: bool,
}
impl<G: Gl> Default for TextureCache<G> {
    fn default() -> Self {
        Self { textures: HashMap::new(), destroyed: false }
    }
}
impl<G: Gl> TextureCache<G> {
    pub fn number_of_textures(&self) -> usize {
        self.textures.len()
    }
    pub fn get_texture(&self, keyword: &str) -> Option<Rc<Texture<G>>> {
        if self.destroyed {
            None
        } else {
            self.textures.get(keyword).filter(|t| !t.is_destroyed()).cloned()
        }
    }
    pub fn add_texture(&mut self, keyword: impl Into<String>, texture: Rc<Texture<G>>) -> Result<()> {
        let key = keyword.into();
        ensure(!self.destroyed, "Texture cache is destroyed")?;
        ensure(!self.textures.contains_key(&key), "Texture keyword is already cached")?;
        self.textures.insert(key, texture);
        Ok(())
    }
    pub fn destroy_released_textures(&mut self) -> Result<()> {
        let keys: Vec<_> =
            self.textures.iter().filter(|(_, t)| Rc::strong_count(t) == 1).map(|(k, _)| k.clone()).collect();
        for key in keys {
            self.textures.remove(&key).unwrap().destroy()?;
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        for texture in self.textures.values() {
            texture.destroy()?;
        }
        self.textures.clear();
        self.destroyed = true;
        Ok(())
    }
}
impl<G: Gl> Drop for TextureCache<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
