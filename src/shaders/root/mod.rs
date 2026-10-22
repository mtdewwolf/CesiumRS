//! Root `Source/Shaders/*.js` modules. None are registered yet.
//!
//! Each root shader is one `shaders.push` line in [`register`]. Its name is
//! the file stem: `PolylineCommon` for `PolylineCommon.js`. Upstream 23.0.0
//! gives these modules no `@alias`.

use super::ShaderModule;

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    super::register_empty(shaders);
}
