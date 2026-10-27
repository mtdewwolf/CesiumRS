//! CPU collection and vertex attribute preparation for CesiumJS `CloudCollection`.
//!
//! # Architecture and Parity Status
//! This module provides partial CPU-side behavior for CesiumJS `CloudCollection`
//! corresponding to `@cesium/engine 23.0.0`:
//! - Complete collection management: `add`, `remove`, `remove_all`, `contains`, `get`, `len`.
//! - Deferred removal and compaction semantics matching CesiumJS `removeClouds`.
//! - Precomputed noise texture dimensions `u_noiseTextureDimensions` (128, 4, 0.25).
//! - Position-only bounding spheres are a CesiumRS extension, not safe for
//!   culling cloud extents; `model_matrix` is also an extension.
//! - Handle borrows must be released before collection methods access them; a
//!   handle borrowed during collection drop can retain stale attachment state.
//! - Methods that compact or inspect clouds may panic on active handle borrows.
//!   After destroy, read/remove operations return empty values rather than
//!   CesiumJS's destroyed-object errors.
//! - Vertex attribute formatting for both instanced (`instanced_attributes`) and batched
//!   quad (`batched_attributes`) vertex layouts; GPU parity remains unverified.
//!
//! # GPU and Rendering Limitations
//! - Volumetric Noise Compute Pass: The generation of 3D Perlin/Worley noise into a
//!   4096x512 RGBA texture via compute command / `CloudNoiseVS` & `CloudNoiseFS` requires
//!   a GPU render pass and framebuffer attachment.
//! - Vertex Array Facade & Buffers: Static index buffer generation (batched vs instanced)
//!   and dynamic vertex array facade buffer commits (`VertexArrayFacade.commit` / `subCommit`)
//!   are deferred to the graphics backend.
//! - Render Pipeline Integration: `CloudCollection.prototype.update` translucent draw command
//!   emission and shader caching (`CloudCollectionVS` / `CloudCollectionFS`) require an active
//!   scene frame state and context.
//! - Avoidance of Fake Resources: Hardware textures, OpenGL/Vulkan pipeline handles, and
//!   dummy compute buffers are omitted to preserve honest CPU testing boundaries.

use super::cumulus_cloud::*;
use crate::core::{BoundingSphere, Cartesian3, Color, EncodedCartesian3, Matrix4};
use std::{cell::RefCell, rc::Rc};

/// Shared handle modeling CesiumJS cloud mutable reference semantics.
pub type CumulusCloudHandle = Rc<RefCell<CumulusCloud>>;

/// Noise texture dimension constants from CesiumJS CloudCollection.
pub const TEXTURE_SLICE_WIDTH: f64 = 128.0;
pub const NOISE_TEXTURE_ROWS: f64 = 4.0;
pub const NOISE_TEXTURE_WIDTH: usize = 4096; // (128 * 128) / 4
pub const NOISE_TEXTURE_HEIGHT: usize = 512; // 128 * 4

/// Vertex attributes packed for a cloud vertex or instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudVertexAttributes {
    /// High-precision position XYZ with scale X in W.
    pub position_high_and_scale_x: [f64; 4],
    /// Low-precision position XYZ with scale Y in W.
    pub position_low_and_scale_y: [f64; 4],
    /// Packed attribute 0: `[show, brightness, u_direction, v_direction]`.
    pub packed_attribute0: [f64; 4],
    /// Packed attribute 1: `[maximumSize.x, maximumSize.y, maximumSize.z, slice]`.
    pub packed_attribute1: [f64; 4],
    /// Normalized color RGBA bytes.
    pub color: [u8; 4],
}

impl CloudVertexAttributes {
    /// Encodes attributes for a single cloud in instanced rendering mode.
    pub fn instanced_from_cloud(cloud: &CumulusCloud) -> Self {
        let encoded = EncodedCartesian3::from_cartesian(cloud.position());
        let scale = cloud.scale();
        let max_size = cloud.maximum_size();
        let color = cloud.color();

        let show = if cloud.show() { 1.0 } else { 0.0 };
        let red = Color::float_to_byte(color.red) as u8;
        let green = Color::float_to_byte(color.green) as u8;
        let blue = Color::float_to_byte(color.blue) as u8;
        let alpha = Color::float_to_byte(color.alpha) as u8;

        Self {
            position_high_and_scale_x: [encoded.high.x, encoded.high.y, encoded.high.z, scale.x],
            position_low_and_scale_y: [encoded.low.x, encoded.low.y, encoded.low.z, scale.y],
            packed_attribute0: [show, cloud.brightness(), 0.0, 0.0],
            packed_attribute1: [max_size.x, max_size.y, max_size.z, cloud.slice()],
            color: [red, green, blue, alpha],
        }
    }

    /// Encodes the 4 quad vertices for a single cloud in batched rendering mode.
    pub fn batched_quad_from_cloud(cloud: &CumulusCloud) -> [Self; 4] {
        let encoded = EncodedCartesian3::from_cartesian(cloud.position());
        let scale = cloud.scale();
        let max_size = cloud.maximum_size();
        let color = cloud.color();

        let show = if cloud.show() { 1.0 } else { 0.0 };
        let brightness = cloud.brightness();
        let red = Color::float_to_byte(color.red) as u8;
        let green = Color::float_to_byte(color.green) as u8;
        let blue = Color::float_to_byte(color.blue) as u8;
        let alpha = Color::float_to_byte(color.alpha) as u8;

        let high_and_scale_x = [encoded.high.x, encoded.high.y, encoded.high.z, scale.x];
        let low_and_scale_y = [encoded.low.x, encoded.low.y, encoded.low.z, scale.y];
        let packed1 = [max_size.x, max_size.y, max_size.z, cloud.slice()];
        let col = [red, green, blue, alpha];

        // Corner UV directions corresponding to writePackedAttribute0 in CloudCollection.js
        let corner_directions = [
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];

        [
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[0][0], corner_directions[0][1]],
                packed_attribute1: packed1,
                color: col,
            },
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[1][0], corner_directions[1][1]],
                packed_attribute1: packed1,
                color: col,
            },
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[2][0], corner_directions[2][1]],
                packed_attribute1: packed1,
                color: col,
            },
            Self {
                position_high_and_scale_x: high_and_scale_x,
                position_low_and_scale_y: low_and_scale_y,
                packed_attribute0: [show, brightness, corner_directions[3][0], corner_directions[3][1]],
                packed_attribute1: packed1,
                color: col,
            },
        ]
    }
}

/// Options for creating a [`CloudCollection`].
#[derive(Clone, Debug, PartialEq)]
pub struct CloudCollectionOptions {
    /// Whether to display clouds in the collection.
    pub show: bool,
    /// Desired amount of detail in the noise texture (power of two, default: 16.0).
    pub noise_detail: f64,
    /// Desired translation offset of noise texture coordinates (default: ZERO).
    pub noise_offset: Cartesian3,
    /// Debug option rendering billboards with opaque colors.
    pub debug_billboards: bool,
    /// Debug option drawing clouds as opaque ellipsoids.
    pub debug_ellipsoids: bool,
    /// CesiumRS extension: transform from model to world space.
    pub model_matrix: Matrix4,
}

impl Default for CloudCollectionOptions {
    fn default() -> Self {
        Self {
            show: true,
            noise_detail: 16.0,
            noise_offset: Cartesian3::ZERO,
            debug_billboards: false,
            debug_ellipsoids: false,
            model_matrix: Matrix4::IDENTITY,
        }
    }
}

/// A CPU collection of clouds in 3D scene space.
pub struct CloudCollection {
    /// Determines if clouds in this collection will be shown.
    pub show: bool,
    /// Controls the detail in the precomputed noise texture.
    pub noise_detail: f64,
    /// Applies a translation to noise texture coordinates.
    pub noise_offset: Cartesian3,
    /// Debug flag: renders billboards with an opaque color.
    pub debug_billboards: bool,
    /// Debug flag: renders clouds as opaque ellipsoids.
    pub debug_ellipsoids: bool,
    /// CesiumRS extension: model transformation matrix.
    pub model_matrix: Matrix4,

    clouds: Vec<Option<CumulusCloudHandle>>,
    clouds_removed: bool,
    destroyed: bool,

    base_volume: BoundingSphere,
    base_volume_wc: BoundingSphere,
}

impl Default for CloudCollection {
    fn default() -> Self {
        Self::with_options(CloudCollectionOptions::default())
    }
}

impl CloudCollection {
    /// Creates a new cloud collection with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new cloud collection with custom options.
    pub fn with_options(options: CloudCollectionOptions) -> Self {
        Self {
            show: options.show,
            noise_detail: options.noise_detail,
            noise_offset: options.noise_offset,
            debug_billboards: options.debug_billboards,
            debug_ellipsoids: options.debug_ellipsoids,
            model_matrix: options.model_matrix,

            clouds: Vec::new(),
            clouds_removed: false,
            destroyed: false,

            base_volume: BoundingSphere::default(),
            base_volume_wc: BoundingSphere::default(),
        }
    }

    /// Returns `true` if the collection has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Compacts removed clouds and re-indexes them; fixes upstream's index reassignment typo.
    /// Panics before changing the collection if a handle is borrowed.
    pub fn compact(&mut self) {
        if !self.clouds_removed {
            return;
        }
        for cloud in self.clouds.iter().flatten() {
            assert!(cloud.try_borrow_mut().is_ok(), "cannot compact while a cloud is borrowed");
        }
        self.clouds_removed = false;
        let mut compacted = Vec::with_capacity(self.clouds.len());
        for handle in self.clouds.drain(..).flatten() {
            handle.borrow_mut().set_collection_index(compacted.len());
            compacted.push(Some(handle));
        }
        self.clouds = compacted;
    }

    /// Returns the number of clouds currently in this collection.
    pub fn len(&mut self) -> usize {
        self.compact();
        self.clouds.len()
    }

    /// Returns `true` if there are no clouds in the collection.
    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    /// Creates and adds a cumulus cloud with the specified options to the collection.
    pub fn add(&mut self, options: CumulusCloudOptions) -> Result<CumulusCloudHandle, &'static str> {
        if self.destroyed {
            return Err("CloudCollection has been destroyed.");
        }
        self.compact();
        let index = self.clouds.len();
        let cloud = Rc::new(RefCell::new(CumulusCloud::new(options, index)?));
        self.clouds.push(Some(cloud.clone()));
        Ok(cloud)
    }

    /// Removes a cloud from the collection.
    pub fn remove(&mut self, cloud: &CumulusCloudHandle) -> bool {
        if self.destroyed {
            return false;
        }
        let pos = self
            .clouds
            .iter()
            .position(|opt| opt.as_ref().is_some_and(|c| Rc::ptr_eq(c, cloud)));
        if let Some(index) = pos {
            let Some(removed) = self.clouds[index].as_ref() else { return false };
            let Ok(mut borrowed) = removed.try_borrow_mut() else { return false };
            borrowed.detach();
            drop(borrowed);
            self.clouds[index] = None;
            self.clouds_removed = true;
            return true;
        }
        false
    }

    /// Removes all clouds from the collection.
    pub fn remove_all(&mut self) {
        for handle in self.clouds.iter().flatten() {
            assert!(handle.try_borrow_mut().is_ok(), "cannot remove clouds while a handle is borrowed");
        }
        for handle in self.clouds.drain(..).flatten() {
            handle.borrow_mut().detach();
        }
        self.clouds_removed = false;
    }

    /// Checks whether this collection contains a given cloud.
    pub fn contains(&self, cloud: &CumulusCloudHandle) -> bool {
        self.clouds
            .iter()
            .any(|opt| opt.as_ref().is_some_and(|c| Rc::ptr_eq(c, cloud)))
    }

    /// Returns the cloud at the specified index.
    pub fn get(&mut self, index: usize) -> Option<CumulusCloudHandle> {
        if self.destroyed {
            return None;
        }
        self.compact();
        self.clouds.get(index).and_then(|opt| opt.clone())
    }

    /// Destroys the collection and detaches all contained clouds.
    pub fn destroy(&mut self) {
        if !self.destroyed {
            self.remove_all();
            self.destroyed = true;
        }
    }

    /// Recomputes bounding spheres from current cloud positions.
    pub fn update_bounding_volumes(&mut self) {
        self.compact();
        let positions: Vec<Cartesian3> = self
            .clouds
            .iter()
            .filter_map(|opt| opt.as_ref().map(|c| c.borrow().position()))
            .collect();

        if positions.is_empty() {
            self.base_volume = BoundingSphere::default();
            self.base_volume_wc = BoundingSphere::default();
        } else {
            self.base_volume = BoundingSphere::from_points(&positions);
            self.base_volume_wc =
                BoundingSphere::transform(&self.base_volume, &self.model_matrix);
        }
    }

    /// Returns the base bounding sphere in model space.
    pub fn base_volume(&mut self) -> &BoundingSphere {
        self.update_bounding_volumes();
        &self.base_volume
    }

    /// Returns the base bounding sphere transformed into world coordinates.
    pub fn base_volume_wc(&mut self) -> &BoundingSphere {
        self.update_bounding_volumes();
        &self.base_volume_wc
    }

    /// Returns the noise texture dimension metrics vector `(128, 4, 0.25)`
    /// corresponding to `u_noiseTextureDimensions` in CesiumJS.
    pub fn noise_texture_dimensions(&self) -> Cartesian3 {
        Cartesian3::new(
            TEXTURE_SLICE_WIDTH,
            NOISE_TEXTURE_ROWS,
            1.0 / NOISE_TEXTURE_ROWS,
        )
    }

    /// Computes instanced vertex attributes for all active clouds.
    pub fn instanced_attributes(&mut self) -> Vec<CloudVertexAttributes> {
        self.compact();
        let mut result = Vec::with_capacity(self.clouds.len());
        for handle in self.clouds.iter().flatten() {
            let mut cloud = handle.borrow_mut();
            cloud.clear_dirty();
            result.push(CloudVertexAttributes::instanced_from_cloud(&cloud));
        }
        result
    }

    /// Computes batched quad vertex attributes (4 vertices per cloud) for all active clouds.
    pub fn batched_attributes(&mut self) -> Vec<[CloudVertexAttributes; 4]> {
        self.compact();
        let mut result = Vec::with_capacity(self.clouds.len());
        for handle in self.clouds.iter().flatten() {
            let mut cloud = handle.borrow_mut();
            cloud.clear_dirty();
            result.push(CloudVertexAttributes::batched_quad_from_cloud(&cloud));
        }
        result
    }

    /// Returns positions of all active clouds in the collection.
    pub fn positions(&mut self) -> Vec<Cartesian3> {
        self.compact();
        self.clouds
            .iter()
            .filter_map(|opt| opt.as_ref().map(|c| c.borrow().position()))
            .collect()
    }

    /// Counts dirty clouds for each property index, not Cesium's cumulative
    /// `_propertiesChanged`. Generating attributes clears the dirty flags.
    pub fn properties_changed_count(&mut self) -> [usize; NUMBER_OF_PROPERTIES] {
        self.compact();
        let mut counts = [0; NUMBER_OF_PROPERTIES];
        for handle in self.clouds.iter().flatten() {
            let cloud = handle.borrow();
            for (i, count) in counts.iter_mut().enumerate() {
                if cloud.is_property_dirty(i) {
                    *count += 1;
                }
            }
        }
        counts
    }
}

impl Drop for CloudCollection {
    fn drop(&mut self) {
        for cloud in self.clouds.iter().flatten() {
            if let Ok(mut c) = cloud.try_borrow_mut() {
                c.detach();
            }
        }
    }
}
