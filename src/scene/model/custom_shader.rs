//! CesiumJS `Scene/Model/CustomShader`: a user GLSL shader plus its uniforms,
//! varyings and built-in variable usage analysis.
//!
//! The uniform/varying/value containers are shared, not cloned: a custom
//! shader is passed to many models and primitives, and `setUniform` must be
//! visible to every draw. Values therefore live in `Rc<RefCell<_>>` and the
//! uniform-map getters capture those cells rather than the shader itself
//! (which would form an ownership cycle).

use super::{
    texture_manager::{TextureManager, TextureManagerWarning},
    texture_uniform::TextureUniform,
    CustomShaderMode, CustomShaderTranslucencyMode, LightingModel, UniformType, VaryingType,
};
use crate::{
    core::{Cartesian2, Cartesian3, Cartesian4, ImageHost, Matrix2, Matrix3, Matrix4, RuntimeError},
    renderer::{Context, Gl, Pass, Texture, UniformValue as RendererUniformValue},
};
use regex::Regex;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::LazyLock};

type Result<T> = std::result::Result<T, RuntimeError>;

static ATTRIBUTE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[vf]sInput\.attributes\.([0-9A-Za-z_]+)").expect("attribute regex"));
static FEATURE_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[vf]sInput\.featureIds\.([0-9A-Za-z_]+)").expect("feature ID regex"));
static METADATA_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[vf]sInput\.(?:metadata|metadataClass|metadataStatistics)\.([0-9A-Za-z_]+)").expect("metadata regex")
});
static MATERIAL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"material\.([0-9A-Za-z_]+)").expect("material regex"));

/// A uniform value accepted by a custom shader.
#[derive(Clone, Debug)]
pub enum CustomShaderUniformValue {
    Bool(bool),
    Number(f64),
    Vec2(Cartesian2),
    Vec3(Cartesian3),
    Vec4(Cartesian4),
    Mat2(Matrix2),
    Mat3(Matrix3),
    Mat4(Matrix4),
    Sampler2D(TextureUniform),
}

pub type UniformValue = CustomShaderUniformValue;

impl CustomShaderUniformValue {
    /// The renderer value for a non-sampler uniform. `INT` and `BOOL` follow
    /// the declared uniform type; everything else is a float vector or matrix.
    pub fn to_renderer(&self, uniform_type: UniformType) -> Option<RendererUniformValue> {
        let values = match self {
            UniformValue::Bool(v) => vec![*v as i32 as f32],
            UniformValue::Number(v) => vec![*v as f32],
            UniformValue::Vec2(v) => v.to_array().iter().map(|v| *v as f32).collect(),
            UniformValue::Vec3(v) => v.to_array().iter().map(|v| *v as f32).collect(),
            UniformValue::Vec4(v) => v.to_array().iter().map(|v| *v as f32).collect(),
            UniformValue::Mat2(v) => v.0.iter().map(|v| *v as f32).collect(),
            UniformValue::Mat3(v) => v.0.iter().map(|v| *v as f32).collect(),
            UniformValue::Mat4(v) => v.0.iter().map(|v| *v as f32).collect(),
            UniformValue::Sampler2D(_) => return None,
        };
        Some(match uniform_type {
            UniformType::Int | UniformType::IntVec2 | UniformType::IntVec3 | UniformType::IntVec4 => {
                RendererUniformValue::Int(values.iter().map(|v| *v as i32).collect())
            }
            UniformType::Mat2 => RendererUniformValue::Matrix { dimension: 2, values },
            UniformType::Mat3 => RendererUniformValue::Matrix { dimension: 3, values },
            UniformType::Mat4 => RendererUniformValue::Matrix { dimension: 4, values },
            _ => RendererUniformValue::Float(values),
        })
    }
}

/// One declared uniform: its GLSL type and current value.
#[derive(Clone)]
pub struct UniformSpecifier {
    pub uniform_type: UniformType,
    pub value: Rc<RefCell<UniformValue>>,
}

impl std::fmt::Debug for UniformSpecifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UniformSpecifier")
            .field("uniform_type", &self.uniform_type)
            .field("value", &self.value.borrow())
            .finish()
    }
}

impl UniformSpecifier {
    pub fn new(uniform_type: UniformType, value: UniformValue) -> Self {
        Self { uniform_type, value: Rc::new(RefCell::new(value)) }
    }
}

/// An insertion-ordered set. Upstream uses plain objects as sets; iteration
/// order is first appearance in the shader text, which decides the order of
/// generated struct fields and initialization lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VariableSet {
    names: Vec<String>,
}

impl VariableSet {
    pub fn insert(&mut self, name: String) -> bool {
        if self.names.contains(&name) {
            false
        } else {
            self.names.push(name);
            true
        }
    }
    pub fn contains(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn as_slice(&self) -> &[String] {
        &self.names
    }
    /// Sorted names, for order-insensitive comparison and diagnostics.
    pub fn sorted(&self) -> Vec<String> {
        let mut names = self.names.clone();
        names.sort();
        names
    }
}

/// Variables used by the vertex shader text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VertexVariableSets {
    pub attribute_set: VariableSet,
    pub feature_id_set: VariableSet,
    pub metadata_set: VariableSet,
}

/// Variables used by the fragment shader text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FragmentVariableSets {
    pub attribute_set: VariableSet,
    pub feature_id_set: VariableSet,
    pub metadata_set: VariableSet,
    pub material_set: VariableSet,
}

/// Constructor options. The JS object is positional; named fields keep the
/// defaults explicit and match `options.x ?? default` evaluation order.
#[derive(Clone, Debug, Default)]
pub struct CustomShaderOptions {
    pub mode: CustomShaderMode,
    pub lighting_model: Option<LightingModel>,
    pub translucency_mode: CustomShaderTranslucencyMode,
    pub uniforms: BTreeMap<String, (UniformType, UniformValue)>,
    pub varyings: BTreeMap<String, VaryingType>,
    pub vertex_shader_text: Option<String>,
    pub fragment_shader_text: Option<String>,
}

pub struct CustomShader<G: Gl> {
    pub mode: CustomShaderMode,
    pub lighting_model: Option<LightingModel>,
    pub uniforms: BTreeMap<String, UniformSpecifier>,
    pub varyings: BTreeMap<String, VaryingType>,
    pub vertex_shader_text: Option<String>,
    pub fragment_shader_text: Option<String>,
    pub translucency_mode: CustomShaderTranslucencyMode,
    /// Owns uploaded sampler textures. Shared with every draw using this shader.
    pub texture_manager: Rc<TextureManager<G>>,
    pub used_variables_vertex: VertexVariableSets,
    pub used_variables_fragment: FragmentVariableSets,
}

impl<G: Gl> std::fmt::Debug for CustomShader<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomShader")
            .field("mode", &self.mode)
            .field("lighting_model", &self.lighting_model)
            .field("translucency_mode", &self.translucency_mode)
            .field("uniforms", &self.uniforms.keys().collect::<Vec<_>>())
            .field("varyings", &self.varyings)
            .field("vertex_shader_text", &self.vertex_shader_text.is_some())
            .field("fragment_shader_text", &self.fragment_shader_text.is_some())
            .finish()
    }
}

impl<G: Gl> CustomShader<G> {
    pub fn new(options: CustomShaderOptions) -> Result<Self> {
        let texture_manager = Rc::new(TextureManager::new());
        let mut uniforms = BTreeMap::new();
        for (name, (uniform_type, value)) in options.uniforms {
            if uniform_type == UniformType::SamplerCube {
                return Err(RuntimeError::new("CustomShader does not support samplerCube uniforms"));
            }
            if uniform_type == UniformType::Sampler2D {
                match &value {
                    UniformValue::Sampler2D(uniform) => texture_manager.load_texture_2d(&name, uniform),
                    _ => return Err(RuntimeError::new("A sampler2D uniform must have a TextureUniform value")),
                }
            }
            uniforms.insert(name, UniformSpecifier::new(uniform_type, value));
        }
        let vertex_shader_text = options.vertex_shader_text;
        let fragment_shader_text = options.fragment_shader_text;
        let (used_variables_vertex, used_variables_fragment) =
            find_used_variables(vertex_shader_text.as_deref(), fragment_shader_text.as_deref());
        let shader = Self {
            mode: options.mode,
            lighting_model: options.lighting_model,
            uniforms,
            varyings: options.varyings,
            vertex_shader_text,
            fragment_shader_text,
            translucency_mode: options.translucency_mode,
            texture_manager,
            used_variables_vertex,
            used_variables_fragment,
        };
        validate_builtin_variables(&shader)?;
        Ok(shader)
    }

    /// The pass forced by `translucencyMode`, if it overrides the material.
    pub fn forced_pass(&self) -> Option<Pass> {
        match self.translucency_mode {
            CustomShaderTranslucencyMode::Translucent => Some(Pass::Translucent),
            CustomShaderTranslucencyMode::Opaque => Some(Pass::Opaque),
            CustomShaderTranslucencyMode::Inherit => None,
        }
    }

    /// Whether the translucency mode is `OPAQUE`, which upstream represents by
    /// clearing the material-provided pass rather than forcing `OPAQUE`.
    pub fn clears_pass(&self) -> bool {
        self.translucency_mode == CustomShaderTranslucencyMode::Opaque
    }

    pub fn set_uniform(&self, uniform_name: &str, value: UniformValue) -> Result<()> {
        let uniform = self.uniforms.get(uniform_name).ok_or_else(|| {
            RuntimeError::new(format!("Uniform {uniform_name} must be declared in the CustomShader constructor."))
        })?;
        match (&value, uniform.uniform_type) {
            (UniformValue::Sampler2D(texture), UniformType::Sampler2D) => {
                self.texture_manager.load_texture_2d(uniform_name, texture);
            }
            (UniformValue::Sampler2D(_), _) => {
                return Err(RuntimeError::new("A sampler2D value requires a sampler2D uniform"))
            }
            (_, UniformType::Sampler2D) => {
                return Err(RuntimeError::new("A sampler2D uniform requires a TextureUniform value"))
            }
            _ => *uniform.value.borrow_mut() = value,
        }
        Ok(())
    }

    /// The current value of a declared uniform.
    pub fn uniform_value(&self, uniform_name: &str) -> Option<UniformValue> {
        self.uniforms.get(uniform_name).map(|u| u.value.borrow().clone())
    }

    /// Upload sampler textures for this frame and remember the context default
    /// texture used while an image is still loading.
    pub fn update<H: ImageHost>(
        &self,
        context: &Context<G>,
        frame_number: u32,
        default_texture: Option<Rc<Texture<G>>>,
        host: &H,
    ) -> Result<Vec<TextureManagerWarning>> {
        self.texture_manager.update(context, frame_number, default_texture, host)
    }

    pub fn sync_uniforms(&self) -> bool {
        self.uniforms.values().any(|u| u.uniform_type == UniformType::Sampler2D)
    }

    pub fn destroy(&self) -> Result<()> {
        self.texture_manager.destroy()
    }
}

fn find_used_variables(
    vertex_shader_text: Option<&str>,
    fragment_shader_text: Option<&str>,
) -> (VertexVariableSets, FragmentVariableSets) {
    let mut vertex = VertexVariableSets::default();
    let mut fragment = FragmentVariableSets::default();
    if let Some(text) = vertex_shader_text {
        collect(text, &ATTRIBUTE_REGEX, &mut vertex.attribute_set);
        collect(text, &FEATURE_ID_REGEX, &mut vertex.feature_id_set);
        collect(text, &METADATA_REGEX, &mut vertex.metadata_set);
    }
    if let Some(text) = fragment_shader_text {
        collect(text, &ATTRIBUTE_REGEX, &mut fragment.attribute_set);
        collect(text, &FEATURE_ID_REGEX, &mut fragment.feature_id_set);
        collect(text, &METADATA_REGEX, &mut fragment.metadata_set);
        collect(text, &MATERIAL_REGEX, &mut fragment.material_set);
    }
    (vertex, fragment)
}

fn collect(text: &str, regex: &Regex, output: &mut VariableSet) {
    for capture in regex.captures_iter(text) {
        output.insert(capture[1].to_string());
    }
}

/// Expand the MC/WC/EC abbreviation used in `DeveloperError` messages.
pub fn expand_coordinate_abbreviations(variable_name: &str) -> String {
    if variable_name.ends_with("MC") {
        format!("{variable_name} (model coordinates)")
    } else if variable_name.ends_with("WC") {
        format!("{variable_name} (Cartesian world coordinates)")
    } else if variable_name.ends_with("EC") {
        format!("{variable_name} (eye coordinates)")
    } else {
        variable_name.into()
    }
}

fn validate_variable_usage(
    variable_set: &VariableSet,
    incorrect: &str,
    correct: &str,
    vertex_or_fragment: &str,
) -> Result<()> {
    if variable_set.contains(incorrect) {
        return Err(RuntimeError::new(format!(
            "{} is not available in the {vertex_or_fragment} shader. Did you mean {} instead?",
            expand_coordinate_abbreviations(incorrect),
            expand_coordinate_abbreviations(correct)
        )));
    }
    Ok(())
}

fn validate_builtin_variables<G: Gl>(custom_shader: &CustomShader<G>) -> Result<()> {
    let vs = &custom_shader.used_variables_vertex.attribute_set;
    validate_variable_usage(vs, "position", "positionMC", "vertex")?;
    validate_variable_usage(vs, "normal", "normalMC", "vertex")?;
    validate_variable_usage(vs, "tangent", "tangentMC", "vertex")?;
    validate_variable_usage(vs, "bitangent", "bitangentMC", "vertex")?;
    validate_variable_usage(vs, "positionWC", "positionMC", "vertex")?;
    validate_variable_usage(vs, "positionEC", "positionMC", "vertex")?;
    validate_variable_usage(vs, "normalEC", "normalMC", "vertex")?;
    validate_variable_usage(vs, "tangentEC", "tangentMC", "vertex")?;
    validate_variable_usage(vs, "bitangentEC", "bitangentMC", "vertex")?;

    let fs = &custom_shader.used_variables_fragment.attribute_set;
    validate_variable_usage(fs, "position", "positionEC", "fragment")?;
    validate_variable_usage(fs, "normal", "normalEC", "fragment")?;
    validate_variable_usage(fs, "tangent", "tangentEC", "fragment")?;
    validate_variable_usage(fs, "bitangent", "bitangentEC", "fragment")?;
    validate_variable_usage(fs, "normalMC", "normalEC", "fragment")?;
    validate_variable_usage(fs, "tangentMC", "tangentEC", "fragment")?;
    validate_variable_usage(fs, "bitangentMC", "bitangentEC", "fragment")?;
    Ok(())
}
