//! CesiumJS `srgbToLinear`: sRGB transfer function to linear.

pub fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        return value * 0.07739938080495356037151702786378;
    }
    ((value + 0.055) * 0.94786729857819905213270142180095).powf(2.4)
}
