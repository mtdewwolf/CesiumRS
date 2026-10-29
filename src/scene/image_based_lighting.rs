//! CesiumJS `ImageBasedLighting`: properties and lifecycle management for
//! image-based lighting on tilesets and models.

use super::frame_state::{FrameResource, FrameState};
use crate::core::{Cartesian2, Cartesian3, RuntimeError};

/// Properties and runtime state for managing image-based lighting (IBL)
/// on 3D tilesets and models.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageBasedLighting {
    image_based_lighting_factor: Cartesian2,
    previous_factor: Cartesian2,
    pub spherical_harmonic_coefficients: Option<[Cartesian3; 9]>,
    previous_spherical_harmonics: Option<[Cartesian3; 9]>,
    pub specular_environment_maps: Option<String>,
    pub luminance_at_zenith: f64,
    should_regenerate_shaders: bool,
    use_default_spherical_harmonics: bool,
    use_default_specular_maps: bool,
    loaded_specular_map: Option<(String, FrameResource)>,
    previous_specular_ready: bool,
    previous_frame: Option<(u32, Option<FrameResource>)>,
    destroyed: bool,
}

impl Default for ImageBasedLighting {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageBasedLighting {
    /// Creates a new `ImageBasedLighting` manager with default lighting parameters.
    pub fn new() -> Self {
        Self {
            image_based_lighting_factor: Cartesian2::new(1.0, 1.0),
            previous_factor: Cartesian2::new(1.0, 1.0),
            spherical_harmonic_coefficients: None,
            previous_spherical_harmonics: None,
            specular_environment_maps: None,
            luminance_at_zenith: 0.2,
            should_regenerate_shaders: false,
            use_default_spherical_harmonics: false,
            use_default_specular_maps: false,
            loaded_specular_map: None,
            previous_specular_ready: false,
            previous_frame: None,
            destroyed: false,
        }
    }

    /// Scaling factors for diffuse and specular image-based lighting.
    pub fn image_based_lighting_factor(&self) -> Cartesian2 {
        self.image_based_lighting_factor
    }

    /// Sets the image-based lighting factor. Components must be in [0.0, 1.0].
    pub fn set_image_based_lighting_factor(&mut self, factor: Cartesian2) -> Result<(), RuntimeError> {
        if factor.x < 0.0 || factor.x > 1.0 || factor.y < 0.0 || factor.y > 1.0 {
            return Err(RuntimeError::new("image_based_lighting_factor components must be between 0.0 and 1.0."));
        }
        self.image_based_lighting_factor = factor;
        Ok(())
    }

    /// Returns `true` if image-based lighting has a non-zero contribution.
    pub fn is_enabled(&self) -> bool {
        self.image_based_lighting_factor.x > 0.0 || self.image_based_lighting_factor.y > 0.0
    }

    /// Whether models using this lighting should regenerate their shader programs.
    pub fn should_regenerate_shaders(&self) -> bool {
        self.should_regenerate_shaders
    }

    /// Whether to fall back to procedural default spherical harmonics coefficients.
    pub fn use_default_spherical_harmonics(&self) -> bool {
        self.use_default_spherical_harmonics
    }

    /// Whether to fall back to procedural default specular environment maps.
    pub fn use_default_specular_maps(&self) -> bool {
        self.use_default_specular_maps
    }

    /// Whether image-based lighting currently uses specular environment maps.
    pub fn use_specular_environment_maps(&self) -> bool {
        self.specular_environment_map().is_some() || self.use_default_specular_maps
    }

    /// Retain a loaded GPU environment resource. A URL by itself is never ready.
    /// The loading owner supplies the atlas/cube map after upload has succeeded.
    pub fn set_specular_environment_map(&mut self, url: String, resource: FrameResource) {
        self.loaded_specular_map = Some((url, resource));
    }

    pub fn specular_environment_map(&self) -> Option<&FrameResource> {
        self.loaded_specular_map
            .as_ref()
            .filter(|(url, _)| Some(url) == self.specular_environment_maps.as_ref())
            .map(|(_, map)| map)
    }

    /// Updates image-based lighting state and detects dirty shader boundaries for the frame.
    pub fn update(&mut self, frame: &mut FrameState) {
        let frame_key = (frame.frame_number, frame.context.clone());
        if self.previous_frame.as_ref() == Some(&frame_key) {
            return;
        }
        self.previous_frame = Some(frame_key);
        if self
            .loaded_specular_map
            .as_ref()
            .is_some_and(|(url, _)| Some(url) != self.specular_environment_maps.as_ref())
        {
            self.loaded_specular_map = None;
        }
        let specular_ready = self.specular_environment_map().is_some();
        let default_specular = self.specular_environment_maps.is_none() && frame.specular_environment_maps.is_some();
        let default_sh =
            self.spherical_harmonic_coefficients.is_none() && frame.spherical_harmonic_coefficients.is_some();
        let current_factor = self.image_based_lighting_factor;
        let factor_changed = ((current_factor.x > 0.0) != (self.previous_factor.x > 0.0))
            || ((current_factor.y > 0.0) != (self.previous_factor.y > 0.0));

        let sh_changed = self.spherical_harmonic_coefficients.is_some() != self.previous_spherical_harmonics.is_some();

        self.should_regenerate_shaders = factor_changed
            || sh_changed
            || specular_ready != self.previous_specular_ready
            || default_specular != self.use_default_specular_maps
            || default_sh != self.use_default_spherical_harmonics;
        self.previous_specular_ready = specular_ready;
        self.use_default_specular_maps = default_specular;
        self.use_default_spherical_harmonics = default_sh;

        self.previous_factor = current_factor;
        self.previous_spherical_harmonics = self.spherical_harmonic_coefficients;
    }

    /// Destroys and cleans up image-based lighting resources.
    pub fn destroy(&mut self) {
        self.loaded_specular_map = None;
        self.destroyed = true;
    }

    /// Returns whether this instance has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
