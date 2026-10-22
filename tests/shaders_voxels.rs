//! Voxels shaders against the CesiumJS modules' default exports.
mod common;

use common::{run, shader_module, EXACT};

#[test]
fn voxels_match_cesium() {
    run("shaders/voxels", "voxels", |_| EXACT, shader_module);
}
