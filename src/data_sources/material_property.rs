//! Resolves a material property against a reusable scene material.
use super::{property::system_clock, DynamicValue, EntityComponent};
use crate::{
    core::{Color, JulianDate, RuntimeError},
    scene::{Material, MaterialImage, MaterialValue},
};

/// The static helper on Cesium's abstract MaterialProperty. Implement the
/// material's getType/getValue contract through [`EntityComponent`].
pub struct MaterialProperty;
impl MaterialProperty {
    /// Reuses the supplied material while its type matches. Move the returned
    /// material into the next evaluation to retain its renderer state.
    pub fn get_value(
        time: Option<&JulianDate>,
        property: Option<&dyn EntityComponent>,
        material: Option<Material>,
    ) -> Result<Material, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (system_clock())());
        let kind = property.map(|p| p.get_type(&time)).transpose()?.flatten();
        let type_name = kind.as_deref().unwrap_or("Color");
        let mut material = match material {
            Some(m) if m.type_name == type_name => m,
            _ => Material::from_type_with_uniforms(type_name, Vec::new())?,
        };
        if let (Some(property), Some(_)) = (property, kind) {
            let mut result = DynamicValue::Record(
                material
                    .uniforms
                    .iter()
                    .filter_map(|(name, v)| read_uniform(v).map(|v| (name.clone(), Some(v))))
                    .collect(),
            );
            property.get_value(&time, Some(&mut result))?;
            if let DynamicValue::Record(fields) = result {
                for (name, value) in fields {
                    let Some(old) = material.uniform(&name) else { continue };
                    if let Some(value) = write_uniform(value, old)? {
                        material.set_uniform(&name, value)?;
                    }
                }
            }
        } else {
            material.set_uniform("color", MaterialValue::from_color(Color::WHITE))?;
        }
        Ok(material)
    }
}
fn read_uniform(value: &MaterialValue) -> Option<DynamicValue> {
    Some(match value {
        MaterialValue::Float(v) => (*v).into(),
        MaterialValue::Bool(v) => (*v).into(),
        MaterialValue::Vec2(v) => (*v).into(),
        MaterialValue::Vec3(v) | MaterialValue::IntVec3(v) => (*v).into(),
        MaterialValue::Vec4(v) => Color::new(v.x, v.y, v.z, v.w).into(),
        MaterialValue::Mat3(v) => (*v).into(),
        MaterialValue::Mat4(v) => (*v).into(),
        MaterialValue::Channels(v) => v.clone().into(),
        MaterialValue::Sampler2D(MaterialImage::Url(s)) => s.clone().into(),
        MaterialValue::Sampler2D(MaterialImage::Default) => crate::scene::material::DEFAULT_IMAGE_ID.into(),
        _ => return None,
    })
}
fn write_uniform(value: Option<DynamicValue>, old: &MaterialValue) -> Result<Option<MaterialValue>, RuntimeError> {
    use DynamicValue as D;
    use MaterialValue as M;
    Ok(Some(match (value, old) {
        (Some(D::Color(v)), _) => M::from_color(v),
        (Some(D::Cartesian2(v)), _) => M::Vec2(v),
        (Some(D::Cartesian3(v)), M::IntVec3(_)) => M::IntVec3(v),
        (Some(D::Cartesian3(v)), _) => M::Vec3(v),
        (Some(D::Cartesian4(v)), _) => M::Vec4(v),
        (Some(D::Matrix3(v)), _) => M::Mat3(*v),
        (Some(D::Matrix4(v)), _) => M::Mat4(*v),
        (Some(D::Boolean(v)), _) => M::Bool(v),
        (Some(D::Number(v)), _) => M::Float(v),
        (Some(D::String(s)), M::Channels(_)) => M::Channels(s),
        (Some(D::String(s)), M::Sampler2D(_)) if s == crate::scene::material::DEFAULT_IMAGE_ID => {
            M::Sampler2D(MaterialImage::Default)
        }
        (Some(D::String(s)), M::Sampler2D(_)) => M::Sampler2D(MaterialImage::Url(s)),
        (Some(D::Resource(v)), M::Sampler2D(_)) => M::Sampler2D(MaterialImage::Url(v.url())),
        (None | Some(D::Null), M::Sampler2D(_)) => M::Sampler2D(MaterialImage::Default),
        (None, _) => return Ok(None),
        _ => return Err(RuntimeError::new("unsupported material uniform value")),
    }))
}
