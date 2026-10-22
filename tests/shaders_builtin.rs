//! Builtin shaders and the `CzmBuiltins.js` table against the CesiumJS
//! modules' default exports.
mod common;

use cesium_rs::shaders;
use common::{run, shader_module, Out, EXACT};
use serde_json::{json, Value};

#[test]
fn builtin_constants_match_cesium() {
    run("shaders/builtin", "constants", |_| EXACT, shader_module);
}

#[test]
fn builtin_functions_match_cesium() {
    run("shaders/builtin", "functions", |_| EXACT, shader_module);
}

#[test]
fn builtin_structs_match_cesium() {
    run("shaders/builtin", "structs", |_| EXACT, shader_module);
}

#[test]
fn czm_builtins_table_matches_cesium() {
    run(
        "shaders/builtin",
        "CzmBuiltins",
        |_| EXACT,
        |function, _| {
            if function != "czmBuiltins" {
                return None;
            }
            let table = shaders::builtin::czm_builtins()
                .iter()
                .map(|module| json!({ "name": module.name, "source": module.source }))
                .collect();
            Some(Out::Ok(Value::Array(table)))
        },
    );
}
