//! Model shaders against the CesiumJS modules' default exports.
mod common;

use common::{run, shader_module, EXACT};

#[test]
fn model_match_cesium() {
    run("shaders/model", "model", |_| EXACT, shader_module);
}
