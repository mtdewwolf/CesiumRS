//! CesiumJS `ShadowVolumeAppearance`: synthesize classification fragment
//! shaders for 3D Tiles and terrain clamping.

use super::appearance::Appearance;
use crate::shaders::root as root_shaders;

#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderDependencies {
    pub requires_ec: bool,
    pub requires_wc: bool,
    pub requires_texture_coordinates: bool,
    pub requires_normal_ec: bool,
    pub normal_ec: bool,
    pub position_to_eye_ec: bool,
    pub tangent_to_eye_matrix: bool,
    pub st: bool,
}

pub struct ShadowVolumeAppearance {
    pub extents_culling: bool,
    pub planar_extents: bool,
    pub appearance: Box<dyn Appearance>,
    pub color_dependencies: ShaderDependencies,
    pub pick_dependencies: ShaderDependencies,
}

impl ShadowVolumeAppearance {
    pub fn new(extents_culling: bool, planar_extents: bool, appearance: Box<dyn Appearance>) -> Self {
        let is_flat = appearance.flat().unwrap_or(false);
        let mut color_dependencies = ShaderDependencies {
            requires_texture_coordinates: extents_culling,
            requires_ec: !is_flat,
            ..Default::default()
        };

        let mut pick_dependencies =
            ShaderDependencies { requires_texture_coordinates: extents_culling, ..Default::default() };

        if let Some(mat) = appearance.material() {
            let combined = format!("{}\n{}", mat.shader_source, appearance.fragment_shader_source());
            color_dependencies.normal_ec =
                combined.contains("materialInput.normalEC") || combined.contains("czm_getDefaultMaterial");
            color_dependencies.position_to_eye_ec = combined.contains("materialInput.positionToEyeEC");
            color_dependencies.tangent_to_eye_matrix = combined.contains("materialInput.tangentToEyeMatrix");
            color_dependencies.st = combined.contains("materialInput.st");
        } else {
            color_dependencies.requires_normal_ec = !is_flat;
        }

        // Upstream dependency setters propagate requirements transitively.
        color_dependencies.requires_normal_ec |=
            color_dependencies.normal_ec || color_dependencies.tangent_to_eye_matrix;
        color_dependencies.requires_texture_coordinates |= color_dependencies.st;
        color_dependencies.requires_wc |=
            color_dependencies.requires_texture_coordinates || color_dependencies.tangent_to_eye_matrix;
        color_dependencies.requires_ec |= color_dependencies.requires_wc
            || color_dependencies.requires_normal_ec
            || color_dependencies.position_to_eye_ec;
        pick_dependencies.requires_wc |= pick_dependencies.requires_texture_coordinates;
        pick_dependencies.requires_ec |= pick_dependencies.requires_wc;
        Self { extents_culling, planar_extents, appearance, color_dependencies, pick_dependencies }
    }

    /// Derive the classification vertex stage with encoded projection extents.
    pub fn create_vertex_shader(
        &self,
        defines: &[String],
        source: &str,
        columbus_view_2d: bool,
        projection: &dyn crate::core::MapProjection,
    ) -> crate::renderer::ShaderSource {
        self.vertex_shader(defines, source, columbus_view_2d, projection, false)
    }

    pub fn create_pick_vertex_shader(
        &self,
        defines: &[String],
        source: &str,
        columbus_view_2d: bool,
        projection: &dyn crate::core::MapProjection,
    ) -> crate::renderer::ShaderSource {
        self.vertex_shader(defines, source, columbus_view_2d, projection, true)
    }

    fn vertex_shader(
        &self,
        defines: &[String],
        source: &str,
        cv: bool,
        projection: &dyn crate::core::MapProjection,
        pick: bool,
    ) -> crate::renderer::ShaderSource {
        let mut shader = crate::renderer::ShaderSource::new(source);
        shader.defines = defines.to_vec();
        if cv {
            for (name, longitude) in [("EAST", std::f64::consts::PI), ("WEST", -std::f64::consts::PI)] {
                let x = projection.project(&crate::core::Cartographic::new(longitude, 0.0, 0.0)).x;
                let encoded = crate::core::EncodedCartesian3::encode(x);
                for (part, value) in [("HIGH", encoded.high), ("LOW", encoded.low)] {
                    let precision = value.to_string().len() + 1;
                    shader.defines.push(format!("{name}_MOST_X_{part} {value:.precision$}"));
                }
            }
        }
        if !pick && self.appearance.material().is_none() {
            shader.defines.push("PER_INSTANCE_COLOR".into());
        }
        let dependencies = if pick { self.pick_dependencies } else { self.color_dependencies };
        if dependencies.requires_texture_coordinates {
            shader.defines.push("TEXTURE_COORDINATES".into());
            if !(self.planar_extents || cv) {
                shader.defines.push("SPHERICAL".into());
            }
            if cv {
                shader.defines.push("COLUMBUS_VIEW_2D".into());
            }
        }
        shader
    }

    /// Creates the fragment shader for the classification color pass.
    pub fn create_fragment_shader(&self, columbus_view_2d: bool) -> String {
        let mut defines = Vec::new();
        if !columbus_view_2d && !self.planar_extents {
            defines.push("#define SPHERICAL");
        }
        if self.color_dependencies.requires_ec {
            defines.push("#define REQUIRES_EC");
        }
        if self.color_dependencies.requires_wc {
            defines.push("#define REQUIRES_WC");
        }
        if self.color_dependencies.requires_texture_coordinates {
            defines.push("#define TEXTURE_COORDINATES");
        }
        if self.extents_culling {
            defines.push("#define CULL_FRAGMENTS");
        }
        if self.color_dependencies.requires_normal_ec {
            defines.push("#define NORMAL_EC");
        }
        if self.appearance.material().is_none() {
            defines.push("#define PER_INSTANCE_COLOR");
        }
        if self.color_dependencies.normal_ec {
            defines.push("#define USES_NORMAL_EC");
        }
        if self.color_dependencies.position_to_eye_ec {
            defines.push("#define USES_POSITION_TO_EYE_EC");
        }
        if self.color_dependencies.tangent_to_eye_matrix {
            defines.push("#define USES_TANGENT_TO_EYE");
        }
        if self.color_dependencies.st {
            defines.push("#define USES_ST");
        }
        if self.appearance.flat() == Some(true) {
            defines.push("#define FLAT");
        }

        let material_source = self.appearance.material().map_or("", |m| m.shader_source.as_str());
        format!("{}\n{}\n{}", defines.join("\n"), material_source, root_shaders::SHADOW_VOLUME_APPEARANCE_FS.source)
    }

    /// Creates the pick fragment shader for classification.
    pub fn create_pick_fragment_shader(&self, columbus_view_2d: bool) -> String {
        let mut defines = vec!["#define PICK"];
        if !columbus_view_2d && !self.planar_extents {
            defines.push("#define SPHERICAL");
        }
        if self.pick_dependencies.requires_ec {
            defines.push("#define REQUIRES_EC");
        }
        if self.pick_dependencies.requires_wc {
            defines.push("#define REQUIRES_WC");
        }
        if self.pick_dependencies.requires_texture_coordinates {
            defines.push("#define TEXTURE_COORDINATES");
        }
        if self.extents_culling {
            defines.push("#define CULL_FRAGMENTS");
        }

        format!("{}\n{}", defines.join("\n"), root_shaders::SHADOW_VOLUME_APPEARANCE_FS.source)
    }
}
