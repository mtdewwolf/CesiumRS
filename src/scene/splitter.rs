//! CesiumJS `Splitter`: the fragment shader wrapper that discards fragments
//! on the hidden side of the scene's split position.

use std::rc::Rc;

use super::SplitDirection;
use crate::renderer::{ShaderSource, UniformMap, UniformValue};

/// Renames `main` to `czm_splitter_main` and appends a `main` that discards
/// by `czm_splitDirection` and `czm_splitPosition`, except in shadow maps.
pub fn modify_fragment_shader(shader: &str) -> String {
    let mut shader = ShaderSource::replace_main(shader, "czm_splitter_main");
    shader.push_str(
        "uniform float czm_splitDirection; \n\
         void main() \n\
         { \n\
         #ifndef SHADOW_MAP\n\
         \x20   if (czm_splitDirection < 0.0 && gl_FragCoord.x > czm_splitPosition) discard; \n\
         \x20   if (czm_splitDirection > 0.0 && gl_FragCoord.x < czm_splitPosition) discard; \n\
         #endif\n\
         \x20   czm_splitter_main(); \n\
         } \n",
    );
    shader
}

/// Cesium's `addUniforms`: a uniform map that adds `czm_splitDirection`,
/// read from `split_direction` at each draw, to `uniform_map`'s uniforms.
pub fn add_uniforms(
    split_direction: impl Fn() -> SplitDirection + 'static,
    uniform_map: Option<UniformMap>,
) -> UniformMap {
    Rc::new(move || {
        let mut uniforms = match &uniform_map {
            Some(map) => map()?,
            None => Vec::new(),
        };
        uniforms.push(("czm_splitDirection".to_string(), UniformValue::Float(vec![split_direction().value() as f32])));
        Ok(uniforms)
    })
}
