//! CPU collection and vertex attribute preparation for CesiumJS points.
//!
//! This is a partial port: GPU buffers, shaders, draw and pick commands,
//! 2D/Columbus projection and Scene frame integration remain to be implemented.
use crate::{
    core::{Cartesian3, EncodedCartesian3, Matrix4},
    scene::{
        point_primitive::{PointPrimitive, PointPrimitiveOptions},
        BlendOption,
    },
};
use std::{cell::RefCell, rc::Rc};

/// Shared identity models Cesium's mutable point objects and collection membership.
pub type PointHandle = Rc<RefCell<PointPrimitive>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointAttributes {
    pub position_high_and_size: [f64; 4],
    pub position_low_and_outline: [f64; 4],
    pub show_and_translucency: [f64; 4],
    pub scale_by_distance: [f64; 4],
    pub distance_depth_split: [f64; 4],
}

impl PointAttributes {
    pub fn from_point(point: &PointPrimitive) -> Self {
        let o = point.options();
        let encoded = EncodedCartesian3::from_cartesian(point.actual_position());
        let translucency = o.translucency_by_distance.unwrap_or(crate::core::NearFarScalar::new(0.0, 1.0, 1.0, 1.0));
        let scale = o.scale_by_distance.unwrap_or(crate::core::NearFarScalar::new(0.0, 1.0, 1.0, 1.0));
        let show = o.show && point.cluster_show() && (o.color.alpha != 0.0 || o.outline_color.alpha != 0.0);
        // JavaScript ToInt32 truncates NaN to zero.
        let quantize = |v: f64| if v.is_nan() { 0.0 } else { (v.clamp(0.0, 1.0) * 255.0).floor() };
        let (near, far) = o.distance_display_condition.map_or((0.0, f64::MAX), |d| (d.near * d.near, d.far * d.far));
        let depth = o.disable_depth_test_distance * o.disable_depth_test_distance;
        Self {
            position_high_and_size: [encoded.high.x, encoded.high.y, encoded.high.z, o.pixel_size],
            position_low_and_outline: [encoded.low.x, encoded.low.y, encoded.low.z, o.outline_width],
            show_and_translucency: [
                (if show { 256.0 } else { 0.0 }) + quantize(translucency.near_value),
                quantize(translucency.far_value),
                translucency.near,
                translucency.far,
            ],
            scale_by_distance: [scale.near, scale.near_value, scale.far, scale.far_value],
            distance_depth_split: [
                near,
                far,
                if depth == f64::INFINITY { -1.0 } else { depth },
                o.split_direction.value() as f64,
            ],
        }
    }
}

pub struct PointPrimitiveCollection {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    pub blend_option: BlendOption,
    points: Vec<PointHandle>,
    destroyed: bool,
}

impl Default for PointPrimitiveCollection {
    fn default() -> Self {
        Self {
            show: true,
            model_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: false,
            blend_option: BlendOption::OpaqueAndTranslucent,
            points: Vec::new(),
            destroyed: false,
        }
    }
}

impl PointPrimitiveCollection {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn add(&mut self, options: PointPrimitiveOptions) -> Result<PointHandle, &'static str> {
        if self.destroyed {
            return Err("collection is destroyed");
        }
        let point = Rc::new(RefCell::new(PointPrimitive::new(options)?));
        self.points.push(point.clone());
        Ok(point)
    }
    pub fn contains(&self, point: &PointHandle) -> bool {
        self.points.iter().any(|p| Rc::ptr_eq(p, point))
    }
    pub fn get(&self, index: usize) -> Option<PointHandle> {
        self.points.get(index).cloned()
    }
    pub fn remove(&mut self, point: &PointHandle) -> bool {
        let Some(index) = self.points.iter().position(|p| Rc::ptr_eq(p, point)) else { return false };
        let removed = self.points.remove(index);
        removed.borrow_mut().detach();
        true
    }
    pub fn remove_all(&mut self) {
        for point in self.points.drain(..) {
            point.borrow_mut().detach();
        }
    }
    pub fn destroy(&mut self) {
        if !self.destroyed {
            self.remove_all();
            self.destroyed = true;
        }
    }
    pub fn attributes(&self) -> Vec<PointAttributes> {
        self.points.iter().map(|p| PointAttributes::from_point(&p.borrow())).collect()
    }
    pub fn positions(&self) -> Vec<Cartesian3> {
        self.points.iter().map(|p| p.borrow().actual_position()).collect()
    }
}
