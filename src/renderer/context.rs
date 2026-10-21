//! Context identity, GL host protocol, and shared GPU object lifetime.
use crate::core::RuntimeError;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};
pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Values crossing the host boundary. Arrays are copied before a host call
/// returns. Opaque host handles use `Object`, including when a native driver
/// represents those handles as integers.
#[derive(Clone, Debug)]
pub enum GlValue<O> {
    Null,
    Number(f64),
    Bool(bool),
    Bools(Vec<bool>),
    String(String),
    Bytes(Vec<u8>),
    Floats(Vec<f32>),
    Ints(Vec<i32>),
    UInts(Vec<u32>),
    Numbers(Vec<f64>),
    Record(serde_json::Value),
    Object(O),
    Pixels { bytes: Vec<u8>, datatype: crate::core::PixelDatatype },
}
impl<O> From<u32> for GlValue<O> {
    fn from(v: u32) -> Self {
        Self::Number(v as f64)
    }
}
impl<O> From<i32> for GlValue<O> {
    fn from(v: i32) -> Self {
        Self::Number(v as f64)
    }
}
impl<O> From<f64> for GlValue<O> {
    fn from(v: f64) -> Self {
        Self::Number(v)
    }
}
impl<O> From<bool> for GlValue<O> {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl<O> From<&str> for GlValue<O> {
    fn from(v: &str) -> Self {
        Self::String(v.into())
    }
}

/// Host implementation of WebGL2 entrypoints. `getBufferSubData` and
/// `readPixels` return `Bytes` containing their output array. All other methods
/// use the WebGL signatures and results. A host must report allocation failures
/// and exceptions as errors; GL errors can be queried with `getError`.
pub trait Gl {
    type Object: Clone;
    fn drawing_buffer_size(&self) -> Result<[u32; 2]>;
    fn invoke(&self, method: &str, args: &[GlValue<Self::Object>]) -> Result<GlValue<Self::Object>>;
}

pub struct Context<G: Gl> {
    pub(crate) gl: Rc<G>,
    pub(crate) uniform_state: Rc<RefCell<super::UniformState>>,
    pub(crate) frame_texture_units: Rc<std::cell::Cell<u32>>,
    pub(crate) state_driver: Rc<RefCell<super::render_state_cache::RenderStateDriver>>,
    pub(crate) webgl2: bool,
    id: String,
    surface_stencil_bits: Option<u32>,
    surface_antialias: Option<bool>,
    throw_on_webgl_error: Rc<Cell<bool>>,
    pub validate_framebuffer: Rc<Cell<bool>>,
    pub validate_shader_program: Rc<Cell<bool>>,
    pub(crate) destroyed: Rc<Cell<bool>>,
    pub(crate) local_destroyed: Option<Rc<Cell<bool>>>,
    pub(crate) dimensions: Option<Rc<dyn Fn() -> Result<[u32; 2]>>>,
    pub(crate) destroy_override: Option<Rc<dyn Fn() -> Result<()>>>,
    registry: Rc<RefCell<BTreeMap<u64, Deletion<G::Object>>>>,
    next_resource: Rc<Cell<u64>>,
    pub(crate) cache: Option<Rc<super::context_helpers::ContextCache<G>>>,
    pub(crate) cache_ref: Weak<super::context_helpers::ContextCache<G>>,
    pub(crate) picks: Rc<super::context_helpers::PickStore>,
}
struct Deletion<O> {
    object: Weak<RefCell<Option<O>>>,
    method: &'static str,
}
impl<G: Gl> Clone for Context<G> {
    fn clone(&self) -> Self {
        Self {
            gl: self.gl.clone(),
            uniform_state: self.uniform_state.clone(),
            frame_texture_units: self.frame_texture_units.clone(),
            state_driver: self.state_driver.clone(),
            webgl2: self.webgl2,
            id: self.id.clone(),
            surface_stencil_bits: self.surface_stencil_bits,
            surface_antialias: self.surface_antialias,
            throw_on_webgl_error: self.throw_on_webgl_error.clone(),
            validate_framebuffer: self.validate_framebuffer.clone(),
            validate_shader_program: self.validate_shader_program.clone(),
            destroyed: self.destroyed.clone(),
            local_destroyed: self.local_destroyed.clone(),
            dimensions: self.dimensions.clone(),
            destroy_override: self.destroy_override.clone(),
            registry: self.registry.clone(),
            next_resource: self.next_resource.clone(),
            cache: self.cache.clone(),
            cache_ref: self.cache_ref.clone(),
            picks: self.picks.clone(),
        }
    }
}
impl<G: Gl> Context<G> {
    pub fn new_webgl2(gl: G) -> Self {
        let cache = Rc::new(super::context_helpers::ContextCache::default());
        // These describe the drawing surface, independent of later framebuffer bindings.
        let surface_stencil_bits = match gl.invoke("getParameter", &[crate::core::webgl_constants::STENCIL_BITS.into()])
        {
            Ok(GlValue::Number(n)) => Some(n as u32),
            _ => None,
        };
        let surface_antialias = match gl.invoke("getParameter", &[crate::core::webgl_constants::SAMPLES.into()]) {
            Ok(GlValue::Number(n)) => Some(n > 0.),
            _ => None,
        };
        Self {
            uniform_state: Rc::new(RefCell::new(Default::default())),
            frame_texture_units: Rc::new(std::cell::Cell::new(0)),
            gl: Rc::new(gl),
            state_driver: Rc::new(RefCell::new(Default::default())),
            webgl2: true,
            id: crate::core::create_guid::create_guid(),
            surface_stencil_bits,
            surface_antialias,
            throw_on_webgl_error: Rc::new(Cell::new(false)),
            validate_framebuffer: Rc::new(Cell::new(false)),
            validate_shader_program: Rc::new(Cell::new(false)),
            destroyed: Rc::new(Cell::new(false)),
            local_destroyed: None,
            dimensions: None,
            destroy_override: None,
            registry: Rc::new(RefCell::new(BTreeMap::new())),
            next_resource: Rc::new(Cell::new(0)),
            cache_ref: Rc::downgrade(&cache),
            cache: Some(cache),
            picks: Rc::new(Default::default()),
        }
    }
    pub fn new_webgl1(gl: G) -> Self {
        let mut context = Self::new_webgl2(gl);
        context.webgl2 = false;
        context
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn set_throw_on_webgl_error(&self, enabled: bool) {
        self.throw_on_webgl_error.set(enabled);
    }
    pub fn throw_on_webgl_error(&self) -> bool {
        self.throw_on_webgl_error.get()
    }
    pub fn stencil_bits(&self) -> Result<u32> {
        ensure(!self.is_destroyed(), "Context is destroyed")?;
        self.surface_stencil_bits.ok_or_else(|| RuntimeError::new("Host did not report the surface stencil bit count"))
    }
    pub fn stencil_buffer(&self) -> Result<bool> {
        Ok(self.stencil_bits()? > 0)
    }
    pub fn antialias(&self) -> Result<bool> {
        ensure(!self.is_destroyed(), "Context is destroyed")?;
        self.surface_antialias.ok_or_else(|| RuntimeError::new("Host did not report the surface sample count"))
    }
    pub fn webgl2(&self) -> bool {
        self.webgl2
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get() || self.local_destroyed.as_ref().is_some_and(|d| d.get())
    }
    pub fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
        ensure(!self.is_destroyed(), "Context is destroyed")?;
        if let Some(size) = &self.dimensions {
            size()
        } else {
            self.gl.drawing_buffer_size()
        }
    }
    pub(crate) fn resource_context(&self) -> Self {
        let mut context = self.clone();
        context.cache = None;
        context.local_destroyed = None;
        context.destroy_override = None;
        context
    }
    pub fn destroy(&self) -> Result<()> {
        if let Some(destroy) = &self.destroy_override {
            return destroy();
        }
        if self.destroyed.get() {
            return Ok(());
        }
        let ids: Vec<_> = self.registry.borrow().keys().copied().collect();
        for id in ids {
            let entry = self.registry.borrow().get(&id).map(|e| (e.object.clone(), e.method));
            if let Some((object, method)) = entry {
                if let Some(object) = object.upgrade() {
                    let value = object.borrow().clone();
                    if let Some(value) = value {
                        self.gl.invoke(method, &[GlValue::Object(value)])?;
                        object.borrow_mut().take();
                    }
                }
                self.registry.borrow_mut().remove(&id);
            }
        }
        self.picks.clear();
        self.destroyed.set(true);
        if let Some(cache) = self.cache.as_ref().cloned().or_else(|| self.cache_ref.upgrade()) {
            cache.clear();
        }
        Ok(())
    }
    /// Direct access is for host integration. Call `invalidate_render_state`
    /// after changing fixed-function GL state outside the renderer.
    pub fn host(&self) -> &G {
        &self.gl
    }
    pub(crate) fn call(&self, name: &str, args: &[GlValue<G::Object>]) -> Result<GlValue<G::Object>> {
        ensure(!self.is_destroyed(), "Context is destroyed")?;
        let result = self.gl.invoke(name, args)?;
        if self.throw_on_webgl_error.get() && name != "getError" {
            if let GlValue::Number(code) = self.gl.invoke("getError", &[])? {
                ensure(code == 0., &format!("WebGL error {code} in {name}"))?;
            }
        }
        Ok(result)
    }
    pub(crate) fn same(&self, other: &Self) -> Result<()> {
        ensure(Rc::ptr_eq(&self.gl, &other.gl), "Resources belong to different contexts")
    }
}
pub(crate) fn ensure(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(RuntimeError::new(message))
    }
}
pub(crate) fn size<O>(n: usize) -> Result<GlValue<O>> {
    ensure(n <= i32::MAX as usize, "GL size or offset exceeds signed 32-bit range")?;
    Ok(GlValue::Number(n as f64))
}
pub(crate) fn range(offset: usize, length: usize, size: usize) -> Result<()> {
    ensure(offset.checked_add(length).is_some_and(|end| end <= size), "Buffer range out of bounds")
}
pub(crate) struct Resource<G: Gl> {
    pub context: Context<G>,
    object: Rc<RefCell<Option<G::Object>>>,
    id: u64,
    delete: &'static str,
}
impl<G: Gl> Resource<G> {
    pub fn create(
        context: &Context<G>,
        method: &str,
        args: &[GlValue<G::Object>],
        delete: &'static str,
    ) -> Result<Self> {
        match context.call(method, args)? {
            GlValue::Object(object) => {
                let object = Rc::new(RefCell::new(Some(object)));
                let id = context.next_resource.get();
                context.next_resource.set(id + 1);
                context.registry.borrow_mut().insert(id, Deletion { object: Rc::downgrade(&object), method: delete });
                Ok(Self { context: context.resource_context(), object, delete, id })
            }
            _ => Err(RuntimeError::new(format!("{method} failed to allocate a GPU object"))),
        }
    }
    pub fn value(&self) -> Result<GlValue<G::Object>> {
        ensure(!self.context.destroyed.get(), "Context is destroyed")?;
        self.object
            .borrow()
            .as_ref()
            .cloned()
            .map(GlValue::Object)
            .ok_or_else(|| RuntimeError::new("GPU resource is destroyed"))
    }
    pub fn is_destroyed(&self) -> bool {
        self.object.borrow().is_none() || self.context.destroyed.get()
    }
    pub fn destroy(&self) -> Result<()> {
        if let Some(object) = self.object.borrow().as_ref() {
            self.context.gl.invoke(self.delete, &[GlValue::Object(object.clone())])?;
        }
        self.object.borrow_mut().take();
        self.context.registry.borrow_mut().remove(&self.id);
        Ok(())
    }
}
impl<G: Gl> Drop for Resource<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}

impl<G: Gl> Context<G> {
    pub fn uniform_state(&self) -> std::cell::RefMut<'_, super::UniformState> {
        self.uniform_state.borrow_mut()
    }
}
