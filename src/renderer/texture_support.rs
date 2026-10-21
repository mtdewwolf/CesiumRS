//! Shared texture host operations. Algorithms and row packing remain in Rust.
use super::{context::*, *};
use crate::core::{webgl_constants as gl, PixelFormat};

pub enum TextureSource<G: Gl> {
    Pixels { width: u32, height: u32, bytes: Vec<u8> },
    Image { width: u32, height: u32, image: G::Object },
}
impl<G: Gl> TextureSource<G> {
    pub fn dimensions(&self) -> [u32; 2] {
        match self {
            Self::Pixels { width, height, .. } | Self::Image { width, height, .. } => [*width, *height],
        }
    }
}
pub(crate) fn color_format(format: PixelFormat, datatype: PixelDatatype) -> bool {
    use PixelDatatype as D;
    use PixelFormat as F;
    match format {
        F::Rgba => matches!(
            datatype,
            D::UnsignedByte | D::Float | D::HalfFloat | D::UnsignedShort4_4_4_4 | D::UnsignedShort5_5_5_1
        ),
        F::Rgb => matches!(datatype, D::UnsignedByte | D::Float | D::HalfFloat | D::UnsignedShort5_6_5),
        F::Red | F::Rg | F::Alpha | F::Luminance | F::LuminanceAlpha => {
            matches!(datatype, D::UnsignedByte | D::Float | D::HalfFloat)
        }
        F::RedInteger | F::RgInteger | F::RgbInteger | F::RgbaInteger => datatype == D::UnsignedInt,
        _ => false,
    }
}
pub(crate) fn pixels_size(format: PixelFormat, datatype: PixelDatatype, dimensions: &[u32]) -> Result<usize> {
    let components = if datatype.is_packed() { 1 } else { format.components_length() };
    let length = dimensions
        .iter()
        .try_fold(components * datatype.size_in_bytes(), |n, &d| n.checked_mul(d as usize))
        .ok_or_else(|| crate::core::RuntimeError::new("Pixel size overflow"))?;
    size::<()>(length)?;
    Ok(length)
}
pub(crate) fn flip_pixels(
    bytes: &[u8],
    format: PixelFormat,
    datatype: PixelDatatype,
    width: u32,
    height: u32,
    flip: bool,
) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if flip && height > 1 {
        let row = width as usize * format.components_length() * datatype.size_in_bytes();
        out.resize(row * height as usize, 0);
        for y in 0..height as usize / 2 {
            let (a, b) = out.split_at_mut((height as usize - y - 1) * row);
            a[y * row..(y + 1) * row].swap_with_slice(&mut b[..row]);
        }
    }
    out
}
pub(crate) fn sampler<G: Gl>(
    context: &Context<G>,
    target: u32,
    format: PixelFormat,
    datatype: PixelDatatype,
    value: Sampler,
) -> Result<()> {
    value.validate()?;
    let mut min = value.minification_filter as u32;
    let mut mag = value.magnification_filter as u32;
    if (datatype == PixelDatatype::Float && !context.extension_supported("OES_texture_float_linear")?)
        || (datatype == PixelDatatype::HalfFloat
            && !context.webgl2()
            && !context.extension_supported("OES_texture_half_float_linear")?)
    {
        min = if matches!(
            value.minification_filter,
            TextureMinificationFilter::Nearest | TextureMinificationFilter::Linear
        ) {
            gl::NEAREST
        } else {
            gl::NEAREST_MIPMAP_NEAREST
        };
        mag = gl::NEAREST;
    }
    if format.is_depth_format() {
        min = gl::NEAREST;
        mag = gl::NEAREST;
    }
    for (p, v) in [
        (gl::TEXTURE_MIN_FILTER, min),
        (gl::TEXTURE_MAG_FILTER, mag),
        (gl::TEXTURE_WRAP_S, value.wrap_s as u32),
        (gl::TEXTURE_WRAP_T, value.wrap_t as u32),
    ] {
        context.call("texParameteri", &[target.into(), p.into(), v.into()])?;
    }
    if target == gl::TEXTURE_3D {
        context.call("texParameteri", &[target.into(), gl::TEXTURE_WRAP_R.into(), (value.wrap_r as u32).into()])?;
    }
    if context.extension_supported("EXT_texture_filter_anisotropic")? {
        context.call("texParameteri", &[target.into(), 0x84fe_u32.into(), value.maximum_anisotropy.into()])?;
    }
    Ok(())
}
pub(crate) fn pixel_store<G: Gl>(
    context: &Context<G>,
    options: &TextureOptions,
    width: u32,
    image: bool,
    skip: bool,
) -> Result<()> {
    context.call(
        "pixelStorei",
        &[
            gl::UNPACK_ALIGNMENT.into(),
            if image {
                4u32.into()
            } else {
                options.pixel_format.alignment_in_bytes(options.pixel_datatype, width as f64).into()
            },
        ],
    )?;
    context.call(
        "pixelStorei",
        &[
            gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL.into(),
            (image
                && (options.pre_multiply_alpha
                    || matches!(options.pixel_format, PixelFormat::Rgb | PixelFormat::Luminance)))
            .into(),
        ],
    )?;
    context.call("pixelStorei", &[gl::UNPACK_FLIP_Y_WEBGL.into(), (image && options.flip_y).into()])?;
    context.call(
        "pixelStorei",
        &[
            gl::UNPACK_COLORSPACE_CONVERSION_WEBGL.into(),
            (if skip { gl::NONE } else { gl::BROWSER_DEFAULT_WEBGL }).into(),
        ],
    )?;
    if context.webgl2() {
        context.call("bindBuffer", &[gl::PIXEL_UNPACK_BUFFER.into(), GlValue::Null])?;
    }
    Ok(())
}

pub(crate) fn validate_format<G: Gl>(context: &Context<G>, format: PixelFormat, datatype: PixelDatatype) -> Result<()> {
    use PixelFormat as F;
    let legacy = matches!(format, F::Alpha | F::Luminance | F::LuminanceAlpha);
    let modern = matches!(format, F::Red | F::Rg | F::RedInteger | F::RgInteger | F::RgbInteger | F::RgbaInteger);
    ensure(if context.webgl2() { !legacy } else { !modern }, "Pixel format is unavailable in this WebGL version")?;
    if !context.webgl2() {
        if datatype == PixelDatatype::Float {
            ensure(context.extension_supported("OES_texture_float")?, "Floating point textures unsupported")?;
        }
        if datatype == PixelDatatype::HalfFloat {
            ensure(context.extension_supported("OES_texture_half_float")?, "Half floating point textures unsupported")?;
        }
        if format.is_depth_format() {
            ensure(context.extension_supported("WEBGL_depth_texture")?, "Depth textures unsupported")?;
        }
    }
    Ok(())
}
