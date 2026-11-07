//! CesiumJS `BatchTable`: per-instance attributes stored in an RGBA texture
//! that vertex shaders read by batch id.
//!
//! Each instance occupies `stride` texels, one per attribute (four when
//! floats are packed into bytes because float textures are unsupported).
//! Values are kept in a CPU array of the texture's type, so a value reads
//! back as Cesium's typed array stores it: float32-rounded, or truncated to a
//! byte modulo 256. `update` creates the texture on first use and uploads the
//! array when it changed; [`BatchTable::vertex_shader_callback`] generates
//! the GLSL accessors. Checked against a SwiftShader capture of the pinned
//! engine in `tests/scene_batch_table.rs`.
use std::rc::Rc;

use crate::{
    core::{math, Cartesian2, Cartesian3, Cartesian4, ComponentDatatype, PixelDatatype, PixelFormat, RuntimeError},
    renderer::{Context, Gl, Sampler, Texture, TextureOptions, TextureRegion, UniformValue},
};

type Result<T> = std::result::Result<T, RuntimeError>;

/// One batched attribute: `{ functionName, componentDatatype,
/// componentsPerAttribute, normalize }`.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchTableAttribute {
    pub function_name: String,
    pub component_datatype: ComponentDatatype,
    pub components_per_attribute: u32,
    pub normalize: bool,
}

/// A batched value: a number, or a Cartesian with 2 to 4 components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BatchValue {
    Scalar(f64),
    Vec2(Cartesian2),
    Vec3(Cartesian3),
    Vec4(Cartesian4),
}

impl BatchValue {
    /// Component `i`, `None` where the JavaScript value has no such property.
    fn component(&self, i: usize) -> Option<f64> {
        match (*self, i) {
            (Self::Scalar(v), 0) => Some(v),
            (Self::Vec2(v), 0) | (Self::Vec2(v), 1) => Some([v.x, v.y][i]),
            (Self::Vec3(v), 0..=2) => Some([v.x, v.y, v.z][i]),
            (Self::Vec4(v), 0..=3) => Some([v.x, v.y, v.z, v.w][i]),
            _ => None,
        }
    }
    fn of(components: u32, v: Cartesian4) -> Self {
        match components {
            2 => Self::Vec2(Cartesian2::new(v.x, v.y)),
            3 => Self::Vec3(Cartesian3::new(v.x, v.y, v.z)),
            4 => Self::Vec4(v),
            _ => Self::Scalar(v.x),
        }
    }
}

impl From<f64> for BatchValue {
    fn from(v: f64) -> Self {
        Self::Scalar(v)
    }
}
impl From<Cartesian2> for BatchValue {
    fn from(v: Cartesian2) -> Self {
        Self::Vec2(v)
    }
}
impl From<Cartesian3> for BatchValue {
    fn from(v: Cartesian3) -> Self {
        Self::Vec3(v)
    }
}
impl From<Cartesian4> for BatchValue {
    fn from(v: Cartesian4) -> Self {
        Self::Vec4(v)
    }
}

/// `_batchValues`: a `Float32Array` or a `Uint8Array`.
#[derive(Clone, Debug, PartialEq)]
pub enum BatchValues {
    Float(Vec<f32>),
    Bytes(Vec<u8>),
}

impl BatchValues {
    fn get(&self, i: usize) -> f64 {
        match self {
            Self::Float(v) => v[i] as f64,
            Self::Bytes(v) => v[i] as f64,
        }
    }
    /// A typed-array store: float32 rounding, or ToUint8 (modulo 256).
    fn set(&mut self, i: usize, value: f64) {
        match self {
            Self::Float(v) => v[i] = value as f32,
            Self::Bytes(v) => v[i] = math::to_int32(value) as u8,
        }
    }
    /// The array's bytes, as uploaded.
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Float(v) => v.iter().flat_map(|f| f.to_le_bytes()).collect(),
            Self::Bytes(v) => v.clone(),
        }
    }
}

/// The texture layout of a non-empty table.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchTableLayout {
    pub offsets: Vec<u32>,
    pub stride: u32,
    pub pack_floats: bool,
    pub pixel_datatype: PixelDatatype,
    /// Texture width and height in texels.
    pub dimensions: Cartesian2,
    /// `(stepX, centerX, stepY, centerY)`.
    pub step: Cartesian4,
}

pub struct BatchTable<G: Gl + 'static> {
    attributes: Vec<BatchTableAttribute>,
    number_of_instances: usize,
    layout: Option<BatchTableLayout>,
    values: Option<BatchValues>,
    dirty: bool,
    texture: Option<Rc<Texture<G>>>,
}

impl<G: Gl + 'static> BatchTable<G> {
    /// `new BatchTable(context, attributes, numberOfInstances)`. Floats are
    /// packed into bytes when the context has no float textures.
    pub fn new(context: &Context<G>, attributes: Vec<BatchTableAttribute>, number_of_instances: usize) -> Result<Self> {
        let mut table =
            Self { attributes, number_of_instances, layout: None, values: None, dirty: false, texture: None };
        if table.attributes.is_empty() {
            return Ok(table);
        }
        let float = table.attributes.iter().any(|a| a.component_datatype != ComponentDatatype::UnsignedByte);
        let pack_floats = float && !context.capabilities()?.floating_point_texture;
        let is_packed =
            |a: &BatchTableAttribute| a.component_datatype != ComponentDatatype::UnsignedByte && pack_floats;
        let mut offsets = Vec::with_capacity(table.attributes.len());
        let mut offset = 0;
        for a in &table.attributes {
            offsets.push(offset);
            offset += if is_packed(a) { 4 } else { 1 };
        }
        let stride = offset;
        let per_row = context.limits()?.maximum_texture_size / stride;
        let per_width = (number_of_instances as u32).min(per_row);
        if per_width == 0 {
            return Err(RuntimeError::new("BatchTable needs at least one instance per texture row."));
        }
        let width = stride * per_width;
        let height = (number_of_instances as u32).div_ceil(per_width);
        let (step_x, step_y) = (1.0 / width as f64, 1.0 / height as f64);
        let length = 4 * (width * height) as usize;
        table.values = Some(if float && !pack_floats {
            BatchValues::Float(vec![0.0; length])
        } else {
            BatchValues::Bytes(vec![0; length])
        });
        table.layout = Some(BatchTableLayout {
            offsets,
            stride,
            pack_floats,
            pixel_datatype: if float && !pack_floats { PixelDatatype::Float } else { PixelDatatype::UnsignedByte },
            dimensions: Cartesian2::new(width as f64, height as f64),
            step: Cartesian4::new(step_x, step_x * 0.5, step_y, step_y * 0.5),
        });
        Ok(table)
    }

    pub fn attributes(&self) -> &[BatchTableAttribute] {
        &self.attributes
    }
    pub fn number_of_instances(&self) -> usize {
        self.number_of_instances
    }
    /// The texture layout; `None` without attributes.
    pub fn layout(&self) -> Option<&BatchTableLayout> {
        self.layout.as_ref()
    }
    /// `_batchValues`.
    pub fn batch_values(&self) -> Option<&BatchValues> {
        self.values.as_ref()
    }
    /// Whether values changed since the last upload.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    /// `batchTexture`, once `update` has created it.
    pub fn texture(&self) -> Option<&Rc<Texture<G>>> {
        self.texture.as_ref()
    }

    fn check(&self, instance: usize, attribute: usize) -> Result<(&BatchTableLayout, usize)> {
        if instance >= self.number_of_instances {
            return Err(RuntimeError::new("instanceIndex is out of range."));
        }
        if attribute >= self.attributes.len() {
            return Err(RuntimeError::new("attributeIndex is out of range"));
        }
        let layout = self.layout.as_ref().expect("a table with attributes has a layout");
        let index = 4 * layout.stride as usize * instance + 4 * layout.offsets[attribute] as usize;
        Ok((layout, index))
    }

    fn packed(&self, layout: &BatchTableLayout, attribute: usize) -> bool {
        layout.pack_floats && self.attributes[attribute].component_datatype != ComponentDatatype::UnsignedByte
    }

    /// `getBatchedAttribute`: the value as stored.
    pub fn get_batched_attribute(&self, instance: usize, attribute: usize) -> Result<BatchValue> {
        let (layout, index) = self.check(instance, attribute)?;
        let values = self.values.as_ref().expect("a table with attributes has values");
        let texel = |i: usize| Cartesian4::new(values.get(i), values.get(i + 1), values.get(i + 2), values.get(i + 3));
        let value = if self.packed(layout, attribute) {
            Cartesian4::new(
                Cartesian4::unpack_float(texel(index)),
                Cartesian4::unpack_float(texel(index + 4)),
                Cartesian4::unpack_float(texel(index + 8)),
                Cartesian4::unpack_float(texel(index + 12)),
            )
        } else {
            texel(index)
        };
        Ok(BatchValue::of(self.attributes[attribute].components_per_attribute, value))
    }

    /// `setBatchedAttribute`: stores `value` unless it equals the stored
    /// value, compared as Cesium's attribute type compares (a missing
    /// component never equals).
    pub fn set_batched_attribute(
        &mut self,
        instance: usize,
        attribute: usize,
        value: impl Into<BatchValue>,
    ) -> Result<()> {
        let value = value.into();
        let current = self.get_batched_attribute(instance, attribute)?;
        let components = self.attributes[attribute].components_per_attribute as usize;
        let equal = if components == 1 {
            matches!(value, BatchValue::Scalar(v) if Some(v) == current.component(0))
        } else {
            (0..components).all(|i| value.component(i).is_some() && value.component(i) == current.component(i))
        };
        if equal {
            return Ok(());
        }
        self.store(instance, attribute, value)
    }

    /// Stores `value` without Cesium's equality check, for callers whose
    /// JavaScript value never compares equal (a boolean component).
    pub(crate) fn set_batched_attribute_unchecked(
        &mut self,
        instance: usize,
        attribute: usize,
        value: impl Into<BatchValue>,
    ) -> Result<()> {
        self.store(instance, attribute, value.into())
    }

    fn store(&mut self, instance: usize, attribute: usize, value: BatchValue) -> Result<()> {
        let v = [
            value.component(0).unwrap_or(f64::NAN),
            value.component(1).unwrap_or(0.0),
            value.component(2).unwrap_or(0.0),
            value.component(3).unwrap_or(0.0),
        ];
        let (layout, index) = self.check(instance, attribute)?;
        let packed = self.packed(layout, attribute);
        let values = self.values.as_mut().expect("a table with attributes has values");
        if packed {
            for (k, component) in v.iter().enumerate() {
                let p = Cartesian4::pack_float(*component);
                for (j, c) in [p.x, p.y, p.z, p.w].into_iter().enumerate() {
                    values.set(index + 4 * k + j, c);
                }
            }
        } else {
            for (j, c) in v.into_iter().enumerate() {
                values.set(index + j, c);
            }
        }
        self.dirty = true;
        Ok(())
    }

    /// `update`: create the texture on first use and upload changed values.
    pub fn update(&mut self, context: &Context<G>) -> Result<()> {
        let Some(layout) = &self.layout else { return Ok(()) };
        if self.texture.is_some() && !self.dirty {
            return Ok(());
        }
        self.dirty = false;
        let (width, height) = (layout.dimensions.x as u32, layout.dimensions.y as u32);
        if self.texture.is_none() {
            let mut options = TextureOptions::rgba(width, height);
            options.pixel_format = PixelFormat::Rgba;
            options.pixel_datatype = layout.pixel_datatype;
            options.sampler = Sampler::NEAREST;
            options.flip_y = false;
            self.texture = Some(Rc::new(Texture::new(context, options, None)?));
        }
        let bytes = self.values.as_ref().expect("a table with attributes has values").bytes();
        self.texture.as_ref().unwrap().copy_from(&bytes, TextureRegion { x: 0, y: 0, width, height })
    }

    /// The uniform names `getUniformMapCallback` adds: none without
    /// attributes.
    pub fn uniform_names(&self) -> &'static [&'static str] {
        if self.attributes.is_empty() {
            &[]
        } else {
            &["batchTexture", "batchTextureDimensions", "batchTextureStep"]
        }
    }

    /// `batchTextureDimensions` and `batchTextureStep`; `batchTexture` is
    /// [`BatchTable::texture`].
    pub fn uniform_values(&self) -> Vec<(String, UniformValue)> {
        let Some(l) = &self.layout else { return Vec::new() };
        vec![
            ("batchTextureDimensions".into(), UniformValue::Float(vec![l.dimensions.x as f32, l.dimensions.y as f32])),
            (
                "batchTextureStep".into(),
                UniformValue::Float(vec![l.step.x as f32, l.step.y as f32, l.step.z as f32, l.step.w as f32]),
            ),
        ]
    }

    fn glsl_compute_st(layout: &BatchTableLayout) -> String {
        let stride = layout.stride;
        if layout.dimensions.y == 1.0 {
            return format!(
                "uniform vec4 batchTextureStep; \nvec2 computeSt(float batchId) \n{{ \n    float stepX = batchTextureStep.x; \n    float centerX = batchTextureStep.y; \n    float numberOfAttributes = float({stride}); \n    return vec2(centerX + (batchId * numberOfAttributes * stepX), 0.5); \n}} \n"
            );
        }
        format!(
            "uniform vec4 batchTextureStep; \nuniform vec2 batchTextureDimensions; \nvec2 computeSt(float batchId) \n{{ \n    float stepX = batchTextureStep.x; \n    float centerX = batchTextureStep.y; \n    float stepY = batchTextureStep.z; \n    float centerY = batchTextureStep.w; \n    float numberOfAttributes = float({stride}); \n    float xId = mod(batchId * numberOfAttributes, batchTextureDimensions.x); \n    float yId = floor(batchId * numberOfAttributes / batchTextureDimensions.x); \n    return vec2(centerX + (xId * stepX), centerY + (yId * stepY)); \n}} \n"
        )
    }

    fn glsl_attribute_function(&self, layout: &BatchTableLayout, index: usize) -> String {
        let a = &self.attributes[index];
        let (ty, swizzle) = match a.components_per_attribute {
            1 => ("float".to_string(), ".x"),
            2 => ("vec2".to_string(), ".xy"),
            3 => ("vec3".to_string(), ".xyz"),
            n => (format!("vec{n}"), ""),
        };
        let mut f = format!(
            "{ty} {}(float batchId) \n{{ \n    vec2 st = computeSt(batchId); \n    st.x += batchTextureStep.x * float({}); \n",
            a.function_name, layout.offsets[index]
        );
        if self.packed(layout, index) {
            f += "vec4 textureValue; \ntextureValue.x = czm_unpackFloat(texture(batchTexture, st)); \ntextureValue.y = czm_unpackFloat(texture(batchTexture, st + vec2(batchTextureStep.x, 0.0))); \ntextureValue.z = czm_unpackFloat(texture(batchTexture, st + vec2(batchTextureStep.x * 2.0, 0.0))); \ntextureValue.w = czm_unpackFloat(texture(batchTexture, st + vec2(batchTextureStep.x * 3.0, 0.0))); \n";
        } else {
            f += "    vec4 textureValue = texture(batchTexture, st); \n";
        }
        f += &format!("    {ty} value = textureValue{swizzle}; \n");
        let byte = a.component_datatype == ComponentDatatype::UnsignedByte;
        if layout.pixel_datatype == PixelDatatype::UnsignedByte && byte && !a.normalize {
            f += "value *= 255.0; \n";
        } else if layout.pixel_datatype == PixelDatatype::Float && byte && a.normalize {
            f += "value /= 255.0; \n";
        }
        f + "    return value; \n} \n"
    }

    /// `getVertexShaderCallback`: inserts the texture, `computeSt` and one
    /// accessor per attribute before `void main`.
    pub fn vertex_shader_callback(&self) -> impl Fn(&str) -> String {
        let shader = self.layout.as_ref().map(|layout| {
            let mut s = String::from("uniform highp sampler2D batchTexture; \n");
            s += &Self::glsl_compute_st(layout);
            s += "\n";
            for i in 0..self.attributes.len() {
                s += &self.glsl_attribute_function(layout, i);
            }
            s
        });
        move |source: &str| match &shader {
            None => source.to_string(),
            Some(shader) => {
                // JavaScript's indexOf: -1 when absent, which substring treats as 0.
                let main = source.find("void main").unwrap_or(0);
                format!("{}\n{shader}\n{}", &source[..main], &source[main..])
            }
        }
    }

    /// Destroys the texture.
    pub fn destroy(&mut self) -> Result<()> {
        if let Some(texture) = self.texture.take() {
            texture.destroy()?;
        }
        Ok(())
    }
}
