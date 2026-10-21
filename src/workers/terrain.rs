//! Terrain and packet worker boundaries: CesiumJS
//! `createVerticesFromCesium3DTilesTerrain`, `upsampleVerticesFromCesium3DTilesTerrain`,
//! `createVerticesFromGoogleEarthEnterpriseBuffer` and `decodeGoogleEarthEnterprisePacket`.
//!
//! Results own their buffers, replacing the transferred `ArrayBuffer`s. Index and
//! edge buffers keep the width the receiving terrain data reads them with.

use crate::core::cesium_3d_tiles_terrain_geometry_processor::{TilesTerrainMeshOptions, TilesTerrainUpsampleOptions};
use crate::core::terrain_provider::TerrainEdgeIndices;
use crate::core::*;
use std::collections::BTreeMap;

/// Cesium's `TerrainMeshProxy`: the transferable parts of a created terrain mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainMeshProxy {
    pub vertices: Vec<f32>,
    /// `Uint16Array` when the mesh has at most 65535 vertices with skirts, else `Uint32Array`.
    pub indices: TypedArray,
    pub vertex_count_without_skirts: usize,
    pub index_count_without_skirts: usize,
    pub encoding: TerrainEncoding,
    /// Stored with the same width as `indices`.
    pub edges: TerrainEdgeIndices,
}

impl TerrainMeshProxy {
    fn from_mesh(mesh: &TerrainMesh) -> Self {
        Self {
            vertices: mesh.vertices.clone(),
            indices: mesh.indices.clone(),
            vertex_count_without_skirts: mesh.vertex_count_without_skirts,
            index_count_without_skirts: mesh.index_count_without_skirts.unwrap_or(mesh.indices.len()),
            encoding: mesh.encoding.clone(),
            edges: mesh.edges.clone(),
        }
    }
    /// Bytes per index, as the receiver sizes its index and edge views.
    pub fn index_bytes_per_element(&self) -> usize {
        if self.vertices.len() / self.encoding.stride <= 65535 {
            2
        } else {
            4
        }
    }
}

/// The upsampling worker's proxy, which adds the child's bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct UpsampledTerrainMeshProxy {
    pub mesh: TerrainMeshProxy,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub bounding_sphere: BoundingSphere,
    pub oriented_bounding_box: Option<OrientedBoundingBox>,
    /// Cesium copies `mesh.horizonOcclusionPoint`, which `TerrainMesh` never defines,
    /// so the upsampled proxy never carries one.
    pub horizon_occlusion_point: Option<Cartesian3>,
}

pub fn create_vertices_from_cesium_3d_tiles_terrain(
    options: TilesTerrainMeshOptions<'_>,
) -> Result<TerrainMeshProxy, RuntimeError> {
    Ok(TerrainMeshProxy::from_mesh(&Cesium3DTilesTerrainGeometryProcessor::create_mesh(options)?))
}

pub fn upsample_vertices_from_cesium_3d_tiles_terrain(
    options: TilesTerrainUpsampleOptions<'_>,
) -> Result<UpsampledTerrainMeshProxy, RuntimeError> {
    let mesh = Cesium3DTilesTerrainGeometryProcessor::upsample(options)?;
    Ok(UpsampledTerrainMeshProxy {
        mesh: TerrainMeshProxy::from_mesh(&mesh),
        minimum_height: mesh.minimum_height,
        maximum_height: mesh.maximum_height,
        bounding_sphere: mesh.bounding_sphere_3d,
        oriented_bounding_box: mesh.oriented_bounding_box,
        horizon_occlusion_point: None,
    })
}

pub use crate::core::google_earth_enterprise_terrain_processor::{
    create_vertices_from_google_earth_enterprise_buffer, GoogleEarthEnterpriseBufferRequest,
    GoogleEarthEnterpriseBufferResult,
};

/// The packet kinds `decodeGoogleEarthEnterprisePacket` accepts by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoogleEarthEnterprisePacketType {
    Metadata,
    Terrain,
    DbRoot,
}

impl GoogleEarthEnterprisePacketType {
    /// Cesium's `Types.fromString`; unknown names have no packet type.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Metadata" => Some(Self::Metadata),
            "Terrain" => Some(Self::Terrain),
            "DbRoot" => Some(Self::DbRoot),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GoogleEarthEnterprisePacket {
    /// Tile information by quad key; `None` marks a known missing tile.
    Metadata(BTreeMap<String, Option<GoogleEarthEnterpriseTileInformation>>),
    /// The five even-level meshes, each with its four odd-level sub-meshes.
    /// Water surface meshes after them are dropped.
    Terrain(Vec<Vec<u8>>),
    DbRoot(Vec<u8>),
}

/// Decrypts and inflates a packet, then decodes it by type. An unknown packet type
/// still decrypts and inflates, and then has no result, as in Cesium.
pub fn decode_google_earth_enterprise_packet(
    packet_type: Option<GoogleEarthEnterprisePacketType>,
    buffer: &[u8],
    key: &[u8],
    quad_key: &str,
) -> Result<Option<GoogleEarthEnterprisePacket>, RuntimeError> {
    let decoded = gee_packet::inflate_packet(buffer, key)?;
    Ok(match packet_type {
        Some(GoogleEarthEnterprisePacketType::Metadata) => {
            Some(GoogleEarthEnterprisePacket::Metadata(gee_packet::metadata_packet(&decoded, quad_key)?))
        }
        Some(GoogleEarthEnterprisePacketType::Terrain) => {
            Some(GoogleEarthEnterprisePacket::Terrain(gee_packet::terrain_meshes(&decoded)?))
        }
        Some(GoogleEarthEnterprisePacketType::DbRoot) => Some(GoogleEarthEnterprisePacket::DbRoot(decoded)),
        None => None,
    })
}
