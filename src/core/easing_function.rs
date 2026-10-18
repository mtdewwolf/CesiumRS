//! CesiumJS `EasingFunction`: the tween.js 25 easing curves Cesium exposes
//! for camera flights and animations.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EasingFunction {
    LinearNone,
    QuadraticIn,
    QuadraticOut,
    QuadraticInOut,
    CubicIn,
    CubicOut,
    CubicInOut,
    QuarticIn,
    QuarticOut,
    QuarticInOut,
    QuinticIn,
    QuinticOut,
    QuinticInOut,
    SinusoidalIn,
    SinusoidalOut,
    SinusoidalInOut,
    ExponentialIn,
    ExponentialOut,
    ExponentialInOut,
    CircularIn,
    CircularOut,
    CircularInOut,
    ElasticIn,
    ElasticOut,
    ElasticInOut,
    BackIn,
    BackOut,
    BackInOut,
    BounceIn,
    BounceOut,
    BounceInOut,
}

impl EasingFunction {
    pub const ALL: [Self; 31] = [
        Self::LinearNone,
        Self::QuadraticIn,
        Self::QuadraticOut,
        Self::QuadraticInOut,
        Self::CubicIn,
        Self::CubicOut,
        Self::CubicInOut,
        Self::QuarticIn,
        Self::QuarticOut,
        Self::QuarticInOut,
        Self::QuinticIn,
        Self::QuinticOut,
        Self::QuinticInOut,
        Self::SinusoidalIn,
        Self::SinusoidalOut,
        Self::SinusoidalInOut,
        Self::ExponentialIn,
        Self::ExponentialOut,
        Self::ExponentialInOut,
        Self::CircularIn,
        Self::CircularOut,
        Self::CircularInOut,
        Self::ElasticIn,
        Self::ElasticOut,
        Self::ElasticInOut,
        Self::BackIn,
        Self::BackOut,
        Self::BackInOut,
        Self::BounceIn,
        Self::BounceOut,
        Self::BounceInOut,
    ];

    /// Cesium's constant name, such as `QUADRATIC_IN_OUT`.
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 31] = [
            "LINEAR_NONE",
            "QUADRATIC_IN",
            "QUADRATIC_OUT",
            "QUADRATIC_IN_OUT",
            "CUBIC_IN",
            "CUBIC_OUT",
            "CUBIC_IN_OUT",
            "QUARTIC_IN",
            "QUARTIC_OUT",
            "QUARTIC_IN_OUT",
            "QUINTIC_IN",
            "QUINTIC_OUT",
            "QUINTIC_IN_OUT",
            "SINUSOIDAL_IN",
            "SINUSOIDAL_OUT",
            "SINUSOIDAL_IN_OUT",
            "EXPONENTIAL_IN",
            "EXPONENTIAL_OUT",
            "EXPONENTIAL_IN_OUT",
            "CIRCULAR_IN",
            "CIRCULAR_OUT",
            "CIRCULAR_IN_OUT",
            "ELASTIC_IN",
            "ELASTIC_OUT",
            "ELASTIC_IN_OUT",
            "BACK_IN",
            "BACK_OUT",
            "BACK_IN_OUT",
            "BOUNCE_IN",
            "BOUNCE_OUT",
            "BOUNCE_IN_OUT",
        ];
        NAMES[self as usize]
    }

    /// Eases `amount` (time in [0, 1]); values outside are extrapolated as
    /// tween.js does.
    pub fn evaluate(self, amount: f64) -> f64 {
        let mut a = amount;
        match self {
            Self::LinearNone => a,
            Self::QuadraticIn => a * a,
            Self::QuadraticOut => a * (2.0 - a),
            Self::QuadraticInOut => {
                a *= 2.0;
                if a < 1.0 {
                    return 0.5 * a * a;
                }
                a -= 1.0;
                -0.5 * (a * (a - 2.0) - 1.0)
            }
            Self::CubicIn => a * a * a,
            Self::CubicOut => {
                a -= 1.0;
                a * a * a + 1.0
            }
            Self::CubicInOut => {
                a *= 2.0;
                if a < 1.0 {
                    return 0.5 * a * a * a;
                }
                a -= 2.0;
                0.5 * (a * a * a + 2.0)
            }
            Self::QuarticIn => a * a * a * a,
            Self::QuarticOut => {
                a -= 1.0;
                1.0 - a * a * a * a
            }
            Self::QuarticInOut => {
                a *= 2.0;
                if a < 1.0 {
                    return 0.5 * a * a * a * a;
                }
                a -= 2.0;
                -0.5 * (a * a * a * a - 2.0)
            }
            Self::QuinticIn => a * a * a * a * a,
            Self::QuinticOut => {
                a -= 1.0;
                a * a * a * a * a + 1.0
            }
            Self::QuinticInOut => {
                a *= 2.0;
                if a < 1.0 {
                    return 0.5 * a * a * a * a * a;
                }
                a -= 2.0;
                0.5 * (a * a * a * a * a + 2.0)
            }
            Self::SinusoidalIn => 1.0 - (((1.0 - a) * PI) / 2.0).sin(),
            Self::SinusoidalOut => ((a * PI) / 2.0).sin(),
            Self::SinusoidalInOut => 0.5 * (1.0 - (PI * (0.5 - a)).sin()),
            Self::ExponentialIn => {
                if a == 0.0 {
                    0.0
                } else {
                    1024f64.powf(a - 1.0)
                }
            }
            Self::ExponentialOut => {
                if a == 1.0 {
                    1.0
                } else {
                    1.0 - 2f64.powf(-10.0 * a)
                }
            }
            Self::ExponentialInOut => {
                if a == 0.0 {
                    return 0.0;
                }
                if a == 1.0 {
                    return 1.0;
                }
                a *= 2.0;
                if a < 1.0 {
                    return 0.5 * 1024f64.powf(a - 1.0);
                }
                0.5 * (-(2f64.powf(-10.0 * (a - 1.0))) + 2.0)
            }
            Self::CircularIn => 1.0 - (1.0 - a * a).sqrt(),
            Self::CircularOut => {
                a -= 1.0;
                (1.0 - a * a).sqrt()
            }
            Self::CircularInOut => {
                a *= 2.0;
                if a < 1.0 {
                    return -0.5 * ((1.0 - a * a).sqrt() - 1.0);
                }
                a -= 2.0;
                0.5 * ((1.0 - a * a).sqrt() + 1.0)
            }
            Self::ElasticIn => {
                if a == 0.0 {
                    return 0.0;
                }
                if a == 1.0 {
                    return 1.0;
                }
                -(2f64.powf(10.0 * (a - 1.0))) * ((a - 1.1) * 5.0 * PI).sin()
            }
            Self::ElasticOut => {
                if a == 0.0 {
                    return 0.0;
                }
                if a == 1.0 {
                    return 1.0;
                }
                2f64.powf(-10.0 * a) * ((a - 0.1) * 5.0 * PI).sin() + 1.0
            }
            Self::ElasticInOut => {
                if a == 0.0 {
                    return 0.0;
                }
                if a == 1.0 {
                    return 1.0;
                }
                a *= 2.0;
                if a < 1.0 {
                    return -0.5 * 2f64.powf(10.0 * (a - 1.0)) * ((a - 1.1) * 5.0 * PI).sin();
                }
                0.5 * 2f64.powf(-10.0 * (a - 1.0)) * ((a - 1.1) * 5.0 * PI).sin() + 1.0
            }
            Self::BackIn => {
                let s = 1.70158;
                if a == 1.0 {
                    1.0
                } else {
                    a * a * ((s + 1.0) * a - s)
                }
            }
            Self::BackOut => {
                let s = 1.70158;
                if a == 0.0 {
                    return 0.0;
                }
                a -= 1.0;
                a * a * ((s + 1.0) * a + s) + 1.0
            }
            Self::BackInOut => {
                let s = 1.70158 * 1.525;
                a *= 2.0;
                if a < 1.0 {
                    return 0.5 * (a * a * ((s + 1.0) * a - s));
                }
                a -= 2.0;
                0.5 * (a * a * ((s + 1.0) * a + s) + 2.0)
            }
            Self::BounceIn => 1.0 - Self::BounceOut.evaluate(1.0 - a),
            Self::BounceOut => {
                if a < 1.0 / 2.75 {
                    7.5625 * a * a
                } else if a < 2.0 / 2.75 {
                    a -= 1.5 / 2.75;
                    7.5625 * a * a + 0.75
                } else if a < 2.5 / 2.75 {
                    a -= 2.25 / 2.75;
                    7.5625 * a * a + 0.9375
                } else {
                    a -= 2.625 / 2.75;
                    7.5625 * a * a + 0.984375
                }
            }
            Self::BounceInOut => {
                if a < 0.5 {
                    return Self::BounceIn.evaluate(a * 2.0) * 0.5;
                }
                Self::BounceOut.evaluate(a * 2.0 - 1.0) * 0.5 + 0.5
            }
        }
    }
}
