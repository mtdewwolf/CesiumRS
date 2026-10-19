//! CesiumJS `TerrainProvider`: a provider contract and cached terrain-grid
//! indices. Native futures replace promises; None requests are retried later.
//! Associated types keep the interface independent of a particular I/O host.

use super::ellipsoid::Ellipsoid;
use super::event::Event;
use super::index_datatype::IndexDatatype;
use super::math::PI;
use super::tile_availability::TileAvailability;
use super::tiling_scheme::TilingScheme;
use super::typed_array::TypedArray;
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

/// Host-independent equivalent of Cesium's abstract provider interface.
/// Implementations supply transport, credits, request tokens and terrain data;
/// the shared error event allows listeners to survive individual requests.
pub trait TerrainProvider {
    type Error;
    type Credit;
    type TerrainData;
    type Request;
    type GeometryFuture<'a>: Future<Output = Result<Self::TerrainData, Self::Error>>
    where
        Self: 'a;
    type AvailabilityFuture<'a>: Future<Output = Result<(), Self::Error>>
    where
        Self: 'a;

    fn error_event(&self) -> Rc<RefCell<Event<Self::Error>>>;
    fn credit(&self) -> Option<&Self::Credit>;
    fn tiling_scheme(&self) -> &dyn TilingScheme;
    fn has_water_mask(&self) -> bool;
    fn has_vertex_normals(&self) -> bool;
    fn availability(&self) -> Option<&TileAvailability>;
    /// None indicates throttling; retry later. Failures arrive through the future.
    fn request_tile_geometry(
        &self,
        x: u32,
        y: u32,
        level: u32,
        request: Option<Self::Request>,
    ) -> Option<Self::GeometryFuture<'_>>;
    fn get_level_maximum_geometric_error(&self, level: u32) -> f64;
    /// None means availability is unknown.
    fn get_tile_data_available(&self, x: u32, y: u32, level: u32) -> Option<bool>;
    /// None means no availability request is necessary or supported.
    fn load_tile_data_availability(&self, x: u32, y: u32, level: u32) -> Option<Self::AvailabilityFuture<'_>>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainEdgeIndices {
    pub west_indices_south_to_north: Vec<u32>,
    pub south_indices_east_to_west: Vec<u32>,
    pub east_indices_north_to_south: Vec<u32>,
    pub north_indices_west_to_east: Vec<u32>,
}

impl TerrainEdgeIndices {
    fn edges(&self) -> [&[u32]; 4] {
        [
            &self.west_indices_south_to_north,
            &self.south_indices_east_to_west,
            &self.east_indices_north_to_south,
            &self.north_indices_west_to_east,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainGridIndices {
    pub indices: Arc<TypedArray>,
    pub edges: TerrainEdgeIndices,
    pub index_count_without_skirts: Option<usize>,
}

type GridCache = HashMap<(usize, usize), Arc<TypedArray>>;
type GridEdgeCache = HashMap<(usize, usize), Arc<TerrainGridIndices>>;
thread_local! {
    static GRID: RefCell<GridCache> = RefCell::new(HashMap::new());
    static EDGES: RefCell<GridEdgeCache> = RefCell::new(HashMap::new());
    static SKIRTS: RefCell<GridEdgeCache> = RefCell::new(HashMap::new());
}

pub const HEIGHTMAP_TERRAIN_QUALITY: f64 = 0.25;

fn grid_counts(width: usize, height: usize) -> Option<(usize, usize)> {
    if width == 0 || height == 0 {
        return None;
    }
    let vertices = width.checked_mul(height)?;
    if vertices as u64 >= 1u64 << 32 {
        return None;
    }
    let indices = (width - 1).checked_mul(height - 1)?.checked_mul(6)?;
    Some((vertices, indices))
}

fn grid_indices(width: usize, height: usize, count: usize) -> Vec<u32> {
    let mut indices = Vec::with_capacity(count);
    for row in 0..height - 1 {
        for column in 0..width - 1 {
            let ul = (row * width + column) as u32;
            let ll = ul + width as u32;
            indices.extend([ul, ll, ul + 1, ul + 1, ll, ll + 1]);
        }
    }
    indices
}

fn edge_indices(width: usize, height: usize) -> TerrainEdgeIndices {
    TerrainEdgeIndices {
        west_indices_south_to_north: (0..height).map(|i| ((height - i - 1) * width) as u32).collect(),
        south_indices_east_to_west: (0..width).map(|i| (width * height - 1 - i) as u32).collect(),
        east_indices_north_to_south: (0..height).map(|i| ((i + 1) * width - 1) as u32).collect(),
        north_indices_west_to_east: (0..width).map(|i| i as u32).collect(),
    }
}

/// Cached immutable index data. Repeated calls in a thread share the same Arc.
pub fn get_regular_grid_indices(width: usize, height: usize) -> Option<Arc<TypedArray>> {
    let (vertices, count) = grid_counts(width, height)?;
    Some(GRID.with(|cache| {
        cache
            .borrow_mut()
            .entry((width, height))
            .or_insert_with(|| {
                Arc::new(IndexDatatype::create_typed_array(vertices, &grid_indices(width, height, count)))
            })
            .clone()
    }))
}

pub fn get_regular_grid_indices_and_edge_indices(width: usize, height: usize) -> Option<Arc<TerrainGridIndices>> {
    grid_counts(width, height)?;
    EDGES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(cached) = cache.get(&(width, height)) {
            return Some(cached.clone());
        }
        let result = Arc::new(TerrainGridIndices {
            indices: get_regular_grid_indices(width, height)?,
            edges: edge_indices(width, height),
            index_count_without_skirts: None,
        });
        cache.insert((width, height), result.clone());
        Some(result)
    })
}

pub fn get_regular_grid_and_skirt_indices_and_edge_indices(
    width: usize,
    height: usize,
) -> Option<Arc<TerrainGridIndices>> {
    let (grid_vertices, grid_count) = grid_counts(width, height)?;
    let vertices = grid_vertices.checked_add(width.checked_add(height)?.checked_mul(2)?)?;
    if vertices as u64 > u32::MAX as u64 {
        return None;
    }
    SKIRTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(cached) = cache.get(&(width, height)) {
            return Some(cached.clone());
        }
        let edges = edge_indices(width, height);
        let mut indices = grid_indices(width, height, grid_count);
        indices.extend(skirt_indices(&edges, grid_vertices as u32, false)?);
        let result = Arc::new(TerrainGridIndices {
            indices: Arc::new(IndexDatatype::create_typed_array(vertices, &indices)),
            edges,
            index_count_without_skirts: Some(grid_count),
        });
        cache.insert((width, height), result.clone());
        Some(result)
    })
}

pub fn get_skirt_vertex_count(edges: &TerrainEdgeIndices) -> Option<usize> {
    edges.edges().iter().try_fold(0usize, |total, edge| total.checked_add(edge.len()))
}

/// Counts assume four nonempty edges, as Cesium's count helpers do.
pub fn get_skirt_index_count(skirt_vertex_count: usize) -> Option<usize> {
    skirt_vertex_count.checked_sub(4)?.checked_mul(6)
}

pub fn get_skirt_index_count_with_filled_corners(skirt_vertex_count: usize) -> Option<usize> {
    get_skirt_index_count(skirt_vertex_count)?.checked_add(12)
}

fn skirt_indices(edges: &TerrainEdgeIndices, vertex_count: u32, caps: bool) -> Option<Vec<u32>> {
    let count = get_skirt_vertex_count(edges)?;
    if vertex_count as u64 + count as u64 > 1u64 << 32 {
        return None;
    }
    let mut out = Vec::new();
    let mut vertex = vertex_count as u64;
    let mut starts = [0u64; 4];
    for (i, edge) in edges.edges().iter().enumerate() {
        starts[i] = vertex;
        for (j, pair) in edge.windows(2).enumerate() {
            let skirt = (vertex + j as u64) as u32;
            out.extend([pair[0], pair[1], skirt, skirt, pair[1], skirt + 1]);
        }
        vertex += edge.len() as u64;
    }
    if caps {
        if edges.edges().iter().any(|e| e.is_empty()) {
            return None;
        }
        let end = |i: usize| (starts[i] + edges.edges()[i].len() as u64 - 1) as u32;
        out.extend([
            edges.west_indices_south_to_north[0],
            starts[0] as u32,
            end(1),
            edges.south_indices_east_to_west[0],
            starts[1] as u32,
            end(2),
            edges.east_indices_north_to_south[0],
            starts[2] as u32,
            end(3),
            edges.north_indices_west_to_east[0],
            starts[3] as u32,
            end(0),
        ]);
    }
    Some(out)
}

fn write_skirts(
    edges: &TerrainEdgeIndices,
    vertex_count: u32,
    indices: &mut TypedArray,
    offset: usize,
    caps: bool,
) -> Option<()> {
    IndexDatatype::from_typed_array(indices)?;
    let values = skirt_indices(edges, vertex_count, caps)?;
    if offset.checked_add(values.len())? > indices.len() {
        return None;
    }
    for (i, value) in values.into_iter().enumerate() {
        indices.set(offset + i, value as f64);
    }
    Some(())
}

/// Invalid lengths/types/overflow leave the destination unchanged. Integer
/// writes retain JavaScript's wrapping behavior for the destination width.
pub fn add_skirt_indices(
    edges: &TerrainEdgeIndices,
    vertex_count: u32,
    indices: &mut TypedArray,
    offset: usize,
) -> Option<()> {
    write_skirts(edges, vertex_count, indices, offset, false)
}

pub fn add_skirt_indices_with_filled_corners(
    edges: &TerrainEdgeIndices,
    vertex_count: u32,
    indices: &mut TypedArray,
    offset: usize,
) -> Option<()> {
    write_skirts(edges, vertex_count, indices, offset, true)
}

/// Quality is explicit instead of Cesium's mutable global property.
pub fn get_estimated_level_zero_geometric_error_for_a_heightmap(
    ellipsoid: &Ellipsoid,
    tile_image_width: f64,
    level_zero_tiles: f64,
    quality: Option<f64>,
) -> f64 {
    (ellipsoid.maximum_radius * 2.0 * PI * quality.unwrap_or(HEIGHTMAP_TERRAIN_QUALITY))
        / (tile_image_width * level_zero_tiles)
}
