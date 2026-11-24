//! CesiumJS `buildVoxelDrawCommands`: shader variants, clipping-plane accessor
//! functions and the render state shared by the voxel draw commands.
use super::{blending_state, voxel_render_resources::VoxelRenderResources, CullFace, VoxelBoundsCollection};
use crate::{
    core::{Cartesian2, RuntimeError},
    renderer::{
        render_state::{Cull, DepthTest},
        RenderState, ShaderDestination, ShaderSource,
    },
};
type Result<T> = std::result::Result<T, RuntimeError>;

/// The shader programs and render state for one voxel primitive.
pub struct VoxelDrawCommandResources {
    pub shader: (ShaderSource, ShaderSource),
    pub shader_pick: (ShaderSource, ShaderSource),
    pub shader_pick_voxel: (ShaderSource, ShaderSource),
    pub render_state: RenderState,
}

/// Builds all three shader variants. The plane texture resolutions come from
/// the collections exactly as Cesium's `getTextureResolution` functions return
/// them; a primitive without clipping planes or render bounds passes zeros.
pub fn build_voxel_draw_commands(
    resources: &mut VoxelRenderResources,
    clipping_texture_resolution: [u32; 2],
    render_bound_texture_resolution: [u32; 2],
) -> Result<VoxelDrawCommandResources> {
    if resources.clipping_planes_length > 0 {
        let resolution = Cartesian2::new(
            clipping_texture_resolution[0] as f64,
            clipping_texture_resolution[1] as f64,
        );
        add_plane_function(&mut resources.shader_builder, "getClippingPlane", resolution)?;
    }
    if resources.render_bound_planes_length > 0 {
        let resolution = Cartesian2::new(
            render_bound_texture_resolution[0] as f64,
            render_bound_texture_resolution[1] as f64,
        );
        add_plane_function(&mut resources.shader_builder, "getBoundPlane", resolution)?;
    }

    let mut pick_builder = resources.shader_builder.clone();
    pick_builder.add_define("PICKING", None, ShaderDestination::FRAGMENT);
    let mut pick_voxel_builder = resources.shader_builder.clone();
    pick_voxel_builder.add_define("PICKING_VOXEL", None, ShaderDestination::FRAGMENT);

    let shader = resources.shader_builder.sources();
    let shader_pick = pick_builder.sources();
    let shader_pick_voxel = pick_voxel_builder.sources();

    let render_state = RenderState {
        cull: Cull { enabled: true, face: CullFace::Back.value() },
        depth_test: DepthTest { enabled: false, ..DepthTest::default() },
        depth_mask: false,
        blending: blending_state::pre_multiplied_alpha_blend(),
        ..RenderState::default()
    };

    Ok(VoxelDrawCommandResources { shader, shader_pick, shader_pick_voxel, render_state })
}

fn add_plane_function(
    builder: &mut crate::renderer::ShaderBuilder,
    function_id: &str,
    resolution: Cartesian2,
) -> Result<()> {
    let signature = format!("vec4 {function_id}(highp sampler2D packedPlanes, int planeNumber)");
    builder.add_function(function_id, &signature, ShaderDestination::FRAGMENT);
    builder.add_function_lines(function_id, &[get_plane_function_body(resolution)])?;
    Ok(())
}

/// `getPlaneFunctionBody`: a packed-plane texel fetch from a center-of-pixel UV.
pub fn get_plane_function_body(texture_resolution: Cartesian2) -> String {
    let width = texture_resolution.x;
    let height = texture_resolution.y;
    let pixel_width = 1.0 / width;
    let pixel_height = 1.0 / height;
    let pixel_width_string = glsl_float(pixel_width);
    let pixel_height_string = glsl_float(pixel_height);
    format!(
        "int pixY = planeNumber / {width};
    int pixX = planeNumber - (pixY * {width});
    // Sample from center of pixel
    float u = (float(pixX) + 0.5) * {pixel_width_string};
    float v = (float(pixY) + 0.5) * {pixel_height_string};
    return texture(packedPlanes, vec2(u, v));"
    )
}

fn glsl_float(value: f64) -> String {
    let text = value.to_string();
    if text.contains('.') { text } else { format!("{text}.0") }
}

/// The render-bound plane texture resolution, matching
/// `VoxelBoundsCollection.getTextureResolution` when no texture is allocated.
pub fn voxel_bounds_texture_resolution(collection: &VoxelBoundsCollection<impl crate::renderer::Gl>) -> [u32; 2] {
    collection.texture_resolution(2048)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_body_matches_cesium_printing() {
        let body = get_plane_function_body(Cartesian2::new(2048.0, 2.0));
        assert!(body.contains("int pixY = planeNumber / 2048;"));
        assert!(body.contains("float u = (float(pixX) + 0.5) * 0.00048828125;"));
        assert!(body.contains("float v = (float(pixY) + 0.5) * 0.5;"));
    }
}
