//! CesiumJS `TilingScheme` (interface), `GeographicTilingScheme`, and
//! `WebMercatorTilingScheme`: how the globe is cut into a quadtree of tiles.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::geographic_projection::GeographicProjection;
use super::map_projection::MapProjection;
use super::math::{to_degrees, to_int32, TWO_PI};
use super::rectangle::Rectangle;
use super::web_mercator_projection::WebMercatorProjection;

pub trait TilingScheme {
    fn ellipsoid(&self) -> &Ellipsoid;
    /// The covered area, in radians.
    fn rectangle(&self) -> &Rectangle;
    fn projection(&self) -> &dyn MapProjection;
    fn get_number_of_x_tiles_at_level(&self, level: u32) -> i32;
    fn get_number_of_y_tiles_at_level(&self, level: u32) -> i32;
    /// A rectangle in radians converted to the scheme's native coordinates.
    fn rectangle_to_native_rectangle(&self, rectangle: &Rectangle) -> Rectangle;
    fn tile_xy_to_native_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle;
    fn tile_xy_to_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle;
    /// The tile containing `position`, or `None` outside the scheme.
    fn position_to_tile_xy(&self, position: &Cartographic, level: u32) -> Option<Cartesian2>;
}

/// JavaScript's `count << level`.
fn shift(count: i32, level: u32) -> i32 {
    count.wrapping_shl(level)
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeographicTilingScheme {
    ellipsoid: Ellipsoid,
    rectangle: Rectangle,
    projection: GeographicProjection,
    number_of_level_zero_tiles_x: i32,
    number_of_level_zero_tiles_y: i32,
}

impl Default for GeographicTilingScheme {
    fn default() -> Self {
        Self::new(Ellipsoid::WGS84, Rectangle::MAX_VALUE, 2, 1)
    }
}

impl GeographicTilingScheme {
    /// Cesium's defaults: WGS84, the whole globe, 2 × 1 tiles at level zero.
    pub fn new(ellipsoid: Ellipsoid, rectangle: Rectangle, level_zero_tiles_x: i32, level_zero_tiles_y: i32) -> Self {
        Self {
            ellipsoid,
            rectangle,
            projection: GeographicProjection::new(ellipsoid),
            number_of_level_zero_tiles_x: level_zero_tiles_x,
            number_of_level_zero_tiles_y: level_zero_tiles_y,
        }
    }
}

impl TilingScheme for GeographicTilingScheme {
    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }

    fn rectangle(&self) -> &Rectangle {
        &self.rectangle
    }

    fn projection(&self) -> &dyn MapProjection {
        &self.projection
    }

    fn get_number_of_x_tiles_at_level(&self, level: u32) -> i32 {
        shift(self.number_of_level_zero_tiles_x, level)
    }

    fn get_number_of_y_tiles_at_level(&self, level: u32) -> i32 {
        shift(self.number_of_level_zero_tiles_y, level)
    }

    /// Degrees.
    fn rectangle_to_native_rectangle(&self, r: &Rectangle) -> Rectangle {
        Rectangle::new(to_degrees(r.west), to_degrees(r.south), to_degrees(r.east), to_degrees(r.north))
    }

    fn tile_xy_to_native_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle {
        self.rectangle_to_native_rectangle(&self.tile_xy_to_rectangle(x, y, level))
    }

    fn tile_xy_to_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle {
        let rectangle = &self.rectangle;
        let x_tiles = f64::from(self.get_number_of_x_tiles_at_level(level));
        let y_tiles = f64::from(self.get_number_of_y_tiles_at_level(level));
        let x_tile_width = rectangle.width() / x_tiles;
        let west = x * x_tile_width + rectangle.west;
        let east = (x + 1.0) * x_tile_width + rectangle.west;
        let y_tile_height = rectangle.height() / y_tiles;
        let north = rectangle.north - y * y_tile_height;
        let south = rectangle.north - (y + 1.0) * y_tile_height;
        Rectangle::new(west, south, east, north)
    }

    fn position_to_tile_xy(&self, position: &Cartographic, level: u32) -> Option<Cartesian2> {
        let rectangle = &self.rectangle;
        if !rectangle.contains(position) {
            return None;
        }
        let x_tiles = self.get_number_of_x_tiles_at_level(level);
        let y_tiles = self.get_number_of_y_tiles_at_level(level);
        let x_tile_width = rectangle.width() / f64::from(x_tiles);
        let y_tile_height = rectangle.height() / f64::from(y_tiles);
        let mut longitude = position.longitude;
        if rectangle.east < rectangle.west {
            longitude += TWO_PI;
        }
        let mut x_tile = to_int32((longitude - rectangle.west) / x_tile_width);
        if x_tile >= x_tiles {
            x_tile = x_tiles - 1;
        }
        let mut y_tile = to_int32((rectangle.north - position.latitude) / y_tile_height);
        if y_tile >= y_tiles {
            y_tile = y_tiles - 1;
        }
        Some(Cartesian2::new(f64::from(x_tile), f64::from(y_tile)))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebMercatorTilingScheme {
    ellipsoid: Ellipsoid,
    projection: WebMercatorProjection,
    number_of_level_zero_tiles_x: i32,
    number_of_level_zero_tiles_y: i32,
    rectangle_southwest_in_meters: Cartesian2,
    rectangle_northeast_in_meters: Cartesian2,
    rectangle: Rectangle,
}

impl Default for WebMercatorTilingScheme {
    fn default() -> Self {
        Self::new(Ellipsoid::WGS84, 1, 1, None)
    }
}

impl WebMercatorTilingScheme {
    /// Cesium's defaults: WGS84, 1 × 1 tiles at level zero, and the square of
    /// ±π × the semimajor axis. `corners_in_meters` is (southwest, northeast).
    pub fn new(
        ellipsoid: Ellipsoid,
        level_zero_tiles_x: i32,
        level_zero_tiles_y: i32,
        corners_in_meters: Option<(Cartesian2, Cartesian2)>,
    ) -> Self {
        let projection = WebMercatorProjection::new(ellipsoid);
        let (southwest_m, northeast_m) = corners_in_meters.unwrap_or_else(|| {
            let semimajor_axis_times_pi = ellipsoid.maximum_radius * std::f64::consts::PI;
            (
                Cartesian2::new(-semimajor_axis_times_pi, -semimajor_axis_times_pi),
                Cartesian2::new(semimajor_axis_times_pi, semimajor_axis_times_pi),
            )
        });
        let southwest = projection.unproject(Cartesian3::new(southwest_m.x, southwest_m.y, 0.0));
        let northeast = projection.unproject(Cartesian3::new(northeast_m.x, northeast_m.y, 0.0));
        Self {
            ellipsoid,
            projection,
            number_of_level_zero_tiles_x: level_zero_tiles_x,
            number_of_level_zero_tiles_y: level_zero_tiles_y,
            rectangle_southwest_in_meters: southwest_m,
            rectangle_northeast_in_meters: northeast_m,
            rectangle: Rectangle::new(southwest.longitude, southwest.latitude, northeast.longitude, northeast.latitude),
        }
    }

    pub fn rectangle_southwest_in_meters(&self) -> Cartesian2 {
        self.rectangle_southwest_in_meters
    }

    pub fn rectangle_northeast_in_meters(&self) -> Cartesian2 {
        self.rectangle_northeast_in_meters
    }
}

impl TilingScheme for WebMercatorTilingScheme {
    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }

    fn rectangle(&self) -> &Rectangle {
        &self.rectangle
    }

    fn projection(&self) -> &dyn MapProjection {
        &self.projection
    }

    fn get_number_of_x_tiles_at_level(&self, level: u32) -> i32 {
        shift(self.number_of_level_zero_tiles_x, level)
    }

    fn get_number_of_y_tiles_at_level(&self, level: u32) -> i32 {
        shift(self.number_of_level_zero_tiles_y, level)
    }

    /// Web Mercator meters.
    fn rectangle_to_native_rectangle(&self, rectangle: &Rectangle) -> Rectangle {
        let southwest = self.projection.project(&rectangle.southwest());
        let northeast = self.projection.project(&rectangle.northeast());
        Rectangle::new(southwest.x, southwest.y, northeast.x, northeast.y)
    }

    fn tile_xy_to_native_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle {
        let (sw, ne) = (self.rectangle_southwest_in_meters, self.rectangle_northeast_in_meters);
        let x_tiles = f64::from(self.get_number_of_x_tiles_at_level(level));
        let y_tiles = f64::from(self.get_number_of_y_tiles_at_level(level));
        let x_tile_width = (ne.x - sw.x) / x_tiles;
        let west = sw.x + x * x_tile_width;
        let east = sw.x + (x + 1.0) * x_tile_width;
        let y_tile_height = (ne.y - sw.y) / y_tiles;
        let north = ne.y - y * y_tile_height;
        let south = ne.y - (y + 1.0) * y_tile_height;
        Rectangle::new(west, south, east, north)
    }

    fn tile_xy_to_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle {
        let native = self.tile_xy_to_native_rectangle(x, y, level);
        let southwest = self.projection.unproject(Cartesian3::new(native.west, native.south, 0.0));
        let northeast = self.projection.unproject(Cartesian3::new(native.east, native.north, 0.0));
        Rectangle::new(southwest.longitude, southwest.latitude, northeast.longitude, northeast.latitude)
    }

    fn position_to_tile_xy(&self, position: &Cartographic, level: u32) -> Option<Cartesian2> {
        if !self.rectangle.contains(position) {
            return None;
        }
        let (sw, ne) = (self.rectangle_southwest_in_meters, self.rectangle_northeast_in_meters);
        let x_tiles = self.get_number_of_x_tiles_at_level(level);
        let y_tiles = self.get_number_of_y_tiles_at_level(level);
        let x_tile_width = (ne.x - sw.x) / f64::from(x_tiles);
        let y_tile_height = (ne.y - sw.y) / f64::from(y_tiles);
        let web_mercator_position = self.projection.project(position);
        let distance_from_west = web_mercator_position.x - sw.x;
        let distance_from_north = ne.y - web_mercator_position.y;
        let mut x_tile = to_int32(distance_from_west / x_tile_width);
        if x_tile >= x_tiles {
            x_tile = x_tiles - 1;
        }
        let mut y_tile = to_int32(distance_from_north / y_tile_height);
        if y_tile >= y_tiles {
            y_tile = y_tiles - 1;
        }
        Some(Cartesian2::new(f64::from(x_tile), f64::from(y_tile)))
    }
}
