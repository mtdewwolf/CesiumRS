//! CesiumJS `TerrainEncoding`: interleaved terrain vertices with optional
//! 12-bit compression, normal fields, projection data and exaggeration.
//!
//! Native f32 slices replace JavaScript buffers. Invalid buffer sizes,
//! missing inputs and unavailable optional fields return None. Writes and
//! layout conversions validate before modifying the caller's buffers.

use super::attribute_compression::AttributeCompression as Compression;
use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::math::{clamp, js_max};
use super::matrix4::Matrix4;
use super::terrain_quantization::TerrainQuantization;
use super::vertical_exaggeration;

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainEncodingOptions {
    pub center: Option<Cartesian3>,
    pub axis_aligned_bounding_box: Option<AxisAlignedBoundingBox>,
    pub minimum_height: Option<f64>,
    pub maximum_height: Option<f64>,
    pub from_enu: Option<Matrix4>,
    pub has_vertex_normals: bool,
    pub has_web_mercator_t: bool,
    pub has_geodetic_surface_normals: bool,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
}

impl Default for TerrainEncodingOptions {
    fn default() -> Self {
        Self {
            center: None,
            axis_aligned_bounding_box: None,
            minimum_height: None,
            maximum_height: None,
            from_enu: None,
            has_vertex_normals: false,
            has_web_mercator_t: false,
            has_geodetic_surface_normals: false,
            exaggeration: 1.0,
            exaggeration_relative_height: 0.0,
        }
    }
}

impl TerrainEncodingOptions {
    pub fn new(center: Cartesian3) -> Self {
        Self { center: Some(center), ..Self::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainVertex {
    pub position: Cartesian3,
    pub texture_coordinates: Cartesian2,
    pub height: f64,
    /// Oct-encoded unsigned 8-bit components, represented as in Cesium.
    pub normal_to_pack: Option<Cartesian2>,
    pub web_mercator_t: Option<f64>,
    pub geodetic_surface_normal: Option<Cartesian3>,
}

impl TerrainVertex {
    pub fn new(position: Cartesian3, texture_coordinates: Cartesian2, height: f64) -> Self {
        Self {
            position,
            texture_coordinates,
            height,
            normal_to_pack: None,
            web_mercator_t: None,
            geodetic_surface_normal: None,
        }
    }
}

/// Vertex layout; the caller supplies the buffer when binding attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainAttribute {
    pub index: u32,
    pub component_datatype: ComponentDatatype,
    pub components_per_attribute: u8,
    pub offset_in_bytes: usize,
    pub stride_in_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainEncoding {
    pub quantization: TerrainQuantization,
    pub minimum_height: Option<f64>,
    pub maximum_height: Option<f64>,
    pub center: Option<Cartesian3>,
    pub to_scaled_enu: Option<Matrix4>,
    pub from_scaled_enu: Option<Matrix4>,
    pub matrix: Option<Matrix4>,
    pub has_vertex_normals: bool,
    pub has_web_mercator_t: bool,
    pub has_geodetic_surface_normals: bool,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
    pub stride: usize,
    offset_vertex_normal: usize,
    offset_geodetic_surface_normal: usize,
}

impl TerrainEncoding {
    /// A complete box/height/frame set computes quantization transforms;
    /// otherwise the encoding is unquantized, as in Cesium's constructor.
    pub fn new(options: TerrainEncodingOptions) -> Option<Self> {
        let mut quantization = TerrainQuantization::None;
        let mut to_scaled_enu = None;
        let mut from_scaled_enu = options.from_enu;
        let mut matrix = None;
        if let (Some(bounds), Some(min_height), Some(max_height), Some(from)) =
            (options.axis_aligned_bounding_box, options.minimum_height, options.maximum_height, options.from_enu)
        {
            let dimensions = bounds.maximum.subtract(bounds.minimum);
            if js_max(dimensions.maximum_component(), max_height - min_height) < 4095.0 {
                quantization = TerrainQuantization::Bits12;
            }
            let st = Matrix4::from_scale(dimensions).set_translation(bounds.minimum);
            let inv_st =
                Matrix4::from_scale(Cartesian3::new(1.0 / dimensions.x, 1.0 / dimensions.y, 1.0 / dimensions.z))
                    .multiply_by_translation(bounds.minimum.negate());
            let rtc = from.get_translation().subtract(options.center?);
            matrix = Some(from.set_translation(rtc).multiply(&st));
            to_scaled_enu = Some(inv_st.multiply(&from.inverse_transformation()));
            from_scaled_enu = Some(from.multiply(&st));
        }
        let mut result = Self {
            quantization,
            minimum_height: options.minimum_height,
            maximum_height: options.maximum_height,
            center: options.center,
            to_scaled_enu,
            from_scaled_enu,
            matrix,
            has_vertex_normals: options.has_vertex_normals,
            has_web_mercator_t: options.has_web_mercator_t,
            has_geodetic_surface_normals: options.has_geodetic_surface_normals,
            exaggeration: options.exaggeration,
            exaggeration_relative_height: options.exaggeration_relative_height,
            stride: 0,
            offset_vertex_normal: 0,
            offset_geodetic_surface_normal: 0,
        };
        result.calculate_stride_and_offsets();
        Some(result)
    }

    /// Call after manually changing layout flags. Conversion helpers update
    /// the layout themselves.
    pub fn calculate_stride_and_offsets(&mut self) {
        let mut stride = if self.quantization == TerrainQuantization::Bits12 { 3 } else { 6 };
        if self.has_web_mercator_t {
            stride += 1;
        }
        if self.has_vertex_normals {
            self.offset_vertex_normal = stride;
            stride += 1;
        }
        if self.has_geodetic_surface_normals {
            self.offset_geodetic_surface_normal = stride;
            stride += 3;
        }
        self.stride = stride;
    }

    pub fn encode(&self, buffer: &mut [f32], index: usize, vertex: &TerrainVertex) -> Option<usize> {
        let end = index.checked_add(self.stride)?;
        buffer.get(index..end)?;
        // The largest layout is eleven floats; keep this hot path off the heap.
        let mut out = [0.0; 11];
        let mut used = 0;
        let mut append = |values: &[f32]| {
            out[used..used + values.len()].copy_from_slice(values);
            used += values.len();
        };
        let uv = vertex.texture_coordinates;
        if self.quantization == TerrainQuantization::Bits12 {
            let p = self.to_scaled_enu?.multiply_by_point(vertex.position);
            let minimum = self.minimum_height?;
            let h = clamp((vertex.height - minimum) / (self.maximum_height? - minimum), 0.0, 1.0);
            append(&[
                Compression::compress_texture_coordinates(Cartesian2::new(clamp(p.x, 0.0, 1.0), clamp(p.y, 0.0, 1.0)))
                    as f32,
                Compression::compress_texture_coordinates(Cartesian2::new(clamp(p.z, 0.0, 1.0), h)) as f32,
                Compression::compress_texture_coordinates(uv) as f32,
            ]);
            if self.has_web_mercator_t {
                append(&[
                    Compression::compress_texture_coordinates(Cartesian2::new(vertex.web_mercator_t?, 0.0)) as f32
                ]);
            }
        } else {
            let p = vertex.position.subtract(self.center?);
            append(&[p.x as f32, p.y as f32, p.z as f32, vertex.height as f32, uv.x as f32, uv.y as f32]);
            if self.has_web_mercator_t {
                append(&[vertex.web_mercator_t? as f32]);
            }
        }
        if self.has_vertex_normals {
            append(&[Compression::oct_pack_float(vertex.normal_to_pack?) as f32]);
        }
        if self.has_geodetic_surface_normals {
            let n = vertex.geodetic_surface_normal?;
            append(&[n.x as f32, n.y as f32, n.z as f32]);
        }
        if used != self.stride {
            return None;
        }
        buffer[index..end].copy_from_slice(&out[..used]);
        Some(end)
    }

    fn vertex<'a>(&self, buffer: &'a [f32], index: usize) -> Option<&'a [f32]> {
        let start = index.checked_mul(self.stride)?;
        buffer.get(start..start.checked_add(self.stride)?)
    }

    pub fn decode_position(&self, buffer: &[f32], index: usize) -> Option<Cartesian3> {
        let v = self.vertex(buffer, index)?;
        if self.quantization == TerrainQuantization::Bits12 {
            let xy = Compression::decompress_texture_coordinates(v[0] as f64);
            let zh = Compression::decompress_texture_coordinates(v[1] as f64);
            Some(self.from_scaled_enu?.multiply_by_point(Cartesian3::new(xy.x, xy.y, zh.x)))
        } else {
            Some(Cartesian3::new(v[0] as f64, v[1] as f64, v[2] as f64).add(self.center?))
        }
    }

    pub fn decode_texture_coordinates(&self, buffer: &[f32], index: usize) -> Option<Cartesian2> {
        let v = self.vertex(buffer, index)?;
        Some(if self.quantization == TerrainQuantization::Bits12 {
            Compression::decompress_texture_coordinates(v[2] as f64)
        } else {
            Cartesian2::new(v[4] as f64, v[5] as f64)
        })
    }

    pub fn decode_height(&self, buffer: &[f32], index: usize) -> Option<f64> {
        let v = self.vertex(buffer, index)?;
        Some(if self.quantization == TerrainQuantization::Bits12 {
            let zh = Compression::decompress_texture_coordinates(v[1] as f64);
            zh.y * (self.maximum_height? - self.minimum_height?) + self.minimum_height?
        } else {
            v[3] as f64
        })
    }

    pub fn decode_web_mercator_t(&self, buffer: &[f32], index: usize) -> Option<f64> {
        if !self.has_web_mercator_t {
            return None;
        }
        let v = self.vertex(buffer, index)?;
        Some(if self.quantization == TerrainQuantization::Bits12 {
            Compression::decompress_texture_coordinates(v[3] as f64).x
        } else {
            v[6] as f64
        })
    }

    pub fn get_oct_encoded_normal(&self, buffer: &[f32], index: usize) -> Option<Cartesian2> {
        if !self.has_vertex_normals {
            return None;
        }
        let value = self.vertex(buffer, index)?[self.offset_vertex_normal] as f64 / 256.0;
        let x = value.floor();
        Some(Cartesian2::new(x, (value - x) * 256.0))
    }

    pub fn decode_normal(&self, buffer: &[f32], index: usize) -> Option<Cartesian3> {
        if !self.has_vertex_normals {
            return None;
        }
        Some(Compression::oct_decode_float(self.vertex(buffer, index)?[self.offset_vertex_normal] as f64))
    }

    pub fn decode_geodetic_surface_normal(&self, buffer: &[f32], index: usize) -> Option<Cartesian3> {
        if !self.has_geodetic_surface_normals {
            return None;
        }
        let v = &self.vertex(buffer, index)?[self.offset_geodetic_surface_normal..];
        Some(Cartesian3::new(v[0] as f64, v[1] as f64, v[2] as f64))
    }

    pub fn get_exaggerated_position(&self, buffer: &[f32], index: usize) -> Option<Cartesian3> {
        let mut p = self.decode_position(buffer, index)?;
        if self.exaggeration != 1.0 && self.has_geodetic_surface_normals {
            let n = self.decode_geodetic_surface_normal(buffer, index)?;
            let height = self.decode_height(buffer, index)?;
            let difference =
                vertical_exaggeration::get_height(height, self.exaggeration, self.exaggeration_relative_height)
                    - height;
            p.x += n.x * difference;
            p.y += n.y * difference;
            p.z += n.z * difference;
        }
        Some(p)
    }

    pub fn add_geodetic_surface_normals(&mut self, old: &[f32], new: &mut [f32], ellipsoid: &Ellipsoid) -> Option<()> {
        if self.has_geodetic_surface_normals {
            return Some(());
        }
        if !old.len().is_multiple_of(self.stride) {
            return None;
        }
        let mut encoding = self.clone();
        encoding.has_geodetic_surface_normals = true;
        encoding.calculate_stride_and_offsets();
        let count = old.len() / self.stride;
        let size = count.checked_mul(encoding.stride)?;
        new.get(..size)?;
        let mut out = vec![0.0; size];
        for i in 0..count {
            let start = i * encoding.stride;
            out[start..start + self.stride].copy_from_slice(&old[i * self.stride..(i + 1) * self.stride]);
            let n = ellipsoid.geodetic_surface_normal(encoding.decode_position(&out, i)?)?;
            let start = start + encoding.offset_geodetic_surface_normal;
            out[start..start + 3].copy_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
        }
        new[..size].copy_from_slice(&out);
        *self = encoding;
        Some(())
    }

    pub fn remove_geodetic_surface_normals(&mut self, old: &[f32], new: &mut [f32]) -> Option<()> {
        if !self.has_geodetic_surface_normals {
            return Some(());
        }
        if !old.len().is_multiple_of(self.stride) {
            return None;
        }
        let mut encoding = self.clone();
        encoding.has_geodetic_surface_normals = false;
        encoding.calculate_stride_and_offsets();
        let count = old.len() / self.stride;
        let size = count.checked_mul(encoding.stride)?;
        new.get(..size)?;
        for i in 0..count {
            let start = i * encoding.stride;
            new[start..start + encoding.stride]
                .copy_from_slice(&old[i * self.stride..i * self.stride + encoding.stride]);
        }
        *self = encoding;
        Some(())
    }

    pub fn get_attributes(&self) -> Vec<TerrainAttribute> {
        let mut attributes = Vec::new();
        let mut offset = 0;
        let mut add = |index, components| {
            attributes.push(TerrainAttribute {
                index,
                component_datatype: ComponentDatatype::Float,
                components_per_attribute: components,
                offset_in_bytes: offset,
                stride_in_bytes: self.stride * 4,
            });
            offset += components as usize * 4;
        };
        if self.quantization == TerrainQuantization::None {
            add(0, 4);
            add(1, 2 + u8::from(self.has_web_mercator_t) + u8::from(self.has_vertex_normals));
        } else {
            add(0, if self.has_web_mercator_t || self.has_vertex_normals { 4 } else { 3 });
            if self.has_web_mercator_t && self.has_vertex_normals {
                add(1, 1);
            }
        }
        if self.has_geodetic_surface_normals {
            add(2, 3);
        }
        attributes
    }

    pub fn get_attribute_locations(&self) -> [(&'static str, u32); 3] {
        if self.quantization == TerrainQuantization::None {
            [("position3DAndHeight", 0), ("textureCoordAndEncodedNormals", 1), ("geodeticSurfaceNormal", 2)]
        } else {
            [("compressed0", 0), ("compressed1", 1), ("geodeticSurfaceNormal", 2)]
        }
    }
}
