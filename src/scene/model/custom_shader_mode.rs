//! CesiumJS `CustomShaderMode`: whether a custom shader modifies or replaces
//! the material.

string_enum!(CustomShaderMode {
    ModifyMaterial = "MODIFY_MATERIAL" => "MODIFY_MATERIAL",
    ReplaceMaterial = "REPLACE_MATERIAL" => "REPLACE_MATERIAL",
});

impl CustomShaderMode {
    /// The shader define for the mode, `CUSTOM_SHADER_<mode>`.
    pub fn get_define_name(self) -> String {
        format!("CUSTOM_SHADER_{}", self.as_str())
    }
}
