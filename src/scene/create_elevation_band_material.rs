//! CesiumJS `createElevationBandMaterial`: combine layered height/color bands
//! into premultiplied-alpha lookup textures and an `ElevationBand` material.

use super::{material::ELEVATION_BAND_TYPE, Material};
use crate::{
    core::{Cartesian4, Color, PixelDatatype, PixelFormat, RuntimeError},
    renderer::{Context, Gl, Sampler, Texture, TextureOptions},
};
use std::{cmp::Ordering, rc::Rc};

/// Height stored for `extendUpwards` entries.
pub const MAXIMUM_HEIGHT: f64 = 5_906_376_425_472.0;
/// Height stored for `extendDownwards` entries.
pub const MINIMUM_HEIGHT: f64 = -5_906_376_425_472.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElevationBandEntry {
    pub height: f64,
    pub color: Color,
}

#[derive(Clone, Debug, Default)]
pub struct ElevationBandLayer {
    pub entries: Vec<ElevationBandEntry>,
    pub extend_downwards: bool,
    pub extend_upwards: bool,
}

/// The CPU-side texture payload used by the material.
#[derive(Clone, Debug, PartialEq)]
pub struct ElevationBandTextureData {
    pub entries: Vec<ElevationBandEntry>,
    pub height_pixel_format: PixelFormat,
    pub height_pixel_datatype: PixelDatatype,
    pub height_bytes: Vec<u8>,
    pub color_bytes: Vec<u8>,
}

fn create_new_entry(height: f64, color: Color) -> ElevationBandEntry {
    ElevationBandEntry { height, color }
}

fn lerp_entry_color(
    height: f64,
    entry_before: &ElevationBandEntry,
    entry_after: &ElevationBandEntry,
    result: &mut Color,
) {
    let factor = if entry_before.height == entry_after.height {
        0.0
    } else {
        (height - entry_before.height) / (entry_after.height - entry_before.height)
    };
    *result = entry_before.color.lerp(&entry_after.color, factor);
}

/// Cesium's three filter passes, each operating on the previous result.
fn remove_duplicates(entries: Vec<ElevationBandEntry>) -> Vec<ElevationBandEntry> {
    let previous = |index: usize| index.checked_sub(1).filter(|_| index > 0);

    let entries: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            let same_height_as_prev = previous(*index).is_none_or(|i| entry.height == entries[i].height);
            let same_height_as_next = entries.get(index + 1).is_none_or(|next| entry.height == next.height);
            !(same_height_as_prev && same_height_as_next)
        })
        .map(|(_, entry)| *entry)
        .collect();

    let entries: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            let same_color_as_prev = previous(*index).is_some_and(|i| entry.color == entries[i].color);
            let same_color_as_next = entries.get(index + 1).is_some_and(|next| entry.color == next.color);
            !(same_color_as_prev && same_color_as_next)
        })
        .map(|(_, entry)| *entry)
        .collect();

    entries
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            let same_color_as_prev = previous(*index).is_some_and(|i| entry.color == entries[i].color);
            let same_height_as_prev = previous(*index).is_none_or(|i| entry.height == entries[i].height);
            !(same_color_as_prev && same_height_as_prev)
        })
        .map(|(_, entry)| *entry)
        .collect()
}

fn preprocess(layers: &[ElevationBandLayer]) -> Result<Vec<Vec<ElevationBandEntry>>, RuntimeError> {
    let mut layered = Vec::with_capacity(layers.len());
    for layer in layers {
        if layer.entries.is_empty() {
            return Err(RuntimeError::new("entries must be an array with size > 0."));
        }
        let mut entries: Vec<_> = layer
            .entries
            .iter()
            .map(|entry| {
                if entry.height.is_nan() {
                    return Err(RuntimeError::new("entry requires a height."));
                }
                let height = entry.height.clamp(MINIMUM_HEIGHT, MAXIMUM_HEIGHT);
                let Color { red, green, blue, alpha } = entry.color;
                Ok(create_new_entry(height, Color::new(red * alpha, green * alpha, blue * alpha, alpha)))
            })
            .collect::<Result<_, _>>()?;

        let mut sorted_ascending = true;
        let mut sorted_descending = true;
        for pair in entries.windows(2) {
            sorted_ascending = sorted_ascending && pair[0].height <= pair[1].height;
            sorted_descending = sorted_descending && pair[0].height >= pair[1].height;
        }
        if sorted_descending {
            entries.reverse();
        } else if !sorted_ascending {
            // Stable sort from lowest to greatest height.
            entries.sort_by(|a, b| a.height.partial_cmp(&b.height).unwrap_or(Ordering::Equal));
        }

        let mut extend_downwards = layer.extend_downwards;
        let mut extend_upwards = layer.extend_upwards;
        if entries.len() == 1 && !extend_downwards && !extend_upwards {
            extend_downwards = true;
            extend_upwards = true;
        }
        if extend_downwards {
            let color = entries[0].color;
            entries.insert(0, create_new_entry(MINIMUM_HEIGHT, color));
        }
        if extend_upwards {
            let color = entries.last().unwrap().color;
            entries.push(create_new_entry(MAXIMUM_HEIGHT, color));
        }
        layered.push(remove_duplicates(entries));
    }
    Ok(layered)
}

fn create_layered_entries(layers: &[ElevationBandLayer]) -> Result<Vec<ElevationBandEntry>, RuntimeError> {
    let layered_entries = preprocess(layers)?;
    let mut entries_accum_next: Vec<ElevationBandEntry> = Vec::new();
    let mut color_above = Color::TRANSPARENT;
    let mut color_below = Color::TRANSPARENT;

    for entries in layered_entries {
        let entries_length = entries.len();
        let mut idx = 0usize;
        let mut accum_idx = 0usize;
        let entries_accum_curr = std::mem::take(&mut entries_accum_next);
        let entries_accum_length = entries_accum_curr.len();

        let add_entry = |height: f64, color: Color, out: &mut Vec<ElevationBandEntry>| {
            out.push(create_new_entry(height, color));
        };

        while idx < entries_length || accum_idx < entries_accum_length {
            let entry = entries.get(idx).copied();
            let prev_entry = idx.checked_sub(1).and_then(|i| entries.get(i)).copied();
            let next_entry = entries.get(idx + 1).copied();
            let entry_accum = entries_accum_curr.get(accum_idx).copied();
            let prev_entry_accum = accum_idx.checked_sub(1).and_then(|i| entries_accum_curr.get(i)).copied();
            let next_entry_accum = entries_accum_curr.get(accum_idx + 1).copied();

            if let (Some(entry), Some(entry_accum)) = (entry, entry_accum) {
                if entry.height == entry_accum.height {
                    let is_split_accum =
                        next_entry_accum.is_some_and(|next| entry_accum.height == next.height);
                    let is_start_accum = prev_entry_accum.is_none();
                    let is_end_accum = next_entry_accum.is_none();
                    let is_split = next_entry.is_some_and(|next| entry.height == next.height);
                    let is_start = prev_entry.is_none();
                    let is_end = next_entry.is_none();

                    if is_split_accum {
                        if is_split {
                            let (a, b) = (entry.color, next_entry.unwrap().color);
                            add_blend_entry(entry.height, a, entry_accum.color, &mut entries_accum_next);
                            add_blend_entry(
                                entry.height,
                                b,
                                next_entry_accum.unwrap().color,
                                &mut entries_accum_next,
                            );
                        } else if is_start {
                            add_entry(entry.height, entry_accum.color, &mut entries_accum_next);
                            add_blend_entry(
                                entry.height,
                                entry.color,
                                next_entry_accum.unwrap().color,
                                &mut entries_accum_next,
                            );
                        } else if is_end {
                            add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                            add_entry(
                                entry.height,
                                next_entry_accum.unwrap().color,
                                &mut entries_accum_next,
                            );
                        } else {
                            add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                            add_blend_entry(
                                entry.height,
                                entry.color,
                                next_entry_accum.unwrap().color,
                                &mut entries_accum_next,
                            );
                        }
                    } else if is_start_accum {
                        if is_split {
                            add_entry(entry.height, entry.color, &mut entries_accum_next);
                            add_blend_entry(
                                entry.height,
                                next_entry.unwrap().color,
                                entry_accum.color,
                                &mut entries_accum_next,
                            );
                        } else if is_end {
                            add_entry(entry.height, entry.color, &mut entries_accum_next);
                            add_entry(entry.height, entry_accum.color, &mut entries_accum_next);
                        } else if is_start {
                            add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                        } else {
                            add_entry(entry.height, entry.color, &mut entries_accum_next);
                            add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                        }
                    } else if is_end_accum {
                        if is_split {
                            add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                            add_entry(
                                entry.height,
                                next_entry.unwrap().color,
                                &mut entries_accum_next,
                            );
                        } else if is_start {
                            add_entry(entry.height, entry_accum.color, &mut entries_accum_next);
                            add_entry(entry.height, entry.color, &mut entries_accum_next);
                        } else if is_end {
                            add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                        } else {
                            add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                            add_entry(entry.height, entry.color, &mut entries_accum_next);
                        }
                    } else if is_split {
                        add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                        add_blend_entry(
                            entry.height,
                            next_entry.unwrap().color,
                            entry_accum.color,
                            &mut entries_accum_next,
                        );
                    } else if is_start {
                        add_entry(entry.height, entry_accum.color, &mut entries_accum_next);
                        add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                    } else if is_end {
                        add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                        add_entry(entry.height, entry_accum.color, &mut entries_accum_next);
                    } else {
                        add_blend_entry(entry.height, entry.color, entry_accum.color, &mut entries_accum_next);
                    }
                    idx += if is_split { 2 } else { 1 };
                    accum_idx += if is_split_accum { 2 } else { 1 };
                    continue;
                }
            }

            if let (Some(entry), Some(entry_accum), Some(prev_accum)) = (entry, entry_accum, prev_entry_accum) {
                if entry.height < entry_accum.height {
                    // New entry between two accum entries.
                    lerp_entry_color(entry.height, &prev_accum, &entry_accum, &mut color_below);
                    if prev_entry.is_none() {
                        add_entry(entry.height, color_below, &mut entries_accum_next);
                        add_blend_entry(entry.height, entry.color, color_below, &mut entries_accum_next);
                    } else if next_entry.is_none() {
                        add_blend_entry(entry.height, entry.color, color_below, &mut entries_accum_next);
                        add_entry(entry.height, color_below, &mut entries_accum_next);
                    } else {
                        add_blend_entry(entry.height, entry.color, color_below, &mut entries_accum_next);
                    }
                    idx += 1;
                    continue;
                }
            }

            if let (Some(entry), Some(entry_accum), Some(prev_entry)) = (entry, entry_accum, prev_entry) {
                if entry_accum.height < entry.height {
                    // Accum entry between two new entries.
                    lerp_entry_color(entry_accum.height, &prev_entry, &entry, &mut color_above);
                    if prev_entry_accum.is_none() {
                        add_entry(entry_accum.height, color_above, &mut entries_accum_next);
                        add_blend_entry(entry_accum.height, color_above, entry_accum.color, &mut entries_accum_next);
                    } else if next_entry_accum.is_none() {
                        add_blend_entry(entry_accum.height, color_above, entry_accum.color, &mut entries_accum_next);
                        add_entry(entry_accum.height, color_above, &mut entries_accum_next);
                    } else {
                        add_blend_entry(entry_accum.height, color_above, entry_accum.color, &mut entries_accum_next);
                    }
                    accum_idx += 1;
                    continue;
                }
            }

            if let Some(entry) = entry.filter(|entry| entry_accum.is_none_or(|accum| entry.height < accum.height)) {
                match (entry_accum, prev_entry_accum, prev_entry, next_entry) {
                    (Some(accum), None, _, None) => {
                        // Insert a blank gap between the last entry and the first accum entry.
                        add_entry(entry.height, entry.color, &mut entries_accum_next);
                        add_entry(entry.height, Color::TRANSPARENT, &mut entries_accum_next);
                        add_entry(accum.height, Color::TRANSPARENT, &mut entries_accum_next);
                    }
                    (None, Some(prev_accum), None, _) => {
                        // Insert a blank gap between the last accum entry and the first entry.
                        add_entry(prev_accum.height, Color::TRANSPARENT, &mut entries_accum_next);
                        add_entry(entry.height, Color::TRANSPARENT, &mut entries_accum_next);
                        add_entry(entry.height, entry.color, &mut entries_accum_next);
                    }
                    _ => {
                        add_entry(entry.height, entry.color, &mut entries_accum_next);
                    }
                }
                idx += 1;
                continue;
            }

            if let Some(entry_accum) = entry_accum.filter(|accum| entry.is_none_or(|entry| accum.height < entry.height)) {
                add_entry(entry_accum.height, entry_accum.color, &mut entries_accum_next);
                accum_idx += 1;
                continue;
            }

            // The JavaScript loop cannot stall, but guard against malformed
            // input rather than spinning forever.
            return Err(RuntimeError::new("Unable to combine elevation band entries."));
        }

        // Keep the scratch colors alive for the next layer's interpolation.
        let _ = (&color_above, &color_below);
    }

    Ok(remove_duplicates(entries_accum_next))
}

fn add_blend_entry(height: f64, top: Color, bottom: Color, out: &mut Vec<ElevationBandEntry>) {
    let result = bottom.multiply_by_scalar(1.0 - top.alpha).add(&top);
    out.push(create_new_entry(height, result));
}

/// `createElevationBandMaterial._useFloatTexture` plus both packing paths.
pub fn elevation_band_texture_data(
    layers: &[ElevationBandLayer],
    floating_point_texture: bool,
    webgl2: bool,
) -> Result<ElevationBandTextureData, RuntimeError> {
    if layers.is_empty() {
        return Err(RuntimeError::new("options.layers.length must be greater than 0."));
    }
    let entries = create_layered_entries(layers)?;
    let length = entries.len();
    let mut height_bytes = Vec::new();
    let (height_pixel_format, height_pixel_datatype) = if !floating_point_texture {
        height_bytes.reserve(length * 4);
        for entry in &entries {
            let packed = Cartesian4::pack_float(entry.height);
            height_bytes.extend([
                packed.x as i64 as u8,
                packed.y as i64 as u8,
                packed.z as i64 as u8,
                packed.w as i64 as u8,
            ]);
        }
        (PixelFormat::Rgba, PixelDatatype::UnsignedByte)
    } else {
        height_bytes.reserve(length * 4);
        for entry in &entries {
            height_bytes.extend((entry.height as f32).to_le_bytes());
        }
        (if webgl2 { PixelFormat::Red } else { PixelFormat::Luminance }, PixelDatatype::Float)
    };
    let mut color_bytes = Vec::with_capacity(length * 4);
    for entry in &entries {
        color_bytes.extend(entry.color.to_bytes().map(|channel| channel as u8));
    }
    Ok(ElevationBandTextureData {
        entries,
        height_pixel_format,
        height_pixel_datatype,
        height_bytes,
        color_bytes,
    })
}

type ElevationBandTextures<G> = (Rc<Texture<G>>, Rc<Texture<G>>);

fn upload_textures<G: Gl + 'static>(
    context: &Context<G>,
    data: &ElevationBandTextureData,
) -> Result<ElevationBandTextures<G>, RuntimeError> {
    let width = u32::try_from(data.entries.len())
        .map_err(|_| RuntimeError::new("Too many elevation band entries."))?
        .max(1);
    let mut height_options = TextureOptions {
        width,
        height: 1,
        pixel_format: data.height_pixel_format,
        pixel_datatype: data.height_pixel_datatype,
        flip_y: false,
        sampler: Sampler::NEAREST,
        pre_multiply_alpha: false,
        skip_color_space_conversion: false,
        id: None,
    };
    height_options.sampler.wrap_r = crate::renderer::TextureWrap::ClampToEdge;
    let heights = Rc::new(Texture::new(context, height_options, Some(&data.height_bytes))?);
    let colors = Rc::new(Texture::new(
        context,
        TextureOptions {
            width,
            height: 1,
            pixel_format: PixelFormat::Rgba,
            pixel_datatype: PixelDatatype::UnsignedByte,
            flip_y: false,
            sampler: Sampler::default(),
            pre_multiply_alpha: false,
            skip_color_space_conversion: false,
            id: None,
        },
        Some(&data.color_bytes),
    )?);
    Ok((heights, colors))
}

/// Creates the `ElevationBand` material from layered height/color bands.
///
/// `layers` are ordered from lowest to highest precedence.
pub fn create_elevation_band_material<G: Gl + 'static>(
    context: &Context<G>,
    layers: &[ElevationBandLayer],
) -> Result<Material, RuntimeError> {
    if layers.is_empty() {
        return Err(RuntimeError::new("options.layers.length must be greater than 0."));
    }
    let capabilities = context.capabilities()?;
    let data = elevation_band_texture_data(layers, capabilities.floating_point_texture, context.webgl2())?;
    let (heights, colors) = upload_textures(context, &data)?;
    let mut material = Material::from_type(ELEVATION_BAND_TYPE);
    material.set_gpu_texture("heights", heights);
    material.set_gpu_texture("colors", colors);
    Ok(material)
}
