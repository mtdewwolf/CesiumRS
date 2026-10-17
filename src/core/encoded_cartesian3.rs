//! CesiumJS `EncodedCartesian3`: high/low splitting for vertex positions.

use super::Cartesian3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EncodedScalar {
    pub high: f64,
    pub low: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EncodedCartesian3 {
    pub high: Cartesian3,
    pub low: Cartesian3,
}

impl EncodedCartesian3 {
    pub fn encode(value: f64) -> EncodedScalar {
        if value >= 0.0 {
            let high = (value / 65536.0).floor() * 65536.0;
            EncodedScalar { high, low: value - high }
        } else {
            let high = (-value / 65536.0).floor() * 65536.0;
            EncodedScalar { high: -high, low: value + high }
        }
    }

    pub fn from_cartesian(cartesian: Cartesian3) -> Self {
        let x = Self::encode(cartesian.x);
        let y = Self::encode(cartesian.y);
        let z = Self::encode(cartesian.z);
        Self { high: Cartesian3::new(x.high, y.high, z.high), low: Cartesian3::new(x.low, y.low, z.low) }
    }

    /// High components followed by low components, as in Cesium's buffers.
    pub fn to_array(&self) -> [f64; 6] {
        [self.high.x, self.high.y, self.high.z, self.low.x, self.low.y, self.low.z]
    }

    /// Writes six elements without changing surrounding buffer contents.
    /// Returns `None` if the destination is too short or the offset overflows.
    pub fn write_elements(cartesian: Cartesian3, buffer: &mut [f64], index: usize) -> Option<()> {
        let target = buffer.get_mut(index..index.checked_add(6)?)?;
        target.copy_from_slice(&Self::from_cartesian(cartesian).to_array());
        Some(())
    }

    /// Float32Array equivalent for GPU vertex buffers.
    pub fn write_elements_f32(cartesian: Cartesian3, buffer: &mut [f32], index: usize) -> Option<()> {
        let target = buffer.get_mut(index..index.checked_add(6)?)?;
        target.copy_from_slice(&Self::from_cartesian(cartesian).to_array().map(|v| v as f32));
        Some(())
    }
}
