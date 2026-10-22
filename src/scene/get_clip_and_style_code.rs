//! CesiumJS `getClipAndStyleCode`: the fragment shader snippet that clips
//! against clipping planes and paints their edges.

/// The GLSL that calls `clip` and colors fragments within the edge width.
pub fn get_clip_and_style_code(
    sampler_uniform_name: &str,
    matrix_uniform_name: &str,
    style_uniform_name: &str,
) -> String {
    format!(
        "    float clipDistance = clip(gl_FragCoord, {sampler_uniform_name}, {matrix_uniform_name}); \n\
         \x20   vec4 clippingPlanesEdgeColor = vec4(1.0); \n\
         \x20   clippingPlanesEdgeColor.rgb = {style_uniform_name}.rgb; \n\
         \x20   float clippingPlanesEdgeWidth = {style_uniform_name}.a; \n\
         \x20   if (clipDistance > 0.0 && clipDistance < clippingPlanesEdgeWidth) \n\
         \x20   {{ \n\
         \x20       out_FragColor = clippingPlanesEdgeColor;\n\
         \x20   }} \n"
    )
}
