//! CesiumJS `LightingModel`: the lighting a model's shader applies.

numeric_enum!(LightingModel: i32 {
    Unlit = 0 => "UNLIT",
    Pbr = 1 => "PBR",
});
