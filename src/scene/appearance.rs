//! CesiumJS `Appearance`: the shader sources and render state used to draw a
//! `Primitive`.
//!
//! `Appearance` is the shared interface; the concrete appearances are
//! [`MaterialAppearance`], [`EllipsoidSurfaceAppearance`],
//! [`PerInstanceColorAppearance`], [`PolylineColorAppearance`],
//! [`PolylineMaterialAppearance`] and [`DebugAppearance`]. They return an
//! owned [`RenderState`], which the renderer applies directly.

use super::blending_state::alpha_blend;
use super::material::Material;
use crate::core::VertexFormat;
use crate::renderer::render_state::RenderState;
use crate::scene::CullFace;

/// The common `Appearance` interface.
pub trait Appearance {
    fn material(&self) -> Option<&Material>;
    fn material_mut(&mut self) -> Option<&mut Material>;
    fn translucent(&self) -> bool;
    fn closed(&self) -> bool;
    /// `Appearance#flat` where the concrete appearance defines it.
    fn flat(&self) -> Option<bool> {
        None
    }
    /// `Appearance#faceForward` where the concrete appearance defines it.
    fn face_forward(&self) -> Option<bool> {
        None
    }
    fn vertex_shader_source(&self) -> String;
    fn fragment_shader_source(&self) -> String;
    /// The state returned by `Appearance#renderState`.\
    fn render_state(&self) -> &RenderState;
    fn vertex_format(&self) -> VertexFormat;

    /// `Appearance#getFragmentShaderSource`.
    fn get_fragment_shader_source(&self) -> String {
        let mut parts = Vec::new();
        if self.flat() == Some(true) {
            parts.push("#define FLAT".to_string());
        }
        if self.face_forward() == Some(true) {
            parts.push("#define FACE_FORWARD".to_string());
        }
        if let Some(material) = self.material() {
            parts.push(material.shader_source.clone());
        }
        parts.push(self.fragment_shader_source());
        parts.join("\n")
    }

    /// `Appearance#isTranslucent`.
    fn is_translucent(&self) -> bool {
        match self.material() {
            Some(material) => material.is_translucent(),
            None => self.translucent(),
        }
    }

    /// `Appearance#getRenderState`.
    fn get_render_state(&self) -> RenderState {
        let mut rs = self.render_state().clone();
        if self.is_translucent() {
            rs.depth_mask = false;
            rs.blending = alpha_blend();
        } else {
            rs.depth_mask = true;
        }
        rs
    }
}

/// `Appearance.getDefaultRenderState(translucent, closed, existing)`.
///
/// A supplied `existing` state wins over the defaults, as Cesium's
/// `combine(existing, defaults)` does. Cesium's `existing` is a partial render
/// state; Rust callers supply the complete state they want.
pub fn default_render_state(translucent: bool, closed: bool, existing: Option<RenderState>) -> RenderState {
    let mut rs = RenderState::default();
    rs.depth_test.enabled = true;
    if translucent {
        rs.depth_mask = false;
        rs.blending = alpha_blend();
    }
    if closed {
        rs.cull.enabled = true;
        rs.cull.face = CullFace::Back.value();
    }
    if let Some(existing) = existing {
        if existing.depth_mask {
            rs.depth_mask = true;
        }
        if existing.depth_test.enabled {
            rs.depth_test = existing.depth_test;
        }
        if existing.cull.enabled {
            rs.cull = existing.cull;
        }
        if existing.blending.enabled {
            rs.blending = existing.blending;
        }
    }
    rs
}

/// Builds the shader sources and render state shared by the concrete
/// appearances.
#[derive(Clone, Debug)]
pub struct AppearanceState {
    pub material: Option<Material>,
    pub translucent: bool,
    pub closed: bool,
    pub flat: Option<bool>,
    pub face_forward: Option<bool>,
    pub vertex_shader_source: String,
    pub fragment_shader_source: String,
    pub render_state: RenderState,
    pub vertex_format: VertexFormat,
}

impl AppearanceState {
    pub fn get_fragment_shader_source(&self) -> String {
        let mut parts = Vec::new();
        if self.flat == Some(true) {
            parts.push("#define FLAT".to_string());
        }
        if self.face_forward == Some(true) {
            parts.push("#define FACE_FORWARD".to_string());
        }
        if let Some(material) = &self.material {
            parts.push(material.shader_source.clone());
        }
        parts.push(self.fragment_shader_source.clone());
        parts.join("\n")
    }

    pub fn is_translucent(&self) -> bool {
        match &self.material {
            Some(material) => material.is_translucent(),
            None => self.translucent,
        }
    }

    pub fn get_render_state(&self) -> RenderState {
        let mut rs = self.render_state.clone();
        if self.is_translucent() {
            rs.depth_mask = false;
            rs.blending = alpha_blend();
        } else {
            rs.depth_mask = true;
        }
        rs
    }
}
