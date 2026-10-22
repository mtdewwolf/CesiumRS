//! CesiumJS `ImageryFlags`: which imagery layer adjustments a model's
//! imagery shader has to apply.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageryFlags {
    pub alpha: bool,
    pub brightness: bool,
    pub contrast: bool,
    pub hue: bool,
    pub saturation: bool,
    pub gamma: bool,
    pub color_to_alpha: bool,
}
