//! CesiumJS shadow shader derivation. Cast shaders and receive vertex shaders.
//! Receive fragment lighting is not implemented here yet.
use crate::renderer::ShaderSource;
pub fn cast_keyword(point: bool, terrain: bool, depth: bool, opaque: bool) -> String {
    format!("castShadow {point} {terrain} {depth} {opaque}")
}
pub fn receive_keyword(flags: [bool; 10]) -> String {
    format!("receiveShadow {}", flags.map(|v| v.to_string()).join(""))
}
pub fn cast_vertex(vs: &ShaderSource, point: bool, terrain: bool) -> ShaderSource {
    let mut s = vs.clone();
    s.defines.push("SHADOW_MAP".into());
    if terrain {
        s.defines.push("GENERATE_POSITION".into());
    }
    if point && vs.find_position_varying().is_none() {
        s.sources = s.sources.iter().map(|s| ShaderSource::replace_main(s, "czm_shadow_cast_main")).collect();
        s.sources.push("out vec3 v_positionEC; \nvoid main() \n{ \n    czm_shadow_cast_main(); \n    v_positionEC = (czm_inverseProjection * gl_Position).xyz; \n}".into());
    }
    s
}
pub fn receive_vertex(vs: &ShaderSource, terrain: bool, normal: bool) -> ShaderSource {
    let mut s = vs.clone();
    s.defines.push("SHADOW_MAP".into());
    if terrain {
        s.defines.push(if normal { "GENERATE_POSITION_AND_NORMAL" } else { "GENERATE_POSITION" }.into());
    }
    s
}
pub fn cast_fragment(fs: &ShaderSource, point: bool, depth: bool, opaque: bool) -> ShaderSource {
    let mut s = fs.clone();
    s.defines.push("SHADOW_MAP".into());
    s.sources = s.sources.iter().map(|s| ShaderSource::replace_main(s, "czm_shadow_cast_main")).collect();
    let position = fs.find_position_varying();
    let mut source = String::new();
    if point {
        if position.is_none() {
            source.push_str("in vec3 v_positionEC; \n");
        }
        source.push_str("uniform vec4 shadowMap_lightPositionEC; \n");
    }
    source.push_str("void main() \n{ \n");
    if !opaque {
        source.push_str(
            "    czm_shadow_cast_main(); \n    if (out_FragColor.a == 0.0) \n    { \n       discard; \n    } \n",
        );
    }
    if point {
        source.push_str(&format!("    float distance = length({}); \n    if (distance >= shadowMap_lightPositionEC.w) \n    {{ \n        discard; \n    }} \n    distance /= shadowMap_lightPositionEC.w; // radius \n    out_FragColor = czm_packDepth(distance); \n",position.unwrap_or("v_positionEC")));
    } else if depth {
        source.push_str("    out_FragColor = vec4(1.0); \n");
    } else {
        source.push_str("    out_FragColor = czm_packDepth(gl_FragCoord.z); \n");
    }
    source.push_str("} \n");
    s.sources.push(source);
    s
}
