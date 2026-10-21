//! Automatic uniform declarations and view/model/projection state.
//! Scene snapshots drive geometry, lighting, astronomy and atmosphere values.
//! Explicit named overrides remain available for custom rendering hosts.
use super::{context::*, *};
use crate::core::{BoundingRectangle, Cartesian3, EncodedCartesian3, Matrix4, RuntimeError};
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};
#[derive(Clone, Debug, Deserialize)]
pub struct AutomaticUniform {
    pub datatype: u32,
    pub size: usize,
}
pub fn automatic_uniforms() -> &'static HashMap<String, AutomaticUniform> {
    static TABLE: OnceLock<HashMap<String, AutomaticUniform>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/renderer/automatic-uniforms.json"))
            .expect("pinned automatic uniforms")
    })
}
impl AutomaticUniform {
    pub fn get_value(&self, name: &str, state: &UniformState) -> Result<Option<UniformValue>> {
        state.get(name)
    }
    pub fn declaration(&self, name: &str) -> Result<String> {
        let ty = match self.datatype {
            5126 => "float",
            35664 => "vec2",
            35665 => "vec3",
            35666 => "vec4",
            5124 => "int",
            35667 => "ivec2",
            35668 => "ivec3",
            35669 => "ivec4",
            35670 => "bool",
            35671 => "bvec2",
            35672 => "bvec3",
            35673 => "bvec4",
            35674 => "mat2",
            35675 => "mat3",
            35676 => "mat4",
            35678 => "sampler2D",
            35680 => "samplerCube",
            _ => return Err(RuntimeError::new("Unsupported automatic uniform datatype")),
        };
        Ok(format!("uniform {ty} {name}{};", if self.size == 1 { String::new() } else { format!("[{}]", self.size) }))
    }
}
#[derive(Clone, Debug)]
pub struct UniformState {
    pub model: Matrix4,
    pub view: Matrix4,
    pub inverse_view: Matrix4,
    pub projection: Matrix4,
    pub infinite_projection: Matrix4,
    pub view_3d: Matrix4,
    pub viewport: Viewport,
    pub camera_position: Cartesian3,
    pub frustum: [f64; 2],
    pub frustum_planes: [f64; 4],
    pub entire_frustum: [f64; 2],
    pub pass: Option<Pass>,
    pub orthographic_in_3d: bool,
    pub scene_mode: u32,
    values: HashMap<String, UniformValue>,
    pub(crate) frame: Option<std::rc::Rc<super::UniformFrame>>,
    pub(crate) frustum_2d_width: f64,
}
impl Default for UniformState {
    fn default() -> Self {
        Self {
            model: Matrix4::IDENTITY,
            view: Matrix4::IDENTITY,
            inverse_view: Matrix4::IDENTITY,
            projection: Matrix4::IDENTITY,
            infinite_projection: Matrix4::IDENTITY,
            view_3d: Matrix4::IDENTITY,
            viewport: Viewport::default(),
            camera_position: Cartesian3::ZERO,
            frustum: [0.; 2],
            entire_frustum: [0.; 2],
            frustum_planes: [0.; 4],
            pass: None,
            orthographic_in_3d: false,
            scene_mode: 3,
            values: HashMap::new(),
            frame: None,
            frustum_2d_width: 0.,
        }
    }
}
impl UniformState {
    pub fn set(&mut self, name: impl Into<String>, value: UniformValue) {
        self.values.insert(name.into(), value);
    }
    pub fn remove(&mut self, name: &str) -> Option<UniformValue> {
        self.values.remove(name)
    }
    pub fn update_camera(&mut self, view: Matrix4, inverse_view: Matrix4, position: Cartesian3) {
        self.view = view;
        self.inverse_view = inverse_view;
        self.camera_position = position;
        if self.scene_mode == 3 {
            self.view_3d = view;
        }
    }
    pub fn update_frustum(&mut self, projection: Matrix4, infinite: Option<Matrix4>, near: f64, far: f64) {
        self.projection = projection;
        if let Some(m) = infinite {
            self.infinite_projection = m;
        }
        self.frustum = [near, far];
    }
    pub fn update_pass(&mut self, pass: Pass) {
        self.pass = Some(pass);
    }
    pub fn get(&self, name: &str) -> Result<Option<UniformValue>> {
        if let Some(v) = self.values.get(name) {
            return Ok(Some(v.clone()));
        }
        let mat = |m: Matrix4| UniformValue::Matrix { dimension: 4, values: m.0.iter().map(|v| *v as f32).collect() };
        let mat3 = |m: crate::core::Matrix3| UniformValue::Matrix {
            dimension: 3,
            values: m.0.iter().map(|v| *v as f32).collect(),
        };
        let vec = |v: &[f64]| UniformValue::Float(v.iter().map(|v| *v as f32).collect());
        let mv = || self.view.multiply(&self.model);
        let mv3 = || self.view_3d.multiply(&self.model);
        let mvp = || self.projection.multiply(&mv());
        let relative = || {
            let mut m = mv();
            m.0[12] = 0.;
            m.0[13] = 0.;
            m.0[14] = 0.;
            m
        };
        let viewport = BoundingRectangle::new(
            self.viewport.x as f64,
            self.viewport.y as f64,
            self.viewport.width as f64,
            self.viewport.height as f64,
        );
        let value = match name {
            "czm_model" => mat(self.model),
            "czm_inverseModel" => mat(self.model.inverse()?),
            "czm_inverseTransposeModel" => mat3(self.model.inverse_transpose()?.get_matrix3()),
            "czm_view" => mat(self.view),
            "czm_view3D" => mat(self.view_3d),
            "czm_inverseView" => mat(self.inverse_view),
            "czm_inverseView3D" => mat(self.view_3d.inverse_transformation()),
            "czm_viewRotation" => mat3(self.view.get_matrix3()),
            "czm_viewRotation3D" => mat3(self.view_3d.get_matrix3()),
            "czm_inverseViewRotation" => mat3(self.inverse_view.get_matrix3()),
            "czm_inverseViewRotation3D" => mat3(self.view_3d.inverse_transformation().get_matrix3()),
            "czm_projection" => mat(self.projection),
            "czm_inverseProjection" => {
                mat(if self.orthographic_in_3d || self.scene_mode == 0 || self.scene_mode == 2 {
                    Matrix4::ZERO
                } else {
                    self.projection.inverse()?
                })
            }
            "czm_infiniteProjection" => mat(self.infinite_projection),
            "czm_modelView" => mat(mv()),
            "czm_modelView3D" => mat(mv3()),
            "czm_modelViewRelativeToEye" => mat(relative()),
            "czm_inverseModelView" => mat(mv().inverse()?),
            "czm_inverseModelView3D" => mat(mv3().inverse()?),
            "czm_viewProjection" => mat(self.projection.multiply(&self.view)),
            "czm_inverseViewProjection" => mat(self.projection.multiply(&self.view).inverse()?),
            "czm_modelViewProjection" => mat(mvp()),
            "czm_inverseModelViewProjection" => mat(mvp().inverse()?),
            "czm_modelViewProjectionRelativeToEye" => mat(self.projection.multiply(&relative())),
            "czm_modelViewInfiniteProjection" => mat(self.infinite_projection.multiply(&mv())),
            "czm_normal" => mat3(mv().inverse_transpose()?.get_matrix3()),
            "czm_normal3D" => mat3(mv3().inverse_transpose()?.get_matrix3()),
            "czm_inverseNormal" => mat3(mv().get_matrix3().transpose()),
            "czm_inverseNormal3D" => mat3(mv3().get_matrix3().transpose()),
            "czm_viewport" => vec(&[viewport.x, viewport.y, viewport.width, viewport.height]),
            "czm_viewportTransformation" => mat(if self.viewport == Viewport::default() {
                Matrix4::IDENTITY
            } else {
                Matrix4::compute_viewport_transformation(&viewport, 0., 1.)
            }),
            "czm_viewportOrthographic" => mat(if self.viewport == Viewport::default() {
                Matrix4::IDENTITY
            } else {
                Matrix4::compute_orthographic_off_center(
                    viewport.x,
                    viewport.x + viewport.width,
                    viewport.y,
                    viewport.y + viewport.height,
                    0.,
                    1.,
                )
            }),
            "czm_currentFrustum" => vec(&self.frustum),
            "czm_entireFrustum" => vec(&self.entire_frustum),
            "czm_frustumPlanes" => vec(&self.frustum_planes),
            "czm_farDepthFromNearPlusOne" => vec(&[self.frustum[1] - self.frustum[0] + 1.]),
            "czm_log2FarDepthFromNearPlusOne" => vec(&[(self.frustum[1] - self.frustum[0] + 1.).log2()]),
            "czm_oneOverLog2FarDepthFromNearPlusOne" => vec(&[1. / (self.frustum[1] - self.frustum[0] + 1.).log2()]),
            "czm_encodedCameraPositionMCHigh" | "czm_encodedCameraPositionMCLow" => {
                let encoded =
                    EncodedCartesian3::from_cartesian(self.model.inverse()?.multiply_by_point(self.camera_position));
                let c = if name.ends_with("High") { encoded.high } else { encoded.low };
                vec(&[c.x, c.y, c.z])
            }
            "czm_viewerPositionWC" => {
                let c = self.inverse_view.get_translation();
                vec(&[c.x, c.y, c.z])
            }
            "czm_pass" => vec(&[self.pass.map_or(0., |p| p as u32 as f64)]),
            "czm_sceneMode" => vec(&[self.scene_mode as f64]),
            "czm_pixelRatio" => vec(&[1.]),
            "czm_orthographicIn3D" => vec(&[if self.orthographic_in_3d { 1. } else { 0. }]),
            "czm_eyeHeight" => vec(&[0.]),
            "czm_eyeHeight2D" | "czm_eyeEllipsoidCurvature" => vec(&[0.; 2]),
            "czm_eyeEllipsoidNormalEC" => vec(&[0.; 3]),
            "czm_modelToEnu" | "czm_enuToModel" => mat(Matrix4::ZERO),
            "czm_ellipsoidRadii" => {
                let v = crate::core::Ellipsoid::WGS84.radii;
                vec(&[v.x, v.y, v.z])
            }
            "czm_ellipsoidInverseRadii" => {
                let v = crate::core::Ellipsoid::WGS84.one_over_radii;
                vec(&[v.x, v.y, v.z])
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
}
impl<G: Gl> ShaderProgram<G> {
    pub fn set_automatic_uniforms(&self, state: &UniformState) -> Result<()> {
        self.set_automatic_uniforms_from(state, 0).map(|_| ())
    }
    pub(crate) fn set_automatic_uniforms_from(&self, state: &UniformState, mut unit: u32) -> Result<u32> {
        let cache = self.resource.context.cache_ref.upgrade();
        for uniform in self.uniforms() {
            let name = self.original_uniform_name(uniform.name.strip_suffix("[0]").unwrap_or(&uniform.name));
            if automatic_uniforms().contains_key(name) {
                if let Some(texture) = cache.as_ref().and_then(|c| c.automatic_textures.borrow().get(name).cloned()) {
                    texture.sampled().bind(unit)?;
                    self.set_uniform(name, &UniformValue::Int(vec![unit as i32]))?;
                    unit += 1;
                    continue;
                }
                let value = state.get(name)?.ok_or_else(|| {
                    RuntimeError::new(format!("Automatic uniform {name} requires a value from the scene host"))
                })?;
                self.set_uniform(name, &value)?;
            }
        }
        Ok(unit)
    }
}
