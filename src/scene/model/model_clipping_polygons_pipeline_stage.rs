//! CesiumJS `Scene/Model/ModelClippingPolygonsPipelineStage`: configures clipping
//! polygon defines, varyings, uniforms, and vertex/fragment shader stages.

use super::ModelRenderResources;
use crate::renderer::{Gl, ShaderDestination, Texture};
use crate::shaders;
use std::cell::RefCell;
use std::rc::Rc;

pub struct ModelClippingPolygonsState<G: Gl> {
    pub inverse: bool,
    pub compute_polygon_bounds_length: usize,
    pub clipping_texture: Rc<RefCell<Option<Rc<Texture<G>>>>>,
    pub extents_texture: Rc<RefCell<Option<Rc<Texture<G>>>>>,
    pub default_texture: Option<Rc<Texture<G>>>,
}

pub fn process<G: Gl + 'static>(resources: &mut ModelRenderResources<G>, state: &ModelClippingPolygonsState<G>) {
    let shader = &mut resources.shader_builder;

    shader.add_define("ENABLE_CLIPPING_POLYGONS", None, ShaderDestination::BOTH);

    if state.inverse {
        shader.add_define("CLIPPING_INVERSE", None, ShaderDestination::FRAGMENT);
    }

    shader.add_define(
        "CLIPPING_POLYGON_REGIONS_LENGTH",
        Some(&state.compute_polygon_bounds_length.to_string()),
        ShaderDestination::BOTH,
    );

    shader.add_uniform("sampler2D", "model_clippingDistance", ShaderDestination::FRAGMENT);
    shader.add_uniform("sampler2D", "model_clippingExtents", ShaderDestination::VERTEX);

    shader.add_varying("vec2", "v_clippingPosition", None);
    shader.add_varying("int", "v_regionIndex", Some("flat"));

    shader.add_vertex_lines(&[shaders::model::MODEL_CLIPPING_POLYGONS_STAGE_VS.source]);
    shader.add_fragment_lines(&[shaders::model::MODEL_CLIPPING_POLYGONS_STAGE_FS.source]);

    let clipping_tex = state.clipping_texture.clone();
    let default_texture = state.default_texture.clone();
    resources.texture_uniform_map.insert(
        "model_clippingDistance".into(),
        Rc::new(move || clipping_tex.borrow().clone().or_else(|| default_texture.clone())),
    );

    let extents_tex = state.extents_texture.clone();
    let default_texture = state.default_texture.clone();
    resources.texture_uniform_map.insert(
        "model_clippingExtents".into(),
        Rc::new(move || extents_tex.borrow().clone().or_else(|| default_texture.clone())),
    );
}
