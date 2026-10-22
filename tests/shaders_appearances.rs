//! Appearances shaders against the CesiumJS modules' default exports.
mod common;

use common::{run, shader_module, EXACT};

#[test]
fn appearances_match_cesium() {
    run("shaders/appearances", "appearances", |_| EXACT, shader_module);
}
