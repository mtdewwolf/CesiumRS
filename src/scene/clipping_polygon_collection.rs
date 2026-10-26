//! CesiumJS `ClippingPolygonCollection`: ordered clipping polygons, their
//! spherical extents, packed texture data and volume intersection tests.
//!
//! The signed-distance compute pass is the remaining renderer-side piece: the
//! port produces the packed polygon and extents arrays and the exact
//! resolution Cesium allocates, and the host performs the upload.

use super::ClippingPolygon;
use crate::core::{math::fast_approximate_atan2, Event, Intersect, Rectangle, RuntimeError};
use std::collections::HashMap;

/// `new ClippingPolygonCollection(options)`.
#[derive(Clone, Debug, Default)]
pub struct ClippingPolygonCollectionOptions {
    pub polygons: Vec<ClippingPolygon>,
    pub enabled: Option<bool>,
    pub inverse: Option<bool>,
}

/// An added/removed polygon notification.
#[derive(Debug, PartialEq)]
pub struct ClippingPolygonEvent {
    pub polygon: ClippingPolygon,
    pub index: usize,
}

impl Clone for ClippingPolygonEvent {
    fn clone(&self) -> Self {
        Self { polygon: self.polygon.clone_with_id(), index: self.index }
    }
}

/// The packed float data for a clipping polygon texture pair.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackedClippingPolygons {
    /// RG float pairs: position count, extents index, then (latitude,
    /// longitude) per position.
    pub polygons: Vec<f32>,
    /// RGBA extents: south, west, inverse latitude range, inverse longitude
    /// range.
    pub extents: Vec<f32>,
}

#[derive(Debug)]
pub struct ClippingPolygonCollection {
    polygons: Vec<ClippingPolygon>,
    total_positions: usize,
    extents_count: usize,
    /// If true, clipping is enabled.
    pub enabled: bool,
    /// If true, regions outside every polygon are clipped.
    pub inverse: bool,
    /// Raised when a polygon is added.
    pub polygon_added: Event<ClippingPolygonEvent>,
    /// Raised when a polygon is removed.
    pub polygon_removed: Event<ClippingPolygonEvent>,
    owner: Option<usize>,
    destroyed: bool,
    last_packed: PackedClippingPolygons,
}

impl Default for ClippingPolygonCollection {
    fn default() -> Self {
        Self {
            polygons: Vec::new(),
            total_positions: 0,
            extents_count: 0,
            enabled: true,
            inverse: false,
            polygon_added: Event::new(),
            polygon_removed: Event::new(),
            owner: None,
            destroyed: false,
            last_packed: PackedClippingPolygons::default(),
        }
    }
}

impl ClippingPolygonCollection {
    pub fn new(options: ClippingPolygonCollectionOptions) -> Self {
        Self {
            polygons: options.polygons,
            enabled: options.enabled.unwrap_or(true),
            inverse: options.inverse.unwrap_or(false),
            ..Self::default()
        }
    }

    pub fn length(&self) -> usize {
        self.polygons.len()
    }
    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }
    /// The total position count from the last `update`/`pack`, as in Cesium.
    pub fn total_positions(&self) -> usize {
        self.total_positions
    }
    pub fn extents_count(&self) -> usize {
        self.extents_count
    }
    /// `pixelsNeededForExtents`; an RGBA texture stores one extent per pixel.
    pub fn pixels_needed_for_extents(&self) -> usize {
        self.length()
    }
    /// `pixelsNeededForPolygonPositions`.
    pub fn pixels_needed_for_polygon_positions(&self) -> usize {
        self.total_positions + self.length()
    }
    /// `clippingPolygonsState`: the sign encodes `inverse`.
    pub fn clipping_polygons_state(&self) -> isize {
        let count = self.extents_count as isize;
        if self.inverse {
            -count
        } else {
            count
        }
    }
    pub fn owner(&self) -> Option<usize> {
        self.owner
    }
    /// `ClippingPolygonCollection.setOwner`; a collection has one owner.
    pub fn assign_owner(&mut self, owner: usize) -> Result<(), RuntimeError> {
        match self.owner {
            Some(existing) if existing != owner => {
                Err(RuntimeError::new("ClippingPolygonCollection should only be assigned to one object"))
            }
            _ => {
                self.owner = Some(owner);
                Ok(())
            }
        }
    }
    pub fn polygons(&self) -> &[ClippingPolygon] {
        &self.polygons
    }
    pub fn get(&self, index: usize) -> Option<&ClippingPolygon> {
        self.polygons.get(index)
    }

    pub fn add(&mut self, polygon: ClippingPolygon) -> ClippingPolygon {
        let index = self.polygons.len();
        let event = ClippingPolygonEvent { polygon: polygon.clone_with_id(), index };
        self.polygons.push(polygon);
        self.polygon_added.raise_event(&event);
        self.polygons.last().map(ClippingPolygon::clone_with_id).expect("just pushed")
    }

    pub fn contains(&self, polygon: &ClippingPolygon) -> bool {
        self.polygons.iter().any(|p| p.equals(polygon))
    }

    pub fn remove(&mut self, polygon: &ClippingPolygon) -> bool {
        let Some(index) = self.polygons.iter().position(|p| p.equals(polygon)) else {
            return false;
        };
        let removed = self.polygons.remove(index);
        self.polygon_removed.raise_event(&ClippingPolygonEvent { polygon: removed, index });
        true
    }

    pub fn remove_all(&mut self) {
        for (index, polygon) in std::mem::take(&mut self.polygons).into_iter().enumerate() {
            self.polygon_removed.raise_event(&ClippingPolygonEvent { polygon, index });
        }
    }

    /// `getExtents`: merge overlapping padded spherical extents. Returns the
    /// extents and the extent index for each polygon.
    pub fn extents(&self) -> (Vec<Rectangle>, HashMap<usize, usize>) {
        let mut extents_list: Vec<Option<Rectangle>> = Vec::new();
        let mut polygon_indices_list: Vec<Option<Vec<usize>>> = Vec::new();

        for (polygon_index, polygon) in self.polygons.iter().enumerate() {
            let extents = polygon.compute_spherical_extents();
            let mut polygon_indices = vec![polygon_index];
            let mut padded = pad_extents(extents, &[]);
            let mut merge_rectangle = extents;

            let mut i = 0;
            while i < extents_list.len() {
                let intersects = extents_list[i]
                    .is_some_and(|e| Rectangle::simple_intersection(&e, &padded).is_some() && !e.equals(&padded));
                if intersects {
                    let existing = extents_list[i].take().expect("checked");
                    let indices = polygon_indices_list[i].take().expect("checked");
                    merge_rectangle = Rectangle::union(&merge_rectangle, &existing);
                    padded = pad_extents(merge_rectangle, &[]);
                    polygon_indices.extend(indices);
                    i = 0;
                    continue;
                }
                i += 1;
            }
            extents_list.push(Some(merge_rectangle));
            polygon_indices_list.push(Some(polygon_indices));
        }

        let mut extents = Vec::new();
        let mut extents_index_by_polygon = HashMap::new();
        for (extent, indices) in extents_list.into_iter().zip(polygon_indices_list) {
            let (Some(extent), Some(indices)) = (extent, indices) else {
                continue;
            };
            let target = extents.len();
            extents.push(extent);
            for polygon_index in &indices {
                extents_index_by_polygon.insert(*polygon_index, target);
            }
        }
        (extents, extents_index_by_polygon)
    }

    /// Pack the polygons and extents, updating `totalPositions` and
    /// `extentsCount`.
    pub fn pack(&mut self) -> PackedClippingPolygons {
        self.total_positions = self.polygons.iter().map(ClippingPolygon::length).sum();
        let (extents_list, extents_index_by_polygon) = self.extents();
        let mut polygons = Vec::with_capacity((self.total_positions + self.polygons.len()) * 2);
        for (polygon_index, polygon) in self.polygons.iter().enumerate() {
            polygons.push(polygon.length() as f32);
            polygons.push(*extents_index_by_polygon.get(&polygon_index).unwrap_or(&0) as f32);
            for point in polygon.positions() {
                let mag_xy = (point.x * point.x + point.y * point.y).sqrt();
                polygons.push(fast_approximate_atan2(mag_xy, point.z) as f32);
                polygons.push(fast_approximate_atan2(point.x, point.y) as f32);
            }
        }
        let mut extents = Vec::with_capacity(extents_list.len() * 4);
        for extents_rectangle in &extents_list {
            extents.push(extents_rectangle.south as f32);
            extents.push(extents_rectangle.west as f32);
            extents.push((1.0 / (extents_rectangle.north - extents_rectangle.south)) as f32);
            extents.push((1.0 / (extents_rectangle.east - extents_rectangle.west)) as f32);
        }
        self.extents_count = extents_list.len();
        self.last_packed = PackedClippingPolygons { polygons, extents };
        self.last_packed.clone()
    }

    /// The most recent packed data.
    pub fn packed(&self) -> &PackedClippingPolygons {
        &self.last_packed
    }

    /// `ClippingPolygonCollection.getTextureResolution`.
    pub fn get_texture_resolution(
        existing: Option<[u32; 2]>,
        pixels_needed: usize,
        maximum_texture_size: u32,
    ) -> [u32; 2] {
        if let Some(size) = existing {
            return size;
        }
        if pixels_needed == 0 {
            return [0, 0];
        }
        let width = (pixels_needed as u32).min(maximum_texture_size);
        let height = (pixels_needed as u32).div_ceil(width);
        [width, height * 2]
    }

    /// `getTextureResolution`. `None` when the collection is empty.
    pub fn texture_resolution(
        &self,
        pixels_needed: usize,
        maximum_texture_size: u32,
        existing: Option<[u32; 2]>,
    ) -> [u32; 2] {
        Self::get_texture_resolution(existing, pixels_needed, maximum_texture_size)
    }

    /// The polygons texture resolution Cesium allocates.
    pub fn polygons_texture_resolution(&self, maximum_texture_size: u32, existing: Option<[u32; 2]>) -> [u32; 2] {
        self.texture_resolution(self.pixels_needed_for_polygon_positions(), maximum_texture_size, existing)
    }
    /// The extents texture resolution Cesium allocates.
    pub fn extents_texture_resolution(&self, maximum_texture_size: u32, existing: Option<[u32; 2]>) -> [u32; 2] {
        self.texture_resolution(self.pixels_needed_for_extents(), maximum_texture_size, existing)
    }
    /// The signed-distance texture resolution: at most 4096².
    pub fn clipping_distance_texture_resolution(&self, maximum_texture_size: u32) -> [u32; 2] {
        let size = maximum_texture_size.min(4096);
        [size, size]
    }

    /// `ClippingPolygonCollection.isSupported(scene)`.
    pub fn is_supported(webgl2: Option<bool>) -> Option<bool> {
        webgl2
    }

    /// `computeIntersectionWithBoundingVolume`. `tile_rectangle` is the
    /// volume's cartographic rectangle; `None` means the volume has no
    /// 2D bounds and must be clipped defensively.
    pub fn compute_intersection_with_bounding_volume(&self, tile_rectangle: Option<Rectangle>) -> Intersect {
        if !self.enabled || self.is_empty() {
            return Intersect::Outside;
        }
        let Some(tile_rectangle) = tile_rectangle else {
            return Intersect::Intersecting;
        };
        let mut intersects = false;
        for polygon in &self.polygons {
            let polygon_rectangle = polygon.compute_rectangle();
            if Rectangle::simple_intersection(&polygon_rectangle, &tile_rectangle).is_some() {
                intersects = true;
                break;
            }
        }
        if intersects {
            Intersect::Intersecting
        } else if self.inverse {
            Intersect::Inside
        } else {
            Intersect::Outside
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    /// Releases ownership; texture handles belong to the host adapter.
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.owner = None;
    }
}

fn pad_extents(extents: Rectangle, _unused: &[f64]) -> Rectangle {
    let height = (extents.height() * 2.5).max(0.001);
    let width = (extents.width() * 2.5).max(0.001);
    let mut padded = extents;
    padded.south -= height;
    padded.west -= width;
    padded.north += height;
    padded.east += width;
    padded.south = padded.south.max(-std::f64::consts::PI);
    padded.west = padded.west.max(-std::f64::consts::PI);
    padded.north = padded.north.min(std::f64::consts::PI);
    padded.east = padded.east.min(std::f64::consts::PI);
    padded
}
