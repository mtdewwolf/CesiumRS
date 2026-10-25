//! CesiumJS `Scene/Model/BatchTexturePipelineStage`: sets up the batch texture
//! uniforms and defines used to look up per-feature styling in a `Cesium3DTileFeatureTable`.

use super::PrimitiveRenderResources;
use crate::core::{Cartesian2, Cartesian4};
use crate::renderer::{Gl, ShaderDestination, Texture, UniformValue};
use std::cell::RefCell;
use std::rc::Rc;

pub struct BatchTextureState<G: Gl> {
    pub features_length: usize,
    pub batch_texture: Option<Rc<Texture<G>>>,
    pub default_texture: Option<Rc<Texture<G>>>,
    pub texture_step: Cartesian4,
    pub texture_dimensions: Cartesian2,
}

pub fn process<G: Gl + 'static>(resources: &mut PrimitiveRenderResources<G>, state: Rc<RefCell<BatchTextureState<G>>>) {
    let shader = &mut resources.shader_builder;

    shader.add_uniform("int", "model_featuresLength", ShaderDestination::BOTH);
    shader.add_uniform("sampler2D", "model_batchTexture", ShaderDestination::BOTH);
    shader.add_uniform("vec4", "model_textureStep", ShaderDestination::BOTH);

    if state.borrow().texture_dimensions.y > 1.0 {
        shader.add_define("MULTILINE_BATCH_TEXTURE", None, ShaderDestination::BOTH);
        shader.add_uniform("vec2", "model_textureDimensions", ShaderDestination::BOTH);

        let state_dims = state.clone();
        resources.uniform_map.insert(
            "model_textureDimensions".into(),
            Rc::new(move || {
                let d = state_dims.borrow().texture_dimensions;
                UniformValue::Float(vec![d.x as f32, d.y as f32])
            }),
        );
    }

    let state_len = state.clone();
    resources.uniform_map.insert(
        "model_featuresLength".into(),
        Rc::new(move || UniformValue::Int(vec![state_len.borrow().features_length as i32])),
    );

    let state_tex = state.clone();
    resources.texture_uniform_map.insert(
        "model_batchTexture".into(),
        Rc::new(move || {
            let s = state_tex.borrow();
            s.batch_texture.clone().or_else(|| s.default_texture.clone())
        }),
    );

    let state_step = state;
    resources.uniform_map.insert(
        "model_textureStep".into(),
        Rc::new(move || {
            let s = state_step.borrow().texture_step;
            UniformValue::Float(vec![s.x as f32, s.y as f32, s.z as f32, s.w as f32])
        }),
    );
}
