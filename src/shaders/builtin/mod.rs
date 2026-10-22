//! `Source/Shaders/Builtin`: `Constants`, `Functions`, `Structs`, and `CzmBuiltins.js`.
//!
//! A new group is two appended lines in this file: `mod <group>;` and
//! `<group>::register(shaders);`. A shader inside a group is one
//! `shaders.push` line in that group. Do not edit another group's line.

pub mod constants;
pub mod functions;
pub mod structs;

use std::sync::OnceLock;

use super::ShaderModule;

/// Tracker id of `CzmBuiltins.js`. It is a table, not a GLSL module, so it is
/// not in [`super::registry`]; [`czm_builtins`] is its port.
pub const CZM_BUILTINS_ID: &str = "Shaders/Builtin/CzmBuiltins";

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    constants::register(shaders);
    structs::register(shaders);
    functions::register(shaders);
}

/// The default export of `CzmBuiltins.js`: every builtin, keyed by its
/// [`ShaderModule::name`], in the upstream key order.
///
/// That order is constants, then structs, then functions, each sorted by file
/// name, which is the order [`register`] pushes them in.
pub fn czm_builtins() -> &'static [&'static ShaderModule] {
    static BUILTINS: OnceLock<Vec<&'static ShaderModule>> = OnceLock::new();
    BUILTINS
        .get_or_init(|| {
            let mut shaders = Vec::new();
            register(&mut shaders);
            shaders
        })
        .as_slice()
}
