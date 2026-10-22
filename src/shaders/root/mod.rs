//! Root `Source/Shaders/*.js` modules. None are registered yet.
//!
//! Each root shader is one `shaders.push` line in [`register`].

use super::ShaderModule;

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    super::register_empty(shaders);
}
