//! CesiumJS `PostProcessStageSampleMode`: how a post-process stage samples its input texture.

numeric_enum!(PostProcessStageSampleMode: i32 {
    Nearest = 0 => "NEAREST",
    Linear = 1 => "LINEAR",
});
