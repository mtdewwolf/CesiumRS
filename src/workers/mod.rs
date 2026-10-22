//! Portable CesiumJS worker calculations and geometry batch dispatch.
//!
//! Worker functions operate on owned Rust data and can run on either host.
//! Native applications can schedule them through `core::native_host::NativeTaskWorker`;
//! browser applications can call them from their Wasm worker entry point.

pub mod create_geometry;
pub mod decode_draco;
pub mod gaussian_splats;
pub mod geometry;
pub mod geometry_results;
pub mod terrain;
pub mod vector_tiles;

pub use create_geometry::{create_geometry, GeometryFactory, GeometrySource, GeometrySubTask, GeometryWorker};
pub use decode_draco::decode_draco;
pub use geometry::*;
pub use geometry_results::PackedGeometryResults;
pub use terrain::*;
pub use vector_tiles::*;
pub use gaussian_splats::*;
