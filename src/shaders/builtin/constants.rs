//! `Source/Shaders/Builtin/Constants`.
//!
//! Each constant is one `shaders.push` line in [`register`].

use super::super::ShaderModule;

/// `czm_pi`, the exact text of `Constants/pi.js`.
pub static PI: ShaderModule =
    ShaderModule { id: "Shaders/Builtin/Constants/pi", name: "czm_pi", source: include_str!("pi.glsl") };

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&PI);
}
