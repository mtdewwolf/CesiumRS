//! CesiumJS `Simon1994PlanetaryPositions`: Sun and Moon positions in the
//! Earth-centered inertial (ICRF) frame, from Simon et al. 1994, "Numerical
//! expressions for precession formulae and mean elements for the Moon and
//! the planets".
//!
//! Cesium's orbit-element checks (negative eccentricity, hyperbolic orbits,
//! Kepler non-convergence) cannot trigger for the Earth-Moon elements used
//! here, so the helpers do not report errors.

use super::cartesian3::Cartesian3;
use super::js_math::JsMath;
use super::julian_date::JulianDate;
use super::math::{zero_to_two_pi, EPSILON8, PI, RADIANS_PER_ARCSECOND, RADIANS_PER_DEGREE, TWO_PI};
use super::matrix3::Matrix3;
use super::time_constants::{DAYS_PER_JULIAN_CENTURY, SECONDS_PER_DAY};

fn compute_tdb_minus_tt_spice(days_since_j2000_in_terrestrial_time: f64) -> f64 {
    let g = 6.239996 + 0.0172019696544 * days_since_j2000_in_terrestrial_time;
    1.657e-3 * (g + 1.671e-2 * g.js_sin()).js_sin()
}

const TDT_MINUS_TAI: f64 = 32.184;
const J2000D: f64 = 2451545.0;

fn tai_to_tdb(date: &JulianDate) -> JulianDate {
    let result = date.add_seconds(TDT_MINUS_TAI);
    let days = result.total_days() - J2000D;
    result.add_seconds(compute_tdb_minus_tt_spice(days))
}

/// J2000 in TDB; Cesium builds it as `new JulianDate(2451545, 0, TAI)`.
const EPOCH_DAY_NUMBER: f64 = 2451545.0;
const EPOCH_SECONDS_OF_DAY: f64 = 0.0;

const METERS_PER_KILOMETER: f64 = 1000.0;
/// IAU 1976 value.
const METERS_PER_ASTRONOMICAL_UNIT: f64 = 1.4959787e11;

fn elements_to_cartesian(
    semimajor_axis: f64,
    eccentricity: f64,
    mut inclination: f64,
    longitude_of_perigee: f64,
    mut longitude_of_node: f64,
    mean_longitude: f64,
) -> Cartesian3 {
    if inclination < 0.0 {
        inclination = -inclination;
        longitude_of_node += PI;
    }
    let radius_of_periapsis = semimajor_axis * (1.0 - eccentricity);
    let argument_of_periapsis = longitude_of_perigee - longitude_of_node;
    let right_ascension_of_ascending_node = longitude_of_node;
    let true_anomaly = mean_anomaly_to_true_anomaly(mean_longitude - longitude_of_perigee, eccentricity);
    let perifocal_to_equatorial =
        perifocal_to_cartesian_matrix(argument_of_periapsis, inclination, right_ascension_of_ascending_node);
    let semilatus = radius_of_periapsis * (1.0 + eccentricity);
    let costheta = true_anomaly.js_cos();
    let sintheta = true_anomaly.js_sin();
    let denom = 1.0 + eccentricity * costheta;
    let radius = semilatus / denom;
    perifocal_to_equatorial.multiply_by_vector(Cartesian3::new(radius * costheta, radius * sintheta, 0.0))
}

fn mean_anomaly_to_true_anomaly(mean_anomaly: f64, eccentricity: f64) -> f64 {
    let eccentric_anomaly = mean_anomaly_to_eccentric_anomaly(mean_anomaly, eccentricity);
    eccentric_anomaly_to_true_anomaly(eccentric_anomaly, eccentricity)
}

const MAX_ITERATION_COUNT: usize = 50;
const KEPLER_EQ_CONVERGENCE: f64 = EPSILON8;

fn mean_anomaly_to_eccentric_anomaly(mut mean_anomaly: f64, eccentricity: f64) -> f64 {
    let revs = (mean_anomaly / TWO_PI).floor();
    mean_anomaly -= revs * TWO_PI;
    let mut iteration_value = mean_anomaly
        + (eccentricity * mean_anomaly.js_sin()) / (1.0 - (mean_anomaly + eccentricity).js_sin() + mean_anomaly.js_sin());
    let mut eccentric_anomaly = f64::MAX;
    let mut count = 0;
    while count < MAX_ITERATION_COUNT && (eccentric_anomaly - iteration_value).abs() > KEPLER_EQ_CONVERGENCE {
        eccentric_anomaly = iteration_value;
        let nr_function = eccentric_anomaly - eccentricity * eccentric_anomaly.js_sin() - mean_anomaly;
        let d_nr_function = 1.0 - eccentricity * eccentric_anomaly.js_cos();
        iteration_value = eccentric_anomaly - nr_function / d_nr_function;
        count += 1;
    }
    iteration_value + revs * TWO_PI
}

fn eccentric_anomaly_to_true_anomaly(mut eccentric_anomaly: f64, eccentricity: f64) -> f64 {
    let revs = (eccentric_anomaly / TWO_PI).floor();
    eccentric_anomaly -= revs * TWO_PI;
    let true_anomaly_x = eccentric_anomaly.js_cos() - eccentricity;
    let true_anomaly_y = eccentric_anomaly.js_sin() * (1.0 - eccentricity * eccentricity).sqrt();
    let mut true_anomaly = zero_to_two_pi(true_anomaly_y.js_atan2(true_anomaly_x));
    if eccentric_anomaly < 0.0 {
        true_anomaly -= TWO_PI;
    }
    true_anomaly + revs * TWO_PI
}

fn perifocal_to_cartesian_matrix(argument_of_periapsis: f64, inclination: f64, right_ascension: f64) -> Matrix3 {
    let (sinap, cosap) = argument_of_periapsis.js_sin_cos();
    let (sini, cosi) = inclination.js_sin_cos();
    let (sinraan, cosraan) = right_ascension.js_sin_cos();
    // Cesium fills its scratch matrix in column-major order.
    Matrix3::from_column_major_array([
        cosraan * cosap - sinraan * sinap * cosi,
        sinraan * cosap + cosraan * sinap * cosi,
        sinap * sini,
        -cosraan * sinap - sinraan * cosap * cosi,
        -sinraan * sinap + cosraan * cosap * cosi,
        cosap * sini,
        sinraan * sini,
        -cosraan * sini,
        cosi,
    ])
}

const SEMI_MAJOR_AXIS0: f64 = 1.0000010178 * METERS_PER_ASTRONOMICAL_UNIT;
const MEAN_LONGITUDE0: f64 = 100.46645683 * RADIANS_PER_DEGREE;
const MEAN_LONGITUDE1: f64 = 1295977422.83429 * RADIANS_PER_ARCSECOND;

/// `(frequency, cosine amplitude, sine amplitude)` terms of the barycenter's
/// semimajor axis (in AU × 1e-7) and mean longitude (× 1e-7).
const SEMIMAJOR_TERMS: [(f64, f64, f64); 8] = [
    (16002.0, 64.0, -150.0),
    (21863.0, -152.0, -46.0),
    (32004.0, 62.0, 68.0),
    (10931.0, -8.0, 54.0),
    (14529.0, 32.0, 14.0),
    (16368.0, -41.0, 24.0),
    (15318.0, 19.0, -28.0),
    (32794.0, -11.0, 22.0),
];
const LONGITUDE_TERMS: [(f64, f64, f64); 8] = [
    (10.0, -325.0, -105.0),
    (16002.0, -322.0, -137.0),
    (21863.0, -79.0, 258.0),
    (10931.0, 232.0, 35.0),
    (1473.0, -52.0, -116.0),
    (32004.0, 97.0, -88.0),
    (4387.0, 55.0, -112.0),
    (73.0, -41.0, -80.0),
];

/// Days from the J2000 epoch to `date` in TDB.
fn days_since_epoch(date: &JulianDate) -> f64 {
    let tdb = tai_to_tdb(date);
    tdb.day_number - EPOCH_DAY_NUMBER + (tdb.seconds_of_day - EPOCH_SECONDS_OF_DAY) / SECONDS_PER_DAY
}

fn compute_simon_earth_moon_barycenter(date: &JulianDate) -> Cartesian3 {
    let t = days_since_epoch(date) / (DAYS_PER_JULIAN_CENTURY * 10.0);
    let u = 0.3595362 * t;
    // Cesium sums term by term, cosine then sine, left to right.
    let mut semimajor_axis = SEMI_MAJOR_AXIS0;
    for (p, ca, sa) in SEMIMAJOR_TERMS {
        semimajor_axis = semimajor_axis
            + ca * 1e-7 * METERS_PER_ASTRONOMICAL_UNIT * (p * u).js_cos()
            + sa * 1e-7 * METERS_PER_ASTRONOMICAL_UNIT * (p * u).js_sin();
    }
    let mut mean_longitude = MEAN_LONGITUDE0 + MEAN_LONGITUDE1 * t;
    for (q, cl, sl) in LONGITUDE_TERMS {
        mean_longitude = mean_longitude + cl * 1e-7 * (q * u).js_cos() + sl * 1e-7 * (q * u).js_sin();
    }
    let eccentricity = 0.0167086342 - 0.0004203654 * t;
    let longitude_of_perigee = 102.93734808 * RADIANS_PER_DEGREE + 11612.3529 * RADIANS_PER_ARCSECOND * t;
    let inclination = 469.97289 * RADIANS_PER_ARCSECOND * t;
    let longitude_of_node = 174.87317577 * RADIANS_PER_DEGREE - 8679.27034 * RADIANS_PER_ARCSECOND * t;
    elements_to_cartesian(
        semimajor_axis,
        eccentricity,
        inclination,
        longitude_of_perigee,
        longitude_of_node,
        mean_longitude,
    )
}

fn compute_simon_moon(date: &JulianDate) -> Cartesian3 {
    let t = days_since_epoch(date) / DAYS_PER_JULIAN_CENTURY;
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;

    let mut semimajor_axis = 383397.7725 + 0.004 * t;
    let mut eccentricity = 0.055545526 - 0.000000016 * t;
    let inclination_constant = 5.15668983 * RADIANS_PER_DEGREE;
    let mut inclination_sec_part = -0.00008 * t + 0.02966 * t2 - 0.000042 * t3 - 0.00000013 * t4;
    let longitude_of_perigee_constant = 83.35324312 * RADIANS_PER_DEGREE;
    let mut longitude_of_perigee_sec_part = 14643420.2669 * t - 38.2702 * t2 - 0.045047 * t3 + 0.00021301 * t4;
    let longitude_of_node_constant = 125.04455501 * RADIANS_PER_DEGREE;
    let mut longitude_of_node_sec_part = -6967919.3631 * t + 6.3602 * t2 + 0.007625 * t3 - 0.00003586 * t4;
    let mean_longitude_constant = 218.31664563 * RADIANS_PER_DEGREE;
    let mut mean_longitude_sec_part = 1732559343.4847 * t - 6.391 * t2 + 0.006588 * t3 - 0.00003169 * t4;

    // Delaunay arguments, and the longitude of the ascending node of the ecliptic.
    let d = 297.85019547 * RADIANS_PER_DEGREE
        + RADIANS_PER_ARCSECOND * (1602961601.209 * t - 6.3706 * t2 + 0.006593 * t3 - 0.00003169 * t4);
    let f = 93.27209062 * RADIANS_PER_DEGREE
        + RADIANS_PER_ARCSECOND * (1739527262.8478 * t - 12.7512 * t2 - 0.001037 * t3 + 0.00000417 * t4);
    let l = 134.96340251 * RADIANS_PER_DEGREE
        + RADIANS_PER_ARCSECOND * (1717915923.2178 * t + 31.8792 * t2 + 0.051635 * t3 - 0.0002447 * t4);
    let lprime = 357.52910918 * RADIANS_PER_DEGREE
        + RADIANS_PER_ARCSECOND * (129596581.0481 * t - 0.5532 * t2 + 0.000136 * t3 - 0.00001149 * t4);
    let psi = 310.17137918 * RADIANS_PER_DEGREE
        - RADIANS_PER_ARCSECOND * (6967051.436 * t + 6.2068 * t2 + 0.007618 * t3 - 0.00003219 * t4);

    let two_d = 2.0 * d;
    let four_d = 4.0 * d;
    let six_d = 6.0 * d;
    let twol = 2.0 * l;
    let threel = 3.0 * l;
    let fourl = 4.0 * l;
    let two_f = 2.0 * f;

    semimajor_axis += 3400.4 * two_d.js_cos() - 635.6 * (two_d - l).js_cos() - 235.6 * l.js_cos()
        + 218.1 * (two_d - lprime).js_cos()
        + 181.0 * (two_d + l).js_cos();

    eccentricity += 0.014216 * (two_d - l).js_cos() + 0.008551 * (two_d - twol).js_cos() - 0.001383 * l.js_cos()
        + 0.001356 * (two_d + l).js_cos()
        - 0.001147 * (four_d - threel).js_cos()
        - 0.000914 * (four_d - twol).js_cos()
        + 0.000869 * (two_d - lprime - l).js_cos()
        - 0.000627 * two_d.js_cos()
        - 0.000394 * (four_d - fourl).js_cos()
        + 0.000282 * (two_d - lprime - twol).js_cos()
        - 0.000279 * (d - l).js_cos()
        - 0.000236 * twol.js_cos()
        + 0.000231 * four_d.js_cos()
        + 0.000229 * (six_d - fourl).js_cos()
        - 0.000201 * (twol - two_f).js_cos();

    inclination_sec_part += 486.26 * (two_d - two_f).js_cos() - 40.13 * two_d.js_cos()
        + 37.51 * two_f.js_cos()
        + 25.73 * (twol - two_f).js_cos()
        + 19.97 * (two_d - lprime - two_f).js_cos();

    longitude_of_perigee_sec_part += -55609.0 * (two_d - l).js_sin() - 34711.0 * (two_d - twol).js_sin() - 9792.0 * l.js_sin()
        + 9385.0 * (four_d - threel).js_sin()
        + 7505.0 * (four_d - twol).js_sin()
        + 5318.0 * (two_d + l).js_sin()
        + 3484.0 * (four_d - fourl).js_sin()
        - 3417.0 * (two_d - lprime - l).js_sin()
        - 2530.0 * (six_d - fourl).js_sin()
        - 2376.0 * two_d.js_sin()
        - 2075.0 * (two_d - threel).js_sin()
        - 1883.0 * twol.js_sin()
        - 1736.0 * (six_d - 5.0 * l).js_sin()
        + 1626.0 * lprime.js_sin()
        - 1370.0 * (six_d - threel).js_sin();

    longitude_of_node_sec_part += -5392.0 * (two_d - two_f).js_sin() - 540.0 * lprime.js_sin() - 441.0 * two_d.js_sin()
        + 423.0 * two_f.js_sin()
        - 288.0 * (twol - two_f).js_sin();

    mean_longitude_sec_part += -3332.9 * two_d.js_sin() + 1197.4 * (two_d - l).js_sin() - 662.5 * lprime.js_sin()
        + 396.3 * l.js_sin()
        - 218.0 * (two_d - lprime).js_sin();

    // Planetary perturbations.
    let two_psi = 2.0 * psi;
    let three_psi = 3.0 * psi;

    inclination_sec_part += 46.997 * psi.js_cos() * t - 0.614 * (two_d - two_f + psi).js_cos() * t
        + 0.614 * (two_d - two_f - psi).js_cos() * t
        - 0.0297 * two_psi.js_cos() * t2
        - 0.0335 * psi.js_cos() * t2
        + 0.0012 * (two_d - two_f + two_psi).js_cos() * t2
        - 0.00016 * psi.js_cos() * t3
        + 0.00004 * three_psi.js_cos() * t3
        + 0.00004 * two_psi.js_cos() * t3;

    let perigee_and_mean = 2.116 * psi.js_sin() * t - 0.111 * (two_d - two_f - psi).js_sin() * t - 0.0015 * psi.js_sin() * t2;
    longitude_of_perigee_sec_part += perigee_and_mean;
    mean_longitude_sec_part += perigee_and_mean;

    longitude_of_node_sec_part +=
        -520.77 * psi.js_sin() * t + 13.66 * (two_d - two_f + psi).js_sin() * t + 1.12 * (two_d - psi).js_sin() * t
            - 1.06 * (two_f - psi).js_sin() * t
            + 0.66 * two_psi.js_sin() * t2
            + 0.371 * psi.js_sin() * t2
            - 0.035 * (two_d - two_f + two_psi).js_sin() * t2
            - 0.015 * (two_d - two_f + psi).js_sin() * t2
            + 0.0014 * psi.js_sin() * t3
            - 0.0011 * three_psi.js_sin() * t3
            - 0.0009 * two_psi.js_sin() * t3;

    semimajor_axis *= METERS_PER_KILOMETER;
    let inclination = inclination_constant + inclination_sec_part * RADIANS_PER_ARCSECOND;
    let longitude_of_perigee = longitude_of_perigee_constant + longitude_of_perigee_sec_part * RADIANS_PER_ARCSECOND;
    let mean_longitude = mean_longitude_constant + mean_longitude_sec_part * RADIANS_PER_ARCSECOND;
    let longitude_of_node = longitude_of_node_constant + longitude_of_node_sec_part * RADIANS_PER_ARCSECOND;

    elements_to_cartesian(
        semimajor_axis,
        eccentricity,
        inclination,
        longitude_of_perigee,
        longitude_of_node,
        mean_longitude,
    )
}

/// From the 1992 mu value in Table 2.
const MOON_EARTH_MASS_RATIO: f64 = 0.012300034;
const FACTOR: f64 = -(MOON_EARTH_MASS_RATIO / (MOON_EARTH_MASS_RATIO + 1.0));

fn compute_simon_earth(date: &JulianDate) -> Cartesian3 {
    compute_simon_moon(date).multiply_by_scalar(FACTOR)
}

/// Rotates from the mean ecliptic and equinox of J2000 to ICRF.
const AXES_TRANSFORMATION: Matrix3 = Matrix3::new(
    1.0000000000000002,
    5.619723173785822e-16,
    4.690511510146299e-19,
    -5.154129427414611e-16,
    0.9174820620691819,
    -0.39777715593191376,
    -2.23970096136568e-16,
    0.39777715593191376,
    0.9174820620691819,
);

/// The Sun's position (meters) relative to the Earth, in ICRF.
pub fn compute_sun_position_in_earth_inertial_frame(julian_date: &JulianDate) -> Cartesian3 {
    let translation = compute_simon_earth_moon_barycenter(julian_date);
    let result = translation.negate();
    let earth = compute_simon_earth(julian_date);
    AXES_TRANSFORMATION.multiply_by_vector(result.subtract(earth))
}

/// The Moon's position (meters) relative to the Earth, in ICRF.
pub fn compute_moon_position_in_earth_inertial_frame(julian_date: &JulianDate) -> Cartesian3 {
    AXES_TRANSFORMATION.multiply_by_vector(compute_simon_moon(julian_date))
}
