//! GLSL generation and executable programs. Shader compilation/link errors retain
//! driver diagnostics. Programs accept explicit uniforms; automatic scene
//! uniforms and Cesium's builtin dependency injection are a later layer.
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
/// Explicit GLSL sources, without the builtin/automatic-uniform dependency
/// table. It supports Cesium's source/define/precision/version assembly for
/// WebGL2. Callers supply declarations for every referenced `czm_` symbol.
#[derive(Clone, Debug, Default)]
pub struct ShaderSource {
    pub sources: Vec<String>,
    pub defines: Vec<String>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderCapabilities {
    pub texture_float_linear: bool,
    pub floating_point_texture: bool,
}
impl ShaderSource {
    pub fn new(source: impl Into<String>) -> Self {
        Self { sources: vec![source.into()], defines: vec![] }
    }
    pub fn replace_main(source: &str, renamed: &str) -> String {
        regex::Regex::new(r"void\s+main\s*\(\s*(?:void)?\s*\)")
            .unwrap()
            .replace_all(source, regex::NoExpand(&format!("void {renamed}()")))
            .into_owned()
    }
    pub fn combine_webgl2(&self, fragment: bool, capabilities: ShaderCapabilities) -> Result<String> {
        let source = self.sources.iter().map(|s| format!("\n#line 0\n{s}")).collect::<String>();
        let source = regex::Regex::new(r"//[^\r\n]*").unwrap().replace_all(&source, "");
        let source = regex::Regex::new(r"(?s)/\*\*.*?\*/")
            .unwrap()
            .replace_all(&source, |c: &regex::Captures<'_>| "\n".repeat(c[0].bytes().filter(|b| *b == b'\n').count()));
        let version_re = regex::Regex::new(r"#version\s+(.*?)\n").unwrap();
        let versions: Vec<_> = version_re.captures_iter(&source).map(|c| c[1].to_string()).collect();
        ensure(versions.windows(2).all(|p| p[0] == p[1]), "Inconsistent GLSL versions")?;
        let source = version_re.replace_all(&source, "\n");
        let ext_re = regex::Regex::new(r"#extension[^\r\n]*\n").unwrap();
        let extensions = ext_re.find_iter(&source).map(|m| m.as_str()).collect::<String>();
        let source = ext_re.replace_all(&source, "\n");
        let source = regex::Regex::new(r"precision\s(lowp|mediump|highp)\s(float|int);").unwrap().replace(&source, "");
        let mut result = format!("#version 300 es\n{extensions}");
        if fragment {
            result.push_str("\n#ifdef GL_FRAGMENT_PRECISION_HIGH\n    precision highp float;\n    precision highp int;\n#else\n    precision mediump float;\n    precision mediump int;\n    #define highp mediump\n#endif\n");
        }
        result.push_str("precision highp sampler3D;\n\n");
        for define in &self.defines {
            if !define.is_empty() {
                result.push_str(&format!("#define {define}\n"));
            }
        }
        if capabilities.texture_float_linear {
            result.push_str("#define OES_texture_float_linear\n\n");
        }
        if capabilities.floating_point_texture {
            result.push_str("#define OES_texture_float\n\n");
        }
        result.push_str("\n#line 0\n");
        if fragment
            && !regex::Regex::new(r"layout\s*\(location\s*=\s*0\)\s*out\s+vec4\s+out_FragColor;")
                .unwrap()
                .is_match(&source)
            && !source.contains("czm_out_FragColor")
            && source.contains("out_FragColor")
        {
            result.push_str("layout(location = 0) out vec4 out_FragColor;\n\n");
        }
        result.push_str(&source);
        Ok(result)
    }
}

pub struct ShaderProgram<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    locations: RefCell<HashMap<String, GlValue<G::Object>>>,
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
    /// Sources must be complete GLSL ES 3.00 shaders; use ShaderSource to assemble
    /// source fragments. Attribute bindings are applied before linking.
    pub fn new(context: &Context<G>, vertex: &str, fragment: &str, attributes: &[(&str, u32)]) -> Result<Self> {
        let compile = |datatype, source: &str| -> Result<Resource<G>> {
            let shader =
                Resource::create(context, "createShader", &[GlValue::Number(datatype as f64)], "deleteShader")?;
            context.call("shaderSource", &[shader.value()?, source.into()])?;
            context.call("compileShader", &[shader.value()?])?;
            status(context, "getShaderParameter", &shader, gl::COMPILE_STATUS, "getShaderInfoLog")?;
            Ok(shader)
        };
        let vs = compile(gl::VERTEX_SHADER, vertex)?;
        let fs = compile(gl::FRAGMENT_SHADER, fragment)?;
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
        Ok(Self { resource, locations: RefCell::new(HashMap::new()) })
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
    pub fn set_uniform(&self, name: &str, value: &UniformValue) -> Result<()> {
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
#[derive(Clone, Debug)]
pub enum UniformValue {
    Float(Vec<f32>),
    Int(Vec<i32>),
    Matrix { dimension: u32, values: Vec<f32> },
}
