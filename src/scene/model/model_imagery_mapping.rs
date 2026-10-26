//! CesiumJS `Scene/Model/ModelImageryMapping`: computes projected texture
//! coordinates for primitives draped with imagery.

use crate::core::{BoundingRectangle, Cartesian2, Cartesian3, Cartographic, MapProjection, Rectangle};

pub use super::mapped_positions::MappedPositions;

/// Helper methods for computing imagery texture coordinates.
pub struct ModelImageryMapping;

impl ModelImageryMapping {
    /// Computes the bounding rectangle of the given cartographic positions.
    pub fn compute_cartographic_bounding_rectangle(cartographic_positions: &[Cartographic]) -> Rectangle {
        let mut north = f64::NEG_INFINITY;
        let mut south = f64::INFINITY;
        let mut east = f64::NEG_INFINITY;
        let mut west = f64::INFINITY;

        for pos in cartographic_positions {
            north = north.max(pos.latitude);
            south = south.min(pos.latitude);
            east = east.max(pos.longitude);
            west = west.min(pos.longitude);
        }

        Rectangle::new(west, south, east, north)
    }

    /// Computes the texture coordinates for the given positions, relative to the given bounding rectangle.
    pub fn compute_tex_coords(positions: &[Cartesian3], bounding_rectangle: &BoundingRectangle) -> Vec<Cartesian2> {
        let inv_size_x = 1.0 / bounding_rectangle.width;
        let inv_size_y = 1.0 / bounding_rectangle.height;

        positions
            .iter()
            .map(|p| {
                let u_raw = (p.x - bounding_rectangle.x) * inv_size_x;
                let v_raw = (p.y - bounding_rectangle.y) * inv_size_y;
                let u = u_raw.clamp(0.0, 1.0);
                let v = v_raw.clamp(0.0, 1.0);
                Cartesian2::new(u, v)
            })
            .collect()
    }

    /// Creates a flat typed array of `f32` coordinates from Cartesian2 elements.
    pub fn create_typed_array_from_cartesians2(elements: &[Cartesian2]) -> Vec<f32> {
        let mut result = Vec::with_capacity(elements.len() * 2);
        for elem in elements {
            result.push(elem.x as f32);
            result.push(elem.y as f32);
        }
        result
    }

    /// Computes texture coordinates from pre-mapped cartographic positions.
    pub fn create_texture_coordinates_for_mapped_positions(
        mapped_positions: &MappedPositions,
        projection: &dyn MapProjection,
    ) -> Vec<f32> {
        let mut coordinates = Self::create_texture_coordinates(
            mapped_positions.cartographic_positions(),
            mapped_positions.cartographic_bounding_rectangle(),
            projection,
        );
        // Cesium allocates numPositions * 2 entries even when the shared
        // iterable has more positions (excess writes are ignored).
        coordinates.resize(mapped_positions.num_positions() * 2, 0.0);
        coordinates
    }

    /// Computes normalized texture coordinates `[u, v]` for positions projected by `projection`.
    pub fn create_texture_coordinates(
        cartographic_positions: &[Cartographic],
        cartographic_bounding_rectangle: &Rectangle,
        projection: &dyn MapProjection,
    ) -> Vec<f32> {
        let bounding_rectangle = BoundingRectangle::from_rectangle(cartographic_bounding_rectangle, Some(projection));
        let mut uvs = Vec::with_capacity(cartographic_positions.len() * 2);

        let inv_w = if bounding_rectangle.width != 0.0 { 1.0 / bounding_rectangle.width } else { 1.0 };
        let inv_h = if bounding_rectangle.height != 0.0 { 1.0 / bounding_rectangle.height } else { 1.0 };

        for pos in cartographic_positions {
            let projected = projection.project(pos);
            let u = ((projected.x - bounding_rectangle.x) * inv_w).clamp(0.0, 1.0);
            let v = ((projected.y - bounding_rectangle.y) * inv_h).clamp(0.0, 1.0);
            uvs.push(u as f32);
            uvs.push(v as f32);
        }

        uvs
    }
}
