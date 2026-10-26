//! CesiumJS `PerInstanceColorAppearance`: one color per geometry instance.

use super::appearance::{default_render_state, Appearance, AppearanceState};
use super::material::Material;
use crate::core::VertexFormat;
use crate::renderer::render_state::RenderState;
use crate::shaders::appearances as appearance_shaders;

#[derive(Clone, Debug, Default)]
pub struct PerInstanceColorAppearanceOptions {
    pub flat: Option<bool>,
    pub face_forward: Option<bool>,
    pub translucent: Option<bool>,
    pub closed: Option<bool>,
    pub vertex_shader_source: Option<String>,
    pub fragment_shader_source: Option<String>,
    pub render_state: Option<RenderState>,
}

#[derive(Clone, Debug)]
pub struct PerInstanceColorAppearance {
    state: AppearanceState,
}

impl PerInstanceColorAppearance {
    /// `PerInstanceColorAppearance.VERTEX_FORMAT`: position and normal.
    pub const VERTEX_FORMAT: VertexFormat = VertexFormat::POSITION_AND_NORMAL;
    /// `PerInstanceColorAppearance.FLAT_VERTEX_FORMAT`: position only.
    pub const FLAT_VERTEX_FORMAT: VertexFormat = VertexFormat::POSITION_ONLY;

    pub fn new(options: PerInstanceColorAppearanceOptions) -> Self {
        let translucent = options.translucent.unwrap_or(true);
        let closed = options.closed.unwrap_or(false);
        let flat = options.flat.unwrap_or(false);
        let (vs, fs) = if flat {
            (
                appearance_shaders::PER_INSTANCE_FLAT_COLOR_APPEARANCE_VS.source,
                appearance_shaders::PER_INSTANCE_FLAT_COLOR_APPEARANCE_FS.source,
            )
        } else {
            (
                appearance_shaders::PER_INSTANCE_COLOR_APPEARANCE_VS.source,
                appearance_shaders::PER_INSTANCE_COLOR_APPEARANCE_FS.source,
            )
        };
        let state = AppearanceState {
            material: None,
            translucent,
            closed,
            flat: Some(flat),
            face_forward: Some(options.face_forward.unwrap_or(!closed)),
            vertex_shader_source: options.vertex_shader_source.unwrap_or_else(|| vs.to_string()),
            fragment_shader_source: options.fragment_shader_source.unwrap_or_else(|| fs.to_string()),
            render_state: default_render_state(translucent, closed, options.render_state),
            vertex_format: if flat { Self::FLAT_VERTEX_FORMAT } else { Self::VERTEX_FORMAT },
        };
        Self { state }
    }
}

impl Appearance for PerInstanceColorAppearance {
    fn material(&self) -> Option<&Material> {
        None
    }
    fn material_mut(&mut self) -> Option<&mut Material> {
        None
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

impl Default for PerInstanceColorAppearance {
    fn default() -> Self {
        Self::new(PerInstanceColorAppearanceOptions::default())
    }
}
