//! CesiumJS `TranslationRotationScale`: an affine transform in parts.

use super::cartesian3::Cartesian3;
use super::quaternion::Quaternion;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TranslationRotationScale {
    pub translation: Cartesian3,
    pub rotation: Quaternion,
    pub scale: Cartesian3,
}

impl Default for TranslationRotationScale {
    fn default() -> Self {
        Self {
            translation: Cartesian3::ZERO,
            rotation: Quaternion::IDENTITY,
            scale: Cartesian3::ONE,
        }
    }
}

impl TranslationRotationScale {
    pub fn new(translation: Cartesian3, rotation: Quaternion, scale: Cartesian3) -> Self {
        Self { translation, rotation, scale }
    }
}
