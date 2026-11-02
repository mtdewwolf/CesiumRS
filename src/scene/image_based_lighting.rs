//! CesiumJS `ImageBasedLighting`: diffuse and specular image-based lighting
//! factors, spherical harmonics and specular environment maps for tilesets
//! and models, and the per-frame shader-regeneration state machine.
//!
//! `SpecularEnvironmentCubeMap` loads KTX2 files and uploads textures, which
//! is host work in this port: the host installs a
//! [`SpecularEnvironmentCubeMapFactory`] that creates
//! [`SpecularEnvironmentCubeMap`] handles. Support is decided from the frame's
//! context capabilities exactly as `SpecularEnvironmentCubeMap.isSupported`
//! does. Without a factory a supported context still goes through Cesium's
//! dirty/regenerate cycle, but no cube map is created. The scene updates the
//! BRDF look-up table generator that Cesium's `update` also touches.

use super::frame_state::{FrameResource, FrameState};
use crate::core::math::js_number_to_string;
use crate::core::{Cartesian2, Cartesian3, RuntimeError};
use crate::renderer::ContextCapabilities;

/// A loading or loaded specular environment cube map.
pub trait SpecularEnvironmentCubeMap {
    fn url(&self) -> &str;
    fn ready(&self) -> bool;
    /// Called once per updated frame while the cube map is current.
    fn update(&mut self, frame: &mut FrameState);
    /// Releases the cube map's resources.
    fn destroy(&mut self) {}
}

/// Creates the cube map for a `specularEnvironmentMaps` URL.
pub type SpecularEnvironmentCubeMapFactory = Box<dyn FnMut(&str) -> Box<dyn SpecularEnvironmentCubeMap>>;

/// `SpecularEnvironmentCubeMap.isSupported`: float or half-float color
/// buffers and textures, and texture LOD.
pub fn specular_environment_cube_map_supported(context: &ContextCapabilities) -> bool {
    let float_buffers_and_textures = (context.color_buffer_half_float && context.half_floating_point_texture)
        || (context.floating_point_texture && context.color_buffer_float);
    float_buffers_and_textures && context.supports_texture_lod
}

/// Constructor options for [`ImageBasedLighting::with_options`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageBasedLightingConstructorOptions {
    /// Defaults to `(1, 1)`.
    pub image_based_lighting_factor: Option<Cartesian2>,
    pub spherical_harmonic_coefficients: Option<[Cartesian3; 9]>,
    pub specular_environment_maps: Option<String>,
}

pub struct ImageBasedLighting {
    image_based_lighting_factor: Cartesian2,
    spherical_harmonic_coefficients: Option<[Cartesian3; 9]>,
    specular_environment_maps: Option<String>,
    specular_environment_cube_map: Option<Box<dyn SpecularEnvironmentCubeMap>>,
    specular_environment_cube_map_dirty: bool,
    specular_environment_map_loaded: bool,
    previous_specular_environment_map_loaded: bool,
    use_default_specular_maps: bool,
    use_default_spherical_harmonics: bool,
    should_regenerate_shaders: bool,
    previous_frame_number: Option<u32>,
    /// `_previousFrameContext`; `Some(None)` is a frame without a context.
    previous_frame_context: Option<Option<FrameResource>>,
    previous_image_based_lighting_factor: Cartesian2,
    previous_spherical_harmonic_coefficients: Option<[Cartesian3; 9]>,
    factory: Option<SpecularEnvironmentCubeMapFactory>,
    destroyed: bool,
}

impl std::fmt::Debug for ImageBasedLighting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageBasedLighting")
            .field("image_based_lighting_factor", &self.image_based_lighting_factor)
            .field("spherical_harmonic_coefficients", &self.spherical_harmonic_coefficients)
            .field("specular_environment_maps", &self.specular_environment_maps)
            .field("specular_environment_cube_map", &self.specular_environment_cube_map.as_ref().map(|m| m.url()))
            .field("should_regenerate_shaders", &self.should_regenerate_shaders)
            .finish_non_exhaustive()
    }
}

impl Default for ImageBasedLighting {
    fn default() -> Self {
        Self::new()
    }
}

/// Cesium's `Check` range tests: NaN passes both.
fn check_factor(prefix: &str, factor: Cartesian2) -> Result<(), RuntimeError> {
    for (axis, value) in [("x", factor.x), ("y", factor.y)] {
        let shown = js_number_to_string(value);
        if value < 0.0 {
            return Err(RuntimeError::new(format!(
                "Expected {prefix}.{axis} to be greater than or equal to 0, actual value was {shown}"
            )));
        }
        if value > 1.0 {
            return Err(RuntimeError::new(format!(
                "Expected {prefix}.{axis} to be less than or equal to 1, actual value was {shown}"
            )));
        }
    }
    Ok(())
}

impl ImageBasedLighting {
    pub fn new() -> Self {
        Self::with_options(ImageBasedLightingConstructorOptions::default())
            .expect("default image-based lighting options are valid")
    }

    pub fn with_options(options: ImageBasedLightingConstructorOptions) -> Result<Self, RuntimeError> {
        let factor = options.image_based_lighting_factor.unwrap_or(Cartesian2::new(1.0, 1.0));
        check_factor("options.imageBasedLightingFactor", factor)?;
        Ok(Self {
            image_based_lighting_factor: factor,
            spherical_harmonic_coefficients: options.spherical_harmonic_coefficients,
            specular_environment_maps: options.specular_environment_maps,
            specular_environment_cube_map: None,
            specular_environment_cube_map_dirty: true,
            specular_environment_map_loaded: false,
            previous_specular_environment_map_loaded: false,
            use_default_specular_maps: false,
            use_default_spherical_harmonics: false,
            should_regenerate_shaders: false,
            previous_frame_number: None,
            previous_frame_context: None,
            previous_image_based_lighting_factor: factor,
            previous_spherical_harmonic_coefficients: options.spherical_harmonic_coefficients,
            factory: None,
            destroyed: false,
        })
    }

    /// Installs the host's cube-map loader.
    pub fn set_specular_environment_cube_map_factory(&mut self, factory: SpecularEnvironmentCubeMapFactory) {
        self.factory = Some(factory);
    }

    pub fn image_based_lighting_factor(&self) -> Cartesian2 {
        self.image_based_lighting_factor
    }

    /// Components must be in [0, 1]. The replaced value becomes the one the
    /// next update compares against.
    pub fn set_image_based_lighting_factor(&mut self, factor: Cartesian2) -> Result<(), RuntimeError> {
        check_factor("imageBasedLightingFactor", factor)?;
        self.previous_image_based_lighting_factor = self.image_based_lighting_factor;
        self.image_based_lighting_factor = factor;
        Ok(())
    }

    pub fn spherical_harmonic_coefficients(&self) -> Option<&[Cartesian3; 9]> {
        self.spherical_harmonic_coefficients.as_ref()
    }

    pub fn set_spherical_harmonic_coefficients(&mut self, coefficients: Option<[Cartesian3; 9]>) {
        self.previous_spherical_harmonic_coefficients = self.spherical_harmonic_coefficients;
        self.spherical_harmonic_coefficients = coefficients;
    }

    pub fn specular_environment_maps(&self) -> Option<&str> {
        self.specular_environment_maps.as_deref()
    }

    /// A different URL marks the cube map dirty and not loaded.
    pub fn set_specular_environment_maps(&mut self, url: Option<String>) {
        if url != self.specular_environment_maps {
            self.specular_environment_cube_map_dirty = true;
            self.specular_environment_map_loaded = false;
        }
        self.specular_environment_maps = url;
    }

    /// `enabled`: either factor is positive.
    pub fn is_enabled(&self) -> bool {
        self.image_based_lighting_factor.x > 0.0 || self.image_based_lighting_factor.y > 0.0
    }

    pub fn should_regenerate_shaders(&self) -> bool {
        self.should_regenerate_shaders
    }

    pub fn specular_environment_cube_map(&self) -> Option<&dyn SpecularEnvironmentCubeMap> {
        self.specular_environment_cube_map.as_deref()
    }

    pub fn use_default_spherical_harmonics(&self) -> bool {
        self.use_default_spherical_harmonics
    }

    pub fn use_default_specular_maps(&self) -> bool {
        self.use_default_specular_maps
    }

    /// A ready cube map, or the scene's default maps.
    pub fn use_specular_environment_maps(&self) -> bool {
        self.specular_environment_cube_map.as_ref().is_some_and(|m| m.ready()) || self.use_default_specular_maps
    }

    /// `createSpecularEnvironmentCubeMap`.
    fn create_specular_environment_cube_map(&mut self, context: &ContextCapabilities) {
        if !specular_environment_cube_map_supported(context) {
            return;
        }
        if let Some(mut old) = self.specular_environment_cube_map.take() {
            old.destroy();
        }
        if let (Some(url), Some(factory)) = (&self.specular_environment_maps, &mut self.factory) {
            self.specular_environment_cube_map = Some(factory(url));
        }
        self.should_regenerate_shaders = true;
    }

    /// Cesium's once-per-frame update, skipped when the frame number and
    /// context both match the previous call.
    pub fn update(&mut self, frame: &mut FrameState) {
        if self.previous_frame_number == Some(frame.frame_number)
            && self.previous_frame_context.as_ref() == Some(&frame.context)
        {
            return;
        }
        self.previous_frame_number = Some(frame.frame_number);
        self.previous_frame_context = Some(frame.context.clone());

        self.should_regenerate_shaders = false;

        let factor = self.image_based_lighting_factor;
        let previous = self.previous_image_based_lighting_factor;
        if factor != previous {
            let toggled =
                |current: f64, before: f64| (current > 0.0 && before == 0.0) || (current == 0.0 && before > 0.0);
            self.should_regenerate_shaders = toggled(factor.x, previous.x) || toggled(factor.y, previous.y);
            self.previous_image_based_lighting_factor = factor;
        }

        if self.previous_spherical_harmonic_coefficients != self.spherical_harmonic_coefficients {
            self.should_regenerate_shaders |= self.previous_spherical_harmonic_coefficients.is_some()
                != self.spherical_harmonic_coefficients.is_some();
            self.previous_spherical_harmonic_coefficients = self.spherical_harmonic_coefficients;
        }

        self.should_regenerate_shaders |=
            self.previous_specular_environment_map_loaded != self.specular_environment_map_loaded;
        self.previous_specular_environment_map_loaded = self.specular_environment_map_loaded;

        if self.specular_environment_cube_map_dirty {
            let capabilities = frame.context_capabilities.clone();
            self.create_specular_environment_cube_map(&capabilities);
            self.specular_environment_cube_map_dirty = false;
        }

        if let Some(cube_map) = &mut self.specular_environment_cube_map {
            cube_map.update(frame);
            if cube_map.ready() {
                self.specular_environment_map_loaded = true;
            }
        }

        let has_cube_map = self.specular_environment_cube_map.is_some();
        let frame_maps = frame.specular_environment_maps.is_some();
        let frame_sh = frame.spherical_harmonic_coefficients.is_some();
        let own_sh = self.spherical_harmonic_coefficients.is_some();
        let recompile_with_default_cube_map = !has_cube_map && frame_maps && !self.use_default_specular_maps;
        let recompile_without_default_cube_map = !frame_maps && self.use_default_specular_maps;
        let recompile_with_default_sh = !own_sh && frame_sh && !self.use_default_spherical_harmonics;
        let recompile_without_default_sh = !frame_sh && self.use_default_spherical_harmonics;
        self.should_regenerate_shaders = self.should_regenerate_shaders
            || recompile_with_default_cube_map
            || recompile_without_default_cube_map
            || recompile_with_default_sh
            || recompile_without_default_sh;
        self.use_default_specular_maps = !has_cube_map && frame_maps;
        self.use_default_spherical_harmonics = !own_sh && frame_sh;
    }

    /// Destroys the cube map.
    pub fn destroy(&mut self) {
        if let Some(mut cube_map) = self.specular_environment_cube_map.take() {
            cube_map.destroy();
        }
        self.destroyed = true;
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
