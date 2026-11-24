//! CesiumJS `Megatexture`: a 3D texture holding packed voxel metadata tiles,
//! with an allocation list of occupied and empty tile slots.

use super::{MetadataComponentType, spatial_node::MegatextureSlots};
use crate::{
    core::{Cartesian3, PixelDatatype, PixelFormat, RuntimeError, TypedArray},
    renderer::{Context, Gl, Sampler, Texture3D, TextureOptions, TextureVolume},
};
use std::{collections::VecDeque, rc::Rc};

type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MegatextureNode {
    index: usize,
    next: Option<usize>,
    previous: Option<usize>,
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

/// `Megatexture.get3DTextureDimension` without a renderer: the texture
/// dimensions that hold `tile_count` tiles, or as many as fit in memory.
///
/// The JavaScript implementation divides by zero and propagates `NaN` for
/// degenerate inputs; this port reports them as errors instead.
pub fn get_3d_texture_dimension(
    dimensions: [u32; 3],
    bytes_per_sample: u32,
    available_bytes: u64,
    tile_count: Option<u64>,
    maximum_size: u32,
) -> Result<[u32; 3]> {
    if dimensions.contains(&0) || bytes_per_sample == 0 {
        return Err(RuntimeError::new("Tile dimensions and bytes per sample must be positive."));
    }
    if dimensions.iter().any(|&dimension| dimension > maximum_size) {
        return Err(RuntimeError::new(
            "The GL context does not support a 3D texture large enough to contain a tile with the given dimensions.",
        ));
    }
    let tile_bytes =
        dimensions.iter().try_fold(u64::from(bytes_per_sample), |v, &d| v.checked_mul(u64::from(d)));
    let max_tiles = tile_bytes.map_or(0, |size| available_bytes / size);
    if max_tiles == 0 {
        return Err(RuntimeError::new(
            "Not enough texture memory available to create a megatexture with the given tile dimensions.",
        ));
    }
    let target = tile_count.unwrap_or(max_tiles).min(max_tiles);
    // Cesium sorts `Object.entries` from largest to smallest; the sort is
    // stable, so equal dimensions keep x, y, z order.
    let mut axes = [0, 1, 2];
    axes.sort_by_key(|&axis| std::cmp::Reverse(dimensions[axis]));
    let mut counts = axes.map(|axis| u64::from(maximum_size / dimensions[axis]));
    for i in 0..3 {
        let volume = counts.iter().product::<u64>();
        if volume < target {
            break;
        }
        let slice = counts[(i + 1) % 3] * counts[(i + 2) % 3];
        if let Some(reduction) = (volume - target).checked_div(slice) {
            counts[i] -= reduction;
        }
    }
    if counts.iter().product::<u64>() > max_tiles {
        counts[2] = max_tiles / (counts[0] * counts[1]);
    }
    let mut result = [0; 3];
    for i in 0..3 {
        result[axes[i]] = counts[i] as u32 * dimensions[axes[i]];
    }
    Ok(result)
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
    destroyed: bool,
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
        let texture_dimension = get_3d_texture_dimension(
            [dimensions.x as u32, dimensions.y as u32, dimensions.z as u32],
            bytes_per_sample as u32,
            available_texture_memory_bytes as u64,
            tile_count.map(|count| count as u64),
            context.limits()?.maximum_3d_texture_size,
        )?;
        let tile_counts = Cartesian3::new(
            f64::from(texture_dimension[0]) / dimensions.x,
            f64::from(texture_dimension[1]) / dimensions.y,
            f64::from(texture_dimension[2]) / dimensions.z,
        );
        let maximum_tile_count = (tile_counts.x * tile_counts.y * tile_counts.z) as usize;
        let texture_memory_byte_length = bytes_per_sample
            * texture_dimension[0] as usize
            * texture_dimension[1] as usize
            * texture_dimension[2] as usize;

        let options = TextureOptions {
            width: texture_dimension[0],
            height: texture_dimension[1],
            pixel_format,
            pixel_datatype: pixel_data_type,
            flip_y: false,
            sampler: Sampler::default(),
            pre_multiply_alpha: false,
            skip_color_space_conversion: false,
            id: None,
        };
        let texture = Rc::new(Texture3D::new(context, options, texture_dimension[2], None)?);

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
            destroyed: false,
        })
    }

    /// Cesium's `Megatexture.get3DTextureDimension` with `Cartesian3` values.
    pub fn get_3d_texture_dimension(
        tile_dimensions: Cartesian3,
        bytes_per_sample: usize,
        available_texture_memory_bytes: usize,
        tile_count: Option<usize>,
        maximum_3d_texture_size: u32,
    ) -> Result<Cartesian3> {
        let result = get_3d_texture_dimension(
            [tile_dimensions.x as u32, tile_dimensions.y as u32, tile_dimensions.z as u32],
            bytes_per_sample as u32,
            available_texture_memory_bytes as u64,
            tile_count.map(|count| count as u64),
            maximum_3d_texture_size,
        )?;
        Ok(Cartesian3::new(f64::from(result[0]), f64::from(result[1]), f64::from(result[2])))
    }

    pub fn nearest_sampling(&self) -> bool {
        self.nearest_sampling
    }

    /// Sets the sampling filter without reallocating the texture.
    pub fn set_nearest_sampling(&mut self, nearest: bool) -> Result<()> {
        self.ensure_alive()?;
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
        self.ensure_alive()?;
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
        self.ensure_alive()?;
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

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// `Megatexture#writeDataToTexture`: copies one tile's metadata into the
    /// atlas at the slot's tile coordinates.
    pub fn write_data_to_texture(&self, index: usize, tile_data: &TypedArray) -> Result<()> {
        self.ensure_alive()?;
        if index >= self.maximum_tile_count {
            return Err(RuntimeError::new("Megatexture index out of bounds"));
        }
        let tiles_per_z = (self.tile_counts.x * self.tile_counts.y) as usize;
        let iz = index / tiles_per_z.max(1);
        let remainder = index - iz * tiles_per_z.max(1);
        let iy = remainder / self.tile_counts.x.max(1.) as usize;
        let ix = remainder - iy * self.tile_counts.x.max(1.) as usize;
        let region = TextureVolume {
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

    fn ensure_alive(&self) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        Ok(())
    }

    /// `Megatexture#destroy`: releases the 3D texture and its slot lists.
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        self.destroyed = true;
        self.texture.destroy()?;
        self.nodes.clear();
        self.occupied_list = None;
        self.empty_list = None;
        self.occupied_count = 0;
        self.texture_memory_byte_length = 0;
        Ok(())
    }
}

impl<G: Gl> MegatextureSlots for Megatexture<G> {
    fn add(&mut self, data: &TypedArray) -> isize {
        Megatexture::add(self, data).map_or(-1, |index| index as isize)
    }
    fn remove(&mut self, index: isize) {
        if index >= 0 {
            let _ = Megatexture::remove(self, index as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_reduces_the_largest_axis_to_fit_the_tile_budget() {
        let full = get_3d_texture_dimension([8, 8, 8], 1, 128 * 1024 * 1024, Some(100), 2048).unwrap();
        assert_eq!(full, [8, 8, 800]);

        assert!(get_3d_texture_dimension([8, 8, 8], 1, 128, None, 2048).is_err());
    }

    #[test]
    fn dimensions_above_the_context_limit_and_out_of_memory_fail() {
        assert!(get_3d_texture_dimension([4096, 8, 8], 1, 128, None, 2048).is_err());
        assert!(get_3d_texture_dimension([8, 8, 8], 1, 8, None, 2048).is_err());
    }
}
