//! GLSL module text from CesiumJS `Source/Shaders`.
//!
//! This is the source of each shader module. [`crate::renderer::shader_source`]
//! still assembles programs from the pinned builtin table. A ported module
//! here is the exact default-export string of the upstream file.
//!
//! # Registration
//!
//! Each upstream subdirectory is one Rust module. It appears in this file as
//! two lines, and those lines are only ever added:
//!
//! 1. `pub mod <directory>;` with the other module declarations.
//! 2. `<directory>::register(&mut shaders);` inside [`registry`].
//!
//! Append both lines. Do not edit, reorder, or delete another directory's
//! line. The two directories owned by different people therefore never share
//! an editable block.
//!
//! Inside a directory, each shader is one `shaders.push(&SHADER);` line in
//! that directory's `register` function. A new group under a directory
//! (`Constants`, `Functions`, `Structs`) is the same pair of lines in that
//! directory's `mod.rs`, not in this file.
//!
//! | Directory | Upstream | Owner |
//! | --- | --- | --- |
//! | `builtin` | `Builtin/` (`Constants`, `Functions`, `Structs`, `CzmBuiltins.js`) | this port |
//! | `appearances` | `Appearances/` | this port |
//! | `materials` | `Materials/` | this port |
//! | `root` | `Shaders/*.js` | the other port |
//! | `model` | `Model/` | the other port |
//! | `post_process_stages` | `PostProcessStages/` | the other port |
//! | `voxels` | `Voxels/` | the other port |
//!
//! `czm_pi` is the one wired example. The other directories register no
//! shaders yet. See `docs/shaders.md`.

pub mod appearances;
pub mod builtin;
pub mod materials;
pub mod model;
pub mod post_process_stages;
pub mod root;
pub mod voxels;

/// One upstream shader module.
pub struct ShaderModule {
    /// Tracker id, such as `Shaders/Builtin/Constants/pi`.
    pub id: &'static str,
    /// Builtin name, such as `czm_pi`. Root shaders that are not builtins use
    /// the file stem.
    pub name: &'static str,
    /// Exact default export of the upstream module, trailing newline included.
    pub source: &'static str,
}

/// Every shader registered by the directory modules, in append order.
///
/// Lookup does not depend on this order. Do not sort or rewrite the calls.
pub fn registry() -> Vec<&'static ShaderModule> {
    let mut shaders = Vec::new();
    builtin::register(&mut shaders);
    appearances::register(&mut shaders);
    materials::register(&mut shaders);
    root::register(&mut shaders);
    model::register(&mut shaders);
    post_process_stages::register(&mut shaders);
    voxels::register(&mut shaders);
    shaders
}

/// The shader with this tracker id.
pub fn shader(id: &str) -> Option<&'static ShaderModule> {
    registry().into_iter().find(|module| module.id == id)
}

/// Keeps an empty directory on the same `&mut` signature as a directory that
/// has shaders, so the first real shader is a `push` line and not a signature
/// edit.
#[allow(clippy::needless_pass_by_ref_mut)]
pub(crate) fn register_empty(shaders: &mut Vec<&'static ShaderModule>) {
    let _ = shaders;
}

#[cfg(test)]
mod tests {
    use super::registry;

    #[test]
    fn shader_ids_are_unique() {
        let shaders = registry();
        let mut ids: Vec<_> = shaders.iter().map(|module| module.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert!(ids.contains(&"Shaders/Builtin/Constants/pi"));
    }
}
