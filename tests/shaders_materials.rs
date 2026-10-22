//! Materials shaders against the CesiumJS modules' default exports.
mod common;

use common::{run, shader_module, EXACT};

#[test]
fn materials_match_cesium() {
    run("shaders/materials", "materials", |_| EXACT, shader_module);
}
