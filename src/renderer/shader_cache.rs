//! CesiumJS ShaderCache. Rc counts replace JS release hooks; released programs
//! remain reusable until destroy_released_shader_programs is called.
use super::{context::*, *};
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};
#[derive(Clone, Debug)]
pub struct ShaderProgramOptions {
    pub vertex_shader_source: ShaderSource,
    pub fragment_shader_source: ShaderSource,
    pub attribute_locations: BTreeMap<String, u32>,
}
struct Entry<G: Gl> {
    program: Rc<ShaderProgram<G>>,
    derived: Vec<String>,
    root: bool,
}
pub struct ShaderCache<G: Gl> {
    context: Context<G>,
    shaders: HashMap<String, Entry<G>>,
    destroyed: bool,
}
impl<G: Gl> ShaderCache<G> {
    pub fn new(context: &Context<G>) -> Self {
        Self { context: context.clone(), shaders: HashMap::new(), destroyed: false }
    }
    fn compile(&self, options: &ShaderProgramOptions) -> Result<Rc<ShaderProgram<G>>> {
        ensure(!self.destroyed, "Shader cache is destroyed")?;
        let capabilities = self.context.capabilities()?;
        let caps = ShaderCapabilities {
            floating_point_texture: capabilities.floating_point_texture,
            texture_float_linear: capabilities.texture_float_linear,
        };
        let attrs: Vec<_> = options.attribute_locations.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        Ok(Rc::new(ShaderProgram::new(
            &self.context,
            &options.vertex_shader_source.combine(false, caps, self.context.webgl2())?,
            &options.fragment_shader_source.combine(true, caps, self.context.webgl2())?,
            &attrs,
        )?))
    }
    fn key(options: &ShaderProgramOptions) -> String {
        format!(
            "{}:{}:{}",
            options.vertex_shader_source.cache_key(),
            options.fragment_shader_source.cache_key(),
            serde_json::to_string(&options.attribute_locations).unwrap()
        )
    }
    pub fn number_of_shaders(&self) -> usize {
        self.shaders.values().filter(|s| s.root).count()
    }
    pub fn get_shader_program(&mut self, options: &ShaderProgramOptions) -> Result<Rc<ShaderProgram<G>>> {
        ensure(!self.destroyed, "Shader cache is destroyed")?;
        let key = Self::key(options);
        if let Some(entry) = self.shaders.get(&key) {
            if !entry.program.is_destroyed() {
                return Ok(entry.program.clone());
            }
        }
        // Explicit destruction invalidates a cached program and its derived
        // tree; clear that tree before replacing the root under the same key.
        self.destroy_key(&key)?;
        let program = self.compile(options)?;
        self.shaders.insert(key, Entry { program: program.clone(), derived: vec![], root: true });
        Ok(program)
    }
    pub fn replace_shader_program(
        &mut self,
        previous: Option<Rc<ShaderProgram<G>>>,
        options: &ShaderProgramOptions,
    ) -> Result<Rc<ShaderProgram<G>>> {
        drop(previous);
        self.get_shader_program(options)
    }
    fn find_key(&self, program: &Rc<ShaderProgram<G>>) -> Result<String> {
        self.shaders
            .iter()
            .find(|(_, e)| Rc::ptr_eq(&e.program, program))
            .map(|(k, _)| k.clone())
            .ok_or_else(|| crate::core::RuntimeError::new("Program does not belong to this shader cache"))
    }
    pub fn get_derived_shader_program(
        &self,
        program: &Rc<ShaderProgram<G>>,
        keyword: &str,
    ) -> Option<Rc<ShaderProgram<G>>> {
        let key = self.find_key(program).ok()?;
        self.shaders.get(&format!("{keyword}{key}")).map(|e| e.program.clone())
    }
    pub fn create_derived_shader_program(
        &mut self,
        program: &Rc<ShaderProgram<G>>,
        keyword: &str,
        options: &ShaderProgramOptions,
    ) -> Result<Rc<ShaderProgram<G>>> {
        let parent = self.find_key(program)?;
        let derived = format!("{keyword}{parent}");
        ensure(!self.shaders.contains_key(&derived), "Derived shader already exists; use replace")?;
        let result = self.compile(options)?;
        self.shaders.get_mut(&parent).unwrap().derived.push(derived.clone());
        self.shaders.insert(derived, Entry { program: result.clone(), derived: vec![], root: false });
        Ok(result)
    }
    pub fn replace_derived_shader_program(
        &mut self,
        program: &Rc<ShaderProgram<G>>,
        keyword: &str,
        options: &ShaderProgramOptions,
    ) -> Result<Rc<ShaderProgram<G>>> {
        let parent = self.find_key(program)?;
        let derived = format!("{keyword}{parent}");
        self.destroy_key(&derived)?;
        self.shaders.get_mut(&parent).unwrap().derived.retain(|k| k != &derived);
        self.create_derived_shader_program(program, keyword, options)
    }
    fn destroy_key(&mut self, key: &str) -> Result<()> {
        if let Some(entry) = self.shaders.remove(key) {
            for child in &entry.derived {
                self.destroy_key(child)?;
            }
            entry.program.destroy()?;
        }
        Ok(())
    }
    pub fn release_shader_program(&self, program: Option<Rc<ShaderProgram<G>>>) {
        drop(program);
    }
    pub fn destroy_released_shader_programs(&mut self) -> Result<()> {
        let keys: Vec<_> = self
            .shaders
            .iter()
            .filter(|(_, e)| e.root && Rc::strong_count(&e.program) == 1)
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            self.destroy_key(&key)?;
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        let keys: Vec<_> = self.shaders.keys().cloned().collect();
        for key in keys {
            self.destroy_key(&key)?;
        }
        self.destroyed = true;
        Ok(())
    }
}
impl<G: Gl> Drop for ShaderCache<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
