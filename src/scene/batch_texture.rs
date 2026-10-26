//! Per-feature visibility, color and pick textures. CPU state is allocated on
//! demand, and GPU uploads happen at the frame boundary.
use crate::core::{Cartesian2, Cartesian4, Color, RuntimeError};
use crate::renderer::{Context, Gl, PickId, Sampler, Texture, TextureOptions, TextureRegion};
use std::{any::Any, cell::Cell, rc::Rc};
type Result<T> = std::result::Result<T, RuntimeError>;
pub type FeatureColorChanged = Rc<dyn Fn(usize, Color)>;

pub struct BatchTexture<G: Gl> {
    id: String,
    features_length: usize,
    dimensions: Option<[u32; 2]>,
    show_alpha: Option<Vec<u8>>,
    values: Option<Vec<u8>>,
    dirty: bool,
    translucent: usize,
    batch_texture: Option<Rc<Texture<G>>>,
    pick_texture: Option<Rc<Texture<G>>>,
    default_texture: Option<Rc<Texture<G>>>,
    pick_ids: Vec<PickId>,
    pub color_changed: Option<FeatureColorChanged>,
    pub statistics_byte_length: Option<Rc<Cell<usize>>>,
    destroyed: bool,
}
impl<G: Gl> BatchTexture<G> {
    pub const DEFAULT_COLOR_VALUE: Color = Color::WHITE;
    pub const DEFAULT_SHOW_VALUE: bool = true;
    pub fn new(features_length: usize, max_texture_size: u32) -> Result<Self> {
        if max_texture_size == 0 {
            return Err(RuntimeError::new("maximumTextureSize must be positive"));
        }
        let height = features_length.div_ceil(max_texture_size as usize);
        if height > max_texture_size as usize {
            return Err(RuntimeError::new("Feature texture exceeds maximumTextureSize"));
        }
        let dimensions =
            (features_length > 0).then_some([features_length.min(max_texture_size as usize) as u32, height as u32]);
        Ok(Self {
            id: crate::core::create_guid::create_guid(),
            features_length,
            dimensions,
            show_alpha: None,
            values: None,
            dirty: false,
            translucent: 0,
            batch_texture: None,
            pick_texture: None,
            default_texture: None,
            pick_ids: vec![],
            color_changed: None,
            statistics_byte_length: None,
            destroyed: false,
        })
    }
    fn check(&self, id: usize) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("BatchTexture is destroyed"));
        }
        if id >= self.features_length {
            return Err(RuntimeError::new("NaN"));
        }
        Ok(())
    }
    fn allocate(&mut self) {
        if self.show_alpha.is_none() {
            self.show_alpha = Some(vec![255; self.features_length * 2]);
        }
        if self.values.is_none() {
            let [w, h] = self.dimensions.unwrap_or([0, 0]);
            self.values = Some(vec![255; w as usize * h as usize * 4]);
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn features_length(&self) -> usize {
        self.features_length
    }
    pub fn translucent_features_length(&self) -> usize {
        self.translucent
    }
    pub fn texture_dimensions(&self) -> Option<Cartesian2> {
        self.dimensions.map(|[w, h]| Cartesian2::new(w as f64, h as f64))
    }
    pub fn texture_step(&self) -> Option<Cartesian4> {
        self.dimensions.map(|[w, h]| Cartesian4::new(1. / w as f64, 0.5 / w as f64, 1. / h as f64, 0.5 / h as f64))
    }
    pub fn batch_texture(&self) -> Option<Rc<Texture<G>>> {
        self.batch_texture.clone()
    }
    pub fn pick_texture(&self) -> Option<Rc<Texture<G>>> {
        self.pick_texture.clone()
    }
    pub fn default_texture(&self) -> Option<Rc<Texture<G>>> {
        self.default_texture.clone()
    }
    pub fn byte_length(&self) -> usize {
        self.batch_texture.as_ref().map_or(0, |t| t.size_in_bytes())
            + self.pick_texture.as_ref().map_or(0, |t| t.size_in_bytes())
    }
    pub fn batch_values(&self) -> Option<&[u8]> {
        self.values.as_deref()
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn get_show(&self, id: usize) -> Result<bool> {
        self.check(id)?;
        Ok(self.show_alpha.as_ref().is_none_or(|v| v[id * 2] == 255))
    }
    pub fn set_show(&mut self, id: usize, show: bool) -> Result<()> {
        self.check(id)?;
        if show && self.show_alpha.is_none() {
            return Ok(());
        }
        self.allocate();
        let sa = self.show_alpha.as_mut().unwrap();
        let new = if show { 255 } else { 0 };
        if sa[id * 2] != new {
            sa[id * 2] = new;
            self.values.as_mut().unwrap()[id * 4 + 3] = if show { sa[id * 2 + 1] } else { 0 };
            self.dirty = true;
        }
        Ok(())
    }
    pub fn set_all_show(&mut self, show: bool) -> Result<()> {
        for i in 0..self.features_length {
            self.set_show(i, show)?;
        }
        Ok(())
    }
    pub fn get_color(&self, id: usize) -> Result<Color> {
        self.check(id)?;
        let Some(v) = &self.values else { return Ok(Color::WHITE) };
        Ok(Color::from_bytes(
            v[id * 4] as f64,
            v[id * 4 + 1] as f64,
            v[id * 4 + 2] as f64,
            self.show_alpha.as_ref().unwrap()[id * 2 + 1] as f64,
        ))
    }
    pub fn set_color(&mut self, id: usize, color: Color) -> Result<()> {
        self.check(id)?;
        if color == Color::WHITE && self.values.is_none() {
            return Ok(());
        }
        self.allocate();
        let b = color.to_bytes().map(|v| v as u8);
        let v = self.values.as_mut().unwrap();
        let sa = self.show_alpha.as_mut().unwrap();
        if v[id * 4..id * 4 + 3] != b[..3] || sa[id * 2 + 1] != b[3] {
            let was = sa[id * 2 + 1] != 255;
            let is = b[3] != 255;
            v[id * 4..id * 4 + 3].copy_from_slice(&b[..3]);
            v[id * 4 + 3] = if sa[id * 2] != 0 { b[3] } else { 0 };
            sa[id * 2 + 1] = b[3];
            if is && !was {
                self.translucent += 1;
            } else if was && !is {
                self.translucent -= 1;
            }
            self.dirty = true;
            if let Some(callback) = &self.color_changed {
                callback(id, color);
            }
        }
        Ok(())
    }
    pub fn set_all_color(&mut self, color: Color) -> Result<()> {
        for i in 0..self.features_length {
            self.set_color(i, color)?;
        }
        Ok(())
    }
    pub fn get_pick_color(&self, id: usize) -> Result<Option<&PickId>> {
        self.check(id)?;
        Ok(self.pick_ids.get(id))
    }
    fn make_texture(&self, context: &Context<G>, bytes: &[u8]) -> Result<Rc<Texture<G>>> {
        let [w, h] = self.dimensions.ok_or_else(|| RuntimeError::new("Empty feature texture"))?;
        Ok(Rc::new(Texture::new(
            context,
            TextureOptions { flip_y: false, sampler: Sampler::NEAREST, ..TextureOptions::rgba(w, h) },
            Some(bytes),
        )?))
    }
    fn count_texture(&self, texture: &Texture<G>) {
        if let Some(stats) = &self.statistics_byte_length {
            stats.set(stats.get() + texture.size_in_bytes());
        }
    }
    pub fn update<T: Any>(
        &mut self,
        context: &Context<G>,
        pick_or_post_process: bool,
        feature: impl Fn(usize) -> Result<Rc<T>>,
    ) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("BatchTexture is destroyed"));
        }
        self.default_texture = Some(context.default_texture()?);
        if pick_or_post_process && self.pick_texture.is_none() && self.features_length > 0 {
            let [w, h] = self.dimensions.unwrap();
            let mut bytes = vec![0; w as usize * h as usize * 4];
            let mut ids = Vec::with_capacity(self.features_length);
            for i in 0..self.features_length {
                let id = context.create_pick_id(feature(i)?)?;
                let color = id.color.to_bytes().map(|v| v as u8);
                bytes[i * 4..i * 4 + 4].copy_from_slice(&color);
                ids.push(id);
            }
            let texture = self.make_texture(context, &bytes)?;
            self.count_texture(&texture);
            self.pick_texture = Some(texture);
            self.pick_ids = ids;
        }
        if self.dirty {
            let bytes = self.values.as_ref().unwrap();
            if let Some(texture) = &self.batch_texture {
                let [w, h] = self.dimensions.unwrap();
                texture.copy_from(bytes, TextureRegion { x: 0, y: 0, width: w, height: h })?;
            } else {
                let texture = self.make_texture(context, bytes)?;
                self.count_texture(&texture);
                self.batch_texture = Some(texture);
            }
            self.dirty = false;
        }
        Ok(())
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        if let Some(t) = self.batch_texture.take() {
            t.destroy()?;
        }
        if let Some(t) = self.pick_texture.take() {
            t.destroy()?;
        }
        self.pick_ids.clear();
        self.default_texture = None;
        self.destroyed = true;
        Ok(())
    }
}
