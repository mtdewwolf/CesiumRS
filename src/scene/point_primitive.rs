//! CPU-side state for CesiumJS `PointPrimitive`.
//!
//! Rendering, pick-id allocation, scene projection and GPU resource lifecycle
//! are not yet implemented; see `point_primitive_collection`.
use crate::{
    core::{BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, NearFarScalar},
    scene::SplitDirection,
};

#[derive(Clone, Debug, PartialEq)]
pub struct PointPrimitiveOptions {
    pub show: bool,
    pub position: Cartesian3,
    pub color: Color,
    pub outline_color: Color,
    pub outline_width: f64,
    pub pixel_size: f64,
    pub scale_by_distance: Option<NearFarScalar>,
    pub translucency_by_distance: Option<NearFarScalar>,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub disable_depth_test_distance: f64,
    pub id: Option<String>,
    pub split_direction: SplitDirection,
}

impl Default for PointPrimitiveOptions {
    fn default() -> Self {
        Self {
            show: true,
            position: Cartesian3::ZERO,
            color: Color::WHITE,
            outline_color: Color::TRANSPARENT,
            outline_width: 0.0,
            pixel_size: 10.0,
            scale_by_distance: None,
            translucency_by_distance: None,
            distance_display_condition: None,
            disable_depth_test_distance: 0.0,
            id: None,
            split_direction: SplitDirection::None,
        }
    }
}

impl PointPrimitiveOptions {
    /// Cesium's debug-time range checks, applied before any mutation.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.disable_depth_test_distance < 0.0 {
            return Err("disableDepthTestDistance must be greater than or equal to 0.0.");
        }
        if self.translucency_by_distance.is_some_and(|v| v.far <= v.near) {
            return Err("translucencyByDistance.far must be greater than translucencyByDistance.near.");
        }
        if self.scale_by_distance.is_some_and(|v| v.far <= v.near) {
            return Err("scaleByDistance.far must be greater than scaleByDistance.near.");
        }
        if self.distance_display_condition.is_some_and(|v| v.far <= v.near) {
            return Err("distanceDisplayCondition.far must be greater than distanceDisplayCondition.near.");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PointPrimitive {
    options: PointPrimitiveOptions,
    cluster_show: bool,
    actual_position: Cartesian3,
    attached: bool,
}

impl PointPrimitive {
    pub(crate) fn new(options: PointPrimitiveOptions) -> Result<Self, &'static str> {
        options.validate()?;
        Ok(Self { actual_position: options.position, options, cluster_show: true, attached: true })
    }

    pub fn options(&self) -> &PointPrimitiveOptions {
        &self.options
    }
    pub fn cluster_show(&self) -> bool {
        self.cluster_show
    }
    pub fn set_cluster_show(&mut self, value: bool) {
        self.cluster_show = value;
    }
    pub fn actual_position(&self) -> Cartesian3 {
        self.actual_position
    }
    pub fn set_actual_position(&mut self, value: Cartesian3) {
        self.actual_position = value;
    }
    pub fn is_attached(&self) -> bool {
        self.attached
    }
    pub(crate) fn detach(&mut self) {
        self.attached = false;
    }

    /// Replace properties atomically. Setting the model position resets the
    /// actual position, matching the upstream property setter in 3D.
    pub fn set_options(&mut self, options: PointPrimitiveOptions) -> Result<(), &'static str> {
        options.validate()?;
        if options.position != self.options.position {
            self.actual_position = options.position;
        }
        self.options = options;
        Ok(())
    }

    pub fn screen_space_bounding_box(&self, center: Cartesian2) -> BoundingRectangle {
        let size = self.options.pixel_size;
        BoundingRectangle::new(center.x - size * 0.5, center.y - size * 0.5, size, size)
    }
}

// Cesium's equals intentionally excludes clusterShow, actualPosition and owner.
impl PartialEq for PointPrimitive {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options
    }
}
