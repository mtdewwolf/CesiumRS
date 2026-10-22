//! CesiumJS vector tile polyline decoding. The quantized input is decoded in place.

use super::{AttributeCompression, Cartographic, Ellipsoid, Rectangle};

pub struct DecodeVectorPolylineOptions<'a> {
    pub rectangle: &'a Rectangle,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub ellipsoid: &'a Ellipsoid,
}

pub fn decode_vector_polyline_positions(
    positions: &mut [u16],
    options: DecodeVectorPolylineOptions<'_>,
) -> Option<Vec<f64>> {
    let DecodeVectorPolylineOptions { rectangle, minimum_height, maximum_height, ellipsoid } = options;
    if !positions.len().is_multiple_of(3) {
        return None;
    }
    let length = positions.len() / 3;
    let (u, rest) = positions.split_at_mut(length);
    let (v, height) = rest.split_at_mut(length);
    AttributeCompression::zig_zag_delta_decode(u, v, Some(height))?;
    let lerp = |a, b, t| (1.0 - t) * a + t * b;
    let mut result = Vec::with_capacity(length * 3);
    for i in 0..length {
        let c = Cartographic::new(
            lerp(rectangle.west, rectangle.east, u[i] as f64 / 32767.0),
            lerp(rectangle.south, rectangle.north, v[i] as f64 / 32767.0),
            lerp(minimum_height, maximum_height, height[i] as f64 / 32767.0),
        );
        let p = ellipsoid.cartographic_to_cartesian(&c);
        result.extend_from_slice(&[p.x, p.y, p.z]);
    }
    Some(result)
}
