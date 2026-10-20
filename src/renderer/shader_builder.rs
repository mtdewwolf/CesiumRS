//! CesiumJS ShaderBuilder with owned, cloneable source parts.
use super::{context::*, *};
use crate::core::RuntimeError;
use std::collections::{BTreeMap, HashMap};
#[derive(Clone, Debug, Default)]
struct Parts {
    defines: Vec<String>,
    uniforms: Vec<String>,
    varyings: Vec<String>,
    lines: Vec<String>,
    structs: Vec<String>,
    functions: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct ShaderBuilder {
    position: Option<String>,
    next_location: u32,
    attribute_locations: BTreeMap<String, u32>,
    attributes: Vec<String>,
    structs: HashMap<String, ShaderStruct>,
    functions: HashMap<String, ShaderFunction>,
    vertex: Parts,
    fragment: Parts,
}
impl Default for ShaderBuilder {
    fn default() -> Self {
        Self {
            position: None,
            next_location: 1,
            attribute_locations: BTreeMap::new(),
            attributes: vec![],
            structs: HashMap::new(),
            functions: HashMap::new(),
            vertex: Parts::default(),
            fragment: Parts::default(),
        }
    }
}
impl ShaderBuilder {
    fn parts(&mut self, d: ShaderDestination, mut f: impl FnMut(&mut Parts)) {
        if d.includes_vertex_shader() {
            f(&mut self.vertex);
        }
        if d.includes_fragment_shader() {
            f(&mut self.fragment);
        }
    }
    pub fn attribute_locations(&self) -> &BTreeMap<String, u32> {
        &self.attribute_locations
    }
    pub fn add_define(&mut self, identifier: &str, value: Option<&str>, destination: ShaderDestination) {
        let line = value.map(|v| format!("{identifier} {v}")).unwrap_or(identifier.into());
        self.parts(destination, |p| p.defines.push(line.clone()));
    }
    pub fn add_uniform(&mut self, datatype: &str, identifier: &str, destination: ShaderDestination) {
        self.parts(destination, |p| p.uniforms.push(format!("uniform {datatype} {identifier};")));
    }
    pub fn set_position_attribute(&mut self, datatype: &str, identifier: &str) -> Result<u32> {
        ensure(self.position.is_none(), "Position attribute must be set exactly once")?;
        self.position = Some(format!("in {datatype} {identifier};"));
        self.attribute_locations.insert(identifier.into(), 0);
        Ok(0)
    }
    pub fn add_attribute(&mut self, datatype: &str, identifier: &str) -> u32 {
        let location = self.next_location;
        self.attributes.push(format!("in {datatype} {identifier};"));
        self.attribute_locations.insert(identifier.into(), location);
        self.next_location += match datatype {
            "mat2" => 2,
            "mat3" => 3,
            "mat4" => 4,
            _ => 1,
        };
        location
    }
    pub fn add_varying(&mut self, datatype: &str, identifier: &str, qualifier: Option<&str>) {
        let q = qualifier.map(|q| format!("{q} ")).unwrap_or_default();
        self.vertex.varyings.push(format!("{q}out {datatype} {identifier};"));
        self.fragment.varyings.push(format!("{q}in {datatype} {identifier};"));
    }
    pub fn add_struct(&mut self, id: &str, name: &str, destination: ShaderDestination) {
        self.structs.insert(id.into(), ShaderStruct::new(name));
        self.parts(destination, |p| p.structs.push(id.into()));
    }
    pub fn add_struct_field(&mut self, id: &str, datatype: &str, identifier: &str) -> Result<()> {
        self.structs
            .get_mut(id)
            .ok_or_else(|| RuntimeError::new("Unknown shader struct"))?
            .add_field(datatype, identifier);
        Ok(())
    }
    pub fn add_function(&mut self, id: &str, signature: &str, destination: ShaderDestination) {
        self.functions.insert(id.into(), ShaderFunction::new(signature));
        self.parts(destination, |p| p.functions.push(id.into()));
    }
    pub fn add_function_lines(&mut self, id: &str, lines: &[impl AsRef<str>]) -> Result<()> {
        self.functions.get_mut(id).ok_or_else(|| RuntimeError::new("Unknown shader function"))?.add_lines(lines);
        Ok(())
    }
    pub fn add_vertex_lines(&mut self, lines: &[impl AsRef<str>]) {
        self.vertex.lines.extend(lines.iter().map(|s| s.as_ref().into()));
    }
    pub fn add_fragment_lines(&mut self, lines: &[impl AsRef<str>]) {
        self.fragment.lines.extend(lines.iter().map(|s| s.as_ref().into()));
    }
    pub fn sources(&self) -> (ShaderSource, ShaderSource) {
        let generate = |parts: &Parts, vertex: bool| {
            let mut lines = vec![];
            if vertex {
                lines.extend(self.position.iter().cloned());
                lines.extend(self.attributes.clone());
            }
            lines.extend(parts.uniforms.clone());
            lines.extend(parts.varyings.clone());
            for id in &parts.structs {
                lines.extend(self.structs[id].generate_glsl_lines());
            }
            for id in &parts.functions {
                lines.extend(self.functions[id].generate_glsl_lines());
            }
            lines.extend(parts.lines.clone());
            ShaderSource { sources: vec![lines.join("\n")], defines: parts.defines.clone(), ..Default::default() }
        };
        (generate(&self.vertex, true), generate(&self.fragment, false))
    }
    pub fn compile_shader_program<G: Gl>(
        &self,
        context: &Context<G>,
        capabilities: ShaderCapabilities,
    ) -> Result<ShaderProgram<G>> {
        let (vertex, fragment) = self.sources();
        let attributes: Vec<_> = self.attribute_locations.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        ShaderProgram::new(
            context,
            &vertex.combine_webgl2(false, capabilities)?,
            &fragment.combine_webgl2(true, capabilities)?,
            &attributes,
        )
    }
    /// Build through an explicitly owned shader cache. Rust cache leases replace
    /// the context-global JavaScript reference-counting hooks.
    pub fn build_shader_program<G: Gl>(&self, cache: &mut ShaderCache<G>) -> Result<std::rc::Rc<ShaderProgram<G>>> {
        let (vertex_shader_source, fragment_shader_source) = self.sources();
        cache.get_shader_program(&ShaderProgramOptions {
            vertex_shader_source,
            fragment_shader_source,
            attribute_locations: self.attribute_locations.clone(),
        })
    }
}
