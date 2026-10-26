//! CesiumJS `PolylineMaterialAppearance`: polylines shaded by a material.

use super::appearance::{default_render_state, Appearance, AppearanceState};
use super::material::{Material, COLOR_TYPE};
use crate::core::VertexFormat;
use crate::renderer::render_state::RenderState;
use crate::shaders::{appearances as appearance_shaders, root as root_shaders};

#[derive(Clone, Debug, Default)]
pub struct PolylineMaterialAppearanceOptions {
    pub translucent: Option<bool>,
    pub material: Option<Material>,
    pub vertex_shader_source: Option<String>,
    pub fragment_shader_source: Option<String>,
    pub render_state: Option<RenderState>,
}

#[derive(Clone, Debug)]
pub struct PolylineMaterialAppearance {
    state: AppearanceState,
}

/// The default `PolylineMaterialAppearance` vertex shader.
pub fn default_vertex_shader_source() -> String {
    format!(
        "#define CLIP_POLYLINE \n{}\n{}",
        root_shaders::POLYLINE_COMMON.source,
        appearance_shaders::POLYLINE_MATERIAL_APPEARANCE_VS.source
    )
}

impl PolylineMaterialAppearance {
    /// `PolylineMaterialAppearance.VERTEX_FORMAT`: position and st.
    pub const VERTEX_FORMAT: VertexFormat = VertexFormat::POSITION_AND_ST;

    pub fn new(options: PolylineMaterialAppearanceOptions) -> Self {
        let translucent = options.translucent.unwrap_or(true);
        let state = AppearanceState {
            material: Some(options.material.unwrap_or_else(|| Material::from_type(COLOR_TYPE))),
            translucent,
            closed: false,
            flat: None,
            face_forward: None,
            vertex_shader_source: options.vertex_shader_source.unwrap_or_else(default_vertex_shader_source),
            fragment_shader_source: options
                .fragment_shader_source
                .unwrap_or_else(|| root_shaders::POLYLINE_FS.source.to_string()),
            render_state: default_render_state(translucent, false, options.render_state),
            vertex_format: Self::VERTEX_FORMAT,
        };
        Self { state }
    }
}

impl Appearance for PolylineMaterialAppearance {
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
    fn vertex_shader_source(&self) -> String {
        // Cesium adds POLYLINE_DASH when the material reads v_polylineAngle.
        let needs_dash = self.state.material.as_ref().is_some_and(|m| polyline_dash_regex().is_match(&m.shader_source));
        if needs_dash {
            format!("#define POLYLINE_DASH\n{}", self.state.vertex_shader_source)
        } else {
            self.state.vertex_shader_source.clone()
        }
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

impl Default for PolylineMaterialAppearance {
    fn default() -> Self {
        Self::new(PolylineMaterialAppearanceOptions::default())
    }
}

fn polyline_dash_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static REGEX: OnceLock<regex::Regex> = OnceLock::new();
    REGEX.get_or_init(|| regex::Regex::new(r"in\s+float\s+v_polylineAngle;").expect("valid polyline dash regex"))
}
