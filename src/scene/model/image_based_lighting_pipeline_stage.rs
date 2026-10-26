//! CesiumJS `Scene/Model/ImageBasedLightingPipelineStage`: adds shader code,
//! uniforms, and defines related to image-based lighting and spherical harmonics.

use super::ModelRenderResources;
use crate::{
    core::{Cartesian2, Cartesian3, Matrix3},
    renderer::{Gl, ShaderDestination, UniformValue},
    shaders,
};
use std::rc::Rc;

/// Options and parameters for image-based lighting on a model.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageBasedLightingOptions {
    /// Scaling factors for diffuse and specular image-based lighting.
    pub image_based_lighting_factor: Cartesian2,
    /// 3rd order spherical harmonic coefficients (9 vectors of XYZ).
    pub spherical_harmonic_coefficients: Option<[Cartesian3; 9]>,
    /// Whether spherical harmonics are used.
    pub use_spherical_harmonics: bool,
    /// Whether specular environment maps are used.
    pub use_specular_environment_maps: bool,
    /// General enable flag.
    pub enabled: bool,
    /// Whether to fall back to procedural default spherical harmonics.
    pub use_default_spherical_harmonics: bool,
    /// Whether to fall back to procedural default specular environment maps.
    pub use_default_specular_maps: bool,
    /// Model's reference frame rotation matrix.
    pub reference_frame_matrix: Matrix3,
    /// Whether a specular environment map or radiance atlas is ready.
    pub specular_cube_map_ready: bool,
    /// Whether an environment map radiance atlas is present.
    pub specular_environment_map_atlas: bool,
    /// Maximum mipmap level of the specular environment map.
    pub maximum_mipmap_level: f64,
    /// Overall intensity multiplier from the environment map manager (defaults to 1.0).
    pub environment_map_intensity: f64,
    /// Whether the WebGL context supports specular environment cube maps.
    pub specular_supported: bool,
}

impl Default for ImageBasedLightingOptions {
    fn default() -> Self {
        Self {
            image_based_lighting_factor: Cartesian2::ONE,
            spherical_harmonic_coefficients: None,
            use_spherical_harmonics: false,
            use_specular_environment_maps: false,
            enabled: true,
            use_default_spherical_harmonics: false,
            use_default_specular_maps: false,
            reference_frame_matrix: Matrix3::IDENTITY,
            specular_cube_map_ready: false,
            specular_environment_map_atlas: false,
            maximum_mipmap_level: 0.0,
            environment_map_intensity: 1.0,
            specular_supported: true,
        }
    }
}

/// Applies image-based lighting configuration to the model render resources.
pub fn process<G: Gl>(render_resources: &mut ModelRenderResources<G>, ibl: &ImageBasedLightingOptions) {
    render_resources.shader_builder.add_define("USE_IBL_LIGHTING", None, ShaderDestination::FRAGMENT);
    render_resources.shader_builder.add_uniform("vec2", "model_iblFactor", ShaderDestination::FRAGMENT);

    if ibl.specular_supported {
        let add_matrix = ibl.use_spherical_harmonics || ibl.use_specular_environment_maps || ibl.enabled;
        if add_matrix {
            render_resources.shader_builder.add_uniform(
                "mat3",
                "model_iblReferenceFrameMatrix",
                ShaderDestination::FRAGMENT,
            );
        }

        if ibl.specular_environment_map_atlas {
            render_resources.shader_builder.add_define("COMPUTE_POSITION_WC_ATMOSPHERE", None, ShaderDestination::BOTH);
        }

        if let Some(sh) = ibl.spherical_harmonic_coefficients {
            render_resources.shader_builder.add_define("DIFFUSE_IBL", None, ShaderDestination::FRAGMENT);
            render_resources.shader_builder.add_define("CUSTOM_SPHERICAL_HARMONICS", None, ShaderDestination::FRAGMENT);
            render_resources.shader_builder.add_uniform(
                "vec3",
                "model_sphericalHarmonicCoefficients[9]",
                ShaderDestination::FRAGMENT,
            );

            let sh_values: Vec<f32> = sh.iter().flat_map(|c| [c.x as f32, c.y as f32, c.z as f32]).collect();
            render_resources.uniform_map.insert(
                "model_sphericalHarmonicCoefficients".into(),
                Rc::new(move || UniformValue::FloatArray { values: sh_values.clone(), components: 3 }),
            );
        } else if ibl.use_default_spherical_harmonics {
            render_resources.shader_builder.add_define("DIFFUSE_IBL", None, ShaderDestination::FRAGMENT);
        }

        if ibl.specular_cube_map_ready || ibl.specular_environment_map_atlas {
            render_resources.shader_builder.add_define("SPECULAR_IBL", None, ShaderDestination::FRAGMENT);
            render_resources.shader_builder.add_define("CUSTOM_SPECULAR_IBL", None, ShaderDestination::FRAGMENT);
            render_resources.shader_builder.add_uniform(
                "samplerCube",
                "model_specularEnvironmentMaps",
                ShaderDestination::FRAGMENT,
            );
            render_resources.shader_builder.add_uniform(
                "float",
                "model_specularEnvironmentMapsMaximumLOD",
                ShaderDestination::FRAGMENT,
            );

            let max_lod = ibl.maximum_mipmap_level as f32;
            render_resources.uniform_map.insert(
                "model_specularEnvironmentMapsMaximumLOD".into(),
                Rc::new(move || UniformValue::Float(vec![max_lod])),
            );
        } else if ibl.use_default_specular_maps {
            render_resources.shader_builder.add_define("SPECULAR_IBL", None, ShaderDestination::FRAGMENT);
        }
    }

    let mat_vals: Vec<f32> = ibl.reference_frame_matrix.0.iter().map(|&v| v as f32).collect();
    render_resources.uniform_map.insert(
        "model_iblReferenceFrameMatrix".into(),
        Rc::new(move || UniformValue::Matrix { dimension: 3, values: mat_vals.clone() }),
    );

    render_resources.shader_builder.add_fragment_lines(&[shaders::model::IMAGE_BASED_LIGHTING_STAGE_FS.source]);

    let factor_x = (ibl.image_based_lighting_factor.x * ibl.environment_map_intensity) as f32;
    let factor_y = (ibl.image_based_lighting_factor.y * ibl.environment_map_intensity) as f32;
    render_resources
        .uniform_map
        .insert("model_iblFactor".into(), Rc::new(move || UniformValue::Float(vec![factor_x, factor_y])));
}
