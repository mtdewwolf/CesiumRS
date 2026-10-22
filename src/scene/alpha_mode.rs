//! CesiumJS `AlphaMode`: how a glTF material's alpha is interpreted.

string_enum!(AlphaMode {
    Opaque = "OPAQUE" => "OPAQUE",
    Mask = "MASK" => "MASK",
    Blend = "BLEND" => "BLEND",
});
