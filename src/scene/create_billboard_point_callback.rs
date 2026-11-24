//! CesiumJS `createBillboardPointCallback`: a canvas with a filled point,
//! optionally ringed by an outline and punched through the middle.
//!
//! The pinned source draws with the browser Canvas 2D API. There is no canvas
//! in a native host, so the callback produces an RGBA8 image with the same
//! drawing commands applied in the same order: an outer disk in the outline
//! colour, an optional `destination-out` erase of the inner disk, then the
//! point colour composited over the result. Coverage is sampled 8x8 per pixel;
//! the browser's own antialiasing is implementation-defined and is not
//! reproduced exactly. Fully covered and fully transparent pixels match the
//! source geometry and compositing.

use super::BillboardImage;
use crate::{core::Color, renderer::Gl};

/// The canvas drawn by the returned callback. Sizes are in pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BillboardPointCanvas {
    pub width: u32,
    pub height: u32,
    /// RGBA8 rows from the top of the canvas, as `getImageData` returns them.
    pub rgba: Vec<u8>,
}

impl BillboardPointCanvas {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba[i..i + 4].try_into().expect("four channels")
    }
}

/// JavaScript's `unsigned long` conversion for `canvas.width = length`:
/// truncate toward zero, then take the result modulo 2^32.
fn js_uint32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.trunc().rem_euclid(4_294_967_296.0) as u32
}

/// Cesium's `Color` string parse, or the canvas default `#000000` when the
/// string is not a supported CSS colour.
fn canvas_color(css: &str) -> Color {
    Color::from_css_color_string(css).unwrap_or(Color::BLACK)
}

fn inside(x: f64, y: f64, radius: f64) -> bool {
    x * x + y * y <= radius * radius
}

/// Fraction of the unit pixel at `(x, y)` inside a centered disk.
fn circle_coverage(x: f64, y: f64, radius: f64) -> f64 {
    const SAMPLES: u32 = 8;
    let far_x = x.abs().max((x + 1.0).abs());
    let far_y = y.abs().max((y + 1.0).abs());
    if inside(far_x, far_y, radius) {
        return 1.0;
    }
    let near_x = x.abs().min((x + 1.0).abs());
    let near_y = y.abs().min((y + 1.0).abs());
    if !inside(near_x, near_y, radius) {
        return 0.0;
    }
    let mut hits = 0u32;
    for row in 0..SAMPLES {
        for col in 0..SAMPLES {
            let sx = x + (f64::from(col) + 0.5) / f64::from(SAMPLES);
            let sy = y + (f64::from(row) + 0.5) / f64::from(SAMPLES);
            hits += u32::from(inside(sx, sy, radius));
        }
    }
    f64::from(hits) / f64::from(SAMPLES * SAMPLES)
}

/// Draws the point canvas exactly as `createBillboardPointCallback`'s returned
/// function does when invoked.
pub fn draw_billboard_point(
    center_alpha: f64,
    css_color: &str,
    css_outline_color: &str,
    css_outline_width: f64,
    pixel_size: f64,
) -> BillboardPointCanvas {
    let fill = canvas_color(css_color);
    let outline = canvas_color(css_outline_color);

    let length = pixel_size + 2.0 * css_outline_width;
    let size = js_uint32(length);
    let center = length * 0.5;
    let inner_radius = pixel_size * 0.5;
    let mut rgba = Vec::with_capacity(size as usize * size as usize * 4);

    for y in 0..size {
        for x in 0..size {
            let dx = f64::from(x) * 1.0 + 0.5 - center;
            let dy = f64::from(y) * 1.0 + 0.5 - center;

            // fillStyle = cssOutlineColor; arc(length/2, length/2, length/2)
            let mut dst = [0.0_f64; 4];
            if css_outline_width != 0.0 {
                let coverage = circle_coverage(dx, dy, center);
                dst = [
                    outline.red,
                    outline.green,
                    outline.blue,
                    outline.alpha * coverage,
                ];
            }

            // destination-out erase of the inner disk.
            if css_outline_width != 0.0 && center_alpha < 1.0 {
                let coverage = circle_coverage(dx, dy, inner_radius);
                dst[3] *= 1.0 - coverage;
            }

            // fillStyle = cssColor; arc(length/2, length/2, pixelSize/2)
            let coverage = circle_coverage(dx, dy, inner_radius);
            let src_alpha = fill.alpha * coverage;
            let dst_alpha = dst[3];
            let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);
            let channel = |src: f64, dst_channel: f64| {
                if out_alpha <= 0.0 {
                    0.0
                } else {
                    (src * src_alpha + dst_channel * dst_alpha * (1.0 - src_alpha)) / out_alpha
                }
            };
            rgba.extend([
                (channel(fill.red, dst[0]) * 255.0).round().clamp(0.0, 255.0) as u8,
                (channel(fill.green, dst[1]) * 255.0).round().clamp(0.0, 255.0) as u8,
                (channel(fill.blue, dst[2]) * 255.0).round().clamp(0.0, 255.0) as u8,
                (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8,
            ]);
        }
    }

    BillboardPointCanvas { width: size, height: size, rgba }
}

/// Creates a callback that returns a billboard image of a point.
///
/// `center_alpha` below one punches the outline out from under the point, as
/// the pinned source does; the point itself is still drawn at its own alpha.
pub fn create_billboard_point_callback<G: Gl + 'static>(
    center_alpha: f64,
    css_color: &str,
    css_outline_color: &str,
    css_outline_width: f64,
    pixel_size: f64,
) -> impl Fn() -> BillboardImage<G> {
    let css_color = css_color.to_string();
    let css_outline_color = css_outline_color.to_string();
    move || {
        let canvas = draw_billboard_point(center_alpha, &css_color, &css_outline_color, css_outline_width, pixel_size);
        BillboardImage::pixels(canvas.width, canvas.height, canvas.rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_size_truncates_like_canvas_width() {
        let canvas = draw_billboard_point(1.0, "#fff", "#000", 2.0, 6.9);
        assert_eq!((canvas.width, canvas.height), (10, 10));
        assert_eq!(canvas.rgba.len(), 10 * 10 * 4);
    }

    #[test]
    fn opaque_point_center_and_outline_ring() {
        let canvas = draw_billboard_point(1.0, "#ff0000", "#0000ff", 2.0, 8.0);
        assert_eq!(canvas.pixel(6, 6), [255, 0, 0, 255]);
        assert_eq!(canvas.pixel(0, 0), [0, 0, 0, 0]);
        // Radius 6: pixel at (0.5, 6.5), fully inside the outline disk but
        // outside the inner radius 4.
        assert_eq!(canvas.pixel(0, 6), [0, 0, 255, 255]);
    }

    #[test]
    fn center_alpha_below_one_erases_outline_under_a_translucent_point() {
        let canvas = draw_billboard_point(0.5, "rgba(255, 0, 0, 0.5)", "#0000ff", 2.0, 8.0);
        let center = canvas.pixel(6, 6);
        assert_eq!(center[0], 255);
        assert_eq!(center[1], 0);
        assert_eq!(center[2], 0);
        assert!((center[3] as i32 - 128).abs() <= 1);
        assert!(canvas.pixel(0, 6)[3] > 200);
    }

    #[test]
    fn unsupported_css_colour_falls_back_to_canvas_black() {
        let canvas = draw_billboard_point(1.0, "not-a-colour", "also-bad", 1.0, 4.0);
        assert_eq!(canvas.pixel(3, 3), [0, 0, 0, 255]);
    }
}
