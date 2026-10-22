//! CesiumJS `SceneMode`: 3D, 2D, Columbus view, or morphing between them.

numeric_enum!(SceneMode: i32 {
    Morphing = 0 => "MORPHING",
    ColumbusView = 1 => "COLUMBUS_VIEW",
    Scene2D = 2 => "SCENE2D",
    Scene3D = 3 => "SCENE3D",
});

impl SceneMode {
    /// The morph time for a settled mode: 1 in 3D, 0 in 2D and Columbus
    /// view, and `None` while morphing.
    pub fn get_morph_time(self) -> Option<f64> {
        match self {
            Self::Scene3D => Some(1.0),
            Self::Morphing => None,
            Self::ColumbusView | Self::Scene2D => Some(0.0),
        }
    }
}
