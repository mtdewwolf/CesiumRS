//! CesiumJS `PixelFormat`: texture formats, their sizes, and conversions.

use super::component_datatype::ComponentDatatype;
use super::pixel_datatype::{PixelArrayType, PixelDatatype};
use super::typed_array::TypedArray;
use super::webgl_constants as gl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum PixelFormat {
    DepthComponent = gl::DEPTH_COMPONENT,
    DepthStencil = gl::DEPTH_STENCIL,
    Alpha = gl::ALPHA,
    Red = gl::RED,
    Rg = gl::RG,
    Rgb = gl::RGB,
    Rgba = gl::RGBA,
    RedInteger = gl::RED_INTEGER,
    RgInteger = gl::RG_INTEGER,
    RgbInteger = gl::RGB_INTEGER,
    RgbaInteger = gl::RGBA_INTEGER,
    Luminance = gl::LUMINANCE,
    LuminanceAlpha = gl::LUMINANCE_ALPHA,
    RgbDxt1 = gl::COMPRESSED_RGB_S3TC_DXT1_EXT,
    RgbaDxt1 = gl::COMPRESSED_RGBA_S3TC_DXT1_EXT,
    RgbaDxt3 = gl::COMPRESSED_RGBA_S3TC_DXT3_EXT,
    RgbaDxt5 = gl::COMPRESSED_RGBA_S3TC_DXT5_EXT,
    RgbPvrtc4bppv1 = gl::COMPRESSED_RGB_PVRTC_4BPPV1_IMG,
    RgbPvrtc2bppv1 = gl::COMPRESSED_RGB_PVRTC_2BPPV1_IMG,
    RgbaPvrtc4bppv1 = gl::COMPRESSED_RGBA_PVRTC_4BPPV1_IMG,
    RgbaPvrtc2bppv1 = gl::COMPRESSED_RGBA_PVRTC_2BPPV1_IMG,
    RgbaAstc = gl::COMPRESSED_RGBA_ASTC_4x4_WEBGL,
    RgbEtc1 = gl::COMPRESSED_RGB_ETC1_WEBGL,
    Rgb8Etc2 = gl::COMPRESSED_RGB8_ETC2,
    Rgba8Etc2Eac = gl::COMPRESSED_RGBA8_ETC2_EAC,
    RgbaBc7 = gl::COMPRESSED_RGBA_BPTC_UNORM,
}

impl PixelFormat {
    pub const ALL: [Self; 26] = [
        Self::DepthComponent,
        Self::DepthStencil,
        Self::Alpha,
        Self::Red,
        Self::Rg,
        Self::Rgb,
        Self::Rgba,
        Self::RedInteger,
        Self::RgInteger,
        Self::RgbInteger,
        Self::RgbaInteger,
        Self::Luminance,
        Self::LuminanceAlpha,
        Self::RgbDxt1,
        Self::RgbaDxt1,
        Self::RgbaDxt3,
        Self::RgbaDxt5,
        Self::RgbPvrtc4bppv1,
        Self::RgbPvrtc2bppv1,
        Self::RgbaPvrtc4bppv1,
        Self::RgbaPvrtc2bppv1,
        Self::RgbaAstc,
        Self::RgbEtc1,
        Self::Rgb8Etc2,
        Self::Rgba8Etc2Eac,
        Self::RgbaBc7,
    ];

    /// Cesium's constant name, such as `RGBA_DXT5`.
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 26] = [
            "DEPTH_COMPONENT",
            "DEPTH_STENCIL",
            "ALPHA",
            "RED",
            "RG",
            "RGB",
            "RGBA",
            "RED_INTEGER",
            "RG_INTEGER",
            "RGB_INTEGER",
            "RGBA_INTEGER",
            "LUMINANCE",
            "LUMINANCE_ALPHA",
            "RGB_DXT1",
            "RGBA_DXT1",
            "RGBA_DXT3",
            "RGBA_DXT5",
            "RGB_PVRTC_4BPPV1",
            "RGB_PVRTC_2BPPV1",
            "RGBA_PVRTC_4BPPV1",
            "RGBA_PVRTC_2BPPV1",
            "RGBA_ASTC",
            "RGB_ETC1",
            "RGB8_ETC2",
            "RGBA8_ETC2_EAC",
            "RGBA_BC7",
        ];
        NAMES[Self::ALL.iter().position(|f| *f == self).unwrap()]
    }

    pub fn from_value(value: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|f| *f as u32 == value)
    }

    pub fn validate(value: u32) -> bool {
        Self::from_value(value).is_some()
    }

    pub fn components_length(self) -> usize {
        match self {
            Self::Rgb | Self::RgbInteger => 3,
            Self::Rgba | Self::RgbaInteger => 4,
            Self::LuminanceAlpha | Self::Rg | Self::RgInteger => 2,
            _ => 1,
        }
    }

    pub fn is_color_format(self) -> bool {
        matches!(self, Self::Red | Self::Alpha | Self::Rgb | Self::Rgba | Self::Luminance | Self::LuminanceAlpha)
    }

    pub fn is_depth_format(self) -> bool {
        matches!(self, Self::DepthComponent | Self::DepthStencil)
    }

    pub fn is_compressed_format(self) -> bool {
        self.is_dxt_format()
            || self.is_pvrtc_format()
            || self.is_astc_format()
            || self.is_etc1_format()
            || self.is_etc2_format()
            || self.is_bc7_format()
    }

    pub fn is_dxt_format(self) -> bool {
        matches!(self, Self::RgbDxt1 | Self::RgbaDxt1 | Self::RgbaDxt3 | Self::RgbaDxt5)
    }

    pub fn is_pvrtc_format(self) -> bool {
        matches!(self, Self::RgbPvrtc4bppv1 | Self::RgbPvrtc2bppv1 | Self::RgbaPvrtc4bppv1 | Self::RgbaPvrtc2bppv1)
    }

    pub fn is_astc_format(self) -> bool {
        self == Self::RgbaAstc
    }

    pub fn is_etc1_format(self) -> bool {
        self == Self::RgbEtc1
    }

    pub fn is_etc2_format(self) -> bool {
        matches!(self, Self::Rgb8Etc2 | Self::Rgba8Etc2Eac)
    }

    pub fn is_bc7_format(self) -> bool {
        self == Self::RgbaBc7
    }

    /// Bytes for one level of a compressed texture; 0 for other formats.
    pub fn compressed_texture_size_in_bytes(self, width: f64, height: f64) -> f64 {
        let blocks = || ((width + 3.0) / 4.0).floor() * ((height + 3.0) / 4.0).floor();
        match self {
            Self::RgbDxt1 | Self::RgbaDxt1 | Self::RgbEtc1 | Self::Rgb8Etc2 => blocks() * 8.0,
            Self::RgbaDxt3 | Self::RgbaDxt5 | Self::RgbaAstc | Self::Rgba8Etc2Eac => blocks() * 16.0,
            Self::RgbPvrtc4bppv1 | Self::RgbaPvrtc4bppv1 => {
                ((width.max(8.0) * height.max(8.0) * 4.0 + 7.0) / 8.0).floor()
            }
            Self::RgbPvrtc2bppv1 | Self::RgbaPvrtc2bppv1 => {
                ((width.max(16.0) * height.max(8.0) * 2.0 + 7.0) / 8.0).floor()
            }
            Self::RgbaBc7 => (width / 4.0).ceil() * (height / 4.0).ceil() * 16.0,
            _ => 0.0,
        }
    }

    pub fn texture_size_in_bytes(self, pixel_datatype: PixelDatatype, width: f64, height: f64) -> f64 {
        let components = if pixel_datatype.is_packed() { 1 } else { self.components_length() };
        components as f64 * pixel_datatype.size_in_bytes() as f64 * width * height
    }

    pub fn texture_3d_size_in_bytes(self, pixel_datatype: PixelDatatype, width: f64, height: f64, depth: f64) -> f64 {
        self.texture_size_in_bytes(pixel_datatype, width, height) * depth
    }

    /// The row alignment for `gl.pixelStorei(UNPACK_ALIGNMENT)`: 4, 2 or 1.
    pub fn alignment_in_bytes(self, pixel_datatype: PixelDatatype, width: f64) -> u32 {
        let m = self.texture_size_in_bytes(pixel_datatype, width, 1.0) % 4.0;
        if m == 0.0 {
            4
        } else if m == 2.0 {
            2
        } else {
            1
        }
    }

    /// A zeroed array for a `width` × `height` texture.
    pub fn create_typed_array(self, pixel_datatype: PixelDatatype, width: usize, height: usize) -> TypedArray {
        let size = self.components_length() * width * height;
        let datatype = match pixel_datatype.get_typed_array_type() {
            PixelArrayType::Uint8 => ComponentDatatype::UnsignedByte,
            PixelArrayType::Uint16 => ComponentDatatype::UnsignedShort,
            PixelArrayType::Uint32 => ComponentDatatype::UnsignedInt,
            PixelArrayType::Float32 => ComponentDatatype::Float,
        };
        TypedArray::zeros(datatype, size)
    }

    /// The rows in reverse order (a one-row image is returned as it is).
    pub fn flip_y(
        self,
        buffer_view: &TypedArray,
        pixel_datatype: PixelDatatype,
        width: usize,
        height: usize,
    ) -> TypedArray {
        if height == 1 {
            return buffer_view.clone();
        }
        let mut flipped = self.create_typed_array(pixel_datatype, width, height);
        let row_length = width * self.components_length();
        for i in 0..height {
            let row = i * row_length;
            let flipped_row = (height - i - 1) * row_length;
            for j in 0..row_length {
                if let Some(v) = buffer_view.get(row + j) {
                    flipped.set(flipped_row + j, v);
                }
            }
        }
        flipped
    }

    /// The sized internal format WebGL 2 needs; WebGL 1 uses the format itself.
    pub fn to_internal_format(self, pixel_datatype: PixelDatatype, webgl2: bool) -> u32 {
        if !webgl2 {
            return self as u32;
        }
        use PixelDatatype as D;
        match (self, pixel_datatype) {
            (Self::DepthStencil, _) => gl::DEPTH24_STENCIL8,
            (Self::DepthComponent, D::UnsignedShort) => gl::DEPTH_COMPONENT16,
            (Self::DepthComponent, D::UnsignedInt) => gl::DEPTH_COMPONENT24,
            (Self::Rgba, D::Float) => gl::RGBA32F,
            (Self::Rgb, D::Float) => gl::RGB32F,
            (Self::Rg, D::Float) => gl::RG32F,
            (Self::Red, D::Float) => gl::R32F,
            (Self::Rgba, D::HalfFloat) => gl::RGBA16F,
            (Self::Rgb, D::HalfFloat) => gl::RGB16F,
            (Self::Rg, D::HalfFloat) => gl::RG16F,
            (Self::Red, D::HalfFloat) => gl::R16F,
            (Self::Rgba, D::UnsignedByte) => gl::RGBA8,
            (Self::Rgb, D::UnsignedByte) => gl::RGB8,
            (Self::Rg, D::UnsignedByte) => gl::RG8,
            (Self::Red, D::UnsignedByte) => gl::R8,
            // Cesium also maps `PixelDatatype.INT` to RGBA32I and friends, but
            // PixelDatatype has no INT, so those cases never match.
            _ => self.int_internal_format(pixel_datatype),
        }
    }

    fn int_internal_format(self, pixel_datatype: PixelDatatype) -> u32 {
        match (self, pixel_datatype) {
            (Self::RgbaInteger, PixelDatatype::UnsignedInt) => gl::RGBA32UI,
            (Self::RgbInteger, PixelDatatype::UnsignedInt) => gl::RGB32UI,
            (Self::RgInteger, PixelDatatype::UnsignedInt) => gl::RG32UI,
            (Self::RedInteger, PixelDatatype::UnsignedInt) => gl::R32UI,
            _ => self as u32,
        }
    }
}
