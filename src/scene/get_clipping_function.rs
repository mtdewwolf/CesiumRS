//! CesiumJS `getClippingFunction`: the GLSL that samples a clipping-plane
//! collection's packed texture, plus the union/intersect clip functions.

use super::ClippingPlaneCollection;
use crate::core::{Cartesian2, RuntimeError};
use crate::renderer::{Context, Gl};

/// `getClippingFunction(clippingPlaneCollection, context)`.
pub fn get_clipping_function<G: Gl>(
    clipping_plane_collection: &ClippingPlaneCollection,
    context: &Context<G>,
) -> Result<String, RuntimeError> {
    let union_clipping_regions = clipping_plane_collection.union_clipping_regions;
    let clipping_planes_length = clipping_plane_collection.len();
    let capabilities = context.capabilities()?;
    let using_float_texture = capabilities.floating_point_texture;
    let maximum = context.limits()?.maximum_texture_size;
    let resolution = clipping_plane_collection.texture_resolution(using_float_texture, maximum);
    let [width, height] = resolution;
    let mut functions = if using_float_texture {
        get_clipping_plane_float(width, height)
    } else {
        get_clipping_plane_uint8(width, height)
    };
    functions.push('\n');
    let clip = if union_clipping_regions {
        clipping_function_union(clipping_planes_length)
    } else {
        clipping_function_intersect(clipping_planes_length)
    };
    functions.push_str(&clip);
    Ok(functions)
}

/// The `getClippingFunction` result for explicit texture dimensions, without
/// a renderer context.
pub fn clipping_functions(
    union_clipping_regions: bool,
    planes_length: usize,
    using_float_texture: bool,
    width: u32,
    height: u32,
) -> String {
    let mut functions = if using_float_texture {
        get_clipping_plane_float(width, height)
    } else {
        get_clipping_plane_uint8(width, height)
    };
    functions.push('\n');
    let clip = if union_clipping_regions {
        clipping_function_union(planes_length)
    } else {
        clipping_function_intersect(planes_length)
    };
    functions.push_str(&clip);
    functions
}

fn clipping_function_union(clipping_planes_length: usize) -> String {
    format!(
        "float clip(vec4 fragCoord, sampler2D clippingPlanes, mat4 clippingPlanesMatrix)
{{
    vec4 position = czm_windowToEyeCoordinates(fragCoord);
    vec3 clipNormal = vec3(0.0);
    vec3 clipPosition = vec3(0.0);
    float clipAmount;
    float pixelWidth = czm_metersPerPixel(position);
    bool breakAndDiscard = false;
    for (int i = 0; i < {clipping_planes_length}; ++i)
    {{
        vec4 clippingPlane = getClippingPlane(clippingPlanes, i, clippingPlanesMatrix);
        clipNormal = clippingPlane.xyz;
        clipPosition = -clippingPlane.w * clipNormal;
        float amount = dot(clipNormal, (position.xyz - clipPosition)) / pixelWidth;
        clipAmount = czm_branchFreeTernary(i == 0, amount, min(amount, clipAmount));
        if (amount <= 0.0)
        {{
            breakAndDiscard = true;
            // HLSL compiler bug if we discard here: https://bugs.chromium.org/p/angleproject/issues/detail?id=1945#c6
            break;
         }}
    }}
    if (breakAndDiscard) {{
        discard;
    }}
    return clipAmount;
}}
"
    )
}

fn clipping_function_intersect(clipping_planes_length: usize) -> String {
    format!(
        "float clip(vec4 fragCoord, sampler2D clippingPlanes, mat4 clippingPlanesMatrix)
{{
    bool clipped = true;
    vec4 position = czm_windowToEyeCoordinates(fragCoord);
    vec3 clipNormal = vec3(0.0);
    vec3 clipPosition = vec3(0.0);
    float clipAmount = 0.0;
    float pixelWidth = czm_metersPerPixel(position);
    for (int i = 0; i < {clipping_planes_length}; ++i)
    {{
        vec4 clippingPlane = getClippingPlane(clippingPlanes, i, clippingPlanesMatrix);
        clipNormal = clippingPlane.xyz;
        clipPosition = -clippingPlane.w * clipNormal;
        float amount = dot(clipNormal, (position.xyz - clipPosition)) / pixelWidth;
        clipAmount = max(amount, clipAmount);
        clipped = clipped && (amount <= 0.0);
    }}
    if (clipped)
    {{
        discard;
    }}
    return clipAmount;
 }}
"
    )
}

fn pixel_string(value: f64) -> String {
    let mut text = format!("{value}");
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

fn get_clipping_plane_float(width: u32, height: u32) -> String {
    let pixel_width = pixel_string(1.0 / width as f64);
    let pixel_height = pixel_string(1.0 / height as f64);
    format!(
        "vec4 getClippingPlane(highp sampler2D packedClippingPlanes, int clippingPlaneNumber, mat4 transform)
{{
    int pixY = clippingPlaneNumber / {width};
    int pixX = clippingPlaneNumber - (pixY * {width});
    // Sample from center of pixel
    float u = (float(pixX) + 0.5) * {pixel_width};
    float v = (float(pixY) + 0.5) * {pixel_height};
    vec4 plane = texture(packedClippingPlanes, vec2(u, v));
    return czm_transformPlane(plane, transform);
}}
"
    )
}

fn get_clipping_plane_uint8(width: u32, height: u32) -> String {
    let pixel_width = pixel_string(1.0 / width as f64);
    let pixel_height = pixel_string(1.0 / height as f64);
    format!(
        "vec4 getClippingPlane(highp sampler2D packedClippingPlanes, int clippingPlaneNumber, mat4 transform)
{{
    int clippingPlaneStartIndex = clippingPlaneNumber * 2;
    int pixY = clippingPlaneStartIndex / {width};
    int pixX = clippingPlaneStartIndex - (pixY * {width});
    // Sample from center of pixel
    float u = (float(pixX) + 0.5) * {pixel_width};
    float v = (float(pixY) + 0.5) * {pixel_height};
    vec4 oct32 = texture(packedClippingPlanes, vec2(u, v)) * 255.0;
    vec2 oct = vec2(oct32.x * 256.0 + oct32.y, oct32.z * 256.0 + oct32.w);
    vec4 plane;
    plane.xyz = czm_octDecode(oct, 65535.0);
    plane.w = czm_unpackFloat(texture(packedClippingPlanes, vec2(u + {pixel_width}, v)));
    return czm_transformPlane(plane, transform);
}}
"
    )
}

/// The `Cartesian2` resolution Cesium's `Check`-friendly helper returns.
pub fn texture_resolution(collection: &ClippingPlaneCollection, maximum: u32) -> Cartesian2 {
    let [x, y] = collection.texture_resolution(true, maximum);
    Cartesian2::new(x as f64, y as f64)
}
