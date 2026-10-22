//! `Source/Shaders/PostProcessStages`. No stage shader is registered yet.
//!
//! Each stage is one `shaders.push` line in [`register`].

use super::ShaderModule;

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    super::register_empty(shaders);
}
