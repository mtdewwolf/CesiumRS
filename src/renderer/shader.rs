//! GLSL generation and executable programs. Shader compilation/link errors retain
//! driver diagnostics. Reflection, explicit/callback uniforms, shader caches
//! and automatic uniform providers share this executable program layer.
use super::context::*;
use crate::core::{webgl_constants as gl, RuntimeError};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderDestination(pub u32);
impl ShaderDestination {
    pub const NONE: Self = Self(0);
    pub const VERTEX: Self = Self(1);
    pub const FRAGMENT: Self = Self(2);
    pub const BOTH: Self = Self(3);
    pub fn includes_vertex_shader(self) -> bool {
        self.0 & 1 != 0
    }
    pub fn includes_fragment_shader(self) -> bool {
        self.0 & 2 != 0
    }
    pub fn union(destinations: &[Self]) -> Self {
        Self(destinations.iter().fold(0, |a, b| a | b.0))
    }
    pub fn intersection(destinations: &[Self]) -> Option<Self> {
        Some(Self(destinations.iter().copied().reduce(|a, b| Self(a.0 & b.0))?.0))
    }
}
#[derive(Clone, Debug)]
pub struct ShaderFunction {
    pub signature: String,
    pub body: Vec<String>,
}
impl ShaderFunction {
    pub fn new(signature: impl Into<String>) -> Self {
        Self { signature: signature.into(), body: vec![] }
    }
    pub fn add_lines(&mut self, lines: &[impl AsRef<str>]) {
        self.body.extend(lines.iter().map(|s| format!("    {}", s.as_ref())));
    }
    pub fn generate_glsl_lines(&self) -> Vec<String> {
        let mut v = vec![self.signature.clone(), "{".into()];
        v.extend(self.body.clone());
        v.push("}".into());
        v
    }
}
#[derive(Clone, Debug)]
pub struct ShaderStruct {
    pub name: String,
    pub fields: Vec<String>,
}
impl ShaderStruct {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), fields: vec![] }
    }
    pub fn add_field(&mut self, datatype: &str, identifier: &str) {
        self.fields.push(format!("    {datatype} {identifier};"));
    }
    pub fn generate_glsl_lines(&self) -> Vec<String> {
        let mut v = vec![format!("struct {}", self.name), "{".into()];
        if self.fields.is_empty() {
            v.push("    float _empty;".into());
        } else {
            v.extend(self.fields.clone());
        }
        v.push("};".into());
        v
    }
}
/// GLSL sources with Cesium builtin dependency and picking support.
#[derive(Clone, Debug)]
pub struct ShaderSource {
    pub sources: Vec<String>,
    pub defines: Vec<String>,
    pub include_built_ins: bool,
    pub pick_color_qualifier: Option<super::shader_source::PickColorQualifier>,
}
impl Default for ShaderSource {
    fn default() -> Self {
        Self { sources: vec![], defines: vec![], include_built_ins: true, pick_color_qualifier: None }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderCapabilities {
    pub texture_float_linear: bool,
    pub floating_point_texture: bool,
}
impl ShaderSource {
    pub fn new(source: impl Into<String>) -> Self {
        Self { sources: vec![source.into()], ..Default::default() }
    }
    pub fn replace_main(source: &str, renamed: &str) -> String {
        regex::Regex::new(r"void\s+main\s*\(\s*(?:void)?\s*\)")
            .unwrap()
            .replace_all(source, regex::NoExpand(&format!("void {renamed}()")))
            .into_owned()
    }
    pub fn combine_webgl2(&self, fragment: bool, capabilities: ShaderCapabilities) -> Result<String> {
        self.combine(fragment, capabilities, true)
    }
}

pub struct ShaderProgram<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    locations: RefCell<HashMap<String, GlValue<G::Object>>>,
    attributes: Vec<ShaderVariable>,
    uniforms: Vec<ShaderVariable>,
    vertex_source: String,
    fragment_source: String,
    source_options: Option<super::ShaderProgramOptions>,
    duplicate_uniforms: HashMap<String, String>,
    uniform_values: RefCell<HashMap<String, UniformValue>>,
}
fn status<G: Gl>(context: &Context<G>, method: &str, resource: &Resource<G>, pname: u32, log: &str) -> Result<()> {
    match context.call(method, &[resource.value()?, pname.into()])? {
        GlValue::Bool(true) => Ok(()),
        _ => {
            let message = match context.call(log, &[resource.value()?])? {
                GlValue::String(s) => s,
                _ => "No driver log".into(),
            };
            Err(RuntimeError::new(message))
        }
    }
}
impl<G: Gl> ShaderProgram<G> {
    /// Compile source fragments and retain them for Scene's derived commands.
    pub fn from_sources(context: &Context<G>, options: &super::ShaderProgramOptions) -> Result<Self> {
        let capabilities = context.capabilities()?;
        let caps = ShaderCapabilities {
            floating_point_texture: capabilities.floating_point_texture,
            texture_float_linear: capabilities.texture_float_linear,
        };
        let attrs: Vec<_> = options.attribute_locations.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let mut program = Self::new(
            context,
            &options.vertex_shader_source.combine(false, caps, context.webgl2())?,
            &options.fragment_shader_source.combine(true, caps, context.webgl2())?,
            &attrs,
        )?;
        program.source_options = Some(options.clone());
        Ok(program)
    }
    pub fn source_options(&self) -> Option<&super::ShaderProgramOptions> {
        self.source_options.as_ref()
    }
    /// Sources must be complete GLSL ES 3.00 shaders; use ShaderSource to assemble
    /// source fragments. Attribute bindings are applied before linking.
    pub fn new(context: &Context<G>, vertex: &str, fragment: &str, attributes: &[(&str, u32)]) -> Result<Self> {
        let precision = |datatype: u32| -> Result<bool> {
            match context.call("getShaderPrecisionFormat", &[gl::FRAGMENT_SHADER.into(), datatype.into()])? {
                GlValue::Record(v) => {
                    Ok(v["precision"].as_f64().unwrap_or(0.) > 0. || v["rangeMax"].as_f64().unwrap_or(0.) > 0.)
                }
                _ => Ok(true),
            }
        };
        let (fragment_text, duplicate_uniforms) = super::shader_precision::handle_uniform_precision_mismatches(
            vertex,
            fragment,
            precision(gl::HIGH_FLOAT)?,
            precision(gl::HIGH_INT)?,
        );
        let compile = |datatype, source: &str| -> Result<Resource<G>> {
            let shader =
                Resource::create(context, "createShader", &[GlValue::Number(datatype as f64)], "deleteShader")?;
            context.call("shaderSource", &[shader.value()?, source.into()])?;
            context.call("compileShader", &[shader.value()?])?;
            status(context, "getShaderParameter", &shader, gl::COMPILE_STATUS, "getShaderInfoLog")?;
            Ok(shader)
        };
        let vs = compile(gl::VERTEX_SHADER, vertex)?;
        let fs = compile(gl::FRAGMENT_SHADER, &fragment_text)?;
        let resource = Rc::new(Resource::create(context, "createProgram", &[], "deleteProgram")?);
        context.call("attachShader", &[resource.value()?, vs.value()?])?;
        context.call("attachShader", &[resource.value()?, fs.value()?])?;
        for (name, index) in attributes {
            context.call("bindAttribLocation", &[resource.value()?, (*index).into(), (*name).into()])?;
        }
        context.call("linkProgram", &[resource.value()?])?;
        status(context, "getProgramParameter", &resource, gl::LINK_STATUS, "getProgramInfoLog")?;
        context.call("detachShader", &[resource.value()?, vs.value()?])?;
        context.call("detachShader", &[resource.value()?, fs.value()?])?;
        let reflect = |count_pname, method: &str| -> Result<Vec<ShaderVariable>> {
            let count =
                match context.call("getProgramParameter", &[resource.value()?, GlValue::Number(count_pname as f64)])? {
                    GlValue::Number(n) => n as u32,
                    _ => 0,
                };
            let mut result = vec![];
            for i in 0..count {
                if let GlValue::Record(v) = context.call(method, &[resource.value()?, i.into()])? {
                    result.push(ShaderVariable {
                        name: v["name"].as_str().unwrap_or("").into(),
                        datatype: v["type"].as_u64().or_else(|| v["type"].as_f64().map(|n| n as u64)).unwrap_or(0)
                            as u32,
                        size: v["size"].as_u64().or_else(|| v["size"].as_f64().map(|n| n as u64)).unwrap_or(0) as usize,
                    });
                }
            }
            Ok(result)
        };
        let attributes = reflect(gl::ACTIVE_ATTRIBUTES, "getActiveAttrib")?;
        let uniforms = reflect(gl::ACTIVE_UNIFORMS, "getActiveUniform")?;
        let result = Self {
            resource,
            locations: RefCell::new(HashMap::new()),
            attributes,
            uniforms,
            vertex_source: vertex.into(),
            fragment_source: fragment_text,
            source_options: None,
            duplicate_uniforms,
            uniform_values: RefCell::new(HashMap::new()),
        };
        let mut unit = 0;
        for uniform in result.uniforms() {
            if matches!(
                uniform.datatype,
                35678
                    | 35679
                    | 35680
                    | 35682
                    | 36289
                    | 36292
                    | 36293
                    | 36298
                    | 36299
                    | 36300
                    | 36303
                    | 36306
                    | 36307
                    | 36308
                    | 36311
            ) {
                let values = (unit..unit + uniform.size).map(|v| v as i32).collect();
                result.set_uniform(
                    uniform.name.strip_suffix("[0]").unwrap_or(&uniform.name),
                    &UniformValue::IntArray { components: 1, values },
                )?;
                unit += uniform.size;
            }
        }
        if unit > 0 {
            context.call("useProgram", &[GlValue::Null])?;
        }
        Ok(result)
    }
    pub fn maximum_texture_unit_index(&self) -> usize {
        self.uniforms
            .iter()
            .filter(|v| {
                matches!(
                    v.datatype,
                    35678
                        | 35680
                        | 35679
                        | 35682
                        | 36289
                        | 36292
                        | 36293
                        | 36298
                        | 36299
                        | 36300
                        | 36303
                        | 36306
                        | 36307
                        | 36308
                        | 36311
                )
            })
            .map(|v| v.size)
            .sum()
    }
    pub fn vertex_attributes(&self) -> &[ShaderVariable] {
        &self.attributes
    }
    pub fn uniforms(&self) -> &[ShaderVariable] {
        &self.uniforms
    }
    pub fn vertex_shader_source(&self) -> &str {
        &self.vertex_source
    }
    pub fn fragment_shader_source(&self) -> &str {
        &self.fragment_source
    }
    pub fn number_of_vertex_attributes(&self) -> usize {
        self.attributes.len()
    }
    pub fn validate(&self) -> Result<()> {
        let c = &self.resource.context;
        c.call("validateProgram", &[self.resource.value()?])?;
        status(c, "getProgramParameter", &self.resource, gl::VALIDATE_STATUS, "getProgramInfoLog")
    }
    pub fn bind(&self) -> Result<()> {
        self.resource.context.call("useProgram", &[self.resource.value()?]).map(|_| ())
    }
    pub fn vertex_attribute_location(&self, name: &str) -> Result<i32> {
        match self.resource.context.call("getAttribLocation", &[self.resource.value()?, name.into()])? {
            GlValue::Number(n) => Ok(n as i32),
            _ => Err(RuntimeError::new("Invalid attribute location from GL host")),
        }
    }
    fn location(&self, name: &str) -> Result<GlValue<G::Object>> {
        self.resource.value()?;
        if let Some(location) = self.locations.borrow().get(name) {
            return Ok(location.clone());
        }
        let location = self.resource.context.call("getUniformLocation", &[self.resource.value()?, name.into()])?;
        self.locations.borrow_mut().insert(name.into(), location.clone());
        Ok(location)
    }
    /// The program is bound here so uniforms can never accidentally target a
    /// different program. Inactive uniforms are ignored as in WebGL.
    pub fn invalidate_uniform_cache(&self) {
        self.uniform_values.borrow_mut().clear();
    }
    pub fn get_uniform(&self, name: &str) -> Result<GlValue<G::Object>> {
        let location = self.location(name)?;
        self.resource.context.call("getUniform", &[self.resource.value()?, location])
    }
    pub fn original_uniform_name<'a>(&'a self, name: &'a str) -> &'a str {
        self.duplicate_uniforms.get(name).map(String::as_str).unwrap_or(name)
    }
    pub fn set_uniform(&self, name: &str, value: &UniformValue) -> Result<()> {
        self.set_uniform_cached(name, value)?;
        for (duplicate, original) in &self.duplicate_uniforms {
            if original == name {
                self.set_uniform_cached(duplicate, value)?;
            }
        }
        Ok(())
    }
    fn set_uniform_cached(&self, name: &str, value: &UniformValue) -> Result<()> {
        self.resource.value()?;
        if self.uniform_values.borrow().get(name) == Some(value) {
            return Ok(());
        }
        self.set_uniform_location(name, value)?;
        self.uniform_values.borrow_mut().insert(name.into(), value.clone());
        Ok(())
    }
    fn set_uniform_location(&self, name: &str, value: &UniformValue) -> Result<()> {
        let location = self.location(name)?;
        if matches!(location, GlValue::Null) {
            return Ok(());
        }
        self.bind()?;
        let (method, data) = match value {
            UniformValue::Float(v) => {
                ensure((1..=4).contains(&v.len()), "Uniform vector length must be 1 through 4")?;
                (format!("uniform{}fv", v.len()), GlValue::Floats(v.clone()))
            }
            UniformValue::Int(v) => {
                ensure((1..=4).contains(&v.len()), "Uniform vector length must be 1 through 4")?;
                (format!("uniform{}iv", v.len()), GlValue::Ints(v.clone()))
            }
            UniformValue::FloatArray { components, values } => {
                ensure(
                    (1..=4).contains(components) && values.len() % *components as usize == 0,
                    "Invalid float uniform array",
                )?;
                (format!("uniform{components}fv"), GlValue::Floats(values.clone()))
            }
            UniformValue::IntArray { components, values } => {
                ensure(
                    (1..=4).contains(components) && values.len() % *components as usize == 0,
                    "Invalid int uniform array",
                )?;
                (format!("uniform{components}iv"), GlValue::Ints(values.clone()))
            }
            UniformValue::UIntArray { components, values } => {
                ensure(
                    (1..=4).contains(components) && values.len() % *components as usize == 0,
                    "Invalid uint uniform array",
                )?;
                (format!("uniform{components}uiv"), GlValue::UInts(values.clone()))
            }
            UniformValue::MatrixArray { columns, rows, values } => {
                ensure(
                    (2..=4).contains(columns)
                        && (2..=4).contains(rows)
                        && values.len() % (*columns * *rows) as usize == 0,
                    "Invalid matrix uniform array",
                )?;
                let dimensions = if columns == rows { columns.to_string() } else { format!("{columns}x{rows}") };
                return self
                    .resource
                    .context
                    .call(
                        &format!("uniformMatrix{dimensions}fv"),
                        &[location, false.into(), GlValue::Floats(values.clone())],
                    )
                    .map(|_| ());
            }
            UniformValue::Matrix { dimension, values } => {
                ensure(
                    (2..=4).contains(dimension) && values.len() == (*dimension * *dimension) as usize,
                    "Invalid uniform matrix",
                )?;
                return self
                    .resource
                    .context
                    .call(
                        &format!("uniformMatrix{dimension}fv"),
                        &[location, false.into(), GlValue::Floats(values.clone())],
                    )
                    .map(|_| ());
            }
        };
        self.resource.context.call(&method, &[location, data]).map(|_| ())
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum UniformValue {
    FloatArray { components: u32, values: Vec<f32> },
    IntArray { components: u32, values: Vec<i32> },
    UIntArray { components: u32, values: Vec<u32> },
    MatrixArray { columns: u32, rows: u32, values: Vec<f32> },
    Float(Vec<f32>),
    Int(Vec<i32>),
    Matrix { dimension: u32, values: Vec<f32> },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ShaderVariable {
    pub name: String,
    pub datatype: u32,
    pub size: usize,
}
