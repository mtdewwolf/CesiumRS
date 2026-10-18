//! CesiumJS `Iau2000Orientation`: the Moon's orientation from the IAU/IAG
//! working group report (2000).

use super::iau_orientation_parameters::IauOrientationParameters;
use super::js_math::JsMath;
use super::julian_date::JulianDate;
use super::math::RADIANS_PER_DEGREE;
use super::time_constants::DAYS_PER_JULIAN_CENTURY;

const TDT_MINUS_TAI: f64 = 32.184;
const J2000D: f64 = 2451545.0;

const C1: f64 = -0.0529921;
const C2: f64 = -0.1059842;
const C3: f64 = 13.0120009;
const C4: f64 = 13.3407154;
const C5: f64 = 0.9856003;
const C6: f64 = 26.4057084;
const C7: f64 = 13.064993;
const C8: f64 = 0.3287146;
const C9: f64 = 1.7484877;
const C10: f64 = -0.1589763;
const C11: f64 = 0.0036096;
const C12: f64 = 0.1643573;
const C13: f64 = 12.9590088;

/// The Moon's pole and prime meridian at a TAI date.
pub fn compute_moon(date: &JulianDate) -> IauOrientationParameters {
    let date_tt = date.add_seconds(TDT_MINUS_TAI);
    let d = date_tt.total_days() - J2000D;
    let t = d / DAYS_PER_JULIAN_CENTURY;

    let e = |a: f64, c: f64| (a + c * d) * RADIANS_PER_DEGREE;
    let e1 = e(125.045, C1);
    let e2 = e(250.089, C2);
    let e3 = e(260.008, C3);
    let e4 = e(176.625, C4);
    let e5 = e(357.529, C5);
    let e6 = e(311.589, C6);
    let e7 = e(134.963, C7);
    let e8 = e(276.617, C8);
    let e9 = e(34.226, C9);
    let e10 = e(15.134, C10);
    let e11 = e(119.743, C11);
    let e12 = e(239.961, C12);
    let e13 = e(25.053, C13);

    let (sin_e1, cos_e1) = e1.js_sin_cos();
    let (sin_e2, cos_e2) = e2.js_sin_cos();
    let (sin_e3, cos_e3) = e3.js_sin_cos();
    let (sin_e4, cos_e4) = e4.js_sin_cos();
    let (sin_e5, cos_e5) = e5.js_sin_cos();
    let (sin_e6, cos_e6) = e6.js_sin_cos();
    let (sin_e7, cos_e7) = e7.js_sin_cos();
    let (sin_e8, cos_e8) = e8.js_sin_cos();
    let (sin_e9, cos_e9) = e9.js_sin_cos();
    let (sin_e10, cos_e10) = e10.js_sin_cos();
    let (sin_e11, cos_e11) = e11.js_sin_cos();
    let (sin_e12, cos_e12) = e12.js_sin_cos();
    let (sin_e13, cos_e13) = e13.js_sin_cos();

    let right_ascension = (269.9949 + 0.0031 * t - 3.8787 * sin_e1 - 0.1204 * sin_e2 + 0.07 * sin_e3 - 0.0172 * sin_e4
        + 0.0072 * sin_e6
        - 0.0052 * sin_e10
        + 0.0043 * sin_e13)
        * RADIANS_PER_DEGREE;
    let declination = (66.5392 + 0.013 * t + 1.5419 * cos_e1 + 0.0239 * cos_e2 - 0.0278 * cos_e3 + 0.0068 * cos_e4
        - 0.0029 * cos_e6
        + 0.0009 * cos_e7
        + 0.0008 * cos_e10
        - 0.0009 * cos_e13)
        * RADIANS_PER_DEGREE;
    let rotation = (38.3213 + 13.17635815 * d - 1.4e-12 * d * d + 3.561 * sin_e1 + 0.1208 * sin_e2 - 0.0642 * sin_e3
        + 0.0158 * sin_e4
        + 0.0252 * sin_e5
        - 0.0066 * sin_e6
        - 0.0047 * sin_e7
        - 0.0046 * sin_e8
        + 0.0028 * sin_e9
        + 0.0052 * sin_e10
        + 0.004 * sin_e11
        + 0.0019 * sin_e12
        - 0.0044 * sin_e13)
        * RADIANS_PER_DEGREE;
    let rotation_rate = ((13.17635815 - 1.4e-12 * (2.0 * d) + 3.561 * cos_e1 * C1 + 0.1208 * cos_e2 * C2
        - 0.0642 * cos_e3 * C3
        + 0.0158 * cos_e4 * C4
        + 0.0252 * cos_e5 * C5
        - 0.0066 * cos_e6 * C6
        - 0.0047 * cos_e7 * C7
        - 0.0046 * cos_e8 * C8
        + 0.0028 * cos_e9 * C9
        + 0.0052 * cos_e10 * C10
        + 0.004 * cos_e11 * C11
        + 0.0019 * cos_e12 * C12
        - 0.0044 * cos_e13 * C13)
        / 86400.0)
        * RADIANS_PER_DEGREE;

    IauOrientationParameters { right_ascension, declination, rotation, rotation_rate }
}
