//! CesiumJS `PolylineColorAppearance`: polylines colored per instance.

use super::appearance::{default_render_state, Appearance, AppearanceState};
use super::material::Material;
use crate::core::VertexFormat;
use crate::renderer::render_state::RenderState;
use crate::shaders::{appearances as appearance_shaders, root as root_shaders};

#[derive(Clone, Debug, Default)]
pub struct PolylineColorAppearanceOptions {
    pub translucent: Option<bool>,
    pub vertex_shader_source: Option<String>,
    pub fragment_shader_source: Option<String>,
    pub render_state: Option<RenderState>,
}

#[derive(Clone, Debug)]
pub struct PolylineColorAppearance {
    state: AppearanceState,
}

/// The default `PolylineColorAppearance` vertex shader.
pub fn default_vertex_shader_source() -> String {
    format!(
        "#define CLIP_POLYLINE \n{}\n{}",
        root_shaders::POLYLINE_COMMON.source,
        appearance_shaders::POLYLINE_COLOR_APPEARANCE_VS.source
    )
}

impl PolylineColorAppearance {
    /// `PolylineColorAppearance.VERTEX_FORMAT`: position only.
    pub const VERTEX_FORMAT: VertexFormat = VertexFormat::POSITION_ONLY;

    pub fn new(options: PolylineColorAppearanceOptions) -> Self {
        let translucent = options.translucent.unwrap_or(true);
        let state = AppearanceState {
            material: None,
            translucent,
            closed: false,
            flat: None,
            face_forward: None,
            vertex_shader_source: options.vertex_shader_source.unwrap_or_else(default_vertex_shader_source),
            fragment_shader_source: options
                .fragment_shader_source
                .unwrap_or_else(|| appearance_shaders::PER_INSTANCE_FLAT_COLOR_APPEARANCE_FS.source.to_string()),
            render_state: default_render_state(translucent, false, options.render_state),
            vertex_format: Self::VERTEX_FORMAT,
        };
        Self { state }
    }
}

impl Appearance for PolylineColorAppearance {
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

impl Default for PolylineColorAppearance {
    fn default() -> Self {
        Self::new(PolylineColorAppearanceOptions::default())
    }
}
