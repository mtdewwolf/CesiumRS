//! `Source/Shaders/Materials`. No material shader is registered yet.
//!
//! Each material is one `shaders.push` line in [`register`].

use super::ShaderModule;

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    super::register_empty(shaders);
}
