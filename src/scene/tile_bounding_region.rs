//! CesiumJS `TileBoundingRegion`: a tile bounding volume specified as a
//! longitude/latitude/height region.

use crate::core::intersection_tests::ray_plane;
use crate::core::{BoundingSphere, Cartesian3, Cartographic, Color, Ellipsoid, Intersect, Matrix4};
use crate::core::{OrientedBoundingBox, Plane, Ray, Rectangle, RectangleOutlineGeometry, RectangleOutlineOptions};

use super::scene_mode::SceneMode;
use super::tile_bounding_volume::{DebugVolume, TileBoundingVolume, TileBoundingVolumeFrame};

#[derive(Clone, Debug, PartialEq)]
pub struct TileBoundingRegion {
    pub rectangle: Rectangle,
    pub minimum_height: f64,
    pub maximum_height: f64,
    /// The world coordinates of the southwest corner at height 0, moved onto
    /// the west plane when the tile is north of the equator.
    pub southwest_corner_cartesian: Cartesian3,
    /// The world coordinates of the northeast corner at height 0, moved onto
    /// the east plane when the tile is south of the equator.
    pub northeast_corner_cartesian: Cartesian3,
    pub west_normal: Cartesian3,
    pub south_normal: Cartesian3,
    pub east_normal: Cartesian3,
    pub north_normal: Cartesian3,
    oriented_bounding_box: Option<OrientedBoundingBox>,
    bounding_sphere: Option<BoundingSphere>,
}

/// `new TileBoundingRegion(options)`.
#[derive(Clone, Debug)]
pub struct TileBoundingRegionOptions {
    pub rectangle: Rectangle,
    /// Defaults to 0.
    pub minimum_height: Option<f64>,
    /// Defaults to 0.
    pub maximum_height: Option<f64>,
    /// Defaults to WGS84.
    pub ellipsoid: Option<Ellipsoid>,
    /// Defaults to true.
    pub compute_bounding_volumes: Option<bool>,
}

impl TileBoundingRegionOptions {
    pub fn new(rectangle: Rectangle) -> Self {
        Self { rectangle, minimum_height: None, maximum_height: None, ellipsoid: None, compute_bounding_volumes: None }
    }
}

impl TileBoundingRegion {
    pub fn new(options: TileBoundingRegionOptions) -> Self {
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let mut region = Self {
            rectangle: options.rectangle,
            minimum_height: options.minimum_height.unwrap_or(0.0),
            maximum_height: options.maximum_height.unwrap_or(0.0),
            southwest_corner_cartesian: Cartesian3::ZERO,
            northeast_corner_cartesian: Cartesian3::ZERO,
            west_normal: Cartesian3::ZERO,
            south_normal: Cartesian3::ZERO,
            east_normal: Cartesian3::ZERO,
            north_normal: Cartesian3::ZERO,
            oriented_bounding_box: None,
            bounding_sphere: None,
        };
        region.compute_box(&ellipsoid);
        if options.compute_bounding_volumes.unwrap_or(true) {
            region.compute_bounding_volumes(&ellipsoid);
        }
        region
    }

    /// The oriented box enclosing the region, used for visibility. `None`
    /// until the bounding volumes are computed, or when Cesium's
    /// `OrientedBoundingBox.fromRectangle` would throw.
    pub fn bounding_volume(&self) -> Option<&OrientedBoundingBox> {
        self.oriented_bounding_box.as_ref()
    }

    /// The sphere around [`Self::bounding_volume`].
    pub fn bounding_sphere_opt(&self) -> Option<&BoundingSphere> {
        self.bounding_sphere.as_ref()
    }

    pub fn compute_bounding_volumes(&mut self, ellipsoid: &Ellipsoid) {
        self.oriented_bounding_box =
            OrientedBoundingBox::from_rectangle(&self.rectangle, self.minimum_height, self.maximum_height, ellipsoid);
        self.bounding_sphere = self.oriented_bounding_box.as_ref().map(BoundingSphere::from_oriented_bounding_box);
    }

    fn compute_box(&mut self, ellipsoid: &Ellipsoid) {
        let rectangle = self.rectangle;
        self.southwest_corner_cartesian = ellipsoid.cartographic_to_cartesian(&rectangle.southwest());
        self.northeast_corner_cartesian = ellipsoid.cartographic_to_cartesian(&rectangle.northeast());

        // The middle latitude on the western edge.
        let mut cartographic = Cartographic::new(rectangle.west, (rectangle.south + rectangle.north) * 0.5, 0.0);
        let western_midpoint = ellipsoid.cartographic_to_cartesian(&cartographic);
        let west_normal = western_midpoint.cross(Cartesian3::UNIT_Z);
        self.west_normal = west_normal.normalize();

        // The middle latitude on the eastern edge.
        cartographic.longitude = rectangle.east;
        let eastern_midpoint = ellipsoid.cartographic_to_cartesian(&cartographic);
        self.east_normal = Cartesian3::UNIT_Z.cross(eastern_midpoint).normalize();

        let mut west_vector = western_midpoint.subtract(eastern_midpoint);
        if west_vector.magnitude() == 0.0 {
            west_vector = west_normal;
        }
        let east_west_normal = west_vector.normalize();

        // A southern plane that does not cut through a tile north of the equator.
        let south_surface_normal = if rectangle.south > 0.0 {
            let center = Cartographic::new((rectangle.west + rectangle.east) * 0.5, rectangle.south, 0.0);
            let south_center = ellipsoid.cartographic_to_cartesian(&center);
            let ray = Ray { origin: south_center, direction: east_west_normal };
            let west_plane = Plane::from_point_normal(self.southwest_corner_cartesian, self.west_normal);
            // A point on the west and south planes; Cesium leaves the corner
            // as it was when the ray misses.
            if let Some(point) = ray_plane(&ray, &west_plane) {
                self.southwest_corner_cartesian = point;
            }
            nan_normal(ellipsoid.geodetic_surface_normal(south_center))
        } else {
            ellipsoid.geodetic_surface_normal_cartographic(&rectangle.southeast())
        };
        self.south_normal = south_surface_normal.cross(west_vector).normalize();

        // A northern plane that does not cut through a tile south of the equator.
        let north_surface_normal = if rectangle.north < 0.0 {
            let center = Cartographic::new((rectangle.west + rectangle.east) * 0.5, rectangle.north, 0.0);
            let north_center = ellipsoid.cartographic_to_cartesian(&center);
            let ray = Ray { origin: north_center, direction: east_west_normal.negate() };
            let east_plane = Plane::from_point_normal(self.northeast_corner_cartesian, self.east_normal);
            if let Some(point) = ray_plane(&ray, &east_plane) {
                self.northeast_corner_cartesian = point;
            }
            nan_normal(ellipsoid.geodetic_surface_normal(north_center))
        } else {
            ellipsoid.geodetic_surface_normal_cartographic(&rectangle.northwest())
        };
        self.north_normal = west_vector.cross(north_surface_normal).normalize();
    }

    fn distance_to_camera_region(&self, frame: &TileBoundingVolumeFrame<'_>) -> f64 {
        let camera_cartesian = frame.camera_position_wc;
        let camera_cartographic = frame.camera_position_cartographic;
        let mut result = 0.0;
        if !self.rectangle.contains(&camera_cartographic) {
            let mut southwest = self.southwest_corner_cartesian;
            let mut northeast = self.northeast_corner_cartesian;
            let mut west_normal = self.west_normal;
            let mut south_normal = self.south_normal;
            let mut east_normal = self.east_normal;
            let mut north_normal = self.north_normal;
            if frame.mode != SceneMode::Scene3D {
                let sw = frame.map_projection.project(&self.rectangle.southwest());
                southwest = Cartesian3::new(0.0, sw.x, sw.y);
                let ne = frame.map_projection.project(&self.rectangle.northeast());
                northeast = Cartesian3::new(0.0, ne.x, ne.y);
                west_normal = Cartesian3::new(0.0, -1.0, 0.0);
                east_normal = Cartesian3::UNIT_Y;
                south_normal = Cartesian3::new(0.0, 0.0, -1.0);
                north_normal = Cartesian3::UNIT_Z;
            }

            let from_southwest = camera_cartesian.subtract(southwest);
            let distance_to_west_plane = from_southwest.dot(west_normal);
            let distance_to_south_plane = from_southwest.dot(south_normal);
            let from_northeast = camera_cartesian.subtract(northeast);
            let distance_to_east_plane = from_northeast.dot(east_normal);
            let distance_to_north_plane = from_northeast.dot(north_normal);

            if distance_to_west_plane > 0.0 {
                result += distance_to_west_plane * distance_to_west_plane;
            } else if distance_to_east_plane > 0.0 {
                result += distance_to_east_plane * distance_to_east_plane;
            }
            if distance_to_south_plane > 0.0 {
                result += distance_to_south_plane * distance_to_south_plane;
            } else if distance_to_north_plane > 0.0 {
                result += distance_to_north_plane * distance_to_north_plane;
            }
        }

        let (camera_height, minimum_height, maximum_height) = if frame.mode == SceneMode::Scene3D {
            (camera_cartographic.height, self.minimum_height, self.maximum_height)
        } else {
            (camera_cartesian.x, 0.0, 0.0)
        };
        if camera_height > maximum_height {
            let above = camera_height - maximum_height;
            result += above * above;
        } else if camera_height < minimum_height {
            let below = minimum_height - camera_height;
            result += below * below;
        }
        result.sqrt()
    }
}

/// Cesium's `geodeticSurfaceNormal` result when the point is the center:
/// the scratch is left holding whatever it had, which the port treats as NaN.
fn nan_normal(normal: Option<Cartesian3>) -> Cartesian3 {
    normal.unwrap_or(Cartesian3::new(f64::NAN, f64::NAN, f64::NAN))
}

impl TileBoundingVolume for TileBoundingRegion {
    /// # Panics
    /// When the bounding volumes were not computed.
    fn bounding_sphere(&self) -> &BoundingSphere {
        self.bounding_sphere.as_ref().expect("TileBoundingRegion bounding volumes were not computed")
    }

    /// In 3D, the larger of the region distance and the distance to the
    /// oriented box.
    fn distance_to_camera(&self, frame: &TileBoundingVolumeFrame<'_>) -> f64 {
        let region_result = self.distance_to_camera_region(frame);
        if let (SceneMode::Scene3D, Some(obb)) = (frame.mode, &self.oriented_bounding_box) {
            return region_result.max(obb.distance_squared_to(frame.camera_position_wc).sqrt());
        }
        region_result
    }

    /// # Panics
    /// When the bounding volumes were not computed, where Cesium throws a
    /// `TypeError`.
    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        self.oriented_bounding_box
            .as_ref()
            .expect("TileBoundingRegion bounding volumes were not computed")
            .intersect_plane(plane)
    }

    /// The rectangle outline from the minimum to the maximum height.
    fn create_debug_volume(&self, color: &Color) -> DebugVolume {
        let mut options = RectangleOutlineOptions::new(self.rectangle);
        options.height = self.minimum_height;
        options.extruded_height = Some(self.maximum_height);
        let geometry = RectangleOutlineGeometry::new(options).create_geometry();
        DebugVolume::outlines(vec![geometry], Matrix4::IDENTITY, color)
    }
}
