//! CesiumJS KTX2 texture parsing and Basis target selection.
//!
//! Uncompressed images are decoded natively. ETC1S/UASTC entropy decoding is
//! supplied by a Basis backend, which may use native code or WebAssembly.

use super::compressed_texture_buffer::CompressedTextureBufferOptions;
use super::{CompressedTextureBuffer, PixelDatatype, PixelFormat, Resource, RuntimeError};
use std::{future::Future, pin::Pin, sync::Arc};

pub type Ktx2Future<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + 'a>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct SupportedTextureFormats {
    pub s3tc: bool,
    pub pvrtc: bool,
    pub astc: bool,
    pub etc: bool,
    pub etc1: bool,
    pub bc7: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasisColorModel {
    Etc1s,
    Uastc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasisFormat {
    Etc1Rgb,
    Etc2Rgba,
    Bc1Rgb,
    Bc3Rgba,
    Pvrtc1Rgb4,
    Pvrtc1Rgba4,
    Astc4x4Rgba,
    Bc7Rgba,
}

#[derive(Clone, Copy, Debug)]
pub struct BasisInfo {
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub has_alpha: bool,
}

pub trait BasisTranscoder {
    /// Lazily initialize the codec and inspect the file. The backend caches its
    /// module initialization and releases file/module handles on every error.
    fn inspect<'a>(&'a self, data: &'a [u8]) -> Ktx2Future<'a, BasisInfo>;
    /// Decode face zero/layer zero of each mip level. This preserves Cesium's
    /// restriction on supercompressed cubemaps. Each vector contains one mip.
    fn transcode<'a>(&'a self, data: &'a [u8], format: BasisFormat) -> Ktx2Future<'a, Vec<Vec<u8>>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubeFace {
    PositiveX,
    NegativeX,
    PositiveY,
    NegativeY,
    PositiveZ,
    NegativeZ,
}
impl CubeFace {
    pub const ALL: [Self; 6] =
        [Self::PositiveX, Self::NegativeX, Self::PositiveY, Self::NegativeY, Self::PositiveZ, Self::NegativeZ];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ktx2Texture {
    Image(CompressedTextureBuffer),
    Mipmaps(Vec<CompressedTextureBuffer>),
    Cubemap(Vec<Vec<(CubeFace, CompressedTextureBuffer)>>),
}

#[derive(Clone, Debug)]
pub struct Ktx2Level {
    pub offset: usize,
    pub byte_length: usize,
    pub uncompressed_byte_length: u64,
}

#[derive(Clone, Debug)]
pub struct Ktx2Header {
    pub vk_format: u32,
    pub type_size: u32,
    pub width: u32,
    pub height: u32,
    pub face_count: u32,
    pub level_count: u32,
    pub supercompression_scheme: u32,
    pub color_model: u8,
    pub levels: Vec<Ktx2Level>,
}

fn invalid() -> RuntimeError {
    RuntimeError::new("Invalid KTX2 file.")
}
fn u32_at(data: &[u8], offset: usize) -> Result<u32, RuntimeError> {
    let bytes = data.get(offset..offset.checked_add(4).ok_or_else(invalid)?).ok_or_else(invalid)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}
fn u64_at(data: &[u8], offset: usize) -> Result<u64, RuntimeError> {
    let bytes = data.get(offset..offset.checked_add(8).ok_or_else(invalid)?).ok_or_else(invalid)?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}
fn check_range(data: &[u8], offset: usize, length: usize) -> Result<(), RuntimeError> {
    data.get(offset..offset.checked_add(length).ok_or_else(invalid)?).map(|_| ()).ok_or_else(invalid)
}

impl Ktx2Header {
    pub fn parse(data: &[u8]) -> Result<Self, RuntimeError> {
        const ID: [u8; 12] = [0xab, 0x4b, 0x54, 0x58, 0x20, 0x32, 0x30, 0xbb, 0x0d, 0x0a, 0x1a, 0x0a];
        if data.get(..12) != Some(ID.as_slice()) || data.len() < 80 {
            return Err(invalid());
        }
        if u32_at(data, 32)? != 0 {
            return Err(RuntimeError::new("KTX2 texture arrays are not supported."));
        }
        if u32_at(data, 28)? != 0 {
            return Err(RuntimeError::new("KTX2 3D textures are unsupported."));
        }
        let level_count = u32_at(data, 40)?;
        let count = level_count.max(1) as usize;
        check_range(data, 80, count.checked_mul(24).ok_or_else(invalid)?)?;
        let mut levels = Vec::with_capacity(count);
        for i in 0..count {
            let offset = usize::try_from(u64_at(data, 80 + i * 24)?).map_err(|_| invalid())?;
            let byte_length = usize::try_from(u64_at(data, 88 + i * 24)?).map_err(|_| invalid())?;
            check_range(data, offset, byte_length)?;
            levels.push(Ktx2Level { offset, byte_length, uncompressed_byte_length: u64_at(data, 96 + i * 24)? });
        }
        let dfd = u32_at(data, 48)? as usize;
        let dfd_length = u32_at(data, 52)? as usize;
        check_range(data, dfd, dfd_length)?;
        if dfd_length < 28 {
            return Err(invalid());
        }
        // Validate descriptor sample storage as ktx-parse does, without copying it.
        let block_size = u16::from_le_bytes(data[dfd + 10..dfd + 12].try_into().unwrap()) as usize;
        if block_size < 24 || block_size % 4 != 0 || block_size + 4 > dfd_length {
            return Err(invalid());
        }
        check_range(data, u32_at(data, 56)? as usize, u32_at(data, 60)? as usize)?;
        let sgd_offset = usize::try_from(u64_at(data, 64)?).map_err(|_| invalid())?;
        let sgd_length = usize::try_from(u64_at(data, 72)?).map_err(|_| invalid())?;
        check_range(data, sgd_offset, sgd_length)?;
        let face_count = u32_at(data, 36)?;
        if face_count != 1 && face_count != 6 {
            return Err(invalid());
        }
        Ok(Self {
            vk_format: u32_at(data, 12)?,
            type_size: u32_at(data, 16)?,
            width: u32_at(data, 20)?,
            height: u32_at(data, 24)?,
            face_count,
            level_count,
            supercompression_scheme: u32_at(data, 44)?,
            color_model: data[dfd + 12],
            levels,
        })
    }
    pub fn basis_color_model(&self) -> Option<BasisColorModel> {
        if self.vk_format != 0 {
            return None;
        }
        match self.color_model {
            163 => Some(BasisColorModel::Etc1s),
            166 => Some(BasisColorModel::Uastc),
            _ => None,
        }
    }
}

/// Uses Cesium's different ETC1S and UASTC preference orders.
pub fn select_basis_target(
    model: BasisColorModel,
    alpha: bool,
    formats: SupportedTextureFormats,
) -> Result<(PixelFormat, BasisFormat), RuntimeError> {
    let etc = || {
        if alpha {
            (PixelFormat::Rgba8Etc2Eac, BasisFormat::Etc2Rgba)
        } else {
            (PixelFormat::Rgb8Etc2, BasisFormat::Etc1Rgb)
        }
    };
    let s3tc = || {
        if alpha {
            (PixelFormat::RgbaDxt5, BasisFormat::Bc3Rgba)
        } else {
            (PixelFormat::RgbDxt1, BasisFormat::Bc1Rgb)
        }
    };
    let pvrtc = || {
        if alpha {
            (PixelFormat::RgbaPvrtc4bppv1, BasisFormat::Pvrtc1Rgba4)
        } else {
            (PixelFormat::RgbPvrtc4bppv1, BasisFormat::Pvrtc1Rgb4)
        }
    };
    let astc = (PixelFormat::RgbaAstc, BasisFormat::Astc4x4Rgba);
    let bc7 = (PixelFormat::RgbaBc7, BasisFormat::Bc7Rgba);
    let target = match model {
        BasisColorModel::Etc1s => {
            if formats.etc {
                Some(etc())
            } else if formats.etc1 && !alpha {
                Some((PixelFormat::RgbEtc1, BasisFormat::Etc1Rgb))
            } else if formats.s3tc {
                Some(s3tc())
            } else if formats.pvrtc {
                Some(pvrtc())
            } else if formats.astc {
                Some(astc)
            } else if formats.bc7 {
                Some(bc7)
            } else {
                None
            }
        }
        BasisColorModel::Uastc => {
            if formats.astc {
                Some(astc)
            } else if formats.bc7 {
                Some(bc7)
            } else if formats.s3tc {
                Some(s3tc())
            } else if formats.etc {
                Some(etc())
            } else if formats.etc1 && !alpha {
                Some((PixelFormat::RgbEtc1, BasisFormat::Etc1Rgb))
            } else if formats.pvrtc {
                Some(pvrtc())
            } else {
                None
            }
        }
    };
    target.ok_or_else(|| {
        RuntimeError::new(match model {
            BasisColorModel::Etc1s => "No transcoding format target available for ETC1S compressed ktx2.",
            BasisColorModel::Uastc => "No transcoding format target available for UASTC compressed ktx2.",
        })
    })
}

fn collapse(mut levels: Vec<Vec<(CubeFace, CompressedTextureBuffer)>>) -> Result<Ktx2Texture, RuntimeError> {
    if levels.first().is_none_or(Vec::is_empty) {
        return Err(invalid());
    }
    if levels[0].len() != 1 {
        return Ok(Ktx2Texture::Cubemap(levels));
    }
    if levels.len() == 1 {
        return Ok(Ktx2Texture::Image(levels.remove(0).remove(0).1));
    }
    Ok(Ktx2Texture::Mipmaps(levels.into_iter().map(|mut level| level.remove(0).1).collect()))
}

fn mip_dimensions(header: &Ktx2Header, level: usize) -> Result<(u32, u32), RuntimeError> {
    let width = (header.width as i32) >> (level & 31);
    let height = (header.height as i32) >> (level & 31);
    if width < 0 || height < 0 {
        return Err(invalid());
    }
    Ok((width as u32, height as u32))
}

pub fn parse_uncompressed_ktx2(data: &[u8]) -> Result<Ktx2Texture, RuntimeError> {
    let header = Ktx2Header::parse(data)?;
    if header.basis_color_model().is_some() {
        return Err(RuntimeError::new("KTX2 image requires a Basis transcoder."));
    }
    parse_uncompressed(data, &header)
}

fn parse_uncompressed(data: &[u8], header: &Ktx2Header) -> Result<Ktx2Texture, RuntimeError> {
    let format = if header.vk_format == 29 { PixelFormat::Rgb } else { PixelFormat::Rgba };
    let datatype = match header.vk_format {
        37 => Some(PixelDatatype::UnsignedByte),
        97 => Some(PixelDatatype::HalfFloat),
        109 => Some(PixelDatatype::Float),
        _ => None,
    };
    let element_size = datatype.map_or(1, PixelDatatype::size_in_bytes);
    let mut levels = Vec::with_capacity(header.levels.len());
    for (i, level) in header.levels.iter().enumerate() {
        let (width, height) = mip_dimensions(header, i)?;
        let length = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(if format == PixelFormat::Rgb { 3 } else { 4 }))
            .ok_or_else(invalid)?;
        let bytes = length.checked_mul(element_size).ok_or_else(invalid)?;
        let stride = length.checked_mul(header.type_size as usize).ok_or_else(invalid)?;
        let mut faces = Vec::with_capacity(header.face_count as usize);
        for face in 0..header.face_count as usize {
            let offset = level.offset.checked_add(stride.checked_mul(face).ok_or_else(invalid)?).ok_or_else(invalid)?;
            if offset % element_size != 0 {
                return Err(invalid());
            }
            // JS views use the entire backing buffer, including bytes outside
            // the declared level slice. Preserve that bounds rule.
            check_range(data, offset, bytes)?;
            faces.push((
                CubeFace::ALL[face],
                CompressedTextureBuffer::new(CompressedTextureBufferOptions {
                    internal_format: format as u32,
                    pixel_datatype: datatype.map(|d| d as u32),
                    width,
                    height,
                    buffer: Arc::from(&data[offset..offset + bytes]),
                }),
            ));
        }
        levels.push(faces);
    }
    collapse(levels)
}

pub struct Ktx2Transcoder<B> {
    pub supported_formats: SupportedTextureFormats,
    pub basis: B,
}
impl<B: BasisTranscoder> Ktx2Transcoder<B> {
    pub async fn transcode(&self, data: &[u8]) -> Result<Ktx2Texture, RuntimeError> {
        let header = Ktx2Header::parse(data)?;
        let Some(model) = header.basis_color_model() else {
            return parse_uncompressed(data, &header);
        };
        let info = self.basis.inspect(data).await?;
        if info.width == 0 || info.height == 0 || info.levels == 0 {
            return Err(RuntimeError::new("Invalid KTX2 file"));
        }
        let (format, target) = select_basis_target(model, info.has_alpha, self.supported_formats)?;
        let buffers = self.basis.transcode(data, target).await?;
        if buffers.len() != header.levels.len() {
            return Err(RuntimeError::new("transcodeImage() failed."));
        }
        let mut levels = Vec::with_capacity(buffers.len());
        for (i, buffer) in buffers.into_iter().enumerate() {
            let (width, height) = mip_dimensions(&header, i)?;
            levels.push(vec![(
                CubeFace::PositiveX,
                CompressedTextureBuffer::new(CompressedTextureBufferOptions {
                    internal_format: format as u32,
                    pixel_datatype: None,
                    width,
                    height,
                    buffer: buffer.into(),
                }),
            )]);
        }
        collapse(levels)
    }
}

pub enum Ktx2Source<'a> {
    Buffer(&'a [u8]),
    Resource(&'a Resource),
}
pub trait Ktx2Loader {
    /// A missing future means the resource request was throttled.
    fn load<'a>(&'a self, resource: &'a Resource) -> Option<Ktx2Future<'a, Vec<u8>>>;
}
pub fn load_ktx2<'a, B: BasisTranscoder>(
    transcoder: &'a Ktx2Transcoder<B>,
    source: Ktx2Source<'a>,
    loader: &'a dyn Ktx2Loader,
) -> Option<Ktx2Future<'a, Ktx2Texture>> {
    match source {
        Ktx2Source::Buffer(data) => Some(Box::pin(transcoder.transcode(data))),
        Ktx2Source::Resource(resource) => {
            let load = loader.load(resource)?;
            Some(Box::pin(async move {
                let bytes = load.await?;
                transcoder.transcode(&bytes).await
            }))
        }
    }
}
