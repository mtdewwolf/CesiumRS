//! PostProcessStages shaders against the CesiumJS modules' default exports.
mod common;

use common::{run, shader_module, EXACT};

#[test]
fn post_process_stages_match_cesium() {
    run("shaders/post_process_stages", "post_process_stages", |_| EXACT, shader_module);
}
