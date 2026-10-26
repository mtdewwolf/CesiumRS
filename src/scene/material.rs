//! CesiumJS `Material`: fabric parsing into GLSL, uniforms and sub-materials.
//!
//! Fabric is JavaScript's material description. The Rust port keeps the same
//! insertion orders and the same token substitution, so the generated shader
//! source and the renamed uniform list match Cesium's exactly. Textures are
//! represented by their source (the default image, a URL or a loaded GPU
//! resource); GPU upload itself happens through [`Material::update_textures`],
//! which a renderer host calls with resolved image data.

use crate::core::{Cartesian2, Cartesian3, Cartesian4, Color, Matrix2, Matrix3, Matrix4, RuntimeError};
use crate::shaders::materials as material_shaders;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::renderer::{TextureMagnificationFilter, TextureMinificationFilter};

/// `Material.DefaultImageId`.
pub const DEFAULT_IMAGE_ID: &str = "czm_defaultImage";
/// `Material.DefaultCubeMapId`.
pub const DEFAULT_CUBE_MAP_ID: &str = "czm_defaultCubeMap";

macro_rules! material_type {
    ($const:ident, $name:literal) => {
        pub const $const: &str = $name;
    };
}

material_type!(COLOR_TYPE, "Color");
material_type!(IMAGE_TYPE, "Image");
material_type!(DIFFUSE_MAP_TYPE, "DiffuseMap");
material_type!(ALPHA_MAP_TYPE, "AlphaMap");
material_type!(SPECULAR_MAP_TYPE, "SpecularMap");
material_type!(EMISSION_MAP_TYPE, "EmissionMap");
material_type!(BUMP_MAP_TYPE, "BumpMap");
material_type!(NORMAL_MAP_TYPE, "NormalMap");
material_type!(GRID_TYPE, "Grid");
material_type!(STRIPE_TYPE, "Stripe");
material_type!(CHECKERBOARD_TYPE, "Checkerboard");
material_type!(DOT_TYPE, "Dot");
material_type!(WATER_TYPE, "Water");
material_type!(RIM_LIGHTING_TYPE, "RimLighting");
material_type!(FADE_TYPE, "Fade");
material_type!(POLYLINE_ARROW_TYPE, "PolylineArrow");
material_type!(POLYLINE_DASH_TYPE, "PolylineDash");
material_type!(POLYLINE_GLOW_TYPE, "PolylineGlow");
material_type!(POLYLINE_OUTLINE_TYPE, "PolylineOutline");
material_type!(ELEVATION_CONTOUR_TYPE, "ElevationContour");
material_type!(ELEVATION_RAMP_TYPE, "ElevationRamp");
material_type!(SLOPE_RAMP_TYPE, "SlopeRamp");
material_type!(ASPECT_RAMP_TYPE, "AspectRamp");
material_type!(ELEVATION_BAND_TYPE, "ElevationBand");
material_type!(WATER_MASK_TYPE, "WaterMask");

/// A texture uniform value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialImage {
    /// `Material.DefaultImageId` (or `undefined`, which Cesium treats the same).
    Default,
    /// A resource URL the host loads.
    Url(String),
    /// A texture the host already owns; the opaque id identifies it.
    Texture(String),
}

/// A cube-map uniform value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialCubeMap {
    Default,
    /// Faces in Cesium's order: +X, -X, +Y, -Y, +Z, -Z.
    Faces([String; 6]),
    /// A cube map the host already owns.
    Texture(String),
}

/// One fabric uniform value. Variants follow Cesium's `getUniformType`.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialValue {
    Float(f64),
    Bool(bool),
    Vec2(Cartesian2),
    Vec3(Cartesian3),
    IntVec3(Cartesian3),
    Vec4(Cartesian4),
    Mat2(Matrix2),
    Mat3(Matrix3),
    Mat4(Matrix4),
    Channels(String),
    Sampler2D(MaterialImage),
    SamplerCube(MaterialCubeMap),
}

impl MaterialValue {
    pub fn from_color(color: Color) -> Self {
        Self::Vec4(Cartesian4::new(color.red, color.green, color.blue, color.alpha))
    }
    pub fn from_cartesian2(v: Cartesian2) -> Self {
        Self::Vec2(v)
    }
    pub fn from_cartesian3(v: Cartesian3) -> Self {
        Self::Vec3(v)
    }
    pub fn from_cartesian4(v: Cartesian4) -> Self {
        Self::Vec4(v)
    }
    /// `getUniformType`: the GLSL type name, including Cesium's `channels`
    /// pseudo-type.
    pub fn glsl_type(&self) -> &'static str {
        match self {
            Self::Float(_) => "float",
            Self::Bool(_) => "bool",
            Self::Vec2(_) => "vec2",
            Self::Vec3(_) => "vec3",
            Self::IntVec3(_) => "ivec3",
            Self::Vec4(_) => "vec4",
            Self::Mat2(_) => "mat2",
            Self::Mat3(_) => "mat3",
            Self::Mat4(_) => "mat4",
            Self::Channels(_) => "channels",
            Self::Sampler2D(_) => "sampler2D",
            Self::SamplerCube(_) => "samplerCube",
        }
    }
    /// The components as numbers, for renderer uniform submission.
    pub fn numbers(&self) -> Option<Vec<f64>> {
        Some(match self {
            Self::Float(v) => vec![*v],
            Self::Bool(v) => vec![if *v { 1. } else { 0. }],
            Self::Vec2(v) => vec![v.x, v.y],
            Self::Vec3(v) | Self::IntVec3(v) => vec![v.x, v.y, v.z],
            Self::Vec4(v) => vec![v.x, v.y, v.z, v.w],
            Self::Mat2(m) => m.0.to_vec(),
            Self::Mat3(m) => m.0.to_vec(),
            Self::Mat4(m) => m.0.to_vec(),
            Self::Channels(_) | Self::Sampler2D(_) | Self::SamplerCube(_) => return None,
        })
    }
    fn color(&self) -> Option<[f64; 4]> {
        match self {
            Self::Vec4(v) => Some([v.x, v.y, v.z, v.w]),
            _ => None,
        }
    }
}

/// A material fabric (one node of the recursive description).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fabric {
    /// Cesium's `type`; absent for anonymous fabrics.
    pub type_name: Option<String>,
    /// Sub-materials by name, in insertion order.
    pub materials: Vec<(String, Fabric)>,
    /// Uniforms by name, in insertion order.
    pub uniforms: Vec<(String, MaterialValue)>,
    /// The components (diffuse, alpha, ...) in insertion order.
    pub components: Vec<(String, String)>,
    /// The full `czm_getMaterial` source, instead of components.
    pub source: Option<String>,
}

impl Fabric {
    pub fn new(type_name: impl Into<String>) -> Self {
        Self { type_name: Some(type_name.into()), ..Self::default() }
    }
    pub fn uniform(mut self, name: impl Into<String>, value: MaterialValue) -> Self {
        self.uniforms.push((name.into(), value));
        self
    }
    pub fn component(mut self, name: impl Into<String>, expression: impl Into<String>) -> Self {
        self.components.push((name.into(), expression.into()));
        self
    }
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
    pub fn sub_material(mut self, name: impl Into<String>, material: Fabric) -> Self {
        self.materials.push((name.into(), material));
        self
    }
    pub fn uniform_value(&self, name: &str) -> Option<&MaterialValue> {
        self.uniforms.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

/// Cesium's translucency predicate for a material type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranslucentRule {
    /// A fixed boolean.
    Flag(bool),
    /// `uniforms.<name>.alpha < 1.0`.
    ColorAlpha(&'static str),
    /// Either color's alpha is below one.
    TwoColorAlpha(&'static str, &'static str),
    /// `color.alpha < 1.0 || cellAlpha < 1.0` (Grid).
    GridAlpha,
}

impl TranslucentRule {
    fn evaluate(self, material: &Material) -> bool {
        let alpha = |name: &str| material.uniform(name).and_then(MaterialValue::color).map(|c| c[3]).unwrap_or(1.0);
        match self {
            Self::Flag(v) => v,
            Self::ColorAlpha(name) => alpha(name) < 1.0,
            Self::TwoColorAlpha(a, b) => alpha(a) < 1.0 || alpha(b) < 1.0,
            Self::GridAlpha => {
                alpha("color") < 1.0
                    || material
                        .uniform("cellAlpha")
                        .and_then(MaterialValue::numbers)
                        .and_then(|v| v.first().copied())
                        .map(|v| v < 1.0)
                        .unwrap_or(false)
            }
        }
    }
}

struct CachedMaterial {
    fabric: Fabric,
    translucent: Option<TranslucentRule>,
}

/// Cesium's `Material._materialCache`.
#[derive(Default)]
pub struct MaterialCache {
    materials: HashMap<String, CachedMaterial>,
}

impl MaterialCache {
    pub fn get(&self, type_name: &str) -> Option<&Fabric> {
        self.materials.get(type_name).map(|m| &m.fabric)
    }
    pub fn get_translucent(&self, type_name: &str) -> Option<TranslucentRule> {
        self.materials.get(type_name).and_then(|m| m.translucent)
    }
    pub fn contains(&self, type_name: &str) -> bool {
        self.materials.contains_key(type_name)
    }
    pub fn add(&mut self, type_name: impl Into<String>, fabric: Fabric, translucent: Option<TranslucentRule>) {
        self.materials.insert(type_name.into(), CachedMaterial { fabric, translucent });
    }
    pub fn len(&self) -> usize {
        self.materials.len()
    }
    pub fn is_empty(&self) -> bool {
        self.materials.is_empty()
    }
}

thread_local! {
    static MATERIAL_CACHE: RefCell<MaterialCache> = RefCell::new(builtin_cache());
}

/// Run `f` with Cesium's global material cache.
pub fn with_material_cache<R>(f: impl FnOnce(&mut MaterialCache) -> R) -> R {
    MATERIAL_CACHE.with(|cache| f(&mut cache.borrow_mut()))
}

/// Options for `new Material(options)`.
#[derive(Clone, Debug, Default)]
pub struct MaterialOptions {
    pub strict: bool,
    /// Cesium accepts a boolean or a function; a boolean override is the
    /// typed equivalent.
    pub translucent: Option<bool>,
    pub minification_filter: Option<TextureMinificationFilter>,
    pub magnification_filter: Option<TextureMagnificationFilter>,
    pub fabric: Fabric,
    /// Internal: the uniform counter shared with sub-materials.
    pub count: usize,
}

pub type TranslucentPredicate = Rc<dyn Fn(&Material) -> bool>;

/// A parsed material with its generated GLSL and uniform bindings.
#[derive(Clone)]
pub struct Material {
    pub type_name: String,
    pub shader_source: String,
    /// Sub-materials by name, in creation order.
    pub materials: Vec<(String, Material)>,
    /// The original uniform values by name, in fabric order.
    pub uniforms: Vec<(String, MaterialValue)>,
    /// Exact original/generated identifiers for this material's own uniforms.
    /// Values are read from the material tree at submission time.
    uniform_binding_names: Vec<(String, String)>,
    pub translucent: Option<bool>,
    pub minification_filter: TextureMinificationFilter,
    pub magnification_filter: TextureMagnificationFilter,
    /// The fabric this material was created from, including cached defaults.
    fabric: Fabric,
    count: usize,
    own_translucent: Option<TranslucentRule>,
    custom_translucent: Option<TranslucentPredicate>,
    generated_type: bool,
}

impl std::fmt::Debug for Material {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Material")
            .field("type_name", &self.type_name)
            .field("shader_source", &self.shader_source)
            .field("materials", &self.materials)
            .field("uniforms", &self.uniforms)
            .field("uniform_bindings", &self.uniform_bindings())
            .field("translucent", &self.translucent)
            .field("minification_filter", &self.minification_filter)
            .field("magnification_filter", &self.magnification_filter)
            .field("fabric", &self.fabric)
            .field("translucent_rule", &self.own_translucent)
            .finish()
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new(MaterialOptions { fabric: MaterialOptions::default().fabric, ..Default::default() })
    }
}

impl Material {
    /// `new Material(options)`. Panics only where Rust's typed API cannot
    /// express Cesium's `DeveloperError`; validation lives in [`Material::try_new`].
    pub fn new(options: MaterialOptions) -> Self {
        Self::try_new(options).expect("invalid material fabric")
    }

    pub fn try_new(options: MaterialOptions) -> Result<Self, RuntimeError> {
        let mut material = Self {
            type_name: String::new(),
            shader_source: String::new(),
            materials: Vec::new(),
            uniforms: Vec::new(),
            uniform_binding_names: Vec::new(),
            translucent: options.translucent,
            minification_filter: options.minification_filter.unwrap_or(TextureMinificationFilter::Linear),
            magnification_filter: options.magnification_filter.unwrap_or(TextureMagnificationFilter::Linear),
            fabric: options.fabric.clone(),
            count: options.count,
            own_translucent: None,
            custom_translucent: None,
            generated_type: false,
        };
        material.initialize(options.strict)?;
        Ok(material)
    }

    /// `Material.fromType(type, uniforms)`: panics for an unknown type, as
    /// Cesium raises a `DeveloperError`.
    pub fn from_type(type_name: &str) -> Self {
        Self::from_type_with_uniforms(type_name, Vec::new()).expect("unknown material type")
    }

    /// `Material.fromType(type, uniforms)` with uniform overrides.
    pub fn from_type_with_uniforms(
        type_name: &str,
        uniforms: Vec<(String, MaterialValue)>,
    ) -> Result<Self, RuntimeError> {
        let exists = with_material_cache(|c| c.contains(type_name));
        if !exists {
            return Err(RuntimeError::new(format!("material with type '{type_name}' does not exist.")));
        }
        let fabric = Fabric { type_name: Some(type_name.to_string()), uniforms, ..Fabric::default() };
        Self::try_new(MaterialOptions { fabric, ..MaterialOptions::default() })
    }

    /// The fabric used to create this material (with cached type defaults).
    pub fn fabric(&self) -> &Fabric {
        &self.fabric
    }

    pub fn uniform(&self, name: &str) -> Option<&MaterialValue> {
        self.uniforms.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// Resolve renderer bindings (`name_count`) from the current material tree.
    /// Call at submission time so uniform edits and texture resolution on any child
    /// are reflected in the parent's uniforms. Returned values are a snapshot.
    pub fn uniform_bindings(&self) -> Vec<(String, MaterialValue)> {
        let mut bindings = Vec::new();
        self.collect_uniform_bindings(&mut bindings);
        bindings
    }

    fn collect_uniform_bindings(&self, bindings: &mut Vec<(String, MaterialValue)>) {
        for (original, generated) in &self.uniform_binding_names {
            if let Some(value) = self.uniform(original) {
                bindings.push((generated.clone(), value.clone()));
            }
        }
        for (_, child) in &self.materials {
            child.collect_uniform_bindings(bindings);
        }
    }

    /// Replaces a uniform's value, keeping its declared type.
    pub fn set_uniform(&mut self, name: &str, value: MaterialValue) -> Result<(), RuntimeError> {
        let datas = &mut self.uniforms;
        let slot = datas
            .iter_mut()
            .find(|(n, _)| n == name)
            .ok_or_else(|| RuntimeError::new(format!("material has no uniform '{name}'")))?;
        if std::mem::discriminant(&slot.1) != std::mem::discriminant(&value) {
            return Err(RuntimeError::new(format!(
                "uniform '{name}' is {}, not {}",
                slot.1.glsl_type(),
                value.glsl_type()
            )));
        }
        slot.1 = value;
        Ok(())
    }

    /// `Material#isTranslucent`: every cached rule, the `translucent` override
    /// and every sub-material has to agree, as Cesium ANDs its
    /// `_translucentFunctions`.
    pub fn is_translucent(&self) -> bool {
        if let Some(custom) = &self.custom_translucent {
            return custom(self);
        }
        let mut translucent = match self.own_translucent {
            Some(rule) => rule.evaluate(self),
            // A type with no cached rule is translucent by default.
            None => true,
        };
        if let Some(value) = self.translucent {
            translucent = translucent && value;
        }
        translucent && self.materials.iter().all(|(_, sub)| sub.is_translucent())
    }

    /// Creates or updates a type in the global cache.
    pub fn add_type_to_cache(type_name: impl Into<String>, fabric: Fabric, rule: Option<TranslucentRule>) {
        with_material_cache(|c| c.add(type_name, fabric, rule));
    }

    fn initialize(&mut self, strict: bool) -> Result<(), RuntimeError> {
        let mut template = self.fabric.clone();
        template.uniforms = self.fabric.uniforms.clone();
        template.materials = self.fabric.materials.clone();

        self.generated_type = template.type_name.is_none();
        self.type_name = template.type_name.clone().unwrap_or_else(|| format!("material-{}", next_guid()));
        self.shader_source = String::new();
        self.materials.clear();
        self.uniforms.clear();
        self.uniform_binding_names.clear();

        let cached =
            with_material_cache(|c| c.get(&self.type_name).cloned().map(|f| (f, c.get_translucent(&self.type_name))));
        let was_cached = cached.is_some();
        if let Some((cached_fabric, cached_translucent)) = cached {
            template = combine_fabric(&template, &cached_fabric);
            self.own_translucent = cached_translucent;
        }

        check_for_template_errors(&template)?;
        create_method_definition(&mut self.shader_source, &template);
        self.create_uniforms(&template, strict)?;
        self.create_sub_materials(&template, strict)?;
        self.fabric = template;

        if !was_cached {
            // A new custom type caches its fabric, not a translucency rule:
            // Cesium stores the material instance, whose `translucent` field
            // is only the caller's override.
            let fabric = self.fabric.clone();
            with_material_cache(|c| c.add(self.type_name.clone(), fabric, self.own_translucent));
        }

        Ok(())
    }

    fn create_uniforms(&mut self, template: &Fabric, strict: bool) -> Result<(), RuntimeError> {
        for (uniform_id, value) in &template.uniforms {
            self.create_uniform(template, uniform_id, value, strict)?;
        }
        Ok(())
    }

    fn create_uniform(
        &mut self,
        _template: &Fabric,
        uniform_id: &str,
        value: &MaterialValue,
        strict: bool,
    ) -> Result<(), RuntimeError> {
        if let MaterialValue::Channels(channels) = value {
            let replaced = replace_token(&mut self.shader_source, uniform_id, channels, false);
            if replaced == 0 && strict {
                return Err(RuntimeError::new(format!("strict: shader source does not use channels '{uniform_id}'.")));
            }
            return Ok(());
        }

        let uniform_type = value.glsl_type();
        if matches!(value, MaterialValue::Sampler2D(_)) {
            let dimensions_name = format!("{uniform_id}Dimensions");
            if get_number_of_tokens(&self.shader_source, &dimensions_name, true) > 0 {
                // Cesium's image-dimensions uniform is an ivec3 with x/y set.
                let dimensions = MaterialValue::IntVec3(Cartesian3::new(1., 1., 0.));
                self.create_uniform(_template, &dimensions_name, &dimensions, strict)?;
            }
        }

        let declaration = format!("uniform {uniform_type} {uniform_id};");
        if !has_uniform_declaration(&self.shader_source, uniform_type, uniform_id) {
            self.shader_source = format!("{declaration}{}", self.shader_source);
        }

        let new_id = format!("{uniform_id}_{}", self.count);
        self.count += 1;
        let replaced = replace_token(&mut self.shader_source, uniform_id, &new_id, true);
        if replaced == 1 && strict {
            return Err(RuntimeError::new(format!("strict: shader source does not use uniform '{uniform_id}'.")));
        }
        self.uniforms.push((uniform_id.to_string(), value.clone()));
        self.uniform_binding_names.push((uniform_id.to_string(), new_id));
        Ok(())
    }

    fn create_sub_materials(&mut self, template: &Fabric, strict: bool) -> Result<(), RuntimeError> {
        let sub_templates = template.materials.clone();
        for (sub_id, sub_fabric) in sub_templates {
            let mut sub = Material::try_new(MaterialOptions {
                strict,
                fabric: sub_fabric,
                count: self.count,
                ..MaterialOptions::default()
            })?;
            self.count = sub.count;
            let original_method = "czm_getMaterial";
            let new_method = format!("{original_method}_{}", self.count);
            self.count += 1;
            replace_token(&mut sub.shader_source, original_method, &new_method, true);
            let call = format!("{new_method}(materialInput)");
            self.materials.push((sub_id.clone(), sub));
            let replaced = replace_token(&mut self.shader_source, &sub_id, &call, true);
            if replaced == 0 && strict {
                return Err(RuntimeError::new(format!("strict: shader source does not use material '{sub_id}'.")));
            }
        }
        // Cesium prepends each sub-material's source as it goes.
        let mut source = String::new();
        for (_, sub) in &self.materials {
            source.push_str(&sub.shader_source);
        }
        self.shader_source = format!("{source}{}", self.shader_source);
        Ok(())
    }

    /// A host hook for the currently unresolved texture values. The renderer
    /// adapter resolves them and calls this with the loaded images.
    pub fn unresolved_textures(&self) -> Vec<(String, MaterialImage)> {
        self.uniforms
            .iter()
            .filter_map(|(name, value)| match value {
                MaterialValue::Sampler2D(MaterialImage::Url(url)) => {
                    Some((name.clone(), MaterialImage::Url(url.clone())))
                }
                _ => None,
            })
            .collect()
    }

    /// Resolve a URL uniform to an owned GPU texture id (host adapter hook).
    pub fn set_texture(&mut self, uniform_name: &str, texture_id: impl Into<String>) {
        let _ = self.set_uniform(uniform_name, MaterialValue::Sampler2D(MaterialImage::Texture(texture_id.into())));
    }

    /// Cesium's `Material#update` texture hook; the Rust renderer owns the
    /// actual textures, so this only validates that every URL is resolved.
    pub fn update_textures(&mut self) -> Result<(), RuntimeError> {
        if let Some((name, _)) =
            self.uniforms.iter().find(|(_, value)| matches!(value, MaterialValue::Sampler2D(MaterialImage::Url(_))))
        {
            return Err(RuntimeError::new(format!("material texture '{name}' has not been loaded")));
        }
        for (_, child) in &mut self.materials {
            child.update_textures()?;
        }
        Ok(())
    }
}

fn next_guid() -> u64 {
    thread_local! { static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
    NEXT.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}

fn check_for_template_errors(template: &Fabric) -> Result<(), RuntimeError> {
    if template.source.is_some() && !template.components.is_empty() {
        return Err(RuntimeError::new("fabric: cannot have source and components in the same template."));
    }
    let template_properties = ["type", "materials", "uniforms", "components", "source"];
    // Fabric is a typed struct, so unknown top-level properties cannot occur;
    // component names still need Cesium's check.
    let component_properties = ["diffuse", "specular", "shininess", "normal", "emission", "alpha"];
    for (name, _) in &template.components {
        if !component_properties.contains(&name.as_str()) {
            return Err(RuntimeError::new(invalid_name_message(name, &component_properties)));
        }
    }
    let _ = template_properties;
    for (uniform_name, _) in &template.uniforms {
        if template.materials.iter().any(|(name, _)| name == uniform_name) {
            return Err(RuntimeError::new(format!(
                "fabric: uniforms and materials cannot share the same property '{uniform_name}'"
            )));
        }
    }
    Ok(())
}

fn invalid_name_message(property: &str, properties: &[&str]) -> String {
    let list: Vec<String> = properties.iter().map(|p| format!("'{p}'")).collect();
    format!("fabric: property name '{property}' is not valid. It should be {}", list.join(", "))
}

fn create_method_definition(shader_source: &mut String, template: &Fabric) {
    if let Some(source) = &template.source {
        shader_source.push_str(source);
        shader_source.push('\n');
        return;
    }
    shader_source.push_str("czm_material czm_getMaterial(czm_materialInput materialInput)\n{\n");
    shader_source.push_str("czm_material material = czm_getDefaultMaterial(materialInput);\n");
    let is_multi_material = !template.materials.is_empty();
    for (component, expression) in &template.components {
        match component.as_str() {
            "diffuse" | "emission" => {
                let is_fusion = is_multi_material && is_material_fused(expression, template);
                let value = if is_fusion { expression.clone() } else { format!("czm_gammaCorrect({expression})") };
                shader_source.push_str(&format!("material.{component} = {value}; \n"));
            }
            "alpha" => {
                shader_source.push_str(&format!("material.alpha = {expression}; \n"));
            }
            _ => {
                shader_source.push_str(&format!("material.{component} = {expression};\n"));
            }
        }
    }
    shader_source.push_str("return material;\n}\n");
}

fn is_material_fused(component: &str, template: &Fabric) -> bool {
    template.materials.iter().any(|(name, _)| component.contains(name.as_str()))
}

fn has_uniform_declaration(source: &str, uniform_type: &str, name: &str) -> bool {
    let pattern = format!(r"uniform\s+{uniform_type}\s+{name}\s*;");
    regex_lite_contains(source, &pattern)
}

/// Cesium's `replaceToken`: replaces whole identifiers unless part of a
/// longer identifier (or a member expression when `exclude_period`).
fn replace_token(source: &mut String, token: &str, new_token: &str, exclude_period: bool) -> usize {
    let prefix = if exclude_period { r"([A-Za-z0-9_.])?" } else { r"([A-Za-z0-9_])?" };
    let pattern = format!(r"{prefix}{token}([A-Za-z0-9_])?");
    let regex = regex::Regex::new(&pattern).expect("valid token regex");
    let mut count = 0;
    let result = regex.replace_all(source, |caps: &regex::Captures<'_>| {
        if caps.get(1).is_some() || caps.get(2).is_some() {
            caps[0].to_string()
        } else {
            count += 1;
            new_token.to_string()
        }
    });
    *source = result.into_owned();
    count
}

fn get_number_of_tokens(source: &str, token: &str, exclude_period: bool) -> usize {
    let mut copy = source.to_string();
    replace_token(&mut copy, token, token, exclude_period)
}

fn regex_lite_contains(source: &str, pattern: &str) -> bool {
    regex::Regex::new(pattern).map(|r| r.is_match(source)).unwrap_or(false)
}

fn combine_fabric(user: &Fabric, cached: &Fabric) -> Fabric {
    // Cesium's combine(): values from the first object win, including nested
    // uniform/material entries by name.
    let mut out = user.clone();
    if out.source.is_none() {
        out.source = cached.source.clone();
    }
    if out.components.is_empty() {
        out.components = cached.components.clone();
    }
    for (name, value) in &cached.uniforms {
        if !out.uniforms.iter().any(|(n, _)| n == name) {
            out.uniforms.push((name.clone(), value.clone()));
        }
    }
    for (name, fabric) in &cached.materials {
        if !out.materials.iter().any(|(n, _)| n == name) {
            out.materials.push((name.clone(), fabric.clone()));
        }
    }
    out
}

// ---- built-in material types ---------------------------------------------

fn vec2(x: f64, y: f64) -> MaterialValue {
    MaterialValue::Vec2(Cartesian2::new(x, y))
}
fn color(r: f64, g: f64, b: f64, a: f64) -> MaterialValue {
    MaterialValue::Vec4(Cartesian4::new(r, g, b, a))
}
fn image() -> MaterialValue {
    MaterialValue::Sampler2D(MaterialImage::Default)
}

fn builtin_cache() -> MaterialCache {
    let mut cache = MaterialCache::default();
    let mut add = |name: &str, fabric: Fabric, rule: TranslucentRule| {
        cache.add(name, fabric, Some(rule));
    };

    add(
        COLOR_TYPE,
        Fabric::new(COLOR_TYPE)
            .uniform("color", color(1., 0., 0., 0.5))
            .component("diffuse", "color.rgb")
            .component("alpha", "color.a"),
        TranslucentRule::ColorAlpha("color"),
    );
    add(
        IMAGE_TYPE,
        Fabric::new(IMAGE_TYPE)
            .uniform("image", image())
            .uniform("repeat", vec2(1., 1.))
            .uniform("color", color(1., 1., 1., 1.))
            .component("diffuse", "texture(image, fract(repeat * materialInput.st)).rgb * color.rgb")
            .component("alpha", "texture(image, fract(repeat * materialInput.st)).a * color.a"),
        TranslucentRule::ColorAlpha("color"),
    );
    add(
        DIFFUSE_MAP_TYPE,
        Fabric::new(DIFFUSE_MAP_TYPE)
            .uniform("image", image())
            .uniform("channels", MaterialValue::Channels("rgb".into()))
            .uniform("repeat", vec2(1., 1.))
            .component("diffuse", "texture(image, fract(repeat * materialInput.st)).channels"),
        TranslucentRule::Flag(false),
    );
    add(
        ALPHA_MAP_TYPE,
        Fabric::new(ALPHA_MAP_TYPE)
            .uniform("image", image())
            .uniform("channel", MaterialValue::Channels("a".into()))
            .uniform("repeat", vec2(1., 1.))
            .component("alpha", "texture(image, fract(repeat * materialInput.st)).channel"),
        TranslucentRule::Flag(true),
    );
    add(
        SPECULAR_MAP_TYPE,
        Fabric::new(SPECULAR_MAP_TYPE)
            .uniform("image", image())
            .uniform("channel", MaterialValue::Channels("r".into()))
            .uniform("repeat", vec2(1., 1.))
            .component("specular", "texture(image, fract(repeat * materialInput.st)).channel"),
        TranslucentRule::Flag(false),
    );
    add(
        EMISSION_MAP_TYPE,
        Fabric::new(EMISSION_MAP_TYPE)
            .uniform("image", image())
            .uniform("channels", MaterialValue::Channels("rgb".into()))
            .uniform("repeat", vec2(1., 1.))
            .component("emission", "texture(image, fract(repeat * materialInput.st)).channels"),
        TranslucentRule::Flag(false),
    );
    add(
        BUMP_MAP_TYPE,
        Fabric::new(BUMP_MAP_TYPE)
            .uniform("image", image())
            .uniform("channel", MaterialValue::Channels("r".into()))
            .uniform("strength", MaterialValue::Float(0.8))
            .uniform("repeat", vec2(1., 1.))
            .source(material_shaders::BUMP_MAP_MATERIAL.source),
        TranslucentRule::Flag(false),
    );
    add(
        NORMAL_MAP_TYPE,
        Fabric::new(NORMAL_MAP_TYPE)
            .uniform("image", image())
            .uniform("channels", MaterialValue::Channels("rgb".into()))
            .uniform("strength", MaterialValue::Float(0.8))
            .uniform("repeat", vec2(1., 1.))
            .source(material_shaders::NORMAL_MAP_MATERIAL.source),
        TranslucentRule::Flag(false),
    );
    add(
        GRID_TYPE,
        Fabric::new(GRID_TYPE)
            .uniform("color", color(0., 1., 0., 1.))
            .uniform("cellAlpha", MaterialValue::Float(0.1))
            .uniform("lineCount", vec2(8., 8.))
            .uniform("lineThickness", vec2(1., 1.))
            .uniform("lineOffset", vec2(0., 0.))
            .source(material_shaders::GRID_MATERIAL.source),
        TranslucentRule::GridAlpha,
    );
    add(
        STRIPE_TYPE,
        Fabric::new(STRIPE_TYPE)
            .uniform("horizontal", MaterialValue::Bool(true))
            .uniform("evenColor", color(1., 1., 1., 0.5))
            .uniform("oddColor", color(0., 0., 1., 0.5))
            .uniform("offset", MaterialValue::Float(0.))
            .uniform("repeat", MaterialValue::Float(5.))
            .source(material_shaders::STRIPE_MATERIAL.source),
        TranslucentRule::TwoColorAlpha("evenColor", "oddColor"),
    );
    add(
        CHECKERBOARD_TYPE,
        Fabric::new(CHECKERBOARD_TYPE)
            .uniform("lightColor", color(1., 1., 1., 0.5))
            .uniform("darkColor", color(0., 0., 0., 0.5))
            .uniform("repeat", vec2(5., 5.))
            .source(material_shaders::CHECKERBOARD_MATERIAL.source),
        TranslucentRule::TwoColorAlpha("lightColor", "darkColor"),
    );
    add(
        DOT_TYPE,
        Fabric::new(DOT_TYPE)
            .uniform("lightColor", color(1., 1., 0., 0.75))
            .uniform("darkColor", color(0., 1., 1., 0.75))
            .uniform("repeat", vec2(5., 5.))
            .source(material_shaders::DOT_MATERIAL.source),
        TranslucentRule::TwoColorAlpha("lightColor", "darkColor"),
    );
    add(
        WATER_TYPE,
        Fabric::new(WATER_TYPE)
            .uniform("baseWaterColor", color(0.2, 0.3, 0.6, 1.))
            .uniform("blendColor", color(0., 1., 0.699, 1.))
            .uniform("specularMap", image())
            .uniform("normalMap", image())
            .uniform("frequency", MaterialValue::Float(10.))
            .uniform("animationSpeed", MaterialValue::Float(0.01))
            .uniform("amplitude", MaterialValue::Float(1.))
            .uniform("specularIntensity", MaterialValue::Float(0.5))
            .uniform("fadeFactor", MaterialValue::Float(1.))
            .source(material_shaders::WATER.source),
        TranslucentRule::TwoColorAlpha("baseWaterColor", "blendColor"),
    );
    add(
        RIM_LIGHTING_TYPE,
        Fabric::new(RIM_LIGHTING_TYPE)
            .uniform("color", color(1., 0., 0., 0.7))
            .uniform("rimColor", color(1., 1., 1., 0.4))
            .uniform("width", MaterialValue::Float(0.3))
            .source(material_shaders::RIM_LIGHTING_MATERIAL.source),
        TranslucentRule::TwoColorAlpha("color", "rimColor"),
    );
    add(
        FADE_TYPE,
        Fabric::new(FADE_TYPE)
            .uniform("fadeInColor", color(1., 0., 0., 1.))
            .uniform("fadeOutColor", color(0., 0., 0., 0.))
            .uniform("maximumDistance", MaterialValue::Float(0.5))
            .uniform("repeat", MaterialValue::Bool(true))
            .uniform("fadeDirection", vec2(1., 1.))
            .uniform("time", vec2(0.5, 0.5))
            .source(material_shaders::FADE_MATERIAL.source),
        TranslucentRule::TwoColorAlpha("fadeInColor", "fadeOutColor"),
    );
    add(
        POLYLINE_ARROW_TYPE,
        Fabric::new(POLYLINE_ARROW_TYPE)
            .uniform("color", color(1., 1., 1., 1.))
            .source(material_shaders::POLYLINE_ARROW_MATERIAL.source),
        TranslucentRule::Flag(true),
    );
    add(
        POLYLINE_DASH_TYPE,
        Fabric::new(POLYLINE_DASH_TYPE)
            .uniform("color", color(1., 0., 1., 1.))
            .uniform("gapColor", color(0., 0., 0., 0.))
            .uniform("dashLength", MaterialValue::Float(16.))
            .uniform("dashPattern", MaterialValue::Float(255.))
            .source(material_shaders::POLYLINE_DASH_MATERIAL.source),
        TranslucentRule::Flag(true),
    );
    add(
        POLYLINE_GLOW_TYPE,
        Fabric::new(POLYLINE_GLOW_TYPE)
            .uniform("color", color(0., 0.5, 1., 1.))
            .uniform("glowPower", MaterialValue::Float(0.25))
            .uniform("taperPower", MaterialValue::Float(1.))
            .source(material_shaders::POLYLINE_GLOW_MATERIAL.source),
        TranslucentRule::Flag(true),
    );
    add(
        POLYLINE_OUTLINE_TYPE,
        Fabric::new(POLYLINE_OUTLINE_TYPE)
            .uniform("color", color(1., 1., 1., 1.))
            .uniform("outlineColor", color(1., 0., 0., 1.))
            .uniform("outlineWidth", MaterialValue::Float(1.))
            .source(material_shaders::POLYLINE_OUTLINE_MATERIAL.source),
        TranslucentRule::TwoColorAlpha("color", "outlineColor"),
    );
    add(
        ELEVATION_CONTOUR_TYPE,
        Fabric::new(ELEVATION_CONTOUR_TYPE)
            .uniform("spacing", MaterialValue::Float(100.))
            .uniform("color", color(1., 0., 0., 1.))
            .uniform("width", MaterialValue::Float(1.))
            .source(material_shaders::ELEVATION_CONTOUR_MATERIAL.source),
        TranslucentRule::Flag(false),
    );
    add(
        ELEVATION_RAMP_TYPE,
        Fabric::new(ELEVATION_RAMP_TYPE)
            .uniform("image", image())
            .uniform("minimumHeight", MaterialValue::Float(0.))
            .uniform("maximumHeight", MaterialValue::Float(10000.))
            .source(material_shaders::ELEVATION_RAMP_MATERIAL.source),
        TranslucentRule::Flag(false),
    );
    add(
        SLOPE_RAMP_TYPE,
        Fabric::new(SLOPE_RAMP_TYPE).uniform("image", image()).source(material_shaders::SLOPE_RAMP_MATERIAL.source),
        TranslucentRule::Flag(false),
    );
    add(
        ASPECT_RAMP_TYPE,
        Fabric::new(ASPECT_RAMP_TYPE).uniform("image", image()).source(material_shaders::ASPECT_RAMP_MATERIAL.source),
        TranslucentRule::Flag(false),
    );
    add(
        ELEVATION_BAND_TYPE,
        Fabric::new(ELEVATION_BAND_TYPE)
            .uniform("heights", image())
            .uniform("colors", image())
            .source(material_shaders::ELEVATION_BAND_MATERIAL.source),
        TranslucentRule::Flag(true),
    );
    add(
        WATER_MASK_TYPE,
        Fabric::new(WATER_MASK_TYPE)
            .uniform("waterColor", color(1., 1., 1., 1.))
            .uniform("landColor", color(0., 0., 0., 0.))
            .source(material_shaders::WATER_MASK_MATERIAL.source),
        TranslucentRule::Flag(false),
    );
    cache
}
