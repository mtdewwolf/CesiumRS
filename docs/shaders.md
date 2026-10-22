# Shaders

`src/shaders` stores the GLSL text of each module in `@cesium/engine` 23.0.0
`Source/Shaders`. This skeleton registers every upstream subdirectory and wires
one builtin, `czm_pi`. The rest of the package is still todo.

The renderer does not read this registry. [`renderer::shader_source`](../src/renderer/shader_source.rs)
still assembles programs from the pinned `assets/renderer/glsl-builtins.json`
table. A module here is the exact default-export string of the upstream file.

## Registration

Each upstream subdirectory is one Rust module. It appears in `src/shaders/mod.rs`
as two lines, and those lines are only ever added:

1. `pub mod <directory>;` with the other module declarations.
2. `<directory>::register(&mut shaders);` inside `registry`.

Append both lines. Do not edit, reorder, or delete another directory's line.
Two people can add directories without editing the same block. Lookup is by
shader id and does not depend on registration order.

Inside a directory, each shader is one `shaders.push(&SHADER);` line in that
directory's `register` function. A new group under a directory (`Constants`,
`Functions`, `Structs`) is the same pair of lines in that directory's `mod.rs`,
not in the crate-root shader module. An empty directory calls `register_empty`
so its signature stays `&mut Vec` and the first shader is a `push` line.

| Directory | Upstream | Owner |
| --- | --- | --- |
| `builtin` | `Builtin/` (`Constants`, `Functions`, `Structs`, `CzmBuiltins.js`) | this port |
| `appearances` | `Appearances/` | this port |
| `materials` | `Materials/` | this port |
| `root` | `Shaders/*.js` | the other port |
| `model` | `Model/` | the other port |
| `post_process_stages` | `PostProcessStages/` | the other port |
| `voxels` | `Voxels/` | the other port |

`builtin`, `appearances`, and `materials` are this port. `root`, `model`,
`post_process_stages`, and `voxels` are empty stubs for the other port. Do not
add shader text to the other port's directories.

## Validation

A ported shader matches the default export of the `@cesium/engine` 23.0.0
module, including the trailing newline. `tools/cesium/fixtures` imports that
module and writes the string into `tests/fixtures`. The Rust replay loads the
fixture and compares with `EXACT`, so the source text has to be identical.

`czm_pi` is the example:

- Upstream module: `Source/Shaders/Builtin/Constants/pi.js`.
- Rust source: `src/shaders/builtin/pi.glsl`, included by `shaders::builtin::constants::PI`.
- Tracker id: `Shaders/Builtin/Constants/pi`.
- Fixture: `tools/cesium/fixtures/shaders-builtin.mjs` records
  `{ name: "czm_pi", source }` for function `shader` and argument
  `["Shaders/Builtin/Constants/pi"]`.
- Generate only that module with `TZ=UTC node tools/cesium/fixtures/generate.mjs pi`.
- Replay: `tests/shaders_builtin.rs` calls `shaders::shader(id)` and returns
  `{ name, source }`.

`shaders::shader` returns `None` for an unknown id. The fixture harness treats
an unknown function name as a missing port, which fails the test.
