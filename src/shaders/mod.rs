//! GLSL module text from CesiumJS `Source/Shaders`.
//!
//! This is the source of each shader module. [`crate::renderer::shader_source`]
//! still assembles programs from the pinned builtin table. A ported module
//! here is the exact default-export string of the upstream file.
//!
//! # Registration
//!
//! Each upstream subdirectory is one Rust module. It appears in this file as
//! two lines:
//!
//! 1. `pub mod <directory>;` with the other module declarations.
//! 2. `<directory>::register(&mut shaders);` inside [`registry`].
//!
//! Inside a directory, each shader is one `shaders.push(&SHADER);` line in
//! that directory's `register` function. A new group under a directory
//! (`Constants`, `Functions`, `Structs`) is the same pair of lines in that
//! directory's `mod.rs`, not in this file.
//!
//! | Directory | Upstream | Modules |
//! | --- | --- | ---: |
//! | `builtin` | `Builtin/` (`Constants`, `Functions`, `Structs`, `CzmBuiltins.js`) | 141 |
//! | `appearances` | `Appearances/` | 14 |
//! | `materials` | `Materials/` | 19 |
//! | `root` | `Shaders/*.js` | 55 |
//! | `model` | `Model/` | 36 |
//! | `post_process_stages` | `PostProcessStages/` | 26 |
//! | `voxels` | `Voxels/` | 16 |
//!
//! Every directory registers every upstream module: all 307 in
//! `@cesium/engine` 23.0.0. See `docs/shaders.md`.
//!
//! A builtin's [`ShaderModule::name`] is its `CzmBuiltins.js` key (`czm_pi`),
//! which is also its `@alias` where the file declares one. Every other module
//! is named by its file stem. None of them declare an `@alias`, and call sites
//! import that stem (`import PolylineCommon from "../Shaders/PolylineCommon.js"`).

use std::sync::OnceLock;

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
    /// Builtin `CzmBuiltins.js` key, such as `czm_pi`. Other modules have no
    /// `@alias`; their name is the file stem (`PolylineCommon` for
    /// `PolylineCommon.js`). Confirmed for all 55 root modules in
    /// `@cesium/engine` 23.0.0: each default export is the GLSL string alone.
    pub name: &'static str,
    /// Exact default export of the upstream module, trailing newline included.
    pub source: &'static str,
}

/// Every shader registered by the directory modules, in append order.
///
/// The list is built once. Lookup does not depend on this order. Do not sort
/// or rewrite the calls.
pub fn registry() -> &'static [&'static ShaderModule] {
    static REGISTRY: OnceLock<Vec<&'static ShaderModule>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let mut shaders = Vec::new();
            builtin::register(&mut shaders);
            appearances::register(&mut shaders);
            materials::register(&mut shaders);
            root::register(&mut shaders);
            model::register(&mut shaders);
            post_process_stages::register(&mut shaders);
            voxels::register(&mut shaders);
            shaders
        })
        .as_slice()
}

/// The shader with this tracker id.
pub fn shader(id: &str) -> Option<&'static ShaderModule> {
    registry().iter().copied().find(|module| module.id == id)
}

#[cfg(test)]
mod tests {
    use super::registry;
    use std::collections::BTreeSet;
    use std::path::Path;

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

    #[test]
    fn registered_ids_match_ported_status() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/cesium/status.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        let status: serde_json::Value = serde_json::from_str(&text).expect("status.json");
        let entries = status.as_object().expect("status object");
        let shaders = registry();
        let mut ids: BTreeSet<&str> = shaders.iter().map(|module| module.id).collect();
        ids.insert(super::builtin::CZM_BUILTINS_ID);
        for module in shaders {
            assert!(entries.contains_key(module.id), "registered shader missing from status.json: {}", module.id);
        }
        for (id, entry) in entries {
            let ported = entry.get("status").and_then(|value| value.as_str()) == Some("ported");
            if id.starts_with("Shaders/") && ported {
                assert!(ids.contains(id.as_str()), "ported shader is not registered: {id}");
            }
        }
    }
}
