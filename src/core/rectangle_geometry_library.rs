//! CesiumJS `RectangleGeometryLibrary`: the grid layout of a (possibly
//! rotated) rectangle and the position of each grid point on the ellipsoid.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::geographic_projection::GeographicProjection;
use super::js_math::JsMath;
use super::map_projection::MapProjection;
use super::math::{js_max, js_min, PI_OVER_TWO, TWO_PI};
use super::matrix2::Matrix2;
use super::matrix3::Matrix3;
use super::rectangle::Rectangle;

/// The rotated texture grid, when the texture has its own rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StGrid {
    pub gran_y_cos: f64,
    pub gran_x_cos: f64,
    pub gran_y_sin: f64,
    pub gran_x_sin: f64,
    pub nw_corner: Cartographic,
    pub west: f64,
    pub south: f64,
}

/// Cesium's `computedOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleComputedOptions {
    pub gran_y_cos: f64,
    pub gran_y_sin: f64,
    pub gran_x_cos: f64,
    pub gran_x_sin: f64,
    /// The (rotated) northwest corner the grid starts from.
    pub nw_corner: Cartographic,
    /// The rectangle's bounds after rotation.
    pub bounding_rectangle: Rectangle,
    /// Grid points per row and per column.
    pub width: usize,
    pub height: usize,
    pub north_cap: bool,
    pub south_cap: bool,
    pub st: Option<StGrid>,
    /// Set by the geometries before computing texture coordinates.
    pub lon_scalar: f64,
    pub lat_scalar: f64,
    pub tangent_rotation_matrix: Matrix3,
}

/// The position of grid point (`row`, `col`), and its texture coordinate
/// when `compute_st`.
pub fn compute_position(
    options: &RectangleComputedOptions,
    ellipsoid: &Ellipsoid,
    compute_st: bool,
    row: f64,
    col: f64,
) -> (Cartesian3, Cartesian2) {
    let radii_squared = ellipsoid.radii_squared;
    let nw = options.nw_corner;
    let rectangle = options.bounding_rectangle;
    let mut st_latitude = nw.latitude - options.gran_y_cos * row + col * options.gran_x_sin;
    let cos_latitude = st_latitude.js_cos();
    let n_z = st_latitude.js_sin();
    let k_z = radii_squared.z * n_z;
    let mut st_longitude = nw.longitude + row * options.gran_y_sin + col * options.gran_x_cos;
    let n_x = cos_latitude * st_longitude.js_cos();
    let n_y = cos_latitude * st_longitude.js_sin();
    let k_x = radii_squared.x * n_x;
    let k_y = radii_squared.y * n_y;
    let gamma = (k_x * n_x + k_y * n_y + k_z * n_z).sqrt();
    let position = Cartesian3::new(k_x / gamma, k_y / gamma, k_z / gamma);
    let mut st = Cartesian2::ZERO;
    if compute_st {
        if let Some(g) = options.st {
            st_latitude = g.nw_corner.latitude - g.gran_y_cos * row + col * g.gran_x_sin;
            st_longitude = g.nw_corner.longitude + row * g.gran_y_sin + col * g.gran_x_cos;
            st = Cartesian2::new((st_longitude - g.west) * options.lon_scalar, (st_latitude - g.south) * options.lat_scalar);
        } else {
            st = Cartesian2::new(
                (st_longitude - rectangle.west) * options.lon_scalar,
                (st_latitude - rectangle.south) * options.lat_scalar,
            );
        }
    }
    (position, st)
}

struct Rotation {
    north: f64,
    south: f64,
    east: f64,
    west: f64,
    gran_y_cos: f64,
    gran_y_sin: f64,
    gran_x_cos: f64,
    gran_x_sin: f64,
    nw_corner: Cartographic,
}

#[allow(clippy::too_many_arguments)]
fn rotation_options(
    nw_corner: Cartographic,
    rotation: f64,
    granularity_x: f64,
    granularity_y: f64,
    center_cartesian: Cartesian3,
    width: usize,
    height: usize,
) -> Rotation {
    let cos_rotation = rotation.js_cos();
    let gran_y_cos = granularity_y * cos_rotation;
    let gran_x_cos = granularity_x * cos_rotation;
    let sin_rotation = rotation.js_sin();
    let gran_y_sin = granularity_y * sin_rotation;
    let gran_x_sin = granularity_x * sin_rotation;

    // Rotate the corner about the center in Cesium's default projection;
    // Matrix2 only touches x and y.
    let projection = GeographicProjection::new(Ellipsoid::WGS84);
    let nw = projection.project(&nw_corner).subtract(center_cartesian);
    let xy = Matrix2::from_rotation(rotation).multiply_by_vector(Cartesian2::new(nw.x, nw.y));
    let nw = Cartesian3::new(xy.x, xy.y, nw.z).add(center_cartesian);
    let nw_corner = projection.unproject(nw);

    let width = width as f64 - 1.0;
    let height = height as f64 - 1.0;
    let latitude = nw_corner.latitude;
    let latitude0 = latitude + width * gran_x_sin;
    let latitude1 = latitude - gran_y_cos * height;
    let latitude2 = latitude - gran_y_cos * height + width * gran_x_sin;
    let longitude = nw_corner.longitude;
    let longitude0 = longitude + width * gran_x_cos;
    let longitude1 = longitude + height * gran_y_sin;
    let longitude2 = longitude + height * gran_y_sin + width * gran_x_cos;
    let max4 = |a: f64, b: f64, c: f64, d: f64| js_max(js_max(js_max(a, b), c), d);
    let min4 = |a: f64, b: f64, c: f64, d: f64| js_min(js_min(js_min(a, b), c), d);
    Rotation {
        north: max4(latitude, latitude0, latitude1, latitude2),
        south: min4(latitude, latitude0, latitude1, latitude2),
        east: max4(longitude, longitude0, longitude1, longitude2),
        west: min4(longitude, longitude0, longitude1, longitude2),
        gran_y_cos,
        gran_y_sin,
        gran_x_cos,
        gran_x_sin,
        nw_corner,
    }
}

/// A grid count from Cesium's `Math.ceil(d / granularity) + 1`.
fn grid_count(d: f64, granularity: f64) -> usize {
    let n = (d / granularity).ceil() + 1.0;
    if n.is_finite() && n > 0.0 {
        n as usize
    } else {
        0
    }
}

/// The grid for `rectangle` at `granularity`, rotated by `rotation`, with
/// texture coordinates rotated by `st_rotation`.
pub fn compute_options(
    rectangle: &Rectangle,
    granularity: f64,
    rotation: f64,
    st_rotation: f64,
) -> RectangleComputedOptions {
    let (east, west, north, south) = (rectangle.east, rectangle.west, rectangle.north, rectangle.south);
    let north_cap = north == PI_OVER_TWO;
    let south_cap = south == -PI_OVER_TWO;
    let dy = north - south;
    let dx = if west > east { TWO_PI - west + east } else { east - west };
    let width = grid_count(dx, granularity);
    let height = grid_count(dy, granularity);
    let granularity_x = dx / (width as f64 - 1.0);
    let granularity_y = dy / (height as f64 - 1.0);
    let nw_corner = rectangle.northwest();
    let mut center = rectangle.center();
    let mut center_cartesian = Cartesian3::ZERO;
    if rotation != 0.0 || st_rotation != 0.0 {
        if center.longitude < nw_corner.longitude {
            center.longitude += TWO_PI;
        }
        center_cartesian = GeographicProjection::new(Ellipsoid::WGS84).project(&center);
    }

    let mut options = RectangleComputedOptions {
        gran_y_cos: granularity_y,
        gran_y_sin: 0.0,
        gran_x_cos: granularity_x,
        gran_x_sin: 0.0,
        nw_corner,
        bounding_rectangle: *rectangle,
        width,
        height,
        north_cap,
        south_cap,
        st: None,
        lon_scalar: 0.0,
        lat_scalar: 0.0,
        tangent_rotation_matrix: Matrix3::IDENTITY,
    };
    if rotation != 0.0 {
        let r = rotation_options(nw_corner, rotation, granularity_x, granularity_y, center_cartesian, width, height);
        options.gran_y_cos = r.gran_y_cos;
        options.gran_y_sin = r.gran_y_sin;
        options.gran_x_cos = r.gran_x_cos;
        options.gran_x_sin = r.gran_x_sin;
        options.nw_corner = r.nw_corner;
        options.bounding_rectangle = Rectangle::new(r.west, r.south, r.east, r.north);
    }
    if st_rotation != 0.0 {
        let st_nw = options.bounding_rectangle.northwest();
        let r = rotation_options(
            st_nw,
            rotation - st_rotation,
            granularity_x,
            granularity_y,
            center_cartesian,
            width,
            height,
        );
        options.st = Some(StGrid {
            gran_y_cos: r.gran_y_cos,
            gran_x_cos: r.gran_x_cos,
            gran_y_sin: r.gran_y_sin,
            gran_x_sin: r.gran_x_sin,
            nw_corner: r.nw_corner,
            west: r.west,
            south: r.south,
        });
    }
    options
}
