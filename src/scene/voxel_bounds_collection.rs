//! Voxel clipping planes: value-based membership, transformed float packing and GPU uploads.
use super::{ClippingPlaneEvent, ClippingPlaneHandle};
use crate::{
    core::{Cartesian4, Event, Matrix4, Plane, RuntimeError},
    renderer::*,
};
use std::rc::Rc;
type Result<T> = std::result::Result<T, RuntimeError>;
pub struct VoxelBoundsCollection<G: Gl> {
    planes: Vec<ClippingPlaneHandle>,
    pub model_matrix: Matrix4,
    pub union_clipping_regions: bool,
    pub plane_added: Event<ClippingPlaneEvent>,
    pub plane_removed: Event<ClippingPlaneEvent>,
    texture: Option<Rc<Texture<G>>>,
    packed: Vec<u8>,
    destroyed: bool,
}
impl<G: Gl> Default for VoxelBoundsCollection<G> {
    fn default() -> Self {
        Self {
            planes: vec![],
            model_matrix: Matrix4::IDENTITY,
            union_clipping_regions: false,
            plane_added: Event::new(),
            plane_removed: Event::new(),
            texture: None,
            packed: vec![],
            destroyed: false,
        }
    }
}
impl<G: Gl> VoxelBoundsCollection<G> {
    pub fn len(&self) -> usize {
        self.planes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.planes.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<&ClippingPlaneHandle> {
        self.planes.get(index)
    }
    pub fn texture(&self) -> Option<&Rc<Texture<G>>> {
        self.texture.as_ref()
    }
    pub fn clipping_planes_state(&self) -> isize {
        self.len() as isize * if self.union_clipping_regions { 1 } else { -1 }
    }
    pub fn add(&mut self, plane: ClippingPlaneHandle) {
        let index = self.len();
        plane.borrow_mut().index = index as isize;
        self.planes.push(plane.clone());
        self.plane_added.raise_event(&ClippingPlaneEvent { plane, index });
    }
    fn index_of(&self, plane: &Plane) -> Option<usize> {
        self.planes.iter().position(|p| p.borrow().as_plane() == *plane)
    }
    pub fn contains(&self, plane: &Plane) -> bool {
        self.index_of(plane).is_some()
    }
    pub fn remove(&mut self, plane: &Plane) -> bool {
        let Some(index) = self.index_of(plane) else {
            return false;
        };
        let removed = self.planes.remove(index);
        removed.borrow_mut().index = -1;
        removed.borrow_mut().on_change_callback = None;
        for (i, p) in self.planes.iter().enumerate().skip(index) {
            p.borrow_mut().index = i as isize;
        }
        self.plane_removed.raise_event(&ClippingPlaneEvent { plane: removed, index });
        true
    }
    pub fn remove_all(&mut self) {
        for (index, plane) in self.planes.drain(..).enumerate() {
            plane.borrow_mut().index = -1;
            plane.borrow_mut().on_change_callback = None;
            self.plane_removed.raise_event(&ClippingPlaneEvent { plane, index });
        }
    }
    pub fn texture_resolution(&self, maximum: u32) -> [u32; 2] {
        if let Some(t) = &self.texture {
            return t.dimensions();
        }
        if self.is_empty() || maximum == 0 {
            return [0, 0];
        }
        let width = (self.len() as u32).min(maximum);
        [width, (self.len() as u32).div_ceil(width) * 2]
    }
    /// The supplied matrix is already the plane transform, as in Cesium; model_matrix is consumed by the caller.
    pub fn packed_planes(&self, transform: Matrix4) -> Vec<f32> {
        self.planes
            .iter()
            .flat_map(|p| {
                let p = p.borrow();
                let n = p.normal();
                let q = transform.multiply_by_vector(Cartesian4::new(n.x, n.y, n.z, p.distance()));
                let m = (q.x * q.x + q.y * q.y + q.z * q.z).sqrt();
                [(q.x / m) as f32, (q.y / m) as f32, (q.z / m) as f32, (q.w / m) as f32]
            })
            .collect()
    }
    pub fn update(&mut self, context: &Context<G>, transform: Matrix4) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("VoxelBoundsCollection was destroyed"));
        }
        if let Some(t) = &self.texture {
            let capacity = t.width() as usize * t.height() as usize;
            if capacity < self.len() || self.len() * 4 < capacity {
                t.destroy()?;
                self.texture = None;
            }
        }
        if self.is_empty() {
            return Ok(());
        }
        if self.texture.is_none() {
            let [w, h] = self.texture_resolution(context.limits()?.maximum_texture_size);
            let mut options = TextureOptions::rgba(w, h);
            options.pixel_datatype = PixelDatatype::Float;
            options.sampler = Sampler::NEAREST;
            options.flip_y = false;
            self.texture = Some(Rc::new(Texture::new(context, options, None)?));
            self.packed = vec![0; w as usize * h as usize * 16];
        }
        let values = self.packed_planes(transform);
        for (i, v) in values.iter().enumerate() {
            self.packed[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        let t = self.texture.as_ref().unwrap();
        t.copy_from(&self.packed, TextureRegion { x: 0, y: 0, width: t.width(), height: t.height() })
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if let Some(t) = self.texture.take() {
            t.destroy()?;
        }
        self.packed.clear();
        self.destroyed = true;
        Ok(())
    }
}
