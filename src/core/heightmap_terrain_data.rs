//! CesiumJS `HeightmapTerrainData`: raw/mesh interpolation, mesh creation,
//! child availability and descendant upsampling, including LERC input.
//! Native mesh creation/upsampling run synchronously; the TerrainData contract
//! supplies ready futures without requiring a particular asynchronous runtime.

use super::component_datatype::ComponentDatatype;
use super::heightmap_encoding::HeightmapEncoding;
use super::heightmap_processor;
use super::heightmap_tessellator::{HeightmapStructure, HeightmapTessellatorOptions};
use super::math::{js_min, lerp, to_int32};
use super::rectangle::Rectangle;
use super::runtime_error::RuntimeError;
use super::terrain_data::{TerrainData, TerrainMeshRequest, TerrainTile};
use super::terrain_mesh::{TerrainMesh, TerrainMeshOptions};
use super::terrain_provider;
use super::tiling_scheme::TilingScheme;
use super::typed_array::TypedArray;
use std::future::{ready, Ready};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeightmapTerrainStructure {
    pub samples: HeightmapStructure,
    pub lowest_encoded_height: Option<f64>,
    pub highest_encoded_height: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct HeightmapTerrainDataOptions {
    pub buffer: TypedArray,
    pub width: usize,
    pub height: usize,
    pub child_tile_mask: u8,
    pub water_mask: Option<Vec<u8>>,
    pub structure: HeightmapTerrainStructure,
    pub encoding: HeightmapEncoding,
    pub created_by_upsampling: bool,
}

impl HeightmapTerrainDataOptions {
    pub fn new(buffer: TypedArray, width: usize, height: usize) -> Self {
        Self {
            buffer,
            width,
            height,
            child_tile_mask: 15,
            water_mask: None,
            structure: HeightmapTerrainStructure::default(),
            encoding: HeightmapEncoding::None,
            created_by_upsampling: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HeightmapTerrainData {
    buffer: Option<TypedArray>,
    width: usize,
    height: usize,
    child_tile_mask: u8,
    water_mask: Option<Vec<u8>>,
    structure: HeightmapTerrainStructure,
    encoding: HeightmapEncoding,
    created_by_upsampling: bool,
    buffer_type: ComponentDatatype,
    skirt_height: Option<f64>,
    mesh: Option<Arc<TerrainMesh>>,
}

impl HeightmapTerrainData {
    pub fn new(o: HeightmapTerrainDataOptions) -> Option<Self> {
        if o.width < 2
            || o.height < 2
            || o.structure.samples.stride == 0
            || o.structure.samples.elements_per_height == 0
        {
            return None;
        }
        o.width.checked_mul(o.height)?.checked_mul(o.structure.samples.stride)?;
        let buffer_type = if o.encoding == HeightmapEncoding::Lerc {
            ComponentDatatype::Float
        } else {
            o.buffer.component_datatype()
        };
        Some(Self {
            buffer: Some(o.buffer),
            width: o.width,
            height: o.height,
            child_tile_mask: o.child_tile_mask,
            water_mask: o.water_mask,
            structure: o.structure,
            encoding: o.encoding,
            created_by_upsampling: o.created_by_upsampling,
            buffer_type,
            skirt_height: None,
            mesh: None,
        })
    }

    pub fn buffer(&self) -> Option<&TypedArray> {
        self.buffer.as_ref()
    }
    pub fn mesh(&self) -> Option<&TerrainMesh> {
        self.mesh.as_deref()
    }
    pub fn mesh_mut(&mut self) -> Option<&mut TerrainMesh> {
        self.mesh.as_mut().map(Arc::make_mut)
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn structure(&self) -> HeightmapTerrainStructure {
        self.structure
    }
    pub fn skirt_height(&self) -> Option<f64> {
        self.skirt_height
    }

    /// Creates the mesh and releases the source buffer only after success.
    /// LERC dimensions come from the compressed stream, as in the JS worker.
    pub fn create_mesh_sync(&mut self, request: TerrainMeshRequest<'_>) -> Result<Arc<TerrainMesh>, RuntimeError> {
        let scheme = request.tiling_scheme;
        let TerrainTile { x, y, level } = request.tile;
        let rectangle = scheme.tile_xy_to_rectangle(x as f64, y as f64, level);
        let center = scheme.ellipsoid().cartographic_to_cartesian(&rectangle.center());
        let error = terrain_provider::get_estimated_level_zero_geometric_error_for_a_heightmap(
            scheme.ellipsoid(),
            self.width as f64,
            scheme.get_number_of_x_tiles_at_level(0) as f64,
            request.heightmap_quality,
        );
        let skirt = js_min(error / (1i32.wrapping_shl(level) as f64) * 4.0, 1000.0);
        self.skirt_height = Some(skirt);
        let buffer = self.buffer.as_ref().ok_or_else(|| RuntimeError::new("Heightmap buffer was released"))?;
        let mut options = HeightmapTessellatorOptions::new(
            buffer,
            self.width,
            self.height,
            scheme.tile_xy_to_native_rectangle(x as f64, y as f64, level),
            skirt,
        );
        options.rectangle = Some(rectangle);
        options.relative_to_center = Some(center);
        options.ellipsoid = *scheme.ellipsoid();
        // GeographicProjection is cylindrical with a constant latitude scale.
        options.is_geographic = scheme.projection().is_geographic();
        options.include_web_mercator_t = true;
        options.structure = self.structure.samples;
        options.exaggeration = request.exaggeration;
        options.exaggeration_relative_height = request.exaggeration_relative_height;
        let result = heightmap_processor::create_vertices_from_heightmap(&options, self.encoding)?;
        let (width, height) = (result.grid_width, result.grid_height);
        let result = result.terrain;
        let topology = if skirt > 0.0 {
            terrain_provider::get_regular_grid_and_skirt_indices_and_edge_indices(width, height)
        } else {
            terrain_provider::get_regular_grid_indices_and_edge_indices(width, height)
        }
        .ok_or_else(|| RuntimeError::new("Invalid terrain grid"))?;
        let mesh = Arc::new(TerrainMesh::new(TerrainMeshOptions {
            center,
            vertices: result.vertices,
            indices: topology.indices.as_ref().clone(),
            index_count_without_skirts: topology.index_count_without_skirts,
            vertex_count_without_skirts: width * height,
            minimum_height: result.minimum_height,
            maximum_height: result.maximum_height,
            rectangle,
            bounding_sphere_3d: result.bounding_sphere_3d,
            occludee_point_in_scaled_space: result.occludee_point_in_scaled_space,
            vertex_stride: Some(result.encoding.stride),
            oriented_bounding_box: result.oriented_bounding_box,
            encoding: result.encoding,
            edges: topology.edges.clone(),
        }));
        self.buffer = None;
        self.mesh = Some(mesh.clone());
        Ok(mesh)
    }

    pub fn interpolate_height(&self, rectangle: &Rectangle, longitude: f64, latitude: f64) -> Option<f64> {
        if self.mesh.is_none() && self.encoding == HeightmapEncoding::Lerc {
            return None;
        }
        let sample = self.interpolate(rectangle, longitude, latitude, self.mesh.is_some());
        // Cesium's mesh branch returns encoded heights, unlike its raw branch.
        Some(if self.mesh.is_some() {
            sample
        } else {
            sample * self.structure.samples.height_scale + self.structure.samples.height_offset
        })
    }

    fn interpolate(&self, r: &Rectangle, lon: f64, lat: f64, mesh: bool) -> f64 {
        let west = ((lon - r.west) * (self.width - 1) as f64) / (r.east - r.west);
        let south = ((lat - r.south) * (self.height - 1) as f64) / (r.north - r.south);
        let (mut wi, mut si) = (to_int32(west) as i64, to_int32(south) as i64);
        let (mut ei, mut ni) = (wi + 1, si + 1);
        if ei >= self.width as i64 {
            ei = self.width as i64 - 1;
            wi = self.width as i64 - 2;
        }
        if ni >= self.height as i64 {
            ni = self.height as i64 - 1;
            si = self.height as i64 - 2;
        }
        let (dx, dy) = (west - wi as f64, south - si as f64);
        si = self.height as i64 - 1 - si;
        ni = self.height as i64 - 1 - ni;
        let get = |y: i64, x: i64| -> f64 {
            let index = y * self.width as i64 + x;
            if mesh {
                let e = &self.mesh.as_ref().unwrap().encoding;
                let height = usize::try_from(index)
                    .ok()
                    .and_then(|i| e.decode_height(&self.mesh.as_ref().unwrap().vertices, i))
                    .unwrap_or(f64::NAN);
                (height - self.structure.samples.height_offset) / self.structure.samples.height_scale
            } else {
                self.raw_height(index)
            }
        };
        let (sw, se, nw, ne) = (get(si, wi), get(si, ei), get(ni, wi), get(ni, ei));
        if dy < dx {
            sw + dx * (se - sw) + dy * (ne - se)
        } else {
            sw + dx * (ne - nw) + dy * (nw - sw)
        }
    }

    fn raw_height(&self, index: i64) -> f64 {
        let s = self.structure.samples;
        let mut height = 0.0;
        let read = |i: usize| {
            usize::try_from(index)
                .ok()
                .and_then(|index| index.checked_mul(s.stride)?.checked_add(i))
                .and_then(|i| self.buffer.as_ref()?.get(i))
                .unwrap_or(f64::NAN)
        };
        if s.is_big_endian {
            for i in 0..s.elements_per_height {
                height = height * s.element_multiplier + read(i);
            }
        } else {
            for i in (0..s.elements_per_height).rev() {
                height = height * s.element_multiplier + read(i);
            }
        }
        height
    }

    /// Requires a mesh. Native ownership replaces Promise.resolve(data).
    /// Multi-level release-build extrapolation is preserved; callers normally
    /// upsample one level at a time, as required by Cesium's debug builds.
    pub fn upsample_sync(
        &self,
        scheme: &dyn TilingScheme,
        source: TerrainTile,
        destination: TerrainTile,
    ) -> Option<Self> {
        self.mesh.as_ref()?;
        let src = scheme.tile_xy_to_rectangle(source.x as f64, source.y as f64, source.level);
        let dst = scheme.tile_xy_to_rectangle(destination.x as f64, destination.y as f64, destination.level);
        let s = self.structure.samples;
        let mut heights = TypedArray::from_values(
            self.buffer_type,
            &vec![0.0; self.width.checked_mul(self.height)?.checked_mul(s.stride)?],
        );
        let order: Vec<_> = if s.is_big_endian {
            (0..s.elements_per_height).collect()
        } else {
            (0..s.elements_per_height).rev().collect()
        };
        for j in 0..self.height {
            let lat = lerp(dst.north, dst.south, j as f64 / (self.height - 1) as f64);
            for i in 0..self.width {
                let lon = lerp(dst.west, dst.east, i as f64 / (self.width - 1) as f64);
                let mut height = self.interpolate(&src, lon, lat, true);
                if let Some(low) = self.structure.lowest_encoded_height {
                    if height < low {
                        height = low;
                    }
                }
                if let Some(high) = self.structure.highest_encoded_height {
                    if height > high {
                        height = high;
                    }
                }
                let mut divisor = s.element_multiplier.powf((s.elements_per_height - 1) as f64);
                let start = (j * self.width + i) * s.stride;
                for (n, &element) in order.iter().enumerate() {
                    let value = if n + 1 == order.len() { height } else { to_int32(height / divisor) as f64 };
                    heights.set(start + element, value);
                    if n + 1 < order.len() {
                        height -= heights.get(start + element).unwrap() * divisor;
                        divisor /= s.element_multiplier;
                    }
                }
            }
        }
        let mut options = HeightmapTerrainDataOptions::new(heights, self.width, self.height);
        options.structure = self.structure;
        options.child_tile_mask = 0;
        options.created_by_upsampling = true;
        Self::new(options)
    }

    pub fn child_tile_mask(&self) -> u8 {
        self.child_tile_mask
    }
    pub fn water_mask(&self) -> Option<&[u8]> {
        self.water_mask.as_deref()
    }
    pub fn was_created_by_upsampling(&self) -> bool {
        self.created_by_upsampling
    }
    pub fn is_child_available(&self, x: u32, y: u32, child_x: u32, child_y: u32) -> bool {
        let bit = 2 + u8::from(child_x as u64 != x as u64 * 2) - 2 * u8::from(child_y as u64 != y as u64 * 2);
        self.child_tile_mask & (1 << bit) != 0
    }
}

impl TerrainData for HeightmapTerrainData {
    type Credit = ();
    type WaterMask = [u8];
    type MeshFuture<'a> = Ready<Result<Arc<TerrainMesh>, RuntimeError>>;
    type UpsampleFuture<'a> = Ready<Result<Self, RuntimeError>>;
    fn credits(&self) -> Option<&[()]> {
        None
    }
    fn water_mask(&self) -> Option<&[u8]> {
        self.water_mask()
    }
    fn interpolate_height(&self, r: &Rectangle, lon: f64, lat: f64) -> Option<f64> {
        self.interpolate_height(r, lon, lat)
    }
    fn is_child_available(&self, x: u32, y: u32, cx: u32, cy: u32) -> bool {
        self.is_child_available(x, y, cx, cy)
    }
    fn create_mesh<'a>(&'a mut self, request: TerrainMeshRequest<'a>) -> Option<Self::MeshFuture<'a>> {
        Some(ready(self.create_mesh_sync(request)))
    }
    fn upsample<'a>(
        &'a self,
        scheme: &'a dyn TilingScheme,
        source: TerrainTile,
        destination: TerrainTile,
    ) -> Option<Self::UpsampleFuture<'a>> {
        self.upsample_sync(scheme, source, destination).map(|v| ready(Ok(v)))
    }
    fn was_created_by_upsampling(&self) -> bool {
        self.created_by_upsampling
    }
}
