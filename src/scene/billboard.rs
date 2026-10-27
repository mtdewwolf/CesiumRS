//! CPU-side state and screen-space geometry for CesiumJS `Billboard`.
//!
//! # Architecture and Parity Status
//! This module provides partial CPU-side behavior for CesiumJS `Billboard`:
//! - Configuration options, defaults, and debug-time parameter validation.
//! - Screen-space bounding box computation based on `Billboard.getScreenSpaceBoundingBox`.
//! - Equivalence checking based on `Billboard.prototype.equals`.
//! - Fine-grained property change tracking and dirty flags for batch attribute updates.
//!
//! # Limitations
//! - GPU Draw & Shader Pipeline: WebGL/WebGPU resource allocation, vertex array formatting,
//!   texture atlas rendering, and draw command emission require graphics backend integration.
//! - Pick IDs: Pick IDs and pick colors are not fabricated on the CPU; true pick IDs require
//!   a GPU render context.
//! - Terrain Clamping: Height clamping queries (`HeightReference::ClampToGround` / `RelativeToGround`)
//!   require a live Scene and Globe terrain provider.

use crate::{
    core::{
        BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, NearFarScalar,
    },
    scene::{
        BillboardLoadState, HeightReference, HorizontalOrigin, SplitDirection, VerticalOrigin,
    },
};

// Property update indices matching CesiumJS Billboard static indices.
pub const SHOW_INDEX: usize = 0;
pub const POSITION_INDEX: usize = 1;
pub const PIXEL_OFFSET_INDEX: usize = 2;
pub const EYE_OFFSET_INDEX: usize = 3;
pub const HORIZONTAL_ORIGIN_INDEX: usize = 4;
pub const VERTICAL_ORIGIN_INDEX: usize = 5;
pub const SCALE_INDEX: usize = 6;
pub const IMAGE_INDEX_INDEX: usize = 7;
pub const COLOR_INDEX: usize = 8;
pub const ROTATION_INDEX: usize = 9;
pub const ALIGNED_AXIS_INDEX: usize = 10;
pub const SCALE_BY_DISTANCE_INDEX: usize = 11;
pub const TRANSLUCENCY_BY_DISTANCE_INDEX: usize = 12;
pub const PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX: usize = 13;
pub const DISTANCE_DISPLAY_CONDITION: usize = 14;
pub const DISABLE_DEPTH_DISTANCE: usize = 15;
pub const TEXTURE_COORDINATE_BOUNDS: usize = 16;
pub const SDF_INDEX: usize = 17;
pub const SPLIT_DIRECTION_INDEX: usize = 18;
pub const NUMBER_OF_PROPERTIES: usize = 19;

/// Initialization and mutation options for [`Billboard`].
#[derive(Clone, Debug, PartialEq)]
pub struct BillboardOptions {
    pub show: bool,
    pub position: Cartesian3,
    pub pixel_offset: Cartesian2,
    pub eye_offset: Cartesian3,
    pub horizontal_origin: HorizontalOrigin,
    pub vertical_origin: VerticalOrigin,
    pub height_reference: HeightReference,
    pub scale: f64,
    pub color: Color,
    pub rotation: f64,
    pub aligned_axis: Cartesian3,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub scale_by_distance: Option<NearFarScalar>,
    pub translucency_by_distance: Option<NearFarScalar>,
    pub pixel_offset_scale_by_distance: Option<NearFarScalar>,
    pub size_in_meters: bool,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub disable_depth_test_distance: Option<f64>,
    pub id: Option<String>,
    pub image: Option<String>,
    pub image_sub_region: Option<BoundingRectangle>,
    pub outline_color: Color,
    pub outline_width: f64,
    pub split_direction: SplitDirection,
}

impl Default for BillboardOptions {
    fn default() -> Self {
        Self {
            show: true,
            position: Cartesian3::ZERO,
            pixel_offset: Cartesian2::ZERO,
            eye_offset: Cartesian3::ZERO,
            horizontal_origin: HorizontalOrigin::Center,
            vertical_origin: VerticalOrigin::Center,
            height_reference: HeightReference::None,
            scale: 1.0,
            color: Color::WHITE,
            rotation: 0.0,
            aligned_axis: Cartesian3::ZERO,
            width: None,
            height: None,
            scale_by_distance: None,
            translucency_by_distance: None,
            pixel_offset_scale_by_distance: None,
            size_in_meters: false,
            distance_display_condition: None,
            disable_depth_test_distance: None,
            id: None,
            image: None,
            image_sub_region: None,
            outline_color: Color::BLACK,
            outline_width: 0.0,
            split_direction: SplitDirection::None,
        }
    }
}

impl BillboardOptions {
    /// Validates parameter ranges matching CesiumJS DeveloperError preconditions.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(distance) = self.disable_depth_test_distance {
            if distance < 0.0 {
                return Err("disableDepthTestDistance must be greater than or equal to 0.0.");
            }
        }
        if let Some(translucency) = self.translucency_by_distance {
            if translucency.far <= translucency.near {
                return Err("translucencyByDistance.far must be greater than translucencyByDistance.near.");
            }
        }
        if let Some(pixel_offset_scale) = self.pixel_offset_scale_by_distance {
            if pixel_offset_scale.far <= pixel_offset_scale.near {
                return Err("pixelOffsetScaleByDistance.far must be greater than pixelOffsetScaleByDistance.near.");
            }
        }
        if let Some(scale) = self.scale_by_distance {
            if scale.far <= scale.near {
                return Err("scaleByDistance.far must be greater than scaleByDistance.near.");
            }
        }
        if let Some(condition) = self.distance_display_condition {
            if condition.far <= condition.near {
                return Err("distanceDisplayCondition.far must be greater than distanceDisplayCondition.near.");
            }
        }
        Ok(())
    }
}

/// CPU-side representation of a viewport-aligned billboard.
#[derive(Debug)]
pub struct Billboard {
    options: BillboardOptions,
    actual_position: Cartesian3,
    clamped_position: Option<Cartesian3>,
    translate: Cartesian2,
    cluster_show: bool,
    load_state: BillboardLoadState,
    load_error: Option<String>,
    texture_dirty: bool,
    dirty: bool,
    attached: bool,
    collection_index: usize,
    properties_changed: [bool; NUMBER_OF_PROPERTIES],
}

impl Billboard {
    pub(crate) fn new(options: BillboardOptions, index: usize) -> Result<Self, &'static str> {
        options.validate()?;
        let actual_position = options.position;
        let mut billboard = Self {
            options,
            actual_position,
            clamped_position: None,
            translate: Cartesian2::ZERO,
            cluster_show: true,
            load_state: BillboardLoadState::None,
            load_error: None,
            texture_dirty: false,
            dirty: true,
            attached: true,
            collection_index: index,
            properties_changed: [true; NUMBER_OF_PROPERTIES],
        };
        if billboard.options.image.is_some() {
            billboard.texture_dirty = true;
            billboard.load_state = BillboardLoadState::Loading;
        }
        Ok(billboard)
    }

    pub fn options(&self) -> &BillboardOptions {
        &self.options
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    pub(crate) fn detach(&mut self) {
        self.attached = false;
        self.collection_index = usize::MAX;
    }

    pub fn collection_index(&self) -> usize {
        self.collection_index
    }

    pub(crate) fn set_collection_index(&mut self, index: usize) {
        self.collection_index = index;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_property_changed(&self, index: usize) -> bool {
        self.properties_changed.get(index).copied().unwrap_or(false)
    }

    pub(crate) fn clear_dirty(&mut self) {
        self.dirty = false;
        self.properties_changed.fill(false);
    }

    fn mark_dirty(&mut self, property_index: usize) {
        self.dirty = true;
        if property_index < NUMBER_OF_PROPERTIES {
            self.properties_changed[property_index] = true;
        }
    }

    // Property getters and setters

    pub fn show(&self) -> bool {
        self.options.show
    }

    pub fn set_show(&mut self, value: bool) {
        if self.options.show != value {
            self.options.show = value;
            self.mark_dirty(SHOW_INDEX);
        }
    }

    pub fn position(&self) -> Cartesian3 {
        self.options.position
    }

    pub fn set_position(&mut self, value: Cartesian3) {
        if self.options.position != value {
            self.options.position = value;
            self.actual_position = value;
            self.clamped_position = None;
            self.mark_dirty(POSITION_INDEX);
        }
    }

    pub fn actual_position(&self) -> Cartesian3 {
        self.clamped_position.unwrap_or(self.actual_position)
    }

    pub fn set_actual_position(&mut self, value: Cartesian3) {
        if self.clamped_position.is_none() {
            self.actual_position = value;
        }
        self.mark_dirty(POSITION_INDEX);
    }

    pub fn clamped_position(&self) -> Option<Cartesian3> {
        self.clamped_position
    }

    pub fn set_clamped_position(&mut self, value: Option<Cartesian3>) {
        self.clamped_position = value;
        self.mark_dirty(POSITION_INDEX);
    }

    pub fn pixel_offset(&self) -> Cartesian2 {
        self.options.pixel_offset
    }

    pub fn set_pixel_offset(&mut self, value: Cartesian2) {
        if self.options.pixel_offset != value {
            self.options.pixel_offset = value;
            self.mark_dirty(PIXEL_OFFSET_INDEX);
        }
    }

    pub fn translate(&self) -> Cartesian2 {
        self.translate
    }

    pub fn set_translate(&mut self, value: Cartesian2) {
        if self.translate != value {
            self.translate = value;
            self.mark_dirty(PIXEL_OFFSET_INDEX);
        }
    }

    pub fn eye_offset(&self) -> Cartesian3 {
        self.options.eye_offset
    }

    pub fn set_eye_offset(&mut self, value: Cartesian3) {
        if self.options.eye_offset != value {
            self.options.eye_offset = value;
            self.mark_dirty(EYE_OFFSET_INDEX);
        }
    }

    pub fn horizontal_origin(&self) -> HorizontalOrigin {
        self.options.horizontal_origin
    }

    pub fn set_horizontal_origin(&mut self, value: HorizontalOrigin) {
        if self.options.horizontal_origin != value {
            self.options.horizontal_origin = value;
            self.mark_dirty(HORIZONTAL_ORIGIN_INDEX);
        }
    }

    pub fn vertical_origin(&self) -> VerticalOrigin {
        self.options.vertical_origin
    }

    pub fn set_vertical_origin(&mut self, value: VerticalOrigin) {
        if self.options.vertical_origin != value {
            self.options.vertical_origin = value;
            self.mark_dirty(VERTICAL_ORIGIN_INDEX);
        }
    }

    pub fn height_reference(&self) -> HeightReference {
        self.options.height_reference
    }

    pub fn set_height_reference(&mut self, value: HeightReference) {
        if self.options.height_reference != value {
            self.options.height_reference = value;
            self.clamped_position = None;
            self.mark_dirty(POSITION_INDEX);
        }
    }

    pub fn scale(&self) -> f64 {
        self.options.scale
    }

    pub fn set_scale(&mut self, value: f64) {
        if self.options.scale != value {
            self.options.scale = value;
            self.mark_dirty(SCALE_INDEX);
        }
    }

    pub fn color(&self) -> Color {
        self.options.color
    }

    pub fn set_color(&mut self, value: Color) {
        if self.options.color != value {
            self.options.color = value;
            self.mark_dirty(COLOR_INDEX);
        }
    }

    pub fn rotation(&self) -> f64 {
        self.options.rotation
    }

    pub fn set_rotation(&mut self, value: f64) {
        if self.options.rotation != value {
            self.options.rotation = value;
            self.mark_dirty(ROTATION_INDEX);
        }
    }

    pub fn aligned_axis(&self) -> Cartesian3 {
        self.options.aligned_axis
    }

    pub fn set_aligned_axis(&mut self, value: Cartesian3) {
        if self.options.aligned_axis != value {
            self.options.aligned_axis = value;
            self.mark_dirty(ALIGNED_AXIS_INDEX);
        }
    }

    pub fn width(&self) -> Option<f64> {
        self.options.width
    }

    pub fn set_width(&mut self, value: Option<f64>) {
        if self.options.width != value {
            self.options.width = value;
            self.mark_dirty(IMAGE_INDEX_INDEX);
        }
    }

    pub fn height(&self) -> Option<f64> {
        self.options.height
    }

    pub fn set_height(&mut self, value: Option<f64>) {
        if self.options.height != value {
            self.options.height = value;
            self.mark_dirty(IMAGE_INDEX_INDEX);
        }
    }

    pub fn scale_by_distance(&self) -> Option<NearFarScalar> {
        self.options.scale_by_distance
    }

    pub fn set_scale_by_distance(
        &mut self,
        value: Option<NearFarScalar>,
    ) -> Result<(), &'static str> {
        if let Some(scalar) = value {
            if scalar.far <= scalar.near {
                return Err("scaleByDistance.far must be greater than scaleByDistance.near.");
            }
        }
        if self.options.scale_by_distance != value {
            self.options.scale_by_distance = value;
            self.mark_dirty(SCALE_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn translucency_by_distance(&self) -> Option<NearFarScalar> {
        self.options.translucency_by_distance
    }

    pub fn set_translucency_by_distance(
        &mut self,
        value: Option<NearFarScalar>,
    ) -> Result<(), &'static str> {
        if let Some(scalar) = value {
            if scalar.far <= scalar.near {
                return Err(
                    "translucencyByDistance.far must be greater than translucencyByDistance.near.",
                );
            }
        }
        if self.options.translucency_by_distance != value {
            self.options.translucency_by_distance = value;
            self.mark_dirty(TRANSLUCENCY_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn pixel_offset_scale_by_distance(&self) -> Option<NearFarScalar> {
        self.options.pixel_offset_scale_by_distance
    }

    pub fn set_pixel_offset_scale_by_distance(
        &mut self,
        value: Option<NearFarScalar>,
    ) -> Result<(), &'static str> {
        if let Some(scalar) = value {
            if scalar.far <= scalar.near {
                return Err("pixelOffsetScaleByDistance.far must be greater than pixelOffsetScaleByDistance.near.");
            }
        }
        if self.options.pixel_offset_scale_by_distance != value {
            self.options.pixel_offset_scale_by_distance = value;
            self.mark_dirty(PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn size_in_meters(&self) -> bool {
        self.options.size_in_meters
    }

    pub fn set_size_in_meters(&mut self, value: bool) {
        if self.options.size_in_meters != value {
            self.options.size_in_meters = value;
            self.mark_dirty(COLOR_INDEX);
        }
    }

    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.options.distance_display_condition
    }

    pub fn set_distance_display_condition(
        &mut self,
        value: Option<DistanceDisplayCondition>,
    ) -> Result<(), &'static str> {
        if let Some(condition) = value {
            if condition.far <= condition.near {
                return Err("distanceDisplayCondition.far must be greater than distanceDisplayCondition.near.");
            }
        }
        if self.options.distance_display_condition != value {
            self.options.distance_display_condition = value;
            self.mark_dirty(DISTANCE_DISPLAY_CONDITION);
        }
        Ok(())
    }

    pub fn disable_depth_test_distance(&self) -> Option<f64> {
        self.options.disable_depth_test_distance
    }

    pub fn set_disable_depth_test_distance(
        &mut self,
        value: Option<f64>,
    ) -> Result<(), &'static str> {
        if let Some(dist) = value {
            if dist < 0.0 {
                return Err("disableDepthTestDistance must be greater than or equal to 0.0.");
            }
        }
        if self.options.disable_depth_test_distance != value {
            self.options.disable_depth_test_distance = value;
            self.mark_dirty(DISABLE_DEPTH_DISTANCE);
        }
        Ok(())
    }

    pub fn id(&self) -> Option<&str> {
        self.options.id.as_deref()
    }

    pub fn set_id(&mut self, value: Option<String>) {
        self.options.id = value;
    }

    pub fn image(&self) -> Option<&str> {
        self.options.image.as_deref()
    }

    pub fn set_image(&mut self, value: Option<String>) {
        if self.options.image != value {
            self.options.image = value;
            self.load_error = None;
            self.texture_dirty = true;
            self.load_state = if self.options.image.is_some() {
                BillboardLoadState::Loading
            } else {
                BillboardLoadState::None
            };
            self.mark_dirty(IMAGE_INDEX_INDEX);
        }
    }

    pub fn image_sub_region(&self) -> Option<BoundingRectangle> {
        self.options.image_sub_region
    }

    pub fn set_image_sub_region(&mut self, value: Option<BoundingRectangle>) {
        if self.options.image_sub_region != value {
            self.options.image_sub_region = value;
            self.mark_dirty(IMAGE_INDEX_INDEX);
        }
    }

    pub fn load_state(&self) -> BillboardLoadState {
        self.load_state
    }

    pub fn set_load_state(&mut self, state: BillboardLoadState) {
        self.load_state = state;
    }

    pub fn ready(&self) -> bool {
        self.load_state == BillboardLoadState::Loaded
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn set_load_error(&mut self, error: Option<String>) {
        self.load_error = error;
        if self.load_error.is_some() {
            self.load_state = BillboardLoadState::Error;
        }
    }

    pub fn is_texture_dirty(&self) -> bool {
        self.texture_dirty
    }

    pub fn set_texture_dirty(&mut self, dirty: bool) {
        self.texture_dirty = dirty;
    }

    pub fn cluster_show(&self) -> bool {
        self.cluster_show
    }

    pub fn set_cluster_show(&mut self, value: bool) {
        if self.cluster_show != value {
            self.cluster_show = value;
            self.mark_dirty(SHOW_INDEX);
        }
    }

    pub fn outline_color(&self) -> Color {
        self.options.outline_color
    }

    pub fn set_outline_color(&mut self, value: Color) {
        if self.options.outline_color != value {
            self.options.outline_color = value;
            self.mark_dirty(SDF_INDEX);
        }
    }

    pub fn outline_width(&self) -> f64 {
        self.options.outline_width
    }

    pub fn set_outline_width(&mut self, value: f64) {
        if self.options.outline_width != value {
            self.options.outline_width = value;
            self.mark_dirty(SDF_INDEX);
        }
    }

    pub fn split_direction(&self) -> SplitDirection {
        self.options.split_direction
    }

    pub fn set_split_direction(&mut self, value: SplitDirection) {
        if self.options.split_direction != value {
            self.options.split_direction = value;
            self.mark_dirty(SPLIT_DIRECTION_INDEX);
        }
    }

    /// Replaces configurable options while retaining collection and runtime state.
    pub fn set_options(&mut self, options: BillboardOptions) -> Result<(), &'static str> {
        options.validate()?;
        if options.position != self.options.position {
            self.actual_position = options.position;
            self.clamped_position = None;
        }
        if options.height_reference != self.options.height_reference {
            self.clamped_position = None;
        }
        if options.image != self.options.image {
            self.texture_dirty = true;
            self.load_error = None;
            self.load_state = if options.image.is_some() { BillboardLoadState::Loading } else { BillboardLoadState::None };
        }
        self.options = options;
        self.dirty = true;
        self.properties_changed.fill(true);
        Ok(())
    }

    /// Computes the billboard's 2D screen space bounding box centered around `screen_space_position`.
    ///
    /// Uses explicit width and height; missing image dimensions currently
    /// produce zero size because atlas image dimensions are unavailable.
    pub fn screen_space_bounding_box(
        &self,
        screen_space_position: Cartesian2,
    ) -> BoundingRectangle {
        let width = self.options.width.unwrap_or(0.0) * self.options.scale;
        let height = self.options.height.unwrap_or(0.0) * self.options.scale;

        let mut x = screen_space_position.x;
        match self.options.horizontal_origin {
            HorizontalOrigin::Right => {
                x -= width;
            }
            HorizontalOrigin::Center => {
                x -= width * 0.5;
            }
            HorizontalOrigin::Left => {}
        }

        let mut y = screen_space_position.y;
        match self.options.vertical_origin {
            VerticalOrigin::Bottom | VerticalOrigin::Baseline => {
                y -= height;
            }
            VerticalOrigin::Center => {
                y -= height * 0.5;
            }
            VerticalOrigin::Top => {}
        }

        BoundingRectangle::new(x, y, width, height)
    }

    /// Parity implementation of CesiumJS `Billboard.prototype.equals`.
    ///
    /// Note: CesiumJS equality compares logical render attributes and intentionally
    /// excludes transient cluster visibility, internal actual position, and owner collection.
    pub fn equals(&self, other: &Self) -> bool {
        self.options.id == other.options.id
            && self.options.position == other.options.position
            && self.options.image == other.options.image
            && self.options.show == other.options.show
            && self.options.scale == other.options.scale
            && self.options.vertical_origin == other.options.vertical_origin
            && self.options.horizontal_origin == other.options.horizontal_origin
            && self.options.height_reference == other.options.height_reference
            && self.options.color == other.options.color
            && self.options.pixel_offset == other.options.pixel_offset
            && self.translate == other.translate
            && self.options.eye_offset == other.options.eye_offset
            && self.options.scale_by_distance == other.options.scale_by_distance
            && self.options.translucency_by_distance == other.options.translucency_by_distance
            && self.options.pixel_offset_scale_by_distance
                == other.options.pixel_offset_scale_by_distance
            && self.options.distance_display_condition
                == other.options.distance_display_condition
            && self.options.disable_depth_test_distance
                == other.options.disable_depth_test_distance
            && self.options.split_direction == other.options.split_direction
    }
}

impl PartialEq for Billboard {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}
