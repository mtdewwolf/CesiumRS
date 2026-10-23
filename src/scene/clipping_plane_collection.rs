//! Plane collection, volume tests and owned float/byte GPU textures.
use super::{ClippingPlaneHandle, FrameResource};
use crate::{core::*, renderer::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
type Result<T> = std::result::Result<T, RuntimeError>;
#[derive(Clone, Debug)]
pub struct ClippingPlaneEvent {
    pub plane: ClippingPlaneHandle,
    pub index: usize,
}
#[derive(Default)]
struct Dirty {
    index: Cell<Option<usize>>,
    multiple: Cell<bool>,
}
impl Dirty {
    fn set(&self, index: usize) {
        self.multiple.set(self.multiple.get() || self.index.get().is_some_and(|i| i != index));
        self.index.set(Some(index));
    }
}
type TextureUpload = Box<dyn Fn(&[u8], TextureRegion) -> Result<()>>;
struct PlaneTexture {
    resource: FrameResource,
    width: u32,
    height: u32,
    float: bool,
    destroy: Box<dyn Fn() -> Result<()>>,
    upload: TextureUpload,
}
pub struct ClippingPlaneCollection {
    planes: Vec<ClippingPlaneHandle>,
    pub enabled: bool,
    pub union_clipping_regions: bool,
    pub model_matrix: Matrix4,
    pub edge_color: Color,
    pub edge_width: f64,
    pub plane_added: Event<ClippingPlaneEvent>,
    pub plane_removed: Event<ClippingPlaneEvent>,
    dirty: Rc<Dirty>,
    texture: Option<PlaneTexture>,
    packed: Vec<u8>,
    owner: Option<usize>,
    destroyed: bool,
}
impl Default for ClippingPlaneCollection {
    fn default() -> Self {
        Self {
            planes: Vec::new(),
            enabled: true,
            union_clipping_regions: false,
            model_matrix: Matrix4::IDENTITY,
            edge_color: Color::WHITE,
            edge_width: 0.,
            plane_added: Event::new(),
            plane_removed: Event::new(),
            dirty: Rc::new(Dirty::default()),
            texture: None,
            packed: Vec::new(),
            owner: None,
            destroyed: false,
        }
    }
}
impl ClippingPlaneCollection {
    pub fn new(planes: impl IntoIterator<Item = ClippingPlaneHandle>) -> Self {
        let mut result = Self::default();
        for plane in planes {
            result.add(plane);
        }
        result
    }
    fn alive(&self) -> Result<()> {
        if self.destroyed {
            Err(RuntimeError::new("ClippingPlaneCollection has been destroyed"))
        } else {
            Ok(())
        }
    }
    pub fn len(&self) -> usize {
        self.planes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.planes.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<&ClippingPlaneHandle> {
        self.planes.get(index)
    }
    pub fn owner(&self) -> Option<usize> {
        self.owner
    }
    pub fn assign_owner(&mut self, owner: usize) -> Result<()> {
        self.alive()?;
        if self.owner.is_some() {
            return Err(RuntimeError::new("ClippingPlaneCollection should only be assigned to one object"));
        }
        self.owner = Some(owner);
        Ok(())
    }
    pub fn state(&self) -> isize {
        if self.union_clipping_regions {
            self.len() as isize
        } else {
            -(self.len() as isize)
        }
    }
    pub fn add(&mut self, plane: ClippingPlaneHandle) {
        let index = self.len();
        let dirty = Rc::clone(&self.dirty);
        {
            let mut p = plane.borrow_mut();
            p.index = index as isize;
            p.on_change_callback = Some(Rc::new(RefCell::new(move |i: isize, _: &super::ClippingPlane| {
                if i >= 0 {
                    dirty.set(i as usize);
                }
            })));
        }
        self.dirty.set(index);
        self.planes.push(Rc::clone(&plane));
        self.plane_added.raise_event(&ClippingPlaneEvent { plane, index });
    }
    fn index_of(&self, plane: &ClippingPlaneHandle) -> Option<usize> {
        let value = plane.borrow().as_plane();
        self.planes.iter().position(|p| p.borrow().as_plane().equals(&value))
    }
    pub fn contains(&self, plane: &ClippingPlaneHandle) -> bool {
        self.index_of(plane).is_some()
    }
    pub fn remove(&mut self, plane: &ClippingPlaneHandle) -> bool {
        let Some(index) = self.index_of(plane) else { return false };
        {
            let mut plane = plane.borrow_mut();
            plane.index = -1;
            plane.on_change_callback = None;
        }
        self.planes.remove(index);
        for (i, plane) in self.planes.iter().enumerate().skip(index) {
            plane.borrow_mut().index = i as isize;
        }
        self.dirty.multiple.set(true);
        self.plane_removed.raise_event(&ClippingPlaneEvent { plane: Rc::clone(plane), index });
        true
    }
    pub fn remove_all(&mut self) {
        for (index, plane) in self.planes.iter().enumerate() {
            let mut value = plane.borrow_mut();
            value.index = -1;
            value.on_change_callback = None;
            drop(value);
            self.plane_removed.raise_event(&ClippingPlaneEvent { plane: Rc::clone(plane), index });
        }
        self.planes.clear();
        self.dirty.multiple.set(true);
    }
    pub fn compute_intersection(
        &self,
        volume: &dyn culling_volume::BoundingVolume,
        transform: Option<&Matrix4>,
    ) -> Result<Intersect> {
        self.alive()?;
        let model = transform.map_or(self.model_matrix, |t| t.multiply(&self.model_matrix));
        let mut intersection =
            if !self.union_clipping_regions && !self.is_empty() { Intersect::Outside } else { Intersect::Inside };
        for plane in &self.planes {
            let plane = plane.borrow().as_plane().transform(&model)?;
            let value = volume.intersect_plane(&plane);
            if value == Intersect::Intersecting {
                intersection = value;
            } else if value == if self.union_clipping_regions { Intersect::Outside } else { Intersect::Inside } {
                return Ok(value);
            }
        }
        Ok(intersection)
    }
    /// Required allocation before the first upload, or the retained allocation.
    pub fn texture_resolution(&self, float: bool, maximum: u32) -> [u32; 2] {
        if let Some(t) = &self.texture {
            return [t.width, t.height];
        }
        let needed = self.len() as u32 * if float { 1 } else { 2 };
        let width = needed.min(maximum);
        if width == 0 {
            return [0, 0];
        }
        [width, 2 * needed.div_ceil(width)]
    }
    pub fn texture<G: Gl + 'static>(&self) -> Option<Rc<Texture<G>>> {
        self.texture.as_ref()?.resource.get::<Rc<Texture<G>>>().cloned()
    }
    pub fn packed_bytes(&self) -> &[u8] {
        &self.packed
    }
    fn pack(&mut self, start: usize, end: usize, float: bool) {
        for (i, plane) in self.planes[start..end].iter().enumerate() {
            let p = plane.borrow();
            let n = p.normal();
            if float {
                for (component, value) in [n.x, n.y, n.z, p.distance()].iter().enumerate() {
                    self.packed[i * 16 + component * 4..i * 16 + component * 4 + 4]
                        .copy_from_slice(&(*value as f32).to_le_bytes());
                }
            } else {
                let normal = AttributeCompression::oct_encode_to_cartesian4(n);
                let distance = Cartesian4::pack_float(p.distance());
                for (component, value) in
                    [normal.x, normal.y, normal.z, normal.w, distance.x, distance.y, distance.z, distance.w]
                        .iter()
                        .enumerate()
                {
                    self.packed[i * 8 + component] = *value as u8;
                }
            }
        }
    }
    pub fn update<G: Gl + 'static>(&mut self, context: &Context<G>) -> Result<()> {
        self.alive()?;
        let float = context.capabilities()?.floating_point_texture;
        let needed = self.len() as u32 * if float { 1 } else { 2 };
        if let Some(texture) = &self.texture {
            let concrete = texture
                .resource
                .get::<Rc<Texture<G>>>()
                .ok_or_else(|| RuntimeError::new("Clipping planes belong to a different renderer type"))?;
            context.same(concrete.context())?;
            let count = texture.width * texture.height;
            if count < needed || (needed as f64) < (count as f64) * 0.25 || texture.float != float {
                (texture.destroy)()?;
                self.texture = None;
            }
        }
        if self.is_empty() {
            return Ok(());
        }
        if self.texture.is_none() {
            let [width, height] = self.texture_resolution(float, context.limits()?.maximum_texture_size);
            let mut options = TextureOptions::rgba(width, height);
            options.pixel_datatype = if float { PixelDatatype::Float } else { PixelDatatype::UnsignedByte };
            options.flip_y = false;
            options.sampler = Sampler::NEAREST;
            let texture = Rc::new(Texture::new(context, options, None)?);
            let upload = Rc::clone(&texture);
            let destroy = Rc::clone(&texture);
            self.packed = vec![0; width as usize * height as usize * if float { 16 } else { 4 }];
            self.texture = Some(PlaneTexture {
                resource: FrameResource::new(texture),
                width,
                height,
                float,
                destroy: Box::new(move || destroy.destroy()),
                upload: Box::new(move |bytes, region| upload.copy_from(bytes, region)),
            });
            self.dirty.multiple.set(true);
        }
        if !self.dirty.multiple.get() && self.dirty.index.get().is_none() {
            return Ok(());
        }
        let texture = self.texture.as_ref().unwrap();
        let (width, height) = (texture.width, texture.height);
        let region = if !self.dirty.multiple.get() {
            let index = self.dirty.index.get().unwrap();
            self.pack(index, index + 1, float);
            let pixel = index as u32 * if float { 1 } else { 2 };
            TextureRegion { x: pixel % width, y: pixel / width, width: if float { 1 } else { 2 }, height: 1 }
        } else {
            self.pack(0, self.len(), float);
            TextureRegion { x: 0, y: 0, width, height }
        };
        let bytes = region.width as usize * region.height as usize * if float { 16 } else { 4 };
        (self.texture.as_ref().unwrap().upload)(&self.packed[..bytes], region)?;
        self.dirty.multiple.set(false);
        self.dirty.index.set(None);
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        if let Some(texture) = &self.texture {
            (texture.destroy)()?;
        }
        self.texture = None;
        self.destroyed = true;
        Ok(())
    }
}
