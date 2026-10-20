//! Context identity, GL host protocol, and shared GPU object lifetime.
use crate::core::RuntimeError;
use std::{cell::RefCell, rc::Rc};
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
}
impl<G: Gl> Clone for Context<G> {
    fn clone(&self) -> Self {
        Self {
            gl: self.gl.clone(),
            uniform_state: self.uniform_state.clone(),
            frame_texture_units: self.frame_texture_units.clone(),
            state_driver: self.state_driver.clone(),
        }
    }
}
impl<G: Gl> Context<G> {
    pub fn new_webgl2(gl: G) -> Self {
        Self {
            uniform_state: Rc::new(RefCell::new(Default::default())),
            frame_texture_units: Rc::new(std::cell::Cell::new(0)),
            gl: Rc::new(gl),
            state_driver: Rc::new(RefCell::new(Default::default())),
        }
    }
    /// Direct access is for host integration. Call `invalidate_render_state`
    /// after changing fixed-function GL state outside the renderer.
    pub fn host(&self) -> &G {
        &self.gl
    }
    pub(crate) fn call(&self, name: &str, args: &[GlValue<G::Object>]) -> Result<GlValue<G::Object>> {
        self.gl.invoke(name, args)
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
    object: RefCell<Option<G::Object>>,
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
                Ok(Self { context: context.clone(), object: RefCell::new(Some(object)), delete })
            }
            _ => Err(RuntimeError::new(format!("{method} failed to allocate a GPU object"))),
        }
    }
    pub fn value(&self) -> Result<GlValue<G::Object>> {
        self.object
            .borrow()
            .as_ref()
            .cloned()
            .map(GlValue::Object)
            .ok_or_else(|| RuntimeError::new("GPU resource is destroyed"))
    }
    pub fn is_destroyed(&self) -> bool {
        self.object.borrow().is_none()
    }
    pub fn destroy(&self) -> Result<()> {
        if let Some(object) = self.object.borrow().as_ref() {
            self.context.call(self.delete, &[GlValue::Object(object.clone())])?;
        }
        self.object.borrow_mut().take();
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
