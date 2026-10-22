//! `czm_pi` against the CesiumJS shader module's default export.
mod common;

use cesium_rs::shaders;
use common::{run, Out, EXACT};
use serde_json::json;

#[test]
fn czm_pi_source_matches_cesium() {
    run(
        "shaders/builtin",
        "pi",
        |_| EXACT,
        |function, args| {
            if function != "shader" {
                return None;
            }
            let id = args.first()?.as_str()?;
            let module = shaders::shader(id)?;
            Some(Out::Ok(json!({
                "name": module.name,
                "source": module.source,
            })))
        },
    );
}
