//! CesiumJS `Rectangle`: a cartographic extent in radians. `east < west`
//! means the rectangle crosses the antimeridian.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::math::{equals_epsilon, js_max, js_min, negative_pi_to_pi, to_radians, EPSILON14, PI, PI_OVER_TWO, TWO_PI};
use super::transforms::Transforms;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rectangle {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl Rectangle {
    pub const PACKED_LENGTH: usize = 4;
    /// The whole globe.
    pub const MAX_VALUE: Self = Self::new(-PI, -PI_OVER_TWO, PI, PI_OVER_TWO);

    pub const fn new(west: f64, south: f64, east: f64, north: f64) -> Self {
        Self { west, south, east, north }
    }

    pub const fn from_radians(west: f64, south: f64, east: f64, north: f64) -> Self {
        Self::new(west, south, east, north)
    }

    pub fn from_degrees(west: f64, south: f64, east: f64, north: f64) -> Self {
        Self::new(to_radians(west), to_radians(south), to_radians(east), to_radians(north))
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index..starting_index + 4].copy_from_slice(&[self.west, self.south, self.east, self.north]);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let a = &array[starting_index..starting_index + 4];
        Self::new(a[0], a[1], a[2], a[3])
    }

    /// Width in radians, accounting for the antimeridian.
    pub fn compute_width(&self) -> f64 {
        let mut east = self.east;
        if east < self.west {
            east += TWO_PI;
        }
        east - self.west
    }

    pub fn compute_height(&self) -> f64 {
        self.north - self.south
    }

    pub fn width(&self) -> f64 {
        self.compute_width()
    }

    pub fn height(&self) -> f64 {
        self.compute_height()
    }

    /// Tightest rectangle around the positions, crossing the antimeridian when narrower.
    pub fn from_cartographic_array(cartographics: &[Cartographic]) -> Self {
        Self::from_lon_lat(cartographics.iter().map(|c| (c.longitude, c.latitude)))
    }

    /// Panics if a position is at the center of the ellipsoid, where Cesium throws.
    pub fn from_cartesian_array(cartesians: &[Cartesian3], ellipsoid: &Ellipsoid) -> Self {
        Self::from_lon_lat(cartesians.iter().map(|c| {
            let p = ellipsoid.cartesian_to_cartographic(*c).expect("position is at the center of the ellipsoid");
            (p.longitude, p.latitude)
        }))
    }

    fn from_lon_lat(points: impl Iterator<Item = (f64, f64)>) -> Self {
        let mut west = f64::MAX;
        let mut east = -f64::MAX;
        let mut west_over_idl = f64::MAX;
        let mut east_over_idl = -f64::MAX;
        let mut south = f64::MAX;
        let mut north = -f64::MAX;
        for (longitude, latitude) in points {
            west = js_min(west, longitude);
            east = js_max(east, longitude);
            south = js_min(south, latitude);
            north = js_max(north, latitude);
            let adjusted = if longitude >= 0.0 { longitude } else { longitude + TWO_PI };
            west_over_idl = js_min(west_over_idl, adjusted);
            east_over_idl = js_max(east_over_idl, adjusted);
        }
        if east - west > east_over_idl - west_over_idl {
            west = west_over_idl;
            east = east_over_idl;
            if east > PI {
                east -= TWO_PI;
            }
            if west > PI {
                west -= TWO_PI;
            }
        }
        Self::new(west, south, east, north)
    }

    /// Rectangle around a bounding sphere, from its four compass points and center.
    pub fn from_bounding_sphere(sphere: &BoundingSphere, ellipsoid: &Ellipsoid) -> Self {
        let center = sphere.center;
        let radius = sphere.radius;
        if center == Cartesian3::ZERO {
            return Self::MAX_VALUE;
        }
        let from_enu = Transforms::east_north_up_to_fixed_frame(center, ellipsoid);
        let east = from_enu.multiply_by_point_as_vector(Cartesian3::UNIT_X).normalize();
        let north = from_enu.multiply_by_point_as_vector(Cartesian3::UNIT_Y).normalize();
        let north = north.multiply_by_scalar(radius);
        let east = east.multiply_by_scalar(radius);
        let south = north.negate();
        let west = east.negate();
        let positions = [center.add(north), center.add(west), center.add(south), center.add(east), center];
        Self::from_cartesian_array(&positions, ellipsoid)
    }

    pub fn equals(&self, other: &Self) -> bool {
        self == other
    }

    pub fn equals_epsilon(&self, other: &Self, absolute_epsilon: f64) -> bool {
        (self.west - other.west).abs() <= absolute_epsilon
            && (self.south - other.south).abs() <= absolute_epsilon
            && (self.east - other.east).abs() <= absolute_epsilon
            && (self.north - other.north).abs() <= absolute_epsilon
    }

    pub fn southwest(&self) -> Cartographic {
        Cartographic::new(self.west, self.south, 0.0)
    }

    pub fn northwest(&self) -> Cartographic {
        Cartographic::new(self.west, self.north, 0.0)
    }

    pub fn northeast(&self) -> Cartographic {
        Cartographic::new(self.east, self.north, 0.0)
    }

    pub fn southeast(&self) -> Cartographic {
        Cartographic::new(self.east, self.south, 0.0)
    }

    pub fn center(&self) -> Cartographic {
        let mut east = self.east;
        if east < self.west {
            east += TWO_PI;
        }
        Cartographic::new(negative_pi_to_pi((self.west + east) * 0.5), (self.south + self.north) * 0.5, 0.0)
    }

    /// Adjusts east/west so that rectangles crossing the antimeridian compare
    /// on a common range. Shared by `intersection` and `union`.
    fn unwrap_pair(&self, other: &Self) -> (f64, f64, f64, f64) {
        let mut rectangle_east = self.east;
        let mut rectangle_west = self.west;
        let mut other_east = other.east;
        let mut other_west = other.west;
        if rectangle_east < rectangle_west && other_east > 0.0 {
            rectangle_east += TWO_PI;
        } else if other_east < other_west && rectangle_east > 0.0 {
            other_east += TWO_PI;
        }
        if rectangle_east < rectangle_west && other_west < 0.0 {
            other_west += TWO_PI;
        } else if other_east < other_west && rectangle_west < 0.0 {
            rectangle_west += TWO_PI;
        }
        (rectangle_west, rectangle_east, other_west, other_east)
    }

    /// Overlap of two rectangles, handling the antimeridian; `None` if disjoint.
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let (rw, re, ow, oe) = self.unwrap_pair(other);
        let west = negative_pi_to_pi(js_max(rw, ow));
        let east = negative_pi_to_pi(js_min(re, oe));
        if (self.west < self.east || other.west < other.east) && east <= west {
            return None;
        }
        let south = js_max(self.south, other.south);
        let north = js_min(self.north, other.north);
        if south >= north {
            return None;
        }
        Some(Self::new(west, south, east, north))
    }

    /// Overlap without antimeridian handling; `None` if disjoint.
    pub fn simple_intersection(&self, other: &Self) -> Option<Self> {
        let west = js_max(self.west, other.west);
        let south = js_max(self.south, other.south);
        let east = js_min(self.east, other.east);
        let north = js_min(self.north, other.north);
        if south >= north || west >= east {
            return None;
        }
        Some(Self::new(west, south, east, north))
    }

    pub fn union(&self, other: &Self) -> Self {
        let (rw, re, ow, oe) = self.unwrap_pair(other);
        Self::new(
            negative_pi_to_pi(js_min(rw, ow)),
            js_min(self.south, other.south),
            negative_pi_to_pi(js_max(re, oe)),
            js_max(self.north, other.north),
        )
    }

    pub fn expand(&self, cartographic: &Cartographic) -> Self {
        Self::new(
            js_min(self.west, cartographic.longitude),
            js_min(self.south, cartographic.latitude),
            js_max(self.east, cartographic.longitude),
            js_max(self.north, cartographic.latitude),
        )
    }

    pub fn contains(&self, cartographic: &Cartographic) -> bool {
        let mut longitude = cartographic.longitude;
        let latitude = cartographic.latitude;
        let west = self.west;
        let mut east = self.east;
        if east < west {
            east += TWO_PI;
            if longitude < 0.0 {
                longitude += TWO_PI;
            }
        }
        (longitude > west || equals_epsilon(longitude, west, EPSILON14, EPSILON14))
            && (longitude < east || equals_epsilon(longitude, east, EPSILON14, EPSILON14))
            && latitude >= self.south
            && latitude <= self.north
    }

    /// Points on the rectangle (corners plus extremes) at `surface_height`,
    /// enough to bound it in 3D.
    pub fn subsample(&self, ellipsoid: &Ellipsoid, surface_height: f64) -> Vec<Cartesian3> {
        let mut out = Vec::with_capacity(10);
        let mut lla = Cartographic::new(self.west, self.north, surface_height);
        out.push(ellipsoid.cartographic_to_cartesian(&lla));
        lla.longitude = self.east;
        out.push(ellipsoid.cartographic_to_cartesian(&lla));
        lla.latitude = self.south;
        out.push(ellipsoid.cartographic_to_cartesian(&lla));
        lla.longitude = self.west;
        out.push(ellipsoid.cartographic_to_cartesian(&lla));

        lla.latitude = if self.north < 0.0 {
            self.north
        } else if self.south > 0.0 {
            self.south
        } else {
            0.0
        };
        for i in 1..8 {
            lla.longitude = -PI + f64::from(i) * PI_OVER_TWO;
            if self.contains(&lla) {
                out.push(ellipsoid.cartographic_to_cartesian(&lla));
            }
        }
        if lla.latitude == 0.0 {
            lla.longitude = self.west;
            out.push(ellipsoid.cartographic_to_cartesian(&lla));
            lla.longitude = self.east;
            out.push(ellipsoid.cartographic_to_cartesian(&lla));
        }
        out
    }

    /// Sub-rectangle at fractional positions within this one.
    pub fn subsection(&self, west_lerp: f64, south_lerp: f64, east_lerp: f64, north_lerp: f64) -> Self {
        let mut r = Self::default();
        if self.west <= self.east {
            let width = self.east - self.west;
            r.west = self.west + west_lerp * width;
            r.east = self.west + east_lerp * width;
        } else {
            let width = TWO_PI + self.east - self.west;
            r.west = negative_pi_to_pi(self.west + west_lerp * width);
            r.east = negative_pi_to_pi(self.west + east_lerp * width);
        }
        let height = self.north - self.south;
        r.south = self.south + south_lerp * height;
        r.north = self.south + north_lerp * height;
        if west_lerp == 1.0 {
            r.west = self.east;
        }
        if east_lerp == 1.0 {
            r.east = self.east;
        }
        if south_lerp == 1.0 {
            r.south = self.north;
        }
        if north_lerp == 1.0 {
            r.north = self.north;
        }
        r
    }
}
