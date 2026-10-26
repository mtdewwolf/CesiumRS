//! CesiumJS `EllipsoidSurfaceAppearance`: geometry on the ellipsoid's surface.

use super::appearance::{default_render_state, Appearance, AppearanceState};
use super::material::{Material, COLOR_TYPE};
use crate::core::VertexFormat;
use crate::renderer::render_state::RenderState;
use crate::shaders::appearances as appearance_shaders;

#[derive(Clone, Debug, Default)]
pub struct EllipsoidSurfaceAppearanceOptions {
    pub flat: Option<bool>,
    pub face_forward: Option<bool>,
    pub translucent: Option<bool>,
    pub above_ground: Option<bool>,
    pub material: Option<Material>,
    pub vertex_shader_source: Option<String>,
    pub fragment_shader_source: Option<String>,
    pub render_state: Option<RenderState>,
}

#[derive(Clone, Debug)]
pub struct EllipsoidSurfaceAppearance {
    state: AppearanceState,
    above_ground: bool,
}

impl EllipsoidSurfaceAppearance {
    /// `EllipsoidSurfaceAppearance.VERTEX_FORMAT`.
    pub const VERTEX_FORMAT: VertexFormat = VertexFormat::POSITION_AND_ST;

    pub fn new(options: EllipsoidSurfaceAppearanceOptions) -> Self {
        let translucent = options.translucent.unwrap_or(true);
        let above_ground = options.above_ground.unwrap_or(false);
        let state = AppearanceState {
            material: Some(options.material.unwrap_or_else(|| Material::from_type(COLOR_TYPE))),
            translucent,
            closed: false,
            flat: Some(options.flat.unwrap_or(false)),
            face_forward: Some(options.face_forward.unwrap_or(above_ground)),
            vertex_shader_source: options
                .vertex_shader_source
                .unwrap_or_else(|| appearance_shaders::ELLIPSOID_SURFACE_APPEARANCE_VS.source.to_string()),
            fragment_shader_source: options
                .fragment_shader_source
                .unwrap_or_else(|| appearance_shaders::ELLIPSOID_SURFACE_APPEARANCE_FS.source.to_string()),
            render_state: default_render_state(translucent, !above_ground, options.render_state),
            vertex_format: Self::VERTEX_FORMAT,
        };
        Self { state, above_ground }
    }
    pub fn above_ground(&self) -> bool {
        self.above_ground
    }
}

impl Appearance for EllipsoidSurfaceAppearance {
    fn material(&self) -> Option<&Material> {
        self.state.material.as_ref()
    }
    fn material_mut(&mut self) -> Option<&mut Material> {
        self.state.material.as_mut()
    }
    fn translucent(&self) -> bool {
        self.state.translucent
    }
    fn closed(&self) -> bool {
        self.state.closed
    }
    fn flat(&self) -> Option<bool> {
        self.state.flat
    }
    fn face_forward(&self) -> Option<bool> {
        self.state.face_forward
    }
    fn vertex_shader_source(&self) -> String {
        self.state.vertex_shader_source.clone()
    }
    fn fragment_shader_source(&self) -> String {
        self.state.fragment_shader_source.clone()
    }
    fn render_state(&self) -> &RenderState {
        &self.state.render_state
    }
    fn vertex_format(&self) -> VertexFormat {
        self.state.vertex_format
    }
    fn get_fragment_shader_source(&self) -> String {
        self.state.get_fragment_shader_source()
    }
    fn is_translucent(&self) -> bool {
        self.state.is_translucent()
    }
    fn get_render_state(&self) -> RenderState {
        self.state.get_render_state()
    }
}

impl Default for EllipsoidSurfaceAppearance {
    fn default() -> Self {
        Self::new(EllipsoidSurfaceAppearanceOptions::default())
    }
}
