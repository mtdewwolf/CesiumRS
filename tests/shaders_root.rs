//! Root `Source/Shaders/*.js` shaders against the CesiumJS modules' default exports.
mod common;

use common::{run, shader_module, EXACT};

#[test]
fn root_match_cesium() {
    run("shaders/root", "root", |_| EXACT, shader_module);
}
