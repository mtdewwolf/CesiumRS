//! Portable CesiumJS worker calculations and geometry batch dispatch.
//!
//! Worker functions operate on owned Rust data and can run on either host.
//! Native applications can schedule them through `core::native_host::NativeTaskWorker`;
//! browser applications can call them from their Wasm worker entry point.

pub mod combine_geometry;
pub mod create_geometry;
pub mod gaussian_splats;
pub mod geometry;
pub mod geometry_results;
pub mod task_processor_worker;
pub mod terrain;
pub mod vector_tiles;

pub use create_geometry::{create_geometry, GeometryFactory, GeometrySource, GeometrySubTask, GeometryWorker};
pub use geometry::*;
pub use geometry_results::PackedGeometryResults;
pub use terrain::*;
pub use vector_tiles::*;
pub use combine_geometry::*;
pub use gaussian_splats::*;
pub use task_processor_worker::{
    deliver_message_error, deliver_task, PostMessageError, TaskMessage, TaskProcessorReply, WorkerException,
    WorkerResponse, WorkerThrown,
};
