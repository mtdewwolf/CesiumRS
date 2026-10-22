//! CesiumJS `BlendOption`: which billboard, label or point passes are drawn.

numeric_enum!(BlendOption: i32 {
    Opaque = 0 => "OPAQUE",
    Translucent = 1 => "TRANSLUCENT",
    OpaqueAndTranslucent = 2 => "OPAQUE_AND_TRANSLUCENT",
});
