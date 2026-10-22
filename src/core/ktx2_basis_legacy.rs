//! Cesium's Basis build disables ASTC's higher opaque quality modes. Newer
//! Basis codecs enable those modes. Keep the legacy CEM12 path using the
//! pinned codec's ETC1S decoder, lookup tables and ASTC bit packer.
//!
//! Algorithm: BinomialLLC/basis_universal 1.16.4, basisu_transcoder.cpp
//! convert_etc1s_to_astc_4x4 (Apache-2.0); table/packing helpers: basisu 0.1.0.
use super::RuntimeError;
use basisu::{
    basislz::{
        astc_pack::{pack_cem_12_weight_range2, AstcBlockParams},
        astc_tables::{
            ise_to_unquant, selector_range_index, single_color_encoding_0, single_color_encoding_1, SELECTOR_MAPPINGS,
        },
        etc1s::{Endpoint, Selector},
        etc1s_astc::{convert_etc1s_to_astc_4x4, convert_etc1s_to_astc_4x4_rgba},
    },
    color::Color32,
    etc::block::DecoderEtcBlock,
    tables::astc::G_ETC1_TO_ASTC,
    DecodeFlags, TargetFormat, Transcoder,
};

fn unpack(block: &[u8]) -> (Endpoint, Selector) {
    let ep = Endpoint {
        color5: Color32::new((block[0] >> 3) as u32, (block[1] >> 3) as u32, (block[2] >> 3) as u32, 0),
        inten5: block[3] >> 5,
    };
    let bits = u32::from_be_bytes(block[4..8].try_into().unwrap());
    let mut sel = Selector::default();
    // ETC1's selector order is +small, +large, -small, -large;
    // Basis tables use ascending intensity instead.
    let to_basis = [2, 3, 1, 0];
    for x in 0..4 {
        for y in 0..4 {
            let i = x * 4 + y;
            let index = ((bits >> i) & 1) | (((bits >> (i + 16)) & 1) << 1);
            sel.set_selector(x, y, to_basis[index as usize]);
        }
    }
    sel.init_flags();
    (ep, sel)
}

fn plane(blk: &mut AstcBlockParams, ep: &Endpoint, sel: &Selector, alpha: bool) {
    let channels: &[usize] = if alpha { &[1] } else { &[0, 1, 2] };
    let start = if alpha { 6 } else { 0 };
    let parity = usize::from(alpha);
    let colors = DecoderEtcBlock::get_block_colors5(ep.color5, ep.inten5 as u32);
    let low = sel.lo_selector as usize;
    let high = sel.hi_selector as usize;
    let mut weights = [0u8; 16];
    if low == high {
        let table = single_color_encoding_1();
        for (i, &channel) in channels.iter().enumerate() {
            let ends = table[colors[low].c[channel] as usize];
            blk.endpoints[start + i * 2..start + i * 2 + 2].copy_from_slice(&ends);
        }
        weights.fill(1);
    } else if ep.inten5 == 7 && sel.num_unique_selectors == 2 && low == 0 && high == 3 {
        let table = single_color_encoding_0();
        for (i, &channel) in channels.iter().enumerate() {
            blk.endpoints[start + i * 2] = table[colors[0].c[channel] as usize];
            blk.endpoints[start + i * 2 + 1] = table[colors[3].c[channel] as usize];
        }
        for y in 0..4 {
            for x in 0..4 {
                weights[(x + y * 4) as usize] = if sel.get_selector(x, y) == 3 { 3 } else { 0 };
            }
        }
    } else {
        let range = selector_range_index()[low][high] as usize;
        let base = |c: usize| (ep.inten5 as usize * 32 + ep.color5.c[c] as usize) * 60 + range * 10;
        let mut best_error = u32::MAX;
        let mut best = 0;
        for m in 0..10 {
            let error = channels.iter().map(|&c| G_ETC1_TO_ASTC[base(c) + m].m_err as u32).sum();
            if error < best_error {
                best_error = error;
                best = m;
            }
        }
        for (i, &channel) in channels.iter().enumerate() {
            let end = &G_ETC1_TO_ASTC[base(channel) + best];
            blk.endpoints[start + i * 2] = end.m_lo;
            blk.endpoints[start + i * 2 + 1] = end.m_hi;
        }
        for y in 0..4 {
            for x in 0..4 {
                weights[(x + y * 4) as usize] = SELECTOR_MAPPINGS[best][sel.get_selector(x, y) as usize];
            }
        }
    }
    if !alpha {
        let ise = ise_to_unquant();
        let sum = |offset: usize| [0, 2, 4].iter().map(|i| ise[blk.endpoints[i + offset] as usize]).sum::<u32>();
        if sum(1) < sum(0) {
            for i in [0, 2, 4] {
                blk.endpoints.swap(i, i + 1);
            }
            for w in &mut weights {
                *w = 3 - *w;
            }
        }
    }
    for (i, w) in weights.into_iter().enumerate() {
        blk.weights[i * 2 + parity] = w;
    }
}

fn convert(color: &[u8], alpha: Option<&[u8]>) -> [u8; 16] {
    let (ep, sel) = unpack(color);
    let alpha = alpha.map(unpack);
    if sel.num_unique_selectors <= 2 && alpha.as_ref().is_none_or(|(_, s)| s.num_unique_selectors <= 2) {
        return match alpha {
            Some((ae, asel)) => convert_etc1s_to_astc_4x4_rgba(&ep, &sel, &ae, &asel),
            None => convert_etc1s_to_astc_4x4(&ep, &sel),
        };
    }
    let mut blk = AstcBlockParams::default();
    plane(&mut blk, &ep, &sel, false);
    if let Some((ae, asel)) = alpha {
        plane(&mut blk, &ae, &asel, true);
    } else {
        blk.endpoints[6] = 1;
        blk.endpoints[7] = 1;
    }
    pack_cem_12_weight_range2(&blk)
}

pub(super) fn transcode(texture: &Transcoder<'_>, level: u32) -> Result<Vec<u8>, RuntimeError> {
    let rgb = texture
        .transcode_image(level, 0, 0, TargetFormat::Etc1Rgb, DecodeFlags::NONE)
        .map_err(|_| RuntimeError::new("transcodeImage() failed."))?;
    let alpha = if texture.has_alpha() {
        Some(
            texture
                .transcode_image(level, 0, 0, TargetFormat::Etc1Rgb, DecodeFlags::TRANSCODE_ALPHA_TO_OPAQUE)
                .map_err(|_| RuntimeError::new("transcodeImage() failed."))?,
        )
    } else {
        None
    };
    let mut result = Vec::with_capacity(rgb.len() * 2);
    for (i, c) in rgb.as_chunks::<8>().0.iter().enumerate() {
        result.extend_from_slice(&convert(c, alpha.as_ref().map(|a| &a[i * 8..i * 8 + 8])));
    }
    Ok(result)
}
