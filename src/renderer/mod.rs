//! Cesium renderer resources, shader construction and executable commands.
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

pub use draw_command::{ClearCommand, DrawCommand, UniformMap};
pub use render_state::{PassState, RenderState, Viewport};
pub use shader::{
    ShaderCapabilities, ShaderDestination, ShaderFunction, ShaderProgram, ShaderSource, ShaderStruct, UniformValue,
};
pub use texture::{FramebufferCopy, Sampler, Texture, TextureOptions, TextureRegion};

pub mod framebuffer;
pub mod renderbuffer;
pub use framebuffer::{Attachment, Framebuffer, FramebufferOptions, PixelReadback, ReadState};
pub use renderbuffer::{Renderbuffer, RenderbufferOptions};

pub mod render_state_cache;
pub use render_state_cache::{CachedRenderState, RenderStateCache};

pub mod shader_source;
pub use shader_source::{demodernize_shader, PickColorQualifier};

pub mod geometry_upload;
pub mod shader_builder;
pub use geometry_upload::GeometryUploadOptions;
pub use shader_builder::ShaderBuilder;

pub mod context_limits;
pub use context_limits::{ContextCapabilities, ContextLimits};

pub mod shader_cache;
pub mod texture_cache;
pub use shader::ShaderVariable;
pub use shader_cache::{ShaderCache, ShaderProgramOptions};
pub use texture_cache::TextureCache;

pub mod command_metadata;
pub use command_metadata::DrawMetadata;

pub mod uniform_state;
pub use uniform_state::{automatic_uniforms, AutomaticUniform, UniformState};

pub mod shader_precision;

pub mod multisample_framebuffer;
pub use multisample_framebuffer::{MultisampleFramebuffer, MultisampleFramebufferOptions};
pub mod sync;
pub use sync::Sync;
pub mod viewport_quad;
pub use viewport_quad::{ViewportQuad, VIEWPORT_QUAD_VERTEX_SHADER};
pub mod compute;
pub use compute::{ComputeCommand, ComputeEngine, ComputePostExecute, ComputePreExecute};
pub mod framebuffer_manager;
pub use framebuffer_manager::{FramebufferManager, FramebufferManagerOptions};
