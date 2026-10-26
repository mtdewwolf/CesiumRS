//! CesiumJS `CustomShaderTranslucencyMode`: whether a custom shader forces a model opaque or translucent.

numeric_enum!(CustomShaderTranslucencyMode: i32 {
    Inherit = 0 => "INHERIT",
    Opaque = 1 => "OPAQUE",
    Translucent = 2 => "TRANSLUCENT",
});

#[allow(clippy::derivable_impls)]
impl Default for CustomShaderTranslucencyMode {
    fn default() -> Self {
        Self::Inherit
    }
}
