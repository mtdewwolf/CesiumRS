//! Shader variants and uniforms for CesiumJS globe surface draws.

use super::{GlobeTranslucencyCommand, SceneMode};
use crate::core::{Cartesian3, Cartesian4, Matrix4, RuntimeError};
use crate::renderer::{
    Context, Gl, ShaderCache, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformMap, UniformValue,
};
use crate::shaders::root::{ATMOSPHERE_COMMON, GLOBE_FS, GLOBE_VS, GROUND_ATMOSPHERE};
use std::{collections::HashMap, fmt::Write, rc::Rc};

/// Shader-key bits, including the day/night alpha flag above the 32-bit range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum GlobeShaderFeature {
    Brightness = 1 << 2,
    Contrast = 1 << 3,
    Hue = 1 << 4,
    Saturation = 1 << 5,
    Gamma = 1 << 6,
    Alpha = 1 << 7,
    WaterMask = 1 << 8,
    ReflectiveOcean = 1 << 9,
    OceanWaves = 1 << 10,
    Lighting = 1 << 11,
    DynamicLighting = 1 << 12,
    SunLighting = 1 << 13,
    GroundAtmosphere = 1 << 14,
    PerFragmentAtmosphere = 1 << 15,
    VertexNormals = 1 << 16,
    WebMercator = 1 << 17,
    Fog = 1 << 18,
    Quantization = 1 << 19,
    Split = 1 << 20,
    ClippingPlanes = 1 << 21,
    ClippingPolygons = 1 << 22,
    TileLimit = 1 << 23,
    Cutout = 1 << 24,
    ColorCorrect = 1 << 25,
    HighlightFill = 1 << 26,
    ColorToAlpha = 1 << 27,
    GeodeticNormals = 1 << 28,
    Exaggeration = 1 << 29,
    Underground = 1 << 30,
    Translucent = 1 << 31,
    DayNightAlpha = 1 << 32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GlobeShaderFeatures(u64);

impl GlobeShaderFeatures {
    pub fn set(&mut self, feature: GlobeShaderFeature, enabled: bool) {
        let bit = feature as u64;
        self.0 = if enabled { self.0 | bit } else { self.0 & !bit };
    }

    pub fn contains(self, feature: GlobeShaderFeature) -> bool {
        self.0 & feature as u64 != 0
    }

    pub fn key(self, mode: SceneMode) -> u64 {
        self.0 | mode as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlobeClippingPlanesShader {
    pub length: usize,
    pub union: bool,
    pub floating_point: bool,
    pub texture_size: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlobeClippingPolygonsShader {
    pub extents_count: usize,
    pub inverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlobeSurfaceShaderOptions {
    pub mode: SceneMode,
    pub texture_count: usize,
    pub features: GlobeShaderFeatures,
    pub clipping_planes: Option<GlobeClippingPlanesShader>,
    pub clipping_polygons: Option<GlobeClippingPolygonsShader>,
    pub attributes: [(&'static str, u32); 3],
}

impl GlobeSurfaceShaderOptions {
    pub fn features(&self) -> GlobeShaderFeatures {
        let mut features = self.features;
        features.set(GlobeShaderFeature::ClippingPlanes, self.clipping_planes.is_some());
        features.set(GlobeShaderFeature::ClippingPolygons, self.clipping_polygons.is_some());
        features
    }

    pub fn sources(&self, material: Option<&str>) -> ShaderProgramOptions {
        use GlobeShaderFeature::*;
        let enabled = |feature| self.features().contains(feature);
        let mut vs = ShaderSource::new(ATMOSPHERE_COMMON.source);
        let mut fs = vs.clone();
        vs.sources.extend([GROUND_ATMOSPHERE.source.into(), GLOBE_VS.source.into()]);
        fs.sources.push(GROUND_ATMOSPHERE.source.into());
        if let Some(material) = material {
            fs.sources.push(material.into());
            vs.defines.push("APPLY_MATERIAL".into());
            fs.defines.push("APPLY_MATERIAL".into());
        }
        fs.sources.push(GLOBE_FS.source.into());

        vs.defines.push("INCLUDE_WEB_MERCATOR_Y".into());
        fs.defines.extend(["INCLUDE_WEB_MERCATOR_Y".into(), format!("TEXTURE_UNITS {}", self.texture_count)]);
        for (feature, name) in [
            (Brightness, "APPLY_BRIGHTNESS"),
            (Contrast, "APPLY_CONTRAST"),
            (Hue, "APPLY_HUE"),
            (Saturation, "APPLY_SATURATION"),
            (Gamma, "APPLY_GAMMA"),
            (Alpha, "APPLY_ALPHA"),
            (DayNightAlpha, "APPLY_DAY_NIGHT_ALPHA"),
            (WaterMask, "HAS_WATER_MASK"),
            (OceanWaves, "SHOW_OCEAN_WAVES"),
            (Split, "APPLY_SPLIT"),
            (TileLimit, "TILE_LIMIT_RECTANGLE"),
            (Cutout, "APPLY_IMAGERY_CUTOUT"),
            (ColorCorrect, "COLOR_CORRECT"),
            (HighlightFill, "HIGHLIGHT_FILL_TILE"),
            (ColorToAlpha, "APPLY_COLOR_TO_ALPHA"),
        ] {
            if enabled(feature) {
                fs.defines.push(name.into());
            }
        }
        for (feature, name) in [
            (Quantization, "QUANTIZATION_BITS12"),
            (GeodeticNormals, "GEODETIC_SURFACE_NORMALS"),
            (Exaggeration, "EXAGGERATION"),
        ] {
            if enabled(feature) {
                vs.defines.push(name.into());
            }
        }
        for (condition, name) in [
            (enabled(ReflectiveOcean), "SHOW_REFLECTIVE_OCEAN"),
            (enabled(Underground), "UNDERGROUND_COLOR"),
            (enabled(Translucent), "TRANSLUCENT"),
            (enabled(Fog), "FOG"),
            (enabled(Lighting) && enabled(VertexNormals), "ENABLE_VERTEX_LIGHTING"),
            (enabled(Lighting) && !enabled(VertexNormals), "ENABLE_DAYNIGHT_SHADING"),
            (enabled(DynamicLighting), "DYNAMIC_ATMOSPHERE_LIGHTING"),
            (enabled(DynamicLighting) && enabled(SunLighting), "DYNAMIC_ATMOSPHERE_LIGHTING_FROM_SUN"),
            (enabled(GroundAtmosphere), "GROUND_ATMOSPHERE"),
            (enabled(GroundAtmosphere) && enabled(PerFragmentAtmosphere), "PER_FRAGMENT_GROUND_ATMOSPHERE"),
        ] {
            if condition {
                vs.defines.push(name.into());
                fs.defines.push(name.into());
            }
        }
        if let Some(planes) = self.clipping_planes {
            fs.defines.push("ENABLE_CLIPPING_PLANES".into());
            fs.sources.insert(
                0,
                super::get_clipping_function::clipping_functions(
                    planes.union,
                    planes.length,
                    planes.floating_point,
                    planes.texture_size[0],
                    planes.texture_size[1],
                ),
            );
        }
        if let Some(polygons) = self.clipping_polygons {
            vs.defines.push("ENABLE_CLIPPING_POLYGONS".into());
            fs.defines.push("ENABLE_CLIPPING_POLYGONS".into());
            if polygons.inverse {
                fs.defines.push("CLIPPING_INVERSE".into());
            }
            let length = format!("CLIPPING_POLYGON_REGIONS_LENGTH {}", polygons.extents_count);
            vs.defines.push(length.clone());
            fs.defines.push(length);
            vs.sources.insert(0, "vec4 unpackClippingExtents(highp sampler2D extents, int i) { return czm_unpackClippingExtents(extents, i); }".into());
            fs.sources.insert(0, "void clipPolygons(highp sampler2D texture_, int length_, vec2 uv, int i) { czm_clipPolygons(texture_, length_, uv, i); }".into());
        }
        let position_mode = match self.mode {
            SceneMode::Scene3D => "getPosition3DMode",
            SceneMode::Scene2D | SceneMode::ColumbusView => "getPositionColumbusViewMode",
            SceneMode::Morphing => "getPositionMorphingMode",
        };
        let projection =
            if enabled(WebMercator) { "get2DMercatorYPositionFraction" } else { "get2DGeographicYPositionFraction" };
        vs.sources.extend([
            format!("vec4 getPosition(vec3 p, float h, vec2 uv) {{ return {position_mode}(p, h, uv); }}"),
            format!("float get2DYPositionFraction(vec2 uv) {{ return {projection}(uv); }}"),
        ]);
        fs.sources.push(day_color(self.texture_count, self.features()));
        ShaderProgramOptions {
            vertex_shader_source: vs,
            fragment_shader_source: fs,
            attribute_locations: self.attributes.into_iter().map(|(name, location)| (name.into(), location)).collect(),
        }
    }
}

fn day_color(count: usize, features: GlobeShaderFeatures) -> String {
    use GlobeShaderFeature::*;
    let mut source = String::from(
        "vec4 computeDayColor(vec4 initialColor, vec3 uv, float nightBlend) {\n    vec4 color = initialColor;\n",
    );
    for i in 0..count {
        let cutout = features.contains(Cutout);
        let dest = if cutout { format!("vec4 sampleColor{i}") } else { "color".into() };
        write!(source, "    {dest} = sampleAndBlend(color, u_dayTextures[{i}], u_dayTextureUseWebMercatorT[{i}] ? uv.xz : uv.xy, u_dayTextureTexCoordsRectangle[{i}], u_dayTextureTranslationAndScale[{i}], ").unwrap();
        for (feature, uniform, fallback) in [
            (Alpha, "u_dayTextureAlpha", "1.0"),
            (DayNightAlpha, "u_dayTextureNightAlpha", "1.0"),
            (DayNightAlpha, "u_dayTextureDayAlpha", "1.0"),
            (Brightness, "u_dayTextureBrightness", "0.0"),
            (Contrast, "u_dayTextureContrast", "0.0"),
            (Hue, "u_dayTextureHue", "0.0"),
            (Saturation, "u_dayTextureSaturation", "0.0"),
            (Gamma, "u_dayTextureOneOverGamma", "0.0"),
            (Split, "u_dayTextureSplit", "0.0"),
            (ColorToAlpha, "u_colorsToAlpha", "vec4(0.0)"),
        ] {
            if features.contains(feature) {
                write!(source, "{uniform}[{i}], ").unwrap();
            } else {
                write!(source, "{fallback}, ").unwrap();
            }
        }
        source.push_str("nightBlend);\n");
        if cutout {
            writeln!(source, "    vec4 cutout{i} = u_dayTextureCutoutRectangles[{i}];\n    bool unclipped{i} = any(lessThan(uv.xy, cutout{i}.xy)) || any(greaterThan(uv.xy, cutout{i}.zw));\n    color = czm_branchFreeTernary(unclipped{i}, sampleColor{i}, color);").unwrap();
        }
    }
    source.push_str("    return color;\n}");
    source
}

pub struct GlobeSurfaceShaderSet<G: Gl> {
    cache: ShaderCache<G>,
    programs: HashMap<GlobeSurfaceShaderOptions, Rc<ShaderProgram<G>>>,
    material: Option<String>,
}

impl<G: Gl> GlobeSurfaceShaderSet<G> {
    pub fn new(context: &Context<G>) -> Self {
        Self { cache: ShaderCache::new(context), programs: HashMap::new(), material: None }
    }

    pub fn set_material(&mut self, source: Option<String>) -> Result<(), RuntimeError> {
        if self.material != source {
            self.material = source;
            self.programs.clear();
            self.cache.destroy_released_shader_programs()?;
        }
        Ok(())
    }

    pub fn get_shader_program(
        &mut self,
        options: &GlobeSurfaceShaderOptions,
    ) -> Result<Rc<ShaderProgram<G>>, RuntimeError> {
        if let Some(program) = self.programs.get(options).filter(|p| !p.is_destroyed()) {
            return Ok(program.clone());
        }
        let program = self.cache.get_shader_program(&options.sources(self.material.as_deref()))?;
        self.programs.insert(options.clone(), program.clone());
        Ok(program)
    }

    pub fn get_derived_program(
        &mut self,
        program: &Rc<ShaderProgram<G>>,
        command: GlobeTranslucencyCommand,
        classification: bool,
    ) -> Result<Rc<ShaderProgram<G>>, RuntimeError> {
        let keyword = format!("globe{command:?}{classification}");
        if let Some(derived) = self.cache.get_derived_shader_program(program, &keyword) {
            if !derived.is_destroyed() {
                return Ok(derived);
            }
            let options = command.shader(program.source_options().unwrap().clone(), classification);
            return self.cache.replace_derived_shader_program(program, &keyword, &options);
        }
        let options = command.shader(program.source_options().unwrap().clone(), classification);
        self.cache.create_derived_shader_program(program, &keyword, &options)
    }

    pub fn number_of_shaders(&self) -> usize {
        self.programs.len()
    }

    pub fn is_destroyed(&self) -> bool {
        self.cache.is_destroyed()
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.programs.clear();
        self.cache.destroy()
    }
}

pub(crate) type Uniforms = Vec<(String, UniformValue)>;

pub(crate) fn float_uniform(name: &str, values: &[f64]) -> (String, UniformValue) {
    (name.into(), UniformValue::Float(values.iter().map(|&value| value as f32).collect()))
}

pub(crate) fn matrix_uniform(name: &str, matrix: Matrix4) -> (String, UniformValue) {
    (name.into(), UniformValue::Matrix { dimension: 4, values: matrix.0.map(|x| x as f32).to_vec() })
}

#[derive(Clone, Copy)]
pub(crate) struct GlobeImageryUniforms {
    pub translation: Cartesian4,
    pub rectangle: Cartesian4,
    pub mercator: bool,
    pub alpha: f64,
    pub night_alpha: f64,
    pub day_alpha: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub hue: f64,
    pub saturation: f64,
    pub one_over_gamma: f64,
    pub split: f64,
    pub cutout: Option<Cartesian4>,
    pub color_to_alpha: Option<Cartesian4>,
}

impl GlobeImageryUniforms {
    pub fn append_batch(imagery: &[Self], features: &mut GlobeShaderFeatures, uniforms: &mut Uniforms) {
        use GlobeShaderFeature::*;
        if imagery.is_empty() {
            return;
        }
        type VectorUniform = (&'static str, fn(&GlobeImageryUniforms) -> Cartesian4);
        let vec4s: [VectorUniform; 4] = [
            ("u_dayTextureTranslationAndScale", |i| i.translation),
            ("u_dayTextureTexCoordsRectangle", |i| i.rectangle),
            ("u_dayTextureCutoutRectangles", |i| i.cutout.unwrap_or(Cartesian4::ZERO)),
            ("u_colorsToAlpha", |i| i.color_to_alpha.unwrap_or(Cartesian4::new(0.0, 0.0, 0.0, -1.0))),
        ];
        for (name, getter) in vec4s {
            let values = imagery
                .iter()
                .flat_map(|i| {
                    let v = getter(i);
                    [v.x as f32, v.y as f32, v.z as f32, v.w as f32]
                })
                .collect();
            uniforms.push((name.into(), UniformValue::FloatArray { components: 4, values }));
        }
        uniforms.push((
            "u_dayTextureUseWebMercatorT".into(),
            UniformValue::IntArray {
                components: 1,
                values: imagery.iter().map(|imagery| i32::from(imagery.mercator)).collect(),
            },
        ));
        type Adjustment = (GlobeShaderFeature, &'static str, fn(&GlobeImageryUniforms) -> f64, f64);
        let adjustments: [Adjustment; 9] = [
            (Alpha, "u_dayTextureAlpha", |i| i.alpha, 1.0),
            (DayNightAlpha, "u_dayTextureNightAlpha", |i| i.night_alpha, 1.0),
            (DayNightAlpha, "u_dayTextureDayAlpha", |i| i.day_alpha, 1.0),
            (Brightness, "u_dayTextureBrightness", |i| i.brightness, 1.0),
            (Contrast, "u_dayTextureContrast", |i| i.contrast, 1.0),
            (Hue, "u_dayTextureHue", |i| i.hue, 0.0),
            (Saturation, "u_dayTextureSaturation", |i| i.saturation, 1.0),
            (Gamma, "u_dayTextureOneOverGamma", |i| i.one_over_gamma, 1.0),
            (Split, "u_dayTextureSplit", |i| i.split, 0.0),
        ];
        for (feature, name, getter, default) in adjustments {
            if imagery.iter().any(|i| getter(i) != default) {
                features.set(feature, true);
            }
            uniforms.push((
                name.into(),
                UniformValue::FloatArray { components: 1, values: imagery.iter().map(|i| getter(i) as f32).collect() },
            ));
        }
        features.set(Cutout, imagery.iter().any(|i| i.cutout.is_some()));
        features.set(ColorToAlpha, imagery.iter().any(|i| i.color_to_alpha.is_some()));
    }
}

pub(crate) fn tile_uniform_map<G: Gl + 'static>(
    context: Context<G>,
    center: Cartesian3,
    clipping: Option<Matrix4>,
    material: Option<UniformMap>,
) -> UniformMap {
    Rc::new(move || {
        let state = context.uniform_state();
        let view = state.view;
        let translated = view.set_translation(view.multiply_by_point(center));
        let projection = state.projection;
        drop(state);
        let mut values = material.as_ref().map_or_else(|| Ok(Vec::new()), |map| map())?;
        values.push(matrix_uniform("u_modifiedModelView", translated));
        values.push(matrix_uniform("u_modifiedModelViewProjection", projection.multiply(&translated)));
        if let Some(clipping) = clipping {
            values.push(matrix_uniform("u_clippingPlanesMatrix", view.multiply(&clipping).inverse_transpose()?));
        }
        Ok(values)
    })
}
