//! `Source/Shaders/Builtin`: `Constants`, `Functions`, `Structs`, and `CzmBuiltins.js`.
//!
//! A new group is two appended lines in this file: `mod <group>;` and
//! `<group>::register(shaders);`. A shader inside a group is one
//! `shaders.push` line in that group. Do not edit another group's line.

mod constants;

use super::ShaderModule;

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    constants::register(shaders);
}
