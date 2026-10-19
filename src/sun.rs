//! Solar direction in Earth-fixed coordinates, from Core's Simon1994
//! ephemeris (`Simon1994PlanetaryPositions`) rotated from ICRF to the fixed
//! frame with Core's `Transforms`.
//!
//! The full ICRF-to-fixed rotation needs Earth orientation parameters and
//! IAU 2006 XYS data, which the viewer does not load; with neither supplied,
//! `Transforms::compute_icrf_to_central_body_fixed_matrix` falls back to the
//! TEME-to-pseudo-fixed rotation — the same fallback Cesium itself uses when
//! that data is unavailable. The result is a lighting direction, not a
//! precision ephemeris.

use crate::core::simon1994_planetary_positions::compute_sun_position_in_earth_inertial_frame;
use crate::core::{Cartesian3, EarthOrientationParameters, Iau2006XysData, JulianDate, TimeStandard, Transforms};

/// Unit vector from the geocenter toward the sun, in Earth-fixed
/// coordinates. `jd` is the Julian date (UTC); it may carry a day fraction.
pub fn sun_direction_ecef(jd: f64) -> Cartesian3 {
    let date = JulianDate::new(jd, 0.0, TimeStandard::Utc);
    let inertial = compute_sun_position_in_earth_inertial_frame(&date);
    let rotation = Transforms::compute_icrf_to_central_body_fixed_matrix(
        &date,
        &EarthOrientationParameters::none(),
        &Iau2006XysData::default(),
    );
    rotation.multiply_by_vector(inertial).normalize()
}
