//! CesiumJS `ClippingPolygon`: a geodesic polygon that clips models, tilesets
//! and the globe together with a [`ClippingPlaneCollection`](super::ClippingPlaneCollection).

use crate::core::{
    math::fast_approximate_atan2, ArcType, Cartesian3, Cartographic, Ellipsoid, PolygonGeometry, Rectangle,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_POLYGON_ID: AtomicU64 = AtomicU64::new(1);

/// Options for `new ClippingPolygon(options)`.
#[derive(Clone, Debug)]
pub struct ClippingPolygonOptions {
    /// Three or more positions. Cesium requires this with a `DeveloperError`.
    pub positions: Vec<Cartesian3>,
    /// `Ellipsoid.default` (WGS84) when absent.
    pub ellipsoid: Option<Ellipsoid>,
}

/// A clipping polygon.
///
/// Cesium compares polygons with `positions ===`, i.e. by position-array
/// identity.
#[derive(Debug)]
pub struct ClippingPolygon {
    pub id: u64,
    ellipsoid: Ellipsoid,
    positions: Vec<Cartesian3>,
    cached_positions: RefCell<Option<Vec<Cartesian3>>>,
    cached_rectangle: RefCell<Option<Rectangle>>,
}

impl PartialEq for ClippingPolygon {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl Clone for ClippingPolygon {
    /// `ClippingPolygon.clone(polygon)`: a fresh position array, so the clone
    /// is a deep copy with no cached rectangle.
    fn clone(&self) -> Self {
        Self {
            id: NEXT_POLYGON_ID.fetch_add(1, Ordering::Relaxed),
            ellipsoid: self.ellipsoid,
            positions: self.positions.clone(),
            cached_positions: RefCell::new(None),
            cached_rectangle: RefCell::new(None),
        }
    }
}

impl ClippingPolygon {
    /// Panics when fewer than three positions are supplied, as Cesium's
    /// `DeveloperError` does. Use [`ClippingPolygon::try_new`] for a `Result`.
    pub fn new(options: ClippingPolygonOptions) -> Self {
        Self::try_new(options).expect("a clipping polygon needs at least three positions")
    }

    pub fn try_new(options: ClippingPolygonOptions) -> Result<Self, crate::core::RuntimeError> {
        if options.positions.len() < 3 {
            return Err(crate::core::RuntimeError::new("options.positions.length must be greater than or equal to 3"));
        }
        Ok(Self {
            id: NEXT_POLYGON_ID.fetch_add(1, Ordering::Relaxed),
            ellipsoid: options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
            positions: options.positions,
            cached_positions: RefCell::new(None),
            cached_rectangle: RefCell::new(None),
        })
    }

    /// `length`: the outer ring's position count.
    pub fn length(&self) -> usize {
        self.positions.len()
    }
    pub fn positions(&self) -> &[Cartesian3] {
        &self.positions
    }
    /// Mutable access invalidates the cached rectangle on the next query.
    pub fn positions_mut(&mut self) -> &mut Vec<Cartesian3> {
        &mut self.positions
    }
    pub fn ellipsoid(&self) -> Ellipsoid {
        self.ellipsoid
    }

    /// `ClippingPolygon.equals`: identity equality matching Cesium's positions === check.
    pub fn equals(&self, other: &Self) -> bool {
        self.id == other.id
    }

    /// Creates a clone sharing the same identity, used when checking
    /// collection containment or removing an element obtained from the collection.
    pub fn clone_with_id(&self) -> Self {
        Self {
            id: self.id,
            ellipsoid: self.ellipsoid,
            positions: self.positions.clone(),
            cached_positions: RefCell::new(self.cached_positions.borrow().clone()),
            cached_rectangle: RefCell::new(*self.cached_rectangle.borrow()),
        }
    }

    /// `computeRectangle`: the cartographic rectangle enclosing the polygon,
    /// including antimeridian and pole cases. The result is cached until the
    /// positions change.
    pub fn compute_rectangle(&self) -> Rectangle {
        let cached = self.cached_positions.borrow();
        if let Some(previous) = cached.as_ref() {
            if *previous == self.positions {
                if let Some(rectangle) = *self.cached_rectangle.borrow() {
                    return rectangle;
                }
            }
        }
        drop(cached);
        let rectangle =
            PolygonGeometry::compute_rectangle_from_positions(&self.positions, &self.ellipsoid, ArcType::Geodesic);
        *self.cached_positions.borrow_mut() = Some(self.positions.clone());
        *self.cached_rectangle.borrow_mut() = Some(rectangle);
        rectangle
    }

    /// `computeSphericalExtents`: the shader-aligned spherical extents of the
    /// rectangle.
    pub fn compute_spherical_extents(&self) -> Rectangle {
        let rectangle = self.compute_rectangle();
        let sphere = |cartographic: Cartographic| {
            let point = cartographic.to_cartesian(&self.ellipsoid);
            let mag_xy = (point.x * point.x + point.y * point.y).sqrt();
            let latitude = fast_approximate_atan2(mag_xy, point.z);
            let longitude = fast_approximate_atan2(point.x, point.y);
            (latitude, longitude)
        };
        let south_west = sphere(Cartographic::new(rectangle.west, rectangle.south, 0.0));
        let north_east = sphere(Cartographic::new(rectangle.east, rectangle.north, 0.0));
        Rectangle::new(south_west.1, south_west.0, north_east.1, north_east.0)
    }
}
