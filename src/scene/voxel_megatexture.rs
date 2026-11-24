//! CesiumJS `Megatexture`: a 3D texture holding packed voxel metadata tiles,
//! with an allocation list of occupied and empty tile slots.
use super::MetadataComponentType;
use crate::{
    core::{Cartesian3, PixelDatatype, PixelFormat, RuntimeError, TypedArray},
    renderer::{Context, Gl, Sampler, Texture3D, TextureOptions},
};
use std::{collections::VecDeque, rc::Rc};
type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MegatextureNode {
    index: usize,
    next: Option<usize>,
    previous: Option<usize>,
}

/// The metadata tile layout of one 3D texture.
pub struct Megatexture<G: Gl> {
    pub channel_count: usize,
    pub component_type: MetadataComponentType,
    pub texture_memory_byte_length: usize,
    pub tile_counts: Cartesian3,
    pub voxel_count_per_tile: Cartesian3,
    pub maximum_tile_count: usize,
    pub texture: Rc<Texture3D<G>>,
    nodes: Vec<MegatextureNode>,
    occupied_list: Option<usize>,
    empty_list: Option<usize>,
    pub occupied_count: usize,
    nearest_sampling: bool,
}

/// `getPixelDataType`: only float32/float64 and uint8 have a texture type.
fn pixel_datatype(component_type: MetadataComponentType) -> Result<PixelDatatype> {
    match component_type {
        MetadataComponentType::Float32 | MetadataComponentType::Float64 => Ok(PixelDatatype::Float),
        MetadataComponentType::Uint8 => Ok(PixelDatatype::UnsignedByte),
        _ => Err(RuntimeError::new("Unsupported megatexture component type")),
    }
}

/// `getPixelFormat`: one to four metadata channels map to the matching format.
fn pixel_format(channel_count: usize) -> Result<PixelFormat> {
    match channel_count {
        1 => Ok(PixelFormat::Red),
        2 => Ok(PixelFormat::Rg),
        3 => Ok(PixelFormat::Rgb),
        4 => Ok(PixelFormat::Rgba),
        _ => Err(RuntimeError::new("Unsupported megatexture channel count")),
    }
}

fn get_volume(dimensions: [f64; 3]) -> f64 {
    dimensions[0] * dimensions[1] * dimensions[2]
}

/// `Megatexture.get3DTextureDimension` without a renderer.
pub fn texture_dimension(
    tile_dimensions: Cartesian3,
    bytes_per_sample: usize,
    available_texture_memory_bytes: usize,
    tile_count: Option<usize>,
    maximum_3d_texture_size: u32,
) -> Result<Cartesian3> {
    let maximum = maximum_3d_texture_size as f64;
    if tile_dimensions.x.max(tile_dimensions.y).max(tile_dimensions.z) > maximum {
        return Err(RuntimeError::new(
            "The GL context does not support a 3D texture large enough to contain a tile with the given dimensions.",
        ));
    }
    let tile_size_in_bytes =
        bytes_per_sample as f64 * tile_dimensions.x * tile_dimensions.y * tile_dimensions.z;
    let max_tile_count = (available_texture_memory_bytes as f64 / tile_size_in_bytes).floor();
    if max_tile_count < 1.0 {
        return Err(RuntimeError::new(
            "Not enough texture memory available to create a megatexture with the given tile dimensions.",
        ));
    }
    let tile_count = tile_count.map_or(max_tile_count, |count| (count as f64).min(max_tile_count));

    // Sort axes from largest to smallest. Cesium sorts Object.entries, whose
    // stable sort keeps x, y, z order for equal dimensions.
    let mut sorted = [
        (0usize, tile_dimensions.x),
        (1usize, tile_dimensions.y),
        (2usize, tile_dimensions.z),
    ];
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut tiles_per_dimension = [
        (maximum / sorted[0].1).floor(),
        (maximum / sorted[1].1).floor(),
        (maximum / sorted[2].1).floor(),
    ];
    for i in 0..3 {
        let current_tile_count = get_volume(tiles_per_dimension);
        if current_tile_count < tile_count {
            break;
        }
        let mut slice = Vec::with_capacity(2);
        for (j, value) in tiles_per_dimension.iter().enumerate() {
            if j != i {
                slice.push(*value);
            }
        }
        let tiles_per_slice = slice[0] * slice[1];
        let excess_tiles = current_tile_count - tile_count;
        let slices_to_remove = (excess_tiles / tiles_per_slice).floor();
        tiles_per_dimension[i] -= slices_to_remove;
    }
    if get_volume(tiles_per_dimension) > max_tile_count {
        tiles_per_dimension[2] =
            (max_tile_count / (tiles_per_dimension[0] * tiles_per_dimension[1])).floor();
    }
    let mut result = Cartesian3::ZERO;
    for i in 0..3 {
        let (axis, dimension) = sorted[i];
        let value = tiles_per_dimension[i] * dimension;
        match axis {
            0 => result.x = value,
            1 => result.y = value,
            _ => result.z = value,
        }
    }
    Ok(result)
}

impl<G: Gl> Megatexture<G> {
    /// `new Megatexture(context, dimensions, channelCount, componentType,
    /// availableTextureMemoryBytes = 128 MB, tileCount)`.
    pub fn new(
        context: &Context<G>,
        dimensions: Cartesian3,
        channel_count: usize,
        component_type: MetadataComponentType,
        available_texture_memory_bytes: usize,
        tile_count: Option<usize>,
    ) -> Result<Self> {
        let pixel_data_type = pixel_datatype(component_type)?;
        let pixel_format = pixel_format(channel_count)?;
        let bytes_per_sample = channel_count * component_type.get_size_in_bytes();
        let texture_dimension = Self::get_3d_texture_dimension(
            dimensions,
            bytes_per_sample,
            available_texture_memory_bytes,
            tile_count,
            context.limits()?.maximum_3d_texture_size,
        )?;
        let tile_counts = Cartesian3::new(
            texture_dimension.x / dimensions.x,
            texture_dimension.y / dimensions.y,
            texture_dimension.z / dimensions.z,
        );
        let maximum_tile_count = (tile_counts.x * tile_counts.y * tile_counts.z) as usize;
        let texture_memory_byte_length = bytes_per_sample
            * texture_dimension.x as usize
            * texture_dimension.y as usize
            * texture_dimension.z as usize;

        let options = TextureOptions {
            width: texture_dimension.x as u32,
            height: texture_dimension.y as u32,
            pixel_format,
            pixel_datatype: pixel_data_type,
            flip_y: false,
            sampler: Sampler::default(),
            pre_multiply_alpha: false,
            skip_color_space_conversion: false,
            id: None,
        };
        let texture = Rc::new(Texture3D::new(context, options, texture_dimension.z as u32, None)?);

        let nodes = (0..maximum_tile_count)
            .map(|index| MegatextureNode {
                index,
                next: (index + 1 < maximum_tile_count).then_some(index + 1),
                previous: index.checked_sub(1),
            })
            .collect();

        Ok(Self {
            channel_count,
            component_type,
            texture_memory_byte_length,
            tile_counts,
            voxel_count_per_tile: dimensions,
            maximum_tile_count,
            texture,
            nodes,
            occupied_list: None,
            empty_list: (maximum_tile_count > 0).then_some(0),
            occupied_count: 0,
            nearest_sampling: false,
        })
    }

    /// Cesium's `Megatexture.get3DTextureDimension`.
    pub fn get_3d_texture_dimension(
        tile_dimensions: Cartesian3,
        bytes_per_sample: usize,
        available_texture_memory_bytes: usize,
        tile_count: Option<usize>,
        maximum_3d_texture_size: u32,
    ) -> Result<Cartesian3> {
        texture_dimension(
            tile_dimensions,
            bytes_per_sample,
            available_texture_memory_bytes,
            tile_count,
            maximum_3d_texture_size,
        )
    }
    pub fn nearest_sampling(&self) -> bool {
        self.nearest_sampling
    }

    /// Sets the sampling filter without reallocating the texture.
    pub fn set_nearest_sampling(&mut self, nearest: bool) -> Result<()> {
        if self.nearest_sampling == nearest {
            return Ok(());
        }
        self.texture
            .set_sampler(if nearest { Sampler::NEAREST } else { Sampler::default() })?;
        self.nearest_sampling = nearest;
        Ok(())
    }

    /// `Megatexture#add`: uploads the tile at the head of the empty list.
    pub fn add(&mut self, data: &TypedArray) -> Result<usize> {
        if self.is_full() {
            return Err(RuntimeError::new("Trying to add when there are no empty spots"));
        }
        let index = self.empty_list.unwrap();
        let next_empty = self.nodes[index].next;
        self.empty_list = next_empty;
        if let Some(next) = self.empty_list {
            self.nodes[next].previous = None;
        }
        self.nodes[index].next = self.occupied_list;
        if let Some(occupied) = self.nodes[index].next {
            self.nodes[occupied].previous = Some(index);
        }
        self.nodes[index].previous = None;
        self.occupied_list = Some(index);
        self.write_data_to_texture(index, data)?;
        self.occupied_count += 1;
        Ok(index)
    }

    /// `Megatexture#remove`: returns a slot to the head of the empty list.
    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index >= self.maximum_tile_count {
            return Err(RuntimeError::new("Megatexture index out of bounds"));
        }
        if let Some(previous) = self.nodes[index].previous {
            self.nodes[previous].next = self.nodes[index].next;
        } else {
            self.occupied_list = self.nodes[index].next;
        }
        if let Some(next) = self.nodes[index].next {
            self.nodes[next].previous = self.nodes[index].previous;
        }
        self.nodes[index].next = self.empty_list;
        if let Some(empty) = self.empty_list {
            self.nodes[empty].previous = Some(index);
        }
        self.nodes[index].previous = None;
        self.empty_list = Some(index);
        self.occupied_count -= 1;
        Ok(())
    }

    pub fn is_full(&self) -> bool {
        self.empty_list.is_none()
    }

    pub fn write_data_to_texture(&self, index: usize, tile_data: &TypedArray) -> Result<()> {
        let tiles_per_z = (self.tile_counts.x * self.tile_counts.y) as usize;
        let iz = index / tiles_per_z.max(1);
        let remainder = index - iz * tiles_per_z.max(1);
        let iy = remainder / self.tile_counts.x.max(1.) as usize;
        let ix = remainder - iy * self.tile_counts.x.max(1.) as usize;
        let region = crate::renderer::TextureVolume {
            x: (ix as f64 * self.voxel_count_per_tile.x) as u32,
            y: (iy as f64 * self.voxel_count_per_tile.y) as u32,
            z: (iz as f64 * self.voxel_count_per_tile.z) as u32,
            width: self.voxel_count_per_tile.x as u32,
            height: self.voxel_count_per_tile.y as u32,
            depth: self.voxel_count_per_tile.z as u32,
        };
        self.texture.copy_from(region, &tile_data.to_le_bytes())
    }

    /// Occupied tile indices, most recently added first.
    pub fn occupied_indices(&self) -> VecDeque<usize> {
        let mut result = VecDeque::new();
        let mut current = self.occupied_list;
        while let Some(index) = current {
            result.push_back(index);
            current = self.nodes[index].next;
        }
        result
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.texture.destroy()?;
        self.nodes.clear();
        self.occupied_list = None;
        self.empty_list = None;
        self.occupied_count = 0;
        self.texture_memory_byte_length = 0;
        Ok(())
    }
}

impl<G: Gl> super::spatial_node::MegatextureSlots for Megatexture<G> {
    fn add(&mut self, data: &TypedArray) -> isize {
        Megatexture::add(self, data).map_or(-1, |index| index as isize)
    }
    fn remove(&mut self, index: isize) {
        if index >= 0 {
            let _ = Megatexture::remove(self, index as usize);
        }
    }
}
