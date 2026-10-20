//! Cesium renderer foundations with an explicit WebGL2 host boundary.
//!
//! Resources belong to a context, use shared Rust ownership, and delete their
//! GPU objects on the last drop. Explicit `destroy` invalidates every alias.
//! The browser adapter executes GL calls only; rendering decisions live in Rust.
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser;
pub mod buffer;
pub mod context;
pub mod draw_command;
pub mod enums;
pub mod render_state;
pub mod shader;
pub mod texture;
pub mod vertex_array;

pub use crate::core::PixelDatatype;
pub use buffer::{Buffer, BufferData, BufferKind};
pub use context::{Context, Gl, GlValue};
pub use enums::*;
pub use vertex_array::{AttributeSource, VertexArray, VertexAttribute};

pub use draw_command::{ClearCommand, DrawCommand};
pub use render_state::{PassState, RenderState, Viewport};
pub use shader::{
    ShaderCapabilities, ShaderDestination, ShaderFunction, ShaderProgram, ShaderSource, ShaderStruct, UniformValue,
};
pub use texture::{Sampler, Texture, TextureOptions, TextureRegion};
