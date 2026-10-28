//! CPU-side state and attribute representation for CesiumJS `CumulusCloud`.
//!
//! # Architecture and Parity Status
//! This module provides partial CPU-side behavior for CesiumJS `CumulusCloud`
//! corresponding to `@cesium/engine 23.0.0`:
//! - Full parameter model: `show`, `position`, `scale`, `maximum_size`, `slice`,
//!   `brightness`, `color`, and `cloud_type`.
//! - Dimension derivation rules matching upstream constructor:
//!   - If `scale` is unspecified but `maximum_size` is provided, `scale` is set to
//!     `(maximum_size.x, maximum_size.y)`.
//!   - Otherwise, `scale` defaults to `(20.0, 12.0)`, and `maximum_size` defaults
//!     to `(scale.x, scale.y, min(scale.x, scale.y) / 1.5)`.
//! - Per-property mutation tracking with CesiumJS-aligned property update indices:
//!   `SHOW_INDEX` (0), `POSITION_INDEX` (1), `SCALE_INDEX` (2), `MAXIMUM_SIZE_INDEX` (3),
//!   `SLICE_INDEX` (4), `BRIGHTNESS_INDEX` (5), and `COLOR_INDEX` (6).
//! - Equality testing matching visual/logical cloud properties.
//!
//! # GPU and Rendering Limitations
//! - Volumetric & Billboard Shaders: `CloudCollectionVS.glsl` and `CloudCollectionFS.glsl`
//!   rendering, noise texture sampling, ray-ellipsoid cross-section slicing, and 3D Perlin/Worley
//!   noise generation require an active WebGL/WebGPU graphics context and shader pipeline.
//! - Vertex Array Facade (VAF): Dynamic GPU vertex buffer allocation and index buffer binding
//!   are deferred to the GPU backend.
//! - Fake/Stub Resource Avoidance: Render resources (textures, framebuffers, uniform maps)
//!   are intentionally omitted here to prevent unverified or fabricated graphics behavior on CPU.

use crate::{
    core::{Cartesian2, Cartesian3, Color},
    scene::CloudType,
};

// Property update indices matching CesiumJS CumulusCloud static indices.
pub const SHOW_INDEX: usize = 0;
pub const POSITION_INDEX: usize = 1;
pub const SCALE_INDEX: usize = 2;
pub const MAXIMUM_SIZE_INDEX: usize = 3;
pub const SLICE_INDEX: usize = 4;
pub const BRIGHTNESS_INDEX: usize = 5;
pub const COLOR_INDEX: usize = 6;
pub const NUMBER_OF_PROPERTIES: usize = 7;

/// Configuration options for creating or mutating a [`CumulusCloud`].
#[derive(Clone, Debug, PartialEq)]
pub struct CumulusCloudOptions {
    /// Determines if this cumulus cloud will be shown.
    pub show: bool,
    /// The Cartesian position of this cumulus cloud.
    pub position: Cartesian3,
    /// The 2D scale in meters of the cloud billboard.
    pub scale: Option<Cartesian2>,
    /// The 3D maximum ellipsoid volume that the cloud can appear in.
    pub maximum_size: Option<Cartesian3>,
    /// Cross-section slice of the cloud (between 0.0 and 1.0, or negative for entire ellipsoid).
    pub slice: f64,
    /// The color of the cloud.
    pub color: Color,
    /// The brightness multiplier of the cloud (0.0 to 1.0+).
    pub brightness: f64,
    /// The type of the cloud (only `CloudType::Cumulus` is supported).
    pub cloud_type: CloudType,
}

impl Default for CumulusCloudOptions {
    fn default() -> Self {
        Self {
            show: true,
            position: Cartesian3::ZERO,
            scale: None,
            maximum_size: None,
            slice: -1.0,
            color: Color::WHITE,
            brightness: 1.0,
            cloud_type: CloudType::Cumulus,
        }
    }
}

impl CumulusCloudOptions {
    /// Resolves effective `(scale, maximum_size)` pair following CesiumJS rules.
    ///
    /// - If `scale` is unspecified but `maximum_size` is specified:
    ///   `scale` = `(maximum_size.x, maximum_size.y)`.
    /// - Otherwise:
    ///   `scale` defaults to `(20.0, 12.0)`.
    ///   `maximum_size` defaults to `(scale.x, scale.y, min(scale.x, scale.y) / 1.5)`.
    pub fn resolve_dimensions(
        scale: Option<Cartesian2>,
        maximum_size: Option<Cartesian3>,
    ) -> (Cartesian2, Cartesian3) {
        if let (None, Some(max_sz)) = (scale, maximum_size) {
            let sc = Cartesian2::new(max_sz.x, max_sz.y);
            (sc, max_sz)
        } else {
            let sc = scale.unwrap_or(Cartesian2::new(20.0, 12.0));
            let default_max_z = sc.x.min(sc.y) / 1.5;
            let default_max = Cartesian3::new(sc.x, sc.y, default_max_z);
            let max_sz = maximum_size.unwrap_or(default_max);
            (sc, max_sz)
        }
    }

    /// Compatibility no-op: Rust types restrict cloud type, and Cesium accepts
    /// the numeric values in these options without range validation.
    pub fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

/// A cumulus cloud billboard positioned in world coordinates.
#[derive(Debug)]
pub struct CumulusCloud {
    show: bool,
    position: Cartesian3,
    scale: Cartesian2,
    maximum_size: Cartesian3,
    slice: f64,
    color: Color,
    brightness: f64,
    cloud_type: CloudType,

    dirty: bool,
    dirty_properties: [bool; NUMBER_OF_PROPERTIES],
    collection_index: usize,
    attached: bool,
}

impl CumulusCloud {
    /// Creates a new cumulus cloud instance from validated options.
    pub(crate) fn new(options: CumulusCloudOptions, collection_index: usize) -> Result<Self, &'static str> {
        options.validate()?;
        let (scale, maximum_size) =
            CumulusCloudOptions::resolve_dimensions(options.scale, options.maximum_size);

        Ok(Self {
            show: options.show,
            position: options.position,
            scale,
            maximum_size,
            slice: options.slice,
            color: options.color,
            brightness: options.brightness,
            cloud_type: options.cloud_type,
            dirty: true,
            dirty_properties: [true; NUMBER_OF_PROPERTIES],
            collection_index,
            attached: true,
        })
    }

    pub fn show(&self) -> bool {
        self.show
    }

    pub fn set_show(&mut self, value: bool) {
        if self.show != value {
            self.show = value;
            self.make_dirty(SHOW_INDEX);
        }
    }

    pub fn position(&self) -> Cartesian3 {
        self.position
    }

    pub fn set_position(&mut self, value: Cartesian3) {
        if self.position != value {
            self.position = value;
            self.make_dirty(POSITION_INDEX);
        }
    }

    pub fn scale(&self) -> Cartesian2 {
        self.scale
    }

    pub fn set_scale(&mut self, value: Cartesian2) {
        if self.scale != value {
            self.scale = value;
            self.make_dirty(SCALE_INDEX);
        }
    }

    pub fn maximum_size(&self) -> Cartesian3 {
        self.maximum_size
    }

    pub fn set_maximum_size(&mut self, value: Cartesian3) {
        if self.maximum_size != value {
            self.maximum_size = value;
            self.make_dirty(MAXIMUM_SIZE_INDEX);
        }
    }

    pub fn slice(&self) -> f64 {
        self.slice
    }

    pub fn set_slice(&mut self, value: f64) {
        if self.slice != value {
            self.slice = value;
            self.make_dirty(SLICE_INDEX);
        }
    }

    pub fn color(&self) -> Color {
        self.color
    }

    pub fn set_color(&mut self, value: Color) {
        if self.color != value {
            self.color = value;
            self.make_dirty(COLOR_INDEX);
        }
    }

    pub fn brightness(&self) -> f64 {
        self.brightness
    }

    pub fn set_brightness(&mut self, value: f64) {
        if self.brightness != value {
            self.brightness = value;
            self.make_dirty(BRIGHTNESS_INDEX);
        }
    }

    pub fn cloud_type(&self) -> CloudType {
        self.cloud_type
    }

    /// Replaces all properties atomically from new options.
    pub fn set_options(&mut self, options: CumulusCloudOptions) -> Result<(), &'static str> {
        options.validate()?;
        let (new_scale, new_max_size) =
            CumulusCloudOptions::resolve_dimensions(options.scale, options.maximum_size);

        self.set_show(options.show);
        self.set_position(options.position);
        self.set_scale(new_scale);
        self.set_maximum_size(new_max_size);
        self.set_slice(options.slice);
        self.set_color(options.color);
        self.set_brightness(options.brightness);
        self.cloud_type = options.cloud_type;
        Ok(())
    }

    #[inline]
    fn make_dirty(&mut self, property_index: usize) {
        self.dirty = true;
        if property_index < NUMBER_OF_PROPERTIES {
            self.dirty_properties[property_index] = true;
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_property_dirty(&self, property_index: usize) -> bool {
        property_index < NUMBER_OF_PROPERTIES && self.dirty_properties[property_index]
    }

    pub fn dirty_properties(&self) -> &[bool; NUMBER_OF_PROPERTIES] {
        &self.dirty_properties
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        self.dirty_properties = [false; NUMBER_OF_PROPERTIES];
    }

    pub fn collection_index(&self) -> usize {
        self.collection_index
    }

    pub(crate) fn set_collection_index(&mut self, index: usize) {
        self.collection_index = index;
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    pub(crate) fn detach(&mut self) {
        self.attached = false;
        self.collection_index = usize::MAX;
    }
}

// Equality tests visual and logical properties, matching CesiumJS semantics.
impl PartialEq for CumulusCloud {
    fn eq(&self, other: &Self) -> bool {
        self.show == other.show
            && self.position == other.position
            && self.scale == other.scale
            && self.maximum_size == other.maximum_size
            && (self.slice == other.slice || (self.slice.is_nan() && other.slice.is_nan()))
            && self.color == other.color
            && (self.brightness == other.brightness
                || (self.brightness.is_nan() && other.brightness.is_nan()))
            && self.cloud_type == other.cloud_type
    }
}
