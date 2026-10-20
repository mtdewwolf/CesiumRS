//! Portable meshoptimizer vertex codec v0/v1 decoder.
//! Adapted from meshoptimizer vertexcodec.cpp (Arseny Kapoulkine), MIT; see NOTICE.
use super::{network_terrain::Reader, RuntimeError};
fn error() -> RuntimeError {
    RuntimeError::new("Invalid meshopt vertex buffer.")
}
pub fn decode(bytes: &[u8], count: usize, stride: usize) -> Result<Vec<u8>, RuntimeError> {
    if stride == 0 || stride > 256 || stride % 4 != 0 {
        return Err(error());
    }
    let mut r = Reader::new(bytes);
    let header = r.u8()?;
    let version = header & 15;
    if header & 0xf0 != 0xa0 || version > 1 {
        return Err(error());
    }
    let tail_size = stride + if version == 0 { 0 } else { stride / 4 };
    let padded = tail_size.max(if version == 0 { 32 } else { 24 });
    if bytes.len() < 1 + padded {
        return Err(error());
    }
    let tail = &bytes[bytes.len() - tail_size..];
    let mut last = tail[..stride].to_vec();
    let channels = if version == 0 { &[][..] } else { &tail[stride..] };
    let block_size = ((8192 / stride) & !15).min(256);
    let mut out = vec![0u8; count.checked_mul(stride).ok_or_else(error)?];
    let mut offset = 0;
    while offset < count {
        let n = (count - offset).min(block_size);
        let aligned = (n + 15) & !15;
        let control = if version == 0 { vec![0; stride / 4] } else { r.take(stride / 4)?.to_vec() };
        for k in (0..stride).step_by(4) {
            let mut values = vec![0u8; n * 4];
            for j in 0..4 {
                let ctrl = (control[k / 4] >> (j * 2)) & 3;
                let decoded = if ctrl == 3 {
                    r.take(n)?.to_vec()
                } else if ctrl == 2 {
                    vec![0; n]
                } else {
                    let bits = if version == 0 { [0, 2, 4, 8, 8] } else { [0, 1, 2, 4, 8] };
                    decode_bytes(&mut r, aligned, &bits[if version == 0 { 0 } else { ctrl as usize }..])?
                };
                values[j * n..(j + 1) * n].copy_from_slice(&decoded[..n]);
            }
            let channel = if version == 0 { 0 } else { channels[k / 4] };
            let unit = match channel & 3 {
                0 => 1,
                1 => 2,
                2 => 4,
                _ => return Err(error()),
            };
            for j in (0..4).step_by(unit) {
                let mut previous = 0u32;
                for b in 0..unit {
                    previous |= (last[k + j + b] as u32) << (b * 8);
                }
                for i in 0..n {
                    let mut value = 0u32;
                    for b in 0..unit {
                        value |= (values[(j + b) * n + i] as u32) << (b * 8);
                    }
                    value = if channel & 3 == 2 {
                        value.rotate_left((32 - (channel as u32 >> 4)) & 31) ^ previous
                    } else {
                        let delta = (value >> 1) ^ (0u32.wrapping_sub(value & 1));
                        previous.wrapping_add(delta)
                    };
                    for b in 0..unit {
                        out[(offset + i) * stride + k + j + b] = (value >> (b * 8)) as u8;
                    }
                    previous = value;
                }
            }
        }
        last.copy_from_slice(&out[(offset + n - 1) * stride..(offset + n) * stride]);
        offset += n;
    }
    if bytes.len() - r.pos != padded {
        return Err(error());
    }
    Ok(out)
}
fn decode_bytes(r: &mut Reader<'_>, count: usize, bits: &[u8]) -> Result<Vec<u8>, RuntimeError> {
    let groups = count / 16;
    let headers = r.take((groups + 3) / 4)?.to_vec();
    let mut out = vec![0; count];
    for group in 0..groups {
        let mode = (headers[group / 4] >> ((group % 4) * 2)) & 3;
        let nbits = bits[mode as usize];
        let dst = &mut out[group * 16..group * 16 + 16];
        if nbits == 0 {
            continue;
        }
        if nbits == 8 {
            dst.copy_from_slice(r.take(16)?);
            continue;
        }
        let packed = r.take(16 * nbits as usize / 8)?.to_vec();
        let mask = (1u8 << nbits) - 1;
        for (i, value) in dst.iter_mut().enumerate() {
            let shift = if nbits == 1 { i % 8 } else { 8 - nbits as usize - (i * nbits as usize % 8) };
            let encoded = packed[i * nbits as usize / 8] >> shift & mask;
            *value = if encoded == mask { r.u8()? } else { encoded };
        }
    }
    Ok(out)
}
