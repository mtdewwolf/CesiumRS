//! `Source/Shaders/Appearances`. No appearance shader is registered yet.
//!
//! Each appearance is one `shaders.push` line in [`register`].

use super::ShaderModule;

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    super::register_empty(shaders);
}
