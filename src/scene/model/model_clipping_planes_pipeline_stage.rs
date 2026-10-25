//! CesiumJS `Scene/Model/ModelClippingPlanesPipelineStage`: configures clipping
//! plane defines, uniforms, and fragment shader stage for a model.

use super::ModelRenderResources;
use crate::core::Matrix4;
use crate::renderer::{Gl, ShaderDestination, Texture, UniformValue};
use crate::scene::ClippingPlaneCollection;
use crate::shaders;
use std::cell::RefCell;
use std::rc::Rc;

pub struct ModelClippingPlanesState<G: Gl> {
    pub clipping_planes: Rc<RefCell<ClippingPlaneCollection>>,
    pub clipping_planes_matrix: Rc<RefCell<Matrix4>>,
    pub use_float_texture: bool,
    pub max_texture_size: u32,
    pub default_texture: Option<Rc<Texture<G>>>,
}

pub fn process<G: Gl + 'static>(resources: &mut ModelRenderResources<G>, state: &ModelClippingPlanesState<G>) {
    let shader = &mut resources.shader_builder;
    let planes = state.clipping_planes.borrow();

    shader.add_define("HAS_CLIPPING_PLANES", None, ShaderDestination::FRAGMENT);
    shader.add_define("CLIPPING_PLANES_LENGTH", Some(&planes.len().to_string()), ShaderDestination::FRAGMENT);

    if planes.union_clipping_regions {
        shader.add_define("UNION_CLIPPING_REGIONS", None, ShaderDestination::FRAGMENT);
    }

    if state.use_float_texture {
        shader.add_define("USE_CLIPPING_PLANES_FLOAT_TEXTURE", None, ShaderDestination::FRAGMENT);
    }

    let [width, height] = planes.texture_resolution(state.use_float_texture, state.max_texture_size);
    shader.add_define("CLIPPING_PLANES_TEXTURE_WIDTH", Some(&width.to_string()), ShaderDestination::FRAGMENT);
    shader.add_define("CLIPPING_PLANES_TEXTURE_HEIGHT", Some(&height.to_string()), ShaderDestination::FRAGMENT);
    drop(planes);

    shader.add_uniform("sampler2D", "model_clippingPlanes", ShaderDestination::FRAGMENT);
    shader.add_uniform("vec4", "model_clippingPlanesEdgeStyle", ShaderDestination::FRAGMENT);
    shader.add_uniform("mat4", "model_clippingPlanesMatrix", ShaderDestination::FRAGMENT);

    shader.add_fragment_lines(&[shaders::model::MODEL_CLIPPING_PLANES_STAGE_FS.source]);

    let planes_for_tex = state.clipping_planes.clone();
    let default_tex = state.default_texture.clone();
    resources.texture_uniform_map.insert(
        "model_clippingPlanes".into(),
        Rc::new(move || planes_for_tex.borrow().texture::<G>().or_else(|| default_tex.clone())),
    );

    let planes_for_style = state.clipping_planes.clone();
    resources.uniform_map.insert(
        "model_clippingPlanesEdgeStyle".into(),
        Rc::new(move || {
            let p = planes_for_style.borrow();
            let mut style = p.edge_color;
            style.alpha = p.edge_width;
            UniformValue::Float(vec![style.red as f32, style.green as f32, style.blue as f32, style.alpha as f32])
        }),
    );

    let matrix = state.clipping_planes_matrix.clone();
    resources.uniform_map.insert(
        "model_clippingPlanesMatrix".into(),
        Rc::new(move || UniformValue::Matrix {
            dimension: 4,
            values: matrix.borrow().0.iter().map(|&v| v as f32).collect(),
        }),
    );
}
