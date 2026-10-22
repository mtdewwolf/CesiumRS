//! CesiumJS `DynamicAtmosphereLightingType`: the light source the atmosphere
//! and fog are lit by.

numeric_enum!(DynamicAtmosphereLightingType: i32 {
    None = 0 => "NONE",
    SceneLight = 1 => "SCENE_LIGHT",
    Sunlight = 2 => "SUNLIGHT",
});

/// The `Globe` flags `from_globe_flags` reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlobeLightingFlags {
    pub enable_lighting: bool,
    pub dynamic_atmosphere_lighting: bool,
    pub dynamic_atmosphere_lighting_from_sun: bool,
}

impl DynamicAtmosphereLightingType {
    pub fn from_globe_flags(globe: GlobeLightingFlags) -> Self {
        if !(globe.enable_lighting && globe.dynamic_atmosphere_lighting) {
            return Self::None;
        }
        if globe.dynamic_atmosphere_lighting_from_sun {
            return Self::Sunlight;
        }
        Self::SceneLight
    }
}
