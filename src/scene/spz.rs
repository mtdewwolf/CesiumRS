//! The SPZ Gaussian-splat decoder that `GltfSpzLoader` calls: a port of the
//! pinned `@spz-loader/core` 0.3.0, which wraps Niantic's C++ `load_spz` and
//! then applies its JavaScript conversions.
//!
//! The C++ unpacks in `f32`; the JavaScript wrapper exponentiates scales and
//! applies a sigmoid to alphas and a color scale in `f64`, storing `f32`
//! results. Both steps are reproduced bit for bit, with the coordinate system
//! left `UNSPECIFIED` as `GltfSpzLoader` requests.
//!
//! The binding aborts with a WebAssembly "divide by zero" for every invalid
//! input after printing the C++ reason. [`load_spz`] returns that reason as the
//! error message instead; gzip failures and empty clouds, which print nothing,
//! get messages of their own.
use crate::core::{js_math::JsMath, RuntimeError};
use std::io::Read;

const MAGIC: u32 = 0x5053_474e;
const MAX_POINTS: u32 = 10_000_000;
const COLOR_SCALE: f32 = 0.15;
/// `@spz-loader/core`'s default `colorScaleFactor`.
const COLOR_SCALE_FACTOR: f64 = 0.282;

/// A decoded Gaussian cloud, as `loadSpz` returns it. Rotations are `x, y, z, w`.
#[derive(Clone, Debug, PartialEq)]
pub struct GaussianCloud {
    pub num_points: usize,
    pub sh_degree: u8,
    pub antialiased: bool,
    pub positions: Vec<f32>,
    pub scales: Vec<f32>,
    pub rotations: Vec<f32>,
    pub alphas: Vec<f32>,
    pub colors: Vec<f32>,
    /// Per point, the degree's coefficients, each as `r, g, b`.
    pub sh: Vec<f32>,
}

/// Spherical-harmonic coefficients per color channel beyond the DC term.
fn sh_dimension(degree: u8) -> usize {
    match degree {
        1 => 3,
        2 => 8,
        3 => 15,
        _ => 0,
    }
}

fn read_error() -> RuntimeError {
    RuntimeError::new("deserializePackedGaussians: read error")
}

/// `loadSpz(bytes, { unpackOptions: { coordinateSystem: "UNSPECIFIED" } })`.
pub fn load_spz(bytes: &[u8]) -> Result<GaussianCloud, RuntimeError> {
    // zlib's gzip mode: the first member only, checked against its CRC and size.
    let mut data = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut data)
        .map_err(|e| RuntimeError::new(format!("Failed to decompress SPZ data: {e}")))?;
    let header = data.get(..16).ok_or_else(|| RuntimeError::new("deserializePackedGaussians: header not found"))?;
    let word = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().expect("four bytes"));
    if word(0) != MAGIC {
        return Err(RuntimeError::new("deserializePackedGaussians: header not found"));
    }
    let version = word(4);
    if !(1..=3).contains(&version) {
        return Err(RuntimeError::new(format!("deserializePackedGaussians: version not supported: {version}")));
    }
    let points = word(8);
    if points > MAX_POINTS {
        return Err(RuntimeError::new(format!("deserializePackedGaussians: Too many points: {}", points as i32)));
    }
    let (sh_degree, fractional_bits, flags) = (header[12], header[13], header[14]);
    if sh_degree > 3 {
        return Err(RuntimeError::new(format!("deserializePackedGaussians: Unsupported SH degree: {sh_degree}")));
    }
    let n = points as usize;
    let sh_dim = sh_dimension(sh_degree);
    let half_positions = version == 1;
    let smallest_three = version >= 3;
    let mut body = &data[16..];
    let mut take = |len: usize| -> Result<&[u8], RuntimeError> {
        if body.len() < len {
            return Err(read_error());
        }
        let (head, rest) = body.split_at(len);
        body = rest;
        Ok(head)
    };
    let positions = take(n * 3 * if half_positions { 2 } else { 3 })?;
    let alphas = take(n)?;
    let colors = take(n * 3)?;
    let scales = take(n * 3)?;
    let rotations = take(n * if smallest_three { 4 } else { 3 })?;
    let sh = take(n * sh_dim * 3)?;
    if n == 0 {
        // A valid empty cloud; the binding then divides by its point count.
        return Err(RuntimeError::new("SPZ data contains no points"));
    }

    let positions = if half_positions {
        positions.as_chunks::<2>().0.iter().map(|h| half_to_float(u16::from_le_bytes(*h))).collect()
    } else {
        // C's `1 << fractionalBits` on an int: the shift count wraps and bit 31 is the sign.
        let scale = (1.0 / f64::from(1i32.wrapping_shl(u32::from(fractional_bits)))) as f32;
        positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|b| {
                let fixed = i32::from(b[0]) | i32::from(b[1]) << 8 | i32::from(b[2]) << 16;
                let fixed = if fixed & 0x80_0000 != 0 { fixed | !0xff_ffff } else { fixed };
                fixed as f32 * scale
            })
            .collect()
    };
    let rotations = if smallest_three {
        rotations.as_chunks::<4>().0.iter().flat_map(|r| quaternion_smallest_three(u32::from_le_bytes(*r))).collect()
    } else {
        rotations.as_chunks::<3>().0.iter().flat_map(quaternion_first_three).collect()
    };
    Ok(GaussianCloud {
        num_points: n,
        sh_degree,
        antialiased: flags & 1 != 0,
        positions,
        scales: scales.iter().map(|&s| f64::from(f32::from(s) / 16.0 - 10.0).js_exp() as f32).collect(),
        rotations,
        alphas: alphas.iter().map(|&a| sigmoid(inverse_sigmoid(f32::from(a) / 255.0))).collect(),
        colors: colors
            .iter()
            .map(|&c| {
                let color = (f32::from(c) / 255.0 - 0.5) / COLOR_SCALE;
                (f64::from(color) * COLOR_SCALE_FACTOR + 0.5) as f32
            })
            .collect(),
        sh: sh.iter().map(|&s| (f32::from(s) - 128.0) / 128.0).collect(),
    })
}

/// spz's `halfToFloat`. Every NaN becomes the positive quiet NaN.
fn half_to_float(h: u16) -> f32 {
    let sign = if h >> 15 == 1 { -1.0f32 } else { 1.0 };
    let exponent = i32::from((h >> 10) & 0x1f);
    let mantissa = f32::from(h & 0x3ff);
    match exponent {
        0 => sign * 2f32.powi(-14) * mantissa / 1024.0,
        31 if mantissa != 0.0 => f32::NAN,
        31 => sign * f32::INFINITY,
        _ => sign * 2f32.powi(exponent - 15) * (1.0 + mantissa / 1024.0),
    }
}

fn quaternion_first_three(r: &[u8; 3]) -> [f32; 4] {
    let [x, y, z] = r.map(|v| f32::from(v) * (1.0f32 / 127.5) + -1.0);
    [x, y, z, (1.0 - (x * x + y * y + z * z)).max(0.0).sqrt()]
}

/// Three 9-bit magnitudes with sign bits, highest index first, and the index
/// of the omitted largest component in the top two bits.
fn quaternion_smallest_three(packed: u32) -> [f32; 4] {
    const MASK: u32 = (1 << 9) - 1;
    let largest = (packed >> 30) as usize;
    let mut comp = packed;
    let mut rotation = [0f32; 4];
    let mut sum_squares = 0f32;
    for i in (0..4).rev() {
        if i == largest {
            continue;
        }
        let magnitude = comp & MASK;
        let negative = (comp >> 9) & 1 == 1;
        comp >>= 10;
        let value = std::f32::consts::FRAC_1_SQRT_2 * magnitude as f32 / MASK as f32;
        rotation[i] = if negative { -value } else { value };
        sum_squares += rotation[i] * rotation[i];
    }
    // No clamp: a sum of squares above one gives NaN, as it does upstream.
    rotation[largest] = (1.0 - sum_squares).sqrt();
    rotation
}

/// spz's `invSigmoid`.
fn inverse_sigmoid(x: f32) -> f32 {
    logf(x / (1.0 - x))
}

/// musl's `logf` (from ARM's optimized-routines), which emscripten links. The
/// `libm` crate's FreeBSD `logf` rounds differently for some alphas. The table
/// matches the `__logf_data` in the pinned decoder's WebAssembly.
fn logf(x: f32) -> f32 {
    const OFF: u32 = 0x3f33_0000;
    const TABLE: [(u64, u64); 16] = [
        (0x3ff661ec79f8f3be, 0xbfd57bf7808caade),
        (0x3ff571ed4aaf883d, 0xbfd2bef0a7c06ddb),
        (0x3ff49539f0f010b0, 0xbfd01eae7f513a67),
        (0x3ff3c995b0b80385, 0xbfcb31d8a68224e9),
        (0x3ff30d190c8864a5, 0xbfc6574f0ac07758),
        (0x3ff25e227b0b8ea0, 0xbfc1aa2bc79c8100),
        (0x3ff1bb4a4a1a343f, 0xbfba4e76ce8c0e5e),
        (0x3ff12358f08ae5ba, 0xbfb1973c5a611ccc),
        (0x3ff0953f419900a7, 0xbfa252f438e10c1e),
        (0x3ff0000000000000, 0x0000000000000000),
        (0x3fee608cfd9a47ac, 0x3faaa5aa5df25984),
        (0x3feca4b31f026aa0, 0x3fbc5e53aa362eb4),
        (0x3feb2036576afce6, 0x3fc526e57720db08),
        (0x3fe9c2d163a1aa2d, 0x3fcbc2860d224770),
        (0x3fe886e6037841ed, 0x3fd1058bc8a07ee1),
        (0x3fe767dcf5534862, 0x3fd4043057b6ee09),
    ];
    const LN2: u64 = 0x3fe62e42fefa39ef;
    const POLY: [u64; 3] = [0xbfd00ea348b88334, 0x3fd5575b0be00b6a, 0xbfdffffef20a4123];
    let mut ix = x.to_bits();
    if ix == 0x3f80_0000 {
        return 0.0;
    }
    if ix.wrapping_sub(0x0080_0000) >= 0x7f80_0000 - 0x0080_0000 {
        // Zero, subnormal, negative, infinite or NaN.
        if ix.wrapping_mul(2) == 0 {
            return f32::NEG_INFINITY;
        }
        if ix == 0x7f80_0000 {
            return x;
        }
        if ix & 0x8000_0000 != 0 || ix.wrapping_mul(2) >= 0xff00_0000 {
            return f32::NAN;
        }
        ix = (x * f32::from_bits(0x4b00_0000)).to_bits().wrapping_sub(23 << 23);
    }
    let tmp = ix.wrapping_sub(OFF);
    let (invc, logc) = TABLE[((tmp >> 19) % 16) as usize];
    let k = (tmp as i32) >> 23;
    let z = f64::from(f32::from_bits(ix.wrapping_sub(tmp & (0x1ff << 23))));
    let [a0, a1, a2] = POLY.map(f64::from_bits);
    let r = z * f64::from_bits(invc) - 1.0;
    let y0 = f64::from_bits(logc) + f64::from(k) * f64::from_bits(LN2);
    let r2 = r * r;
    let y = a1 * r + a2;
    let y = a0 * r2 + y;
    (y * r2 + (y0 + r)) as f32
}

/// `@spz-loader/core`'s `1 / (1 + Math.exp(-x))`, stored in a `Float32Array`.
fn sigmoid(x: f32) -> f32 {
    (1.0 / (1.0 + (-f64::from(x)).js_exp())) as f32
}
