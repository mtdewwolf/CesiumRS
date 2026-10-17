//! CesiumJS `scaleToGeodeticSurface`: Newton iteration onto the ellipsoid
//! along the geodetic normal.

use super::cartesian3::Cartesian3;
use super::math::EPSILON12;

/// `None` when `cartesian` is the center (`squaredNorm` below the tolerance
/// and the ratio infinite).
pub fn scale_to_geodetic_surface(
    cartesian: Cartesian3,
    one_over_radii: Cartesian3,
    one_over_radii_squared: Cartesian3,
    center_tolerance_squared: f64,
) -> Option<Cartesian3> {
    let (px, py, pz) = (cartesian.x, cartesian.y, cartesian.z);
    let x2 = px * px * one_over_radii.x * one_over_radii.x;
    let y2 = py * py * one_over_radii.y * one_over_radii.y;
    let z2 = pz * pz * one_over_radii.z * one_over_radii.z;
    let squared_norm = x2 + y2 + z2;
    let ratio = (1.0 / squared_norm).sqrt();
    let intersection = cartesian.multiply_by_scalar(ratio);
    if squared_norm < center_tolerance_squared {
        return if ratio.is_finite() { Some(intersection) } else { None };
    }
    let (ox, oy, oz) = (one_over_radii_squared.x, one_over_radii_squared.y, one_over_radii_squared.z);
    let gradient = Cartesian3::new(
        intersection.x * ox * 2.0,
        intersection.y * oy * 2.0,
        intersection.z * oz * 2.0,
    );
    let mut lambda = ((1.0 - ratio) * cartesian.magnitude()) / (0.5 * gradient.magnitude());
    let mut correction = 0.0;
    let (mut xm, mut ym, mut zm);
    loop {
        lambda -= correction;
        xm = 1.0 / (1.0 + lambda * ox);
        ym = 1.0 / (1.0 + lambda * oy);
        zm = 1.0 / (1.0 + lambda * oz);
        let (xm2, ym2, zm2) = (xm * xm, ym * ym, zm * zm);
        let (xm3, ym3, zm3) = (xm2 * xm, ym2 * ym, zm2 * zm);
        let func = x2 * xm2 + y2 * ym2 + z2 * zm2 - 1.0;
        let denominator = x2 * xm3 * ox + y2 * ym3 * oy + z2 * zm3 * oz;
        let derivative = -2.0 * denominator;
        correction = func / derivative;
        // JS loops `while (|func| > EPSILON12)`, which also stops on NaN.
        if func.abs() <= EPSILON12 || func.is_nan() {
            break;
        }
    }
    Some(Cartesian3::new(px * xm, py * ym, pz * zm))
}
