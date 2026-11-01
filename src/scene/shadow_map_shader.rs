//! CesiumJS shadow cast and receive shader derivation.
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

include!("shadow_receive_glsl.rs");

/// One of the shadow map's receiver biases (`_pointBias`, `_terrainBias`,
/// `_primitiveBias`), as far as shader derivation reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReceiveBias {
    pub normal_shading: bool,
    pub normal_shading_smooth: f64,
    pub normal_offset: bool,
}

/// What `createShadowReceiveFragmentShader` and
/// `getShadowReceiveShaderKeyword` read from the shadow map, plus their
/// `castShadows`, `isTerrain` and `hasTerrainNormal` arguments. It stands in
/// for the `ShadowMap` object.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReceiveOptions {
    pub uses_depth_texture: bool,
    pub polygon_offset_supported: bool,
    pub point_light: bool,
    pub spot_light: bool,
    /// `_numberOfCascades`; more than one enables cascade selection.
    pub number_of_cascades: u32,
    pub debug_cascade_colors: bool,
    pub soft_shadows: bool,
    pub cast_shadows: bool,
    pub terrain: bool,
    pub terrain_normal: bool,
    pub point_bias: ReceiveBias,
    pub terrain_bias: ReceiveBias,
    pub primitive_bias: ReceiveBias,
}

impl ReceiveOptions {
    pub fn has_cascades(&self) -> bool {
        self.number_of_cascades > 1
    }

    /// The bias Cesium applies: the point-light bias, else the terrain bias
    /// for terrain, else the primitive bias.
    pub fn bias(&self) -> ReceiveBias {
        if self.point_light {
            self.point_bias
        } else if self.terrain {
            self.terrain_bias
        } else {
            self.primitive_bias
        }
    }
}

/// `getShadowReceiveShaderKeyword` for the same inputs as [`receive_fragment`].
pub fn receive_shader_keyword(o: &ReceiveOptions) -> String {
    receive_keyword([
        o.uses_depth_texture,
        o.polygon_offset_supported,
        o.point_light,
        o.spot_light,
        o.has_cascades(),
        o.debug_cascade_colors,
        o.soft_shadows,
        o.cast_shadows,
        o.terrain,
        o.terrain_normal,
    ])
}

/// Derive lighting, bias, cascade selection and shadow visibility from an
/// existing material shader, preserving its alpha and original main function.
pub fn receive_fragment(fs: &ShaderSource, o: &ReceiveOptions) -> ShaderSource {
    let bias = o.bias();
    let cascades = o.has_cascades();
    let normal = fs.find_normal_varying();
    let has_normal = (!o.terrain && normal.is_some()) || (o.terrain && o.terrain_normal);
    let mut s = fs.clone();
    s.sources = fs.sources.iter().map(|s| ShaderSource::replace_main(s, "czm_shadow_receive_main")).collect();
    if o.point_light {
        s.defines.push("USE_CUBE_MAP_SHADOW".into());
    } else if o.uses_depth_texture {
        s.defines.push("USE_SHADOW_DEPTH_TEXTURE".into());
    }
    if o.soft_shadows && !o.point_light {
        s.defines.push("USE_SOFT_SHADOWS".into());
    }
    if cascades && o.cast_shadows && o.terrain {
        s.defines.push(if has_normal { "ENABLE_VERTEX_LIGHTING" } else { "ENABLE_DAYNIGHT_SHADING" }.into());
    }
    if o.cast_shadows && bias.normal_shading && has_normal {
        s.defines.push("USE_NORMAL_SHADING".into());
        if bias.normal_shading_smooth > 0.0 {
            s.defines.push("USE_NORMAL_SHADING_SMOOTH".into());
        }
    }
    let mut src = String::from(if o.point_light {
        "uniform samplerCube shadowMap_textureCube; \n"
    } else {
        "uniform sampler2D shadowMap_texture; \n"
    });
    src.push_str(UNIFORMS);
    src.push_str("vec4 getPositionEC() \n{ \n");
    if let Some(position) = fs.find_position_varying() {
        src.push_str(&format!("    return vec4({position}, 1.0); \n"));
    } else {
        src.push_str("#ifndef LOG_DEPTH \n    return czm_windowToEyeCoordinates(gl_FragCoord); \n#else \n    return vec4(v_logPositionEC, 1.0); \n#endif \n");
    }
    src.push_str("} \nvec3 getNormalEC() \n{ \n");
    if has_normal {
        src.push_str(&format!("    return normalize({}); \n", normal.unwrap_or("undefined")));
    } else {
        src.push_str("    return vec3(1.0); \n");
    }
    src.push_str("} \nvoid applyNormalOffset(inout vec4 positionEC, vec3 normalEC, float nDotL) \n{ \n");
    if bias.normal_offset && has_normal {
        src.push_str("    float normalOffset = shadowMap_normalOffsetScaleDistanceMaxDistanceAndDarkness.x; \n    float normalOffsetScale = 1.0 - nDotL; \n    vec3 offset = normalOffset * normalOffsetScale * normalEC; \n    positionEC.xyz += offset; \n");
    }
    src.push_str("} \n");
    src.push_str(MAIN);
    if o.terrain {
        src.push_str("    shadowParameters.depthBias *= max(depth * 0.01, 1.0); \n");
    } else if !o.polygon_offset_supported {
        src.push_str("    shadowParameters.depthBias *= mix(1.0, 100.0, depth * 0.0015); \n");
    }
    src.push_str(if o.point_light {
        POINT
    } else if o.spot_light {
        SPOT
    } else if cascades {
        CASCADE
    } else {
        DIRECTIONAL
    });
    if !o.point_light && !o.spot_light && cascades && o.debug_cascade_colors {
        src.push_str("    // Draw cascade colors for debugging \n    out_FragColor *= czm_cascadeColor(weights); \n");
    }
    src.push_str("    out_FragColor.rgb *= visibility; \n} \n");
    s.sources.push(src);
    s
}
