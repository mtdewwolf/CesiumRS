//! Context caches, default resources, viewport commands and color-buffer picking.
use super::{context::*, *};
use crate::core::{Color, PixelFormat, RuntimeError};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    rc::{Rc, Weak},
};
pub(crate) struct ContextCache<G: Gl> {
    defaults: RefCell<HashMap<u8, Rc<Texture<G>>>>,
    cube: RefCell<Option<Rc<CubeMap<G>>>>,
    quad: RefCell<Option<Rc<VertexArray<G>>>>,
    shaders: RefCell<Option<ShaderCache<G>>>,
    textures: RefCell<TextureCache<G>>,
    pub(crate) automatic_textures: RefCell<HashMap<String, TextureBinding<G>>>,
}
impl<G: Gl> Default for ContextCache<G> {
    fn default() -> Self {
        Self {
            defaults: RefCell::new(HashMap::new()),
            cube: RefCell::new(None),
            quad: RefCell::new(None),
            shaders: RefCell::new(None),
            textures: RefCell::new(TextureCache::default()),
            automatic_textures: RefCell::new(HashMap::new()),
        }
    }
}
impl<G: Gl> ContextCache<G> {
    pub(crate) fn clear(&self) {
        self.automatic_textures.borrow_mut().clear();
        self.defaults.borrow_mut().clear();
        self.cube.borrow_mut().take();
        self.quad.borrow_mut().take();
        self.shaders.borrow_mut().take();
        let _ = self.textures.borrow_mut().destroy();
    }
}
#[derive(Default)]
pub(crate) struct PickStore {
    objects: RefCell<BTreeMap<u32, Rc<dyn Any>>>,
    next: Cell<u32>,
}
impl PickStore {
    pub(crate) fn clear(&self) {
        self.objects.borrow_mut().clear();
    }
}
pub struct PickId {
    pub key: u32,
    pub color: Color,
    store: Weak<PickStore>,
    destroyed: Cell<bool>,
}
impl PickId {
    pub fn object(&self) -> Option<Rc<dyn Any>> {
        if self.destroyed.get() {
            return None;
        }
        self.store.upgrade()?.objects.borrow().get(&self.key).cloned()
    }
    pub fn object_as<T: Any>(&self) -> Option<Rc<T>> {
        self.object()?.downcast().ok()
    }
    pub fn set_object<T: Any>(&self, object: Rc<T>) -> Result<()> {
        ensure(!self.destroyed.get(), "Pick ID is destroyed")?;
        let store = self.store.upgrade().ok_or_else(|| RuntimeError::new("Pick context no longer exists"))?;
        ensure(store.objects.borrow().contains_key(&self.key), "Pick context is destroyed")?;
        store.objects.borrow_mut().insert(self.key, object);
        Ok(())
    }
    pub fn destroy(&self) {
        if let Some(store) = self.store.upgrade() {
            store.objects.borrow_mut().remove(&self.key);
        }
        self.destroyed.set(true);
    }
}
impl Drop for PickId {
    fn drop(&mut self) {
        self.destroy();
    }
}
pub struct ViewportCommandOptions<G: Gl> {
    pub render_state: RenderState,
    pub framebuffer: Option<Rc<Framebuffer<G>>>,
    pub uniform_map: Option<UniformMap>,
    pub owner: Option<String>,
    pub pass: Option<Pass>,
}
impl<G: Gl> Default for ViewportCommandOptions<G> {
    fn default() -> Self {
        Self { render_state: RenderState::default(), framebuffer: None, uniform_map: None, owner: None, pass: None }
    }
}
pub struct ViewportCommand<G: Gl> {
    pub vertex_array: Rc<VertexArray<G>>,
    pub shader_program: Rc<ShaderProgram<G>>,
    pub options: ViewportCommandOptions<G>,
    pub uniforms: Vec<(String, UniformValue)>,
    pub textures: Vec<(String, TextureBinding<G>)>,
}
impl<G: Gl> ViewportCommand<G> {
    pub fn execute(&self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        let mut command = DrawCommand::new(&self.vertex_array, &self.shader_program);
        command.render_state = self.options.render_state.clone();
        command.framebuffer = self.options.framebuffer.clone();
        command.uniform_map = self.options.uniform_map.clone();
        command.metadata.owner = self.options.owner.clone();
        command.pass = self.options.pass;
        command.uniforms = self.uniforms.clone();
        command.samplers = self.textures.iter().map(|(n, t)| (n.clone(), t.sampled())).collect();
        command.execute(context, pass)
    }
}
impl<G: Gl> Context<G> {
    pub(crate) fn cache(&self) -> Result<Rc<ContextCache<G>>> {
        ensure(!self.is_destroyed(), "Context is destroyed")?;
        self.cache
            .as_ref()
            .cloned()
            .or_else(|| self.cache_ref.upgrade())
            .ok_or_else(|| RuntimeError::new("Context cache owner has been dropped"))
    }
    fn default_color(&self, kind: u8) -> Result<Rc<Texture<G>>> {
        let cache = self.cache()?;
        if let Some(t) = cache.defaults.borrow().get(&kind) {
            if !t.is_destroyed() {
                return Ok(t.clone());
            }
        }
        let mut options = TextureOptions::rgba(1, 1);
        options.flip_y = false;
        let bytes = match kind {
            0 => vec![255; 4],
            1 => {
                options.pixel_format = PixelFormat::Rgb;
                vec![0; 3]
            }
            _ => {
                options.pixel_format = PixelFormat::Rgb;
                vec![128, 128, 255]
            }
        };
        let texture = Rc::new(Texture::new(self, options, Some(&bytes))?);
        cache.defaults.borrow_mut().insert(kind, texture.clone());
        Ok(texture)
    }
    pub fn default_texture(&self) -> Result<Rc<Texture<G>>> {
        self.default_color(0)
    }
    pub fn default_emissive_texture(&self) -> Result<Rc<Texture<G>>> {
        self.default_color(1)
    }
    pub fn default_normal_texture(&self) -> Result<Rc<Texture<G>>> {
        self.default_color(2)
    }
    pub fn default_cube_map(&self) -> Result<Rc<CubeMap<G>>> {
        let cache = self.cache()?;
        if let Some(cube) = cache.cube.borrow().as_ref() {
            if !cube.is_destroyed() {
                return Ok(cube.clone());
            }
        }
        let mut options = TextureOptions::rgba(1, 1);
        options.flip_y = false;
        let cube = Rc::new(CubeMap::new(
            self,
            options,
            Some(std::array::from_fn(|_| TextureSource::Pixels { width: 1, height: 1, bytes: vec![255; 4] })),
        )?);
        *cache.cube.borrow_mut() = Some(cube.clone());
        Ok(cube)
    }
    pub fn get_viewport_quad_vertex_array(&self) -> Result<Rc<VertexArray<G>>> {
        let cache = self.cache()?;
        if let Some(quad) = cache.quad.borrow().as_ref() {
            if !quad.is_destroyed() {
                return Ok(quad.clone());
            }
        }
        let quad = ViewportQuad::new(self)?.vertex_array().clone();
        *cache.quad.borrow_mut() = Some(quad.clone());
        Ok(quad)
    }
    pub fn with_shader_cache<R>(&self, f: impl FnOnce(&mut ShaderCache<G>) -> Result<R>) -> Result<R> {
        let cache = self.cache()?;
        let mut shaders = cache.shaders.borrow_mut();
        if shaders.is_none() {
            *shaders = Some(ShaderCache::new(&self.resource_context()));
        }
        f(shaders.as_mut().unwrap())
    }
    pub fn with_texture_cache<R>(&self, f: impl FnOnce(&mut TextureCache<G>) -> Result<R>) -> Result<R> {
        let cache = self.cache()?;
        let result = f(&mut cache.textures.borrow_mut());
        result
    }
    pub fn create_viewport_quad_command(
        &self,
        fragment: ShaderSource,
        options: ViewportCommandOptions<G>,
    ) -> Result<ViewportCommand<G>> {
        let vertex_array = self.get_viewport_quad_vertex_array()?;
        let shader_program =
            self.with_shader_cache(|c| c.get_shader_program(&ViewportQuad::<G>::shader_options(fragment)))?;
        Ok(ViewportCommand { vertex_array, shader_program, options, uniforms: vec![], textures: vec![] })
    }
    pub fn create_pick_id<T: Any>(&self, object: Rc<T>) -> Result<PickId> {
        ensure(!self.is_destroyed(), "Context is destroyed")?;
        let key = self.picks.next.get().checked_add(1).ok_or_else(|| RuntimeError::new("Out of unique Pick IDs."))?;
        self.picks.next.set(key);
        self.picks.objects.borrow_mut().insert(key, object);
        Ok(PickId { key, color: Color::from_rgba(key), store: Rc::downgrade(&self.picks), destroyed: Cell::new(false) })
    }
    pub fn get_object_by_pick_color(&self, color: u32) -> Option<Rc<dyn Any>> {
        self.picks.objects.borrow().get(&color).cloned()
    }
    pub fn pick_object<T: Any>(&self, color: u32) -> Option<Rc<T>> {
        self.get_object_by_pick_color(color)?.downcast().ok()
    }
}
