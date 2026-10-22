//! CesiumJS `InstanceAttributeSemantic`: the per-instance attributes of
//! `EXT_mesh_gpu_instancing` and i3dm.

string_enum!(InstanceAttributeSemantic {
    Translation = "TRANSLATION" => "TRANSLATION",
    Rotation = "ROTATION" => "ROTATION",
    Scale = "SCALE" => "SCALE",
    FeatureId = "_FEATURE_ID" => "FEATURE_ID",
});

impl InstanceAttributeSemantic {
    /// The semantic of a glTF instancing attribute name. A trailing `_<n>` set
    /// index is ignored; unknown names give `None`.
    pub fn from_gltf_semantic(gltf_semantic: &str) -> Option<Self> {
        Self::from_str_value(super::strip_set_index(gltf_semantic))
    }
}
