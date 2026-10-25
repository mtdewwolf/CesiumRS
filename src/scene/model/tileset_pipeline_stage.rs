//! CesiumJS `Scene/Model/TilesetPipelineStage`: configures 3D Tiles skip-LOD
//! polygon offset and stencil bit render state.

use super::PipelineResources;
use crate::renderer::{ShaderDestination, UniformValue};
use crate::scene::stencil_constants;
use std::rc::Rc;

pub fn process(resources: &mut PipelineResources, has_skip_level_of_detail: bool) {
    if has_skip_level_of_detail {
        resources.shader_builder.add_define("POLYGON_OFFSET", None, ShaderDestination::FRAGMENT);
        resources.uniform_map.insert("u_polygonOffset".into(), Rc::new(|| UniformValue::Float(vec![0.0, 0.0])));
        resources.has_skip_level_of_detail = true;
    }

    resources.render_state_options.stencil_test = stencil_constants::set_cesium_3d_tile_bit();
    resources.render_state_options.stencil_mask = stencil_constants::CESIUM_3D_TILE_MASK as i64;
}
