//! CesiumJS `MaterialAppearance`: arbitrary geometry shaded by a material.

use super::appearance::{default_render_state, Appearance, AppearanceState};
use super::material::{Material, COLOR_TYPE};
use crate::core::VertexFormat;
use crate::renderer::render_state::RenderState;
use crate::shaders::appearances as appearance_shaders;

/// `MaterialAppearance.MaterialSupport`, a trade-off between material
/// flexibility and the required vertex attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaterialSupport {
    Basic,
    #[default]
    Textured,
    All,
}

impl MaterialSupport {
    pub fn vertex_format(self) -> VertexFormat {
        match self {
            Self::Basic => VertexFormat::POSITION_AND_NORMAL,
            Self::Textured => VertexFormat::POSITION_NORMAL_AND_ST,
            Self::All => VertexFormat::ALL,
        }
    }
    pub fn vertex_shader_source(self) -> &'static str {
        match self {
            Self::Basic => appearance_shaders::BASIC_MATERIAL_APPEARANCE_VS.source,
            Self::Textured => appearance_shaders::TEXTURED_MATERIAL_APPEARANCE_VS.source,
            Self::All => appearance_shaders::ALL_MATERIAL_APPEARANCE_VS.source,
        }
    }
    pub fn fragment_shader_source(self) -> &'static str {
        match self {
            Self::Basic => appearance_shaders::BASIC_MATERIAL_APPEARANCE_FS.source,
            Self::Textured => appearance_shaders::TEXTURED_MATERIAL_APPEARANCE_FS.source,
            Self::All => appearance_shaders::ALL_MATERIAL_APPEARANCE_FS.source,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MaterialAppearanceOptions {
    pub flat: Option<bool>,
    pub face_forward: Option<bool>,
    pub translucent: Option<bool>,
    pub closed: Option<bool>,
    pub material_support: Option<MaterialSupport>,
    pub material: Option<Material>,
    pub vertex_shader_source: Option<String>,
    pub fragment_shader_source: Option<String>,
    pub render_state: Option<RenderState>,
}

#[derive(Clone, Debug)]
pub struct MaterialAppearance {
    state: AppearanceState,
    material_support: MaterialSupport,
}

impl MaterialAppearance {
    pub fn new(options: MaterialAppearanceOptions) -> Self {
        let translucent = options.translucent.unwrap_or(true);
        let closed = options.closed.unwrap_or(false);
        let support = options.material_support.unwrap_or_default();
        let state = AppearanceState {
            material: Some(options.material.unwrap_or_else(|| Material::from_type(COLOR_TYPE))),
            translucent,
            closed,
            flat: Some(options.flat.unwrap_or(false)),
            face_forward: Some(options.face_forward.unwrap_or(!closed)),
            vertex_shader_source: options
                .vertex_shader_source
                .unwrap_or_else(|| support.vertex_shader_source().to_string()),
            fragment_shader_source: options
                .fragment_shader_source
                .unwrap_or_else(|| support.fragment_shader_source().to_string()),
            render_state: default_render_state(translucent, closed, options.render_state),
            vertex_format: support.vertex_format(),
        };
        Self { state, material_support: support }
    }
    pub fn material_support(&self) -> MaterialSupport {
        self.material_support
    }
    pub fn state(&self) -> &AppearanceState {
        &self.state
    }
}

impl Appearance for MaterialAppearance {
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

impl Default for MaterialAppearance {
    fn default() -> Self {
        Self::new(MaterialAppearanceOptions::default())
    }
}
