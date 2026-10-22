//! CesiumJS `ModelAnimationLoop`: how a model animation repeats.

numeric_enum!(ModelAnimationLoop: i32 {
    None = 0 => "NONE",
    Repeat = 1 => "REPEAT",
    MirroredRepeat = 2 => "MIRRORED_REPEAT",
});
