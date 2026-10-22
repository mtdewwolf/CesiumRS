//! Cesium createUniform/createUniformArray with typed values and sampler leases.
use super::{context::*, *};
use crate::core::webgl_constants as gl;
pub struct UniformBinding<'a, G: Gl> {
    program: &'a ShaderProgram<G>,
    pub name: String,
    pub datatype: u32,
    pub size: usize,
    array: bool,
    pub texture_unit_index: Option<u32>,
    textures: Vec<TextureBinding<G>>,
}
fn shape(datatype: u32) -> Option<(u32, u32, u32)> {
    // category: floating point, signed/bool, unsigned, matrix, sampler.
    Some(match datatype {
        gl::FLOAT => (0, 1, 1),
        gl::FLOAT_VEC2 => (0, 2, 1),
        gl::FLOAT_VEC3 => (0, 3, 1),
        gl::FLOAT_VEC4 => (0, 4, 1),
        gl::INT | gl::BOOL => (1, 1, 1),
        gl::INT_VEC2 | gl::BOOL_VEC2 => (1, 2, 1),
        gl::INT_VEC3 | gl::BOOL_VEC3 => (1, 3, 1),
        gl::INT_VEC4 | gl::BOOL_VEC4 => (1, 4, 1),
        gl::UNSIGNED_INT => (2, 1, 1),
        gl::UNSIGNED_INT_VEC2 => (2, 2, 1),
        gl::UNSIGNED_INT_VEC3 => (2, 3, 1),
        gl::UNSIGNED_INT_VEC4 => (2, 4, 1),
        gl::FLOAT_MAT2 => (3, 2, 2),
        gl::FLOAT_MAT3 => (3, 3, 3),
        gl::FLOAT_MAT4 => (3, 4, 4),
        gl::SAMPLER_2D | gl::UNSIGNED_INT_SAMPLER_2D => (4, gl::TEXTURE_2D, 1),
        gl::SAMPLER_CUBE => (4, gl::TEXTURE_CUBE_MAP, 1),
        gl::SAMPLER_3D => (4, gl::TEXTURE_3D, 1),
        _ => return None,
    })
}
pub fn create_uniform<'a, G: Gl>(
    program: &'a ShaderProgram<G>,
    name: impl Into<String>,
    datatype: u32,
) -> Result<UniformBinding<'a, G>> {
    create(program, name.into(), datatype, 1, false)
}
pub fn create_uniform_array<'a, G: Gl>(
    program: &'a ShaderProgram<G>,
    name: impl Into<String>,
    datatype: u32,
    length: usize,
) -> Result<UniformBinding<'a, G>> {
    create(program, name.into(), datatype, length, true)
}
fn create<G: Gl>(
    program: &ShaderProgram<G>,
    name: String,
    datatype: u32,
    size: usize,
    array: bool,
) -> Result<UniformBinding<'_, G>> {
    ensure(shape(datatype).is_some(), &format!("Unrecognized uniform type: {datatype} for uniform \"{name}\"."))?;
    ensure(size > 0, "Uniform array must not be empty")?;
    Ok(UniformBinding { program, name, datatype, size, array, texture_unit_index: None, textures: vec![] })
}
impl<G: Gl> UniformBinding<'_, G> {
    pub fn set(&self, value: &UniformValue) -> Result<()> {
        let (category, components, rows) = shape(self.datatype)
            .ok_or_else(|| crate::core::RuntimeError::new(format!("Unrecognized uniform type: {}", self.datatype)))?;
        let (actual, width, height, length) = match value {
            UniformValue::Float(v) => (0, v.len() as u32, 1, v.len()),
            UniformValue::Int(v) => (1, v.len() as u32, 1, v.len()),
            UniformValue::FloatArray { components, values } => (0, *components, 1, values.len()),
            UniformValue::IntArray { components, values } => (1, *components, 1, values.len()),
            UniformValue::UIntArray { components, values } => (2, *components, 1, values.len()),
            UniformValue::Matrix { dimension, values } => (3, *dimension, *dimension, values.len()),
            UniformValue::MatrixArray { columns, rows, values } => (3, *columns, *rows, values.len()),
        };
        ensure(
            (category == actual || category == 4 && actual == 1)
                && width == if category == 4 { 1 } else { components }
                && height == rows
                && length == if category == 4 { self.size } else { self.size * components as usize * rows as usize },
            "Uniform value does not match its declaration",
        )?;
        self.program.set_uniform(&self.name, value)
    }
    pub fn set_sampler(&mut self, first: u32) -> Result<u32> {
        let category = shape(self.datatype)
            .ok_or_else(|| crate::core::RuntimeError::new(format!("Unrecognized uniform type: {}", self.datatype)))?
            .0;
        ensure(category == 4, "Uniform is not a sampler")?;
        let end = first
            .checked_add(
                u32::try_from(self.size).map_err(|_| crate::core::RuntimeError::new("Sampler count overflow"))?,
            )
            .ok_or_else(|| crate::core::RuntimeError::new("Sampler unit overflow"))?;
        if let GlValue::Number(max) =
            self.program.resource.context.call("getParameter", &[gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS.into()])?
        {
            ensure(end as f64 <= max, "Sampler units exceed context limits")?;
        }
        let units: Vec<_> = (first..end).map(|n| n as i32).collect();
        self.program.set_uniform(
            &self.name,
            &if self.array {
                UniformValue::IntArray { components: 1, values: units }
            } else {
                UniformValue::Int(units)
            },
        )?;
        self.texture_unit_index = Some(first);
        Ok(end)
    }
    pub fn set_textures(&mut self, textures: Vec<TextureBinding<G>>) -> Result<()> {
        let (category, target, _) = shape(self.datatype)
            .ok_or_else(|| crate::core::RuntimeError::new(format!("Unrecognized uniform type: {}", self.datatype)))?;
        ensure(category == 4 && textures.len() == self.size, "Sampler texture count does not match declaration")?;
        let first = self
            .texture_unit_index
            .ok_or_else(|| crate::core::RuntimeError::new("Assign sampler units before binding textures"))?;
        for t in &textures {
            let t = t.sampled();
            self.program.resource.context.same(t.context())?;
            ensure(t.target() == target && !t.is_destroyed(), "Invalid sampler texture target or destroyed resource")?;
        }
        for (i, t) in textures.iter().enumerate() {
            t.sampled().bind(first + i as u32)?;
        }
        let context = &self.program.resource.context;
        context.frame_texture_units.set(context.frame_texture_units.get().max(first + self.size as u32));
        self.textures = textures;
        Ok(())
    }
}
