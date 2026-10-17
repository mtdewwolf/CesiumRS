//! Ray–ellipsoid intersection. Ported from CesiumJS `IntersectionTests.rayEllipsoid`.

use crate::ellipsoid::{cartesian_to_cartographic, Ellipsoid};
use crate::math::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Interval {
    pub start: f64,
    pub stop: f64,
}

/// `direction` does not need to be a unit vector; `t` is in units of that vector.
pub fn ray_ellipsoid(origin: Vec3, direction: Vec3, ellipsoid: &Ellipsoid) -> Option<Interval> {
    let q = ellipsoid.one_over_radii.mul_components(origin);
    let w = ellipsoid.one_over_radii.mul_components(direction);
    let q2 = q.magnitude_squared();
    let qw = q.dot(w);

    if q2 > 1.0 {
        if qw >= 0.0 {
            return None;
        }
        let qw2 = qw * qw;
        let difference = q2 - 1.0;
        let w2 = w.magnitude_squared();
        let product = w2 * difference;
        if qw2 < product {
            None
        } else if qw2 > product {
            let discriminant = qw * qw - product;
            let temp = -qw + discriminant.sqrt();
            let root0 = temp / w2;
            let root1 = difference / temp;
            if root0 < root1 {
                Some(Interval { start: root0, stop: root1 })
            } else {
                Some(Interval { start: root1, stop: root0 })
            }
        } else {
            let root = (difference / w2).sqrt();
            Some(Interval { start: root, stop: root })
        }
    } else if q2 < 1.0 {
        let difference = q2 - 1.0;
        let w2 = w.magnitude_squared();
        let product = w2 * difference;
        let discriminant = qw * qw - product;
        let temp = -qw + discriminant.sqrt();
        Some(Interval { start: 0.0, stop: temp / w2 })
    } else if qw < 0.0 {
        let w2 = w.magnitude_squared();
        Some(Interval { start: 0.0, stop: -qw / w2 })
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Pick {
    pub t: f64,
    pub longitude: f64,
    pub latitude: f64,
    pub height: f64,
    pub position: Vec3,
}

/// First non-negative hit of a ray with the ellipsoid, as a cartographic point.
pub fn pick_ray(origin: Vec3, direction: Vec3, ellipsoid: &Ellipsoid) -> Option<Pick> {
    let interval = ray_ellipsoid(origin, direction, ellipsoid)?;
    let t = if interval.start >= 0.0 {
        interval.start
    } else if interval.stop >= 0.0 {
        interval.stop
    } else {
        return None;
    };
    let position = origin + direction * t;
    let c = cartesian_to_cartographic(ellipsoid, position)?;
    Some(Pick { t, longitude: c.longitude, latitude: c.latitude, height: c.height, position })
}
