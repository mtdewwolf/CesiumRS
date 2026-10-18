//! CesiumJS `Math` (`CesiumMath`): constants and scalar helpers.

use super::js_math::JsMath;
use std::cell::RefCell;

pub const EPSILON1: f64 = 0.1;
pub const EPSILON2: f64 = 0.01;
pub const EPSILON3: f64 = 0.001;
pub const EPSILON4: f64 = 0.0001;
pub const EPSILON5: f64 = 0.00001;
pub const EPSILON6: f64 = 0.000001;
pub const EPSILON7: f64 = 0.0000001;
pub const EPSILON8: f64 = 0.00000001;
pub const EPSILON9: f64 = 0.000000001;
pub const EPSILON10: f64 = 0.0000000001;
pub const EPSILON11: f64 = 0.00000000001;
pub const EPSILON12: f64 = 0.000000000001;
pub const EPSILON13: f64 = 0.0000000000001;
pub const EPSILON14: f64 = 0.00000000000001;
pub const EPSILON15: f64 = 0.000000000000001;
pub const EPSILON16: f64 = 0.0000000000000001;
pub const EPSILON17: f64 = 0.00000000000000001;
pub const EPSILON18: f64 = 0.000000000000000001;
pub const EPSILON19: f64 = 0.0000000000000000001;
pub const EPSILON20: f64 = 0.00000000000000000001;
pub const EPSILON21: f64 = 0.000000000000000000001;

/// Earth's gravitational parameter in m³/s².
pub const GRAVITATIONALPARAMETER: f64 = 3.986004418e14;
/// Radius of the sun in meters.
pub const SOLAR_RADIUS: f64 = 6.955e8;
/// Mean radius of the moon in meters.
pub const LUNAR_RADIUS: f64 = 1737400.0;
pub const SIXTY_FOUR_KILOBYTES: u32 = 64 * 1024;
pub const FOUR_GIGABYTES: u64 = 4 * 1024 * 1024 * 1024;

pub const PI: f64 = std::f64::consts::PI;
pub const ONE_OVER_PI: f64 = 1.0 / PI;
pub const PI_OVER_TWO: f64 = PI / 2.0;
pub const PI_OVER_THREE: f64 = PI / 3.0;
pub const PI_OVER_FOUR: f64 = PI / 4.0;
pub const PI_OVER_SIX: f64 = PI / 6.0;
pub const THREE_PI_OVER_TWO: f64 = (3.0 * PI) / 2.0;
pub const TWO_PI: f64 = 2.0 * PI;
pub const ONE_OVER_TWO_PI: f64 = 1.0 / (2.0 * PI);
pub const RADIANS_PER_DEGREE: f64 = PI / 180.0;
pub const DEGREES_PER_RADIAN: f64 = 180.0 / PI;
pub const RADIANS_PER_ARCSECOND: f64 = RADIANS_PER_DEGREE / 3600.0;

/// JavaScript `Math.max` for two values: NaN wins.
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && b.is_sign_negative()) {
        a
    } else {
        b
    }
}

/// JavaScript `Math.min` for two values: NaN wins.
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}

/// JavaScript `Math.round`: halves round toward positive infinity.
pub fn js_round(value: f64) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `1`, `-1`, or the value itself for `0`, `-0`, and NaN.
pub fn sign(value: f64) -> f64 {
    if value == 0.0 || value.is_nan() {
        value
    } else if value > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `1` for zero and positive values, `-1` for negative values.
pub fn sign_not_zero(value: f64) -> f64 {
    if value < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// Maps `[-1, 1]` to `[0, range_maximum]`. Cesium's default range is 255.
pub fn to_snorm(value: f64, range_maximum: f64) -> f64 {
    js_round((clamp(value, -1.0, 1.0) * 0.5 + 0.5) * range_maximum)
}

/// Maps `[0, range_maximum]` to `[-1, 1]`.
pub fn from_snorm(value: f64, range_maximum: f64) -> f64 {
    (clamp(value, 0.0, range_maximum) / range_maximum) * 2.0 - 1.0
}

/// Maps `[range_minimum, range_maximum]` to `[0, 1]`.
pub fn normalize(value: f64, range_minimum: f64, range_maximum: f64) -> f64 {
    let range = js_max(range_maximum - range_minimum, 0.0);
    if range == 0.0 {
        0.0
    } else {
        clamp((value - range_minimum) / range, 0.0, 1.0)
    }
}

pub fn sinh(value: f64) -> f64 {
    value.sinh()
}

pub fn cosh(value: f64) -> f64 {
    value.js_cosh()
}

pub fn lerp(p: f64, q: f64, time: f64) -> f64 {
    (1.0 - time) * p + time * q
}

pub fn to_radians(degrees: f64) -> f64 {
    degrees * RADIANS_PER_DEGREE
}

pub fn to_degrees(radians: f64) -> f64 {
    radians * DEGREES_PER_RADIAN
}

/// Longitude in `[-π, π)`.
pub fn convert_longitude_range(angle: f64) -> f64 {
    let simplified = angle - (angle / TWO_PI).floor() * TWO_PI;
    if simplified < -PI {
        simplified + TWO_PI
    } else if simplified >= PI {
        simplified - TWO_PI
    } else {
        simplified
    }
}

pub fn clamp_to_latitude_range(angle: f64) -> f64 {
    clamp(angle, -PI_OVER_TWO, PI_OVER_TWO)
}

/// Angle in `[-π, π]`.
pub fn negative_pi_to_pi(angle: f64) -> f64 {
    if (-PI..=PI).contains(&angle) {
        return angle;
    }
    zero_to_two_pi(angle + PI) - PI
}

/// Angle in `[0, 2π]`.
pub fn zero_to_two_pi(angle: f64) -> f64 {
    if (0.0..=TWO_PI).contains(&angle) {
        return angle;
    }
    let m = modulo(angle, TWO_PI);
    if m.abs() < EPSILON14 && angle.abs() > EPSILON14 {
        return TWO_PI;
    }
    m
}

/// Cesium `Math.mod`: the result has the sign of the divisor.
pub fn modulo(m: f64, n: f64) -> f64 {
    assert!(n != 0.0, "divisor cannot be 0.");
    if sign(m) == sign(n) && m.abs() < n.abs() {
        return m;
    }
    ((m % n) + n) % n
}

/// Cesium `Math.equalsEpsilon`: absolute or relative closeness.
/// Cesium's `absoluteEpsilon` defaults to `relative_epsilon`.
pub fn equals_epsilon(left: f64, right: f64, relative_epsilon: f64, absolute_epsilon: f64) -> bool {
    let abs_diff = (left - right).abs();
    abs_diff <= absolute_epsilon || abs_diff <= relative_epsilon * js_max(left.abs(), right.abs())
}

pub fn less_than(left: f64, right: f64, absolute_epsilon: f64) -> bool {
    left - right < -absolute_epsilon
}

pub fn less_than_or_equals(left: f64, right: f64, absolute_epsilon: f64) -> bool {
    left - right < absolute_epsilon
}

pub fn greater_than(left: f64, right: f64, absolute_epsilon: f64) -> bool {
    left - right > absolute_epsilon
}

pub fn greater_than_or_equals(left: f64, right: f64, absolute_epsilon: f64) -> bool {
    left - right > -absolute_epsilon
}

/// `n!` as a double, like Cesium (exact up to 22!, then rounded).
pub fn factorial(n: u32) -> f64 {
    let mut sum = 1.0;
    for i in 1..=n {
        sum *= f64::from(i);
    }
    sum
}

/// `n + 1`, wrapping to `minimum_value` past `maximum_value`.
pub fn increment_wrap(n: f64, maximum_value: f64, minimum_value: f64) -> f64 {
    assert!(maximum_value > minimum_value, "maximumValue must be greater than minimumValue.");
    let n = n + 1.0;
    if n > maximum_value {
        minimum_value
    } else {
        n
    }
}

pub fn is_power_of_two(n: u32) -> bool {
    n != 0 && (n & n.wrapping_sub(1)) == 0
}

/// Smallest power of two `>= n`; `0` for `0`. Defined for `n <= 2³¹`.
pub fn next_power_of_two(n: u32) -> u32 {
    assert!(n <= 1 << 31, "A number between 0 and 2^31 is required.");
    let mut n = n.wrapping_sub(1);
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    n.wrapping_add(1)
}

/// Largest power of two `<= n`; `0` for `0`.
pub fn previous_power_of_two(n: u32) -> u32 {
    let mut n = n;
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    n - (n >> 1)
}

pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// MT19937, matching the `mersenne-twister` package Cesium uses.
#[derive(Clone)]
pub struct MersenneTwister {
    mt: [u32; 624],
    mti: usize,
}

impl MersenneTwister {
    const N: usize = 624;
    const M: usize = 397;
    const MATRIX_A: u32 = 0x9908_b0df;
    const UPPER_MASK: u32 = 0x8000_0000;
    const LOWER_MASK: u32 = 0x7fff_ffff;

    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = seed;
        for i in 1..Self::N {
            let s = mt[i - 1] ^ (mt[i - 1] >> 30);
            mt[i] = 1_812_433_253u32.wrapping_mul(s).wrapping_add(i as u32);
        }
        Self { mt, mti: Self::N }
    }

    /// `genrand_int32`: uniform on `[0, 2³² - 1]`.
    pub fn random_int(&mut self) -> u32 {
        let mag01 = [0u32, Self::MATRIX_A];
        if self.mti >= Self::N {
            let mt = &mut self.mt;
            for kk in 0..Self::N - Self::M {
                let y = (mt[kk] & Self::UPPER_MASK) | (mt[kk + 1] & Self::LOWER_MASK);
                mt[kk] = mt[kk + Self::M] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            }
            for kk in Self::N - Self::M..Self::N - 1 {
                let y = (mt[kk] & Self::UPPER_MASK) | (mt[kk + 1] & Self::LOWER_MASK);
                mt[kk] = mt[kk + Self::M - Self::N] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            }
            let y = (mt[Self::N - 1] & Self::UPPER_MASK) | (mt[0] & Self::LOWER_MASK);
            mt[Self::N - 1] = mt[Self::M - 1] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            self.mti = 0;
        }
        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// Uniform on `[0, 1)` with 32 bits of resolution.
    pub fn random(&mut self) -> f64 {
        f64::from(self.random_int()) * (1.0 / 4_294_967_296.0)
    }
}

thread_local! {
    static RNG: RefCell<MersenneTwister> = RefCell::new(MersenneTwister::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u32)
            .unwrap_or(5489),
    ));
}

/// Seeds the generator behind [`next_random_number`] for this thread.
pub fn set_random_number_seed(seed: u32) {
    RNG.with(|r| *r.borrow_mut() = MersenneTwister::new(seed));
}

pub fn next_random_number() -> f64 {
    RNG.with(|r| r.borrow_mut().random())
}

pub fn random_between(min: f64, max: f64) -> f64 {
    next_random_number() * (max - min) + min
}

pub fn acos_clamped(value: f64) -> f64 {
    clamp(value, -1.0, 1.0).js_acos()
}

pub fn asin_clamped(value: f64) -> f64 {
    clamp(value, -1.0, 1.0).js_asin()
}

/// Length of the chord subtending `angle` on a circle of `radius`.
pub fn chord_length(angle: f64, radius: f64) -> f64 {
    2.0 * radius * (angle * 0.5).js_sin()
}

pub fn log_base(number: f64, base: f64) -> f64 {
    number.js_ln() / base.js_ln()
}

pub fn cbrt(number: f64) -> f64 {
    number.cbrt()
}

pub fn log2(number: f64) -> f64 {
    // `Math.log2`, rounded as V8 rounds it.
    libm::log2(number)
}

/// Fog density at a distance, as in Cesium's fog shader.
pub fn fog(distance_to_camera: f64, density: f64) -> f64 {
    let scalar = distance_to_camera * density;
    1.0 - (-(scalar * scalar)).js_exp()
}

/// Polynomial `atan` for `x` in `[-1, 1]`.
pub fn fast_approximate_atan(x: f64) -> f64 {
    x * (-0.1784 * x.abs() - 0.0663 * x * x + 1.0301)
}

/// Polynomial `atan2(y, x)`. Panics when both are zero, like Cesium.
pub fn fast_approximate_atan2(x: f64, y: f64) -> f64 {
    let ax = x.abs();
    let ay = y.abs();
    let adjacent = js_max(ax, ay);
    let opposite = js_min(ax, ay);
    let ratio = opposite / adjacent;
    assert!(!ratio.is_nan(), "either x or y must be nonzero");
    let mut t = fast_approximate_atan(ratio);
    t = if ay > ax { PI_OVER_TWO - t } else { t };
    t = if x < 0.0 { PI - t } else { t };
    if y < 0.0 {
        -t
    } else {
        t
    }
}

/// JavaScript `ToInt32`: truncate, wrap modulo 2³², reinterpret as signed.
/// NaN and infinities map to 0.
pub fn to_int32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let t = value.trunc();
    let m = t.rem_euclid(4_294_967_296.0);
    m as u32 as i32
}

/// JavaScript `ToUint32`.
pub fn to_uint32(value: f64) -> u32 {
    to_int32(value) as u32
}

/// JavaScript `Number.prototype.toString()` for finite and special values.
pub fn js_number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if value == 0.0 {
        return "0".into();
    }
    let abs = value.abs();
    if (1e-7..1e21).contains(&abs) {
        return format!("{value}");
    }
    // Rust's `{:e}` gives the same shortest digits; JS writes an explicit `+`.
    let s = format!("{value:e}");
    match s.split_once('e') {
        Some((mantissa, exp)) if !exp.starts_with('-') => format!("{mantissa}e+{exp}"),
        _ => s,
    }
}

/// JavaScript `Number.prototype.toFixed(digits)` for `|value| < 1e21`:
/// exact decimal rounding with ties going up in magnitude (Rust's own
/// formatting rounds ties to even).
pub fn js_to_fixed(value: f64, digits: usize) -> String {
    if !value.is_finite() || value.abs() >= 1e21 {
        return js_number_to_string(value);
    }
    // f64 values have finite decimal expansions of at most 1074 fractional digits.
    let exact = format!("{:.1100}", value.abs());
    let (int_part, frac_part) = exact.split_once('.').unwrap();
    let mut digits_vec: Vec<u8> = int_part.bytes().chain(frac_part.bytes().take(digits)).map(|b| b - b'0').collect();
    let round_up = frac_part.as_bytes().get(digits).is_some_and(|b| *b >= b'5');
    if round_up {
        let mut i = digits_vec.len();
        loop {
            if i == 0 {
                digits_vec.insert(0, 1);
                break;
            }
            i -= 1;
            if digits_vec[i] == 9 {
                digits_vec[i] = 0;
            } else {
                digits_vec[i] += 1;
                break;
            }
        }
    }
    let int_len = digits_vec.len() - digits;
    let mut s: String = digits_vec[..int_len].iter().map(|d| char::from(b'0' + d)).collect();
    if digits > 0 {
        s.push('.');
        s.extend(digits_vec[int_len..].iter().map(|d| char::from(b'0' + d)));
    }
    let is_zero = digits_vec.iter().all(|d| *d == 0);
    if value < 0.0 && !is_zero {
        s.insert(0, '-');
    }
    s
}
