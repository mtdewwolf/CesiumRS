//! The `NGA_gpm_local` extension modules under `Scene/Model/Extensions/Gpm`.

pub mod anchor_point_direct;
pub mod anchor_point_indirect;
pub mod correlation_group;
pub mod gltf_gpm_loader;
pub mod gltf_gpm_local;
pub mod mesh_primitive_gpm_local;
pub mod ppe_metadata;
pub mod ppe_source;
pub mod ppe_texture;
pub mod spdcf;
pub mod storage_type;

pub use anchor_point_direct::AnchorPointDirect;
pub use anchor_point_indirect::AnchorPointIndirect;
pub use correlation_group::CorrelationGroup;
pub use gltf_gpm_local::GltfGpmLocal;
pub use mesh_primitive_gpm_local::MeshPrimitiveGpmLocal;
pub use ppe_metadata::PpeMetadata;
pub use ppe_source::PpeSource;
pub use ppe_texture::PpeTexture;
pub use spdcf::Spdcf;
pub use storage_type::StorageType;
