//! `Source/Shaders/Voxels`. No voxel shader is registered yet.
//!
//! Each voxel shader is one `shaders.push` line in [`register`].

use super::ShaderModule;

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    super::register_empty(shaders);
}
