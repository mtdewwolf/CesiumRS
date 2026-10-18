//! CesiumJS `ArticulationStageType`: the transform an articulation stage applies.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArticulationStageType {
    XTranslate,
    YTranslate,
    ZTranslate,
    XRotate,
    YRotate,
    ZRotate,
    XScale,
    YScale,
    ZScale,
    UniformScale,
}

impl ArticulationStageType {
    pub const ALL: [Self; 10] = [
        Self::XTranslate,
        Self::YTranslate,
        Self::ZTranslate,
        Self::XRotate,
        Self::YRotate,
        Self::ZRotate,
        Self::XScale,
        Self::YScale,
        Self::ZScale,
        Self::UniformScale,
    ];

    /// Cesium's string value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::XTranslate => "xTranslate",
            Self::YTranslate => "yTranslate",
            Self::ZTranslate => "zTranslate",
            Self::XRotate => "xRotate",
            Self::YRotate => "yRotate",
            Self::ZRotate => "zRotate",
            Self::XScale => "xScale",
            Self::YScale => "yScale",
            Self::ZScale => "zScale",
            Self::UniformScale => "uniformScale",
        }
    }

    pub fn from_str_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == value)
    }
}
