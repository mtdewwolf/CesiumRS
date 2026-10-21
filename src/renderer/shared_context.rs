//! Cesium SharedContext: one GPU context, independent scene surfaces and leases.
use super::{context::*, *};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
pub trait SharedSurfaceHost<G: Gl> {
    type Surface: Clone;
    fn same_surface(&self, a: &Self::Surface, b: &Self::Surface) -> bool;
    fn validate_surface(&self, surface: &Self::Surface) -> Result<()>;
    fn dimensions(&self, surface: &Self::Surface) -> Result<[u32; 2]>;
    fn grow_offscreen(&self, context: &Context<G>, minimum: [u32; 2]) -> Result<()>;
    fn blit(
        &self,
        context: &Context<G>,
        surface: &Self::Surface,
        source: TextureRegion,
        destination: [u32; 2],
    ) -> Result<()>;
}
struct SharedState<G: Gl, H: SharedSurfaceHost<G>> {
    context: Context<G>,
    host: Rc<H>,
    surfaces: RefCell<Vec<H::Surface>>,
    auto_destroy: bool,
}
pub struct SharedContext<G: Gl, H: SharedSurfaceHost<G>> {
    state: Rc<SharedState<G, H>>,
}
pub struct SceneContext<G: Gl, H: SharedSurfaceHost<G>> {
    state: Rc<SharedState<G, H>>,
    context: Context<G>,
    surface: H::Surface,
    alive: Rc<Cell<bool>>,
}
impl<G: Gl + 'static, H: SharedSurfaceHost<G> + 'static> SharedContext<G, H> {
    pub fn new(context: Context<G>, host: H, auto_destroy: bool) -> Self {
        Self {
            state: Rc::new(SharedState { context, host: Rc::new(host), surfaces: RefCell::new(vec![]), auto_destroy }),
        }
    }
    pub fn create_scene_context(&self, surface: H::Surface) -> Result<SceneContext<G, H>> {
        ensure(!self.is_destroyed(), "Shared context is destroyed")?;
        self.state.host.validate_surface(&surface)?;
        ensure(
            !self.state.surfaces.borrow().iter().any(|s| self.state.host.same_surface(s, &surface)),
            "Canvas is already associated with a scene",
        )?;
        self.state.surfaces.borrow_mut().push(surface.clone());
        let alive = Rc::new(Cell::new(false));
        let mut context = self.state.context.clone();
        context.local_destroyed = Some(alive.clone());
        let host = self.state.host.clone();
        let dimensions_surface = surface.clone();
        context.dimensions = Some(Rc::new(move || host.dimensions(&dimensions_surface)));
        let state = Rc::downgrade(&self.state);
        let destroy_surface = surface.clone();
        let destroyed = alive.clone();
        context.destroy_override = Some(Rc::new(move || {
            let already_destroyed = destroyed.replace(true);
            if let Some(state) = state.upgrade() {
                if !already_destroyed {
                    state.surfaces.borrow_mut().retain(|s| !state.host.same_surface(s, &destroy_surface));
                }
                if state.auto_destroy && state.surfaces.borrow().is_empty() {
                    state.context.destroy()?;
                }
            }
            Ok(())
        }));
        Ok(SceneContext { state: self.state.clone(), context, surface, alive })
    }
    pub fn context(&self) -> &Context<G> {
        &self.state.context
    }
    pub fn active_scenes(&self) -> usize {
        self.state.surfaces.borrow().len()
    }
    pub fn destroy(&self) -> Result<()> {
        self.state.context.destroy()
    }
    pub fn is_destroyed(&self) -> bool {
        self.state.context.is_destroyed()
    }
}
impl<G: Gl, H: SharedSurfaceHost<G>> SceneContext<G, H> {
    pub fn context(&self) -> Result<&Context<G>> {
        ensure(!self.is_destroyed(), "Scene context is destroyed")?;
        Ok(&self.context)
    }
    pub fn begin_frame(&self) -> Result<()> {
        let context = self.context()?;
        self.state.host.grow_offscreen(&self.state.context, context.drawing_buffer_size()?)?;
        context.invalidate_render_state();
        Ok(())
    }
    pub fn end_frame(&self) -> Result<()> {
        let context = self.context()?;
        let [w, h] = context.drawing_buffer_size()?;
        let offscreen = self.state.context.drawing_buffer_size()?;
        ensure(offscreen[0] >= w && offscreen[1] >= h, "Call begin_frame before presenting a resized scene")?;
        self.state.host.blit(
            &self.state.context,
            &self.surface,
            TextureRegion { x: 0, y: offscreen[1] - h, width: w, height: h },
            [w, h],
        )?;
        self.state.context.end_frame()
    }
    pub fn destroy(&self) -> Result<()> {
        self.context.destroy()
    }
    pub fn is_destroyed(&self) -> bool {
        self.alive.get() || self.state.context.is_destroyed()
    }
}
impl<G: Gl, H: SharedSurfaceHost<G>> Drop for SceneContext<G, H> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}

#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub struct BrowserSharedSurfaceHost;
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
impl SharedSurfaceHost<super::browser::BrowserGl> for BrowserSharedSurfaceHost {
    type Surface = wasm_bindgen::JsValue;
    fn same_surface(&self, a: &Self::Surface, b: &Self::Surface) -> bool {
        a == b
    }
    fn validate_surface(&self, s: &Self::Surface) -> Result<()> {
        use wasm_bindgen::JsCast;
        let f = js_sys::Reflect::get(s, &"getContext".into())
            .map_err(js_error)?
            .dyn_into::<js_sys::Function>()
            .map_err(js_error)?;
        ensure(!f.call1(s, &"2d".into()).map_err(js_error)?.is_null(), "Shared surface must provide a 2D context")
    }
    fn dimensions(&self, s: &Self::Surface) -> Result<[u32; 2]> {
        Ok([
            js_sys::Reflect::get(s, &"width".into()).map_err(js_error)?.as_f64().unwrap_or(0.) as u32,
            js_sys::Reflect::get(s, &"height".into()).map_err(js_error)?.as_f64().unwrap_or(0.) as u32,
        ])
    }
    fn grow_offscreen(&self, context: &Context<super::browser::BrowserGl>, minimum: [u32; 2]) -> Result<()> {
        let canvas = js_sys::Reflect::get(context.host().raw_context(), &"canvas".into()).map_err(js_error)?;
        let current = self.dimensions(&canvas)?;
        for (i, name) in ["width", "height"].into_iter().enumerate() {
            if current[i] < minimum[i] {
                js_sys::Reflect::set(&canvas, &name.into(), &minimum[i].into()).map_err(js_error)?;
            }
        }
        Ok(())
    }
    fn blit(
        &self,
        context: &Context<super::browser::BrowserGl>,
        surface: &Self::Surface,
        source: TextureRegion,
        dest: [u32; 2],
    ) -> Result<()> {
        use wasm_bindgen::JsCast;
        let canvas = js_sys::Reflect::get(context.host().raw_context(), &"canvas".into()).map_err(js_error)?;
        let get = js_sys::Reflect::get(surface, &"getContext".into())
            .map_err(js_error)?
            .dyn_into::<js_sys::Function>()
            .map_err(js_error)?;
        let target = get.call1(surface, &"2d".into()).map_err(js_error)?;
        let draw = js_sys::Reflect::get(&target, &"drawImage".into())
            .map_err(js_error)?
            .dyn_into::<js_sys::Function>()
            .map_err(js_error)?;
        let args = js_sys::Array::new();
        args.push(&canvas);
        for n in [source.x, source.y, source.width, source.height, 0, 0, dest[0], dest[1]] {
            args.push(&n.into());
        }
        draw.apply(&target, &args).map_err(js_error)?;
        Ok(())
    }
}
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
fn js_error(e: wasm_bindgen::JsValue) -> crate::core::RuntimeError {
    crate::core::RuntimeError::new(format!("Shared canvas host: {e:?}"))
}
