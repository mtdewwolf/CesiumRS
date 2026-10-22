//! CesiumJS `VertexAttributeSemantic`: the per-vertex attributes a model
//! primitive can carry, and their shader names and types.

string_enum!(VertexAttributeSemantic {
    Position = "POSITION" => "POSITION",
    Normal = "NORMAL" => "NORMAL",
    Tangent = "TANGENT" => "TANGENT",
    Texcoord = "TEXCOORD" => "TEXCOORD",
    Color = "COLOR" => "COLOR",
    Joints = "JOINTS" => "JOINTS",
    Weights = "WEIGHTS" => "WEIGHTS",
    FeatureId = "_FEATURE_ID" => "FEATURE_ID",
    Scale = "KHR_gaussian_splatting:SCALE" => "SCALE",
    Rotation = "KHR_gaussian_splatting:ROTATION" => "ROTATION",
});

impl VertexAttributeSemantic {
    /// Whether glTF numbers the semantic with a `_<n>` set index.
    pub fn has_set_index(self) -> bool {
        !matches!(self, Self::Position | Self::Normal | Self::Tangent)
    }

    /// The semantic of a glTF attribute name. A trailing `_<n>` set index is
    /// ignored, and `_SCALE` / `_ROTATION` are the pre-ratification splat
    /// names. Unknown names give `None`.
    pub fn from_gltf_semantic(gltf_semantic: &str) -> Option<Self> {
        match super::strip_set_index(gltf_semantic) {
            "_SCALE" => Some(Self::Scale),
            "_ROTATION" => Some(Self::Rotation),
            semantic => Self::from_str_value(semantic),
        }
    }

    /// The semantic of a pnts feature-table property; `None` for any other
    /// name, where Cesium throws.
    pub fn from_pnts_semantic(pnts_semantic: &str) -> Option<Self> {
        Some(match pnts_semantic {
            "POSITION" | "POSITION_QUANTIZED" => Self::Position,
            "RGBA" | "RGB" | "RGB565" => Self::Color,
            "NORMAL" | "NORMAL_OCT16P" => Self::Normal,
            "BATCH_ID" => Self::FeatureId,
            _ => return None,
        })
    }

    pub fn get_glsl_type(self) -> &'static str {
        match self {
            Self::Position | Self::Normal | Self::Tangent | Self::Scale => "vec3",
            Self::Texcoord => "vec2",
            Self::Color | Self::Weights | Self::Rotation => "vec4",
            Self::Joints => "ivec4",
            Self::FeatureId => "int",
        }
    }

    /// The shader variable: `positionMC`, `texCoord_0`, and so on.
    pub fn get_variable_name(self, set_index: Option<u32>) -> String {
        let name = match self {
            Self::Position => "positionMC",
            Self::Normal => "normalMC",
            Self::Tangent => "tangentMC",
            Self::Texcoord => "texCoord",
            Self::Color => "color",
            Self::Joints => "joints",
            Self::Weights => "weights",
            Self::FeatureId => "featureId",
            Self::Scale => "scale",
            Self::Rotation => "rotation",
        };
        match set_index {
            Some(index) => format!("{name}_{index}"),
            None => name.to_string(),
        }
    }
}
