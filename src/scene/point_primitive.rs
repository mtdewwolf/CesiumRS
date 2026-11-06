//! CesiumJS `PointPrimitive`: one point of a
//! [`PointPrimitiveCollection`](super::point_primitive_collection::PointPrimitiveCollection).
//!
//! A [`PointPrimitive`] is a shared handle, as Cesium's point is a mutable
//! object the application keeps after `add`. Every setter compares the old and
//! new value and, on a change, marks the point dirty in its collection exactly
//! as Cesium's `makeDirty` does: the point is queued for a buffer rewrite once,
//! and the changed property's counter feeds the collection's buffer-usage
//! heuristic.
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use crate::{
    core::{
        BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, Matrix4, NearFarScalar,
        RuntimeError,
    },
    renderer::{Context, Gl, PickId},
    scene::{
        scene_transforms::{SceneTransformState, SceneTransforms},
        MetadataValue, SceneMode, ScenePickObject, SplitDirection,
    },
};

pub const SHOW_INDEX: usize = 0;
pub const POSITION_INDEX: usize = 1;
pub const COLOR_INDEX: usize = 2;
pub const OUTLINE_COLOR_INDEX: usize = 3;
pub const OUTLINE_WIDTH_INDEX: usize = 4;
pub const PIXEL_SIZE_INDEX: usize = 5;
pub const SCALE_BY_DISTANCE_INDEX: usize = 6;
pub const TRANSLUCENCY_BY_DISTANCE_INDEX: usize = 7;
pub const DISTANCE_DISPLAY_CONDITION_INDEX: usize = 8;
pub const DISABLE_DEPTH_DISTANCE_INDEX: usize = 9;
pub const SPLIT_DIRECTION_INDEX: usize = 10;
pub const NUMBER_OF_PROPERTIES: usize = 11;

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
    /// The constructor's debug-time range checks.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.disable_depth_test_distance < 0.0 {
            return Err(RuntimeError::new("disableDepthTestDistance must be greater than or equal to 0.0."));
        }
        if self.translucency_by_distance.is_some_and(|v| v.far <= v.near) {
            return Err(RuntimeError::new(
                "translucencyByDistance.far must be greater than translucencyByDistance.near.",
            ));
        }
        if self.scale_by_distance.is_some_and(|v| v.far <= v.near) {
            return Err(RuntimeError::new("scaleByDistance.far must be greater than scaleByDistance.near."));
        }
        if self.distance_display_condition.is_some_and(|v| v.far <= v.near) {
            return Err(RuntimeError::new(
                "distanceDisplayCondition.far must be greater than distanceDisplayCondition.near.",
            ));
        }
        Ok(())
    }
}

/// The collection side of `makeDirty`: `_pointPrimitivesToUpdate` and
/// `_propertiesChanged`.
#[derive(Default)]
pub(crate) struct UpdateQueue {
    pub(crate) to_update: Vec<PointPrimitive>,
    pub(crate) properties_changed: [u32; NUMBER_OF_PROPERTIES],
}

pub(crate) struct PointState {
    pub(crate) options: PointPrimitiveOptions,
    pub(crate) actual_position: Cartesian3,
    pub(crate) cluster_show: bool,
    pub(crate) pick_id: Option<PickId>,
    pub(crate) dirty: bool,
    pub(crate) index: usize,
    collection: Option<Weak<RefCell<UpdateQueue>>>,
    collection_id: Option<u64>,
}

/// A point in a collection. Clones share the point; equality of handles is
/// identity ([`PointPrimitive::ptr_eq`]), and [`PointPrimitive::equals`] is
/// Cesium's property comparison.
#[derive(Clone)]
pub struct PointPrimitive(pub(crate) Rc<RefCell<PointState>>);

impl std::fmt::Debug for PointPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.0.borrow();
        f.debug_struct("PointPrimitive").field("index", &s.index).field("options", &s.options).finish_non_exhaustive()
    }
}

/// The object a point registers with `Context.createPickId`: Cesium's
/// `{ primitive, collection, id }`.
pub struct PointPrimitivePick {
    pub primitive: PointPrimitive,
    pub collection: Option<u64>,
    pub id: Option<String>,
}

impl PointPrimitive {
    pub(crate) fn new(
        options: PointPrimitiveOptions,
        collection: &Rc<RefCell<UpdateQueue>>,
        collection_id: u64,
        index: usize,
    ) -> Result<Self, RuntimeError> {
        options.validate()?;
        Ok(Self(Rc::new(RefCell::new(PointState {
            actual_position: options.position,
            options,
            cluster_show: true,
            pick_id: None,
            dirty: false,
            index,
            collection: Some(Rc::downgrade(collection)),
            collection_id: Some(collection_id),
        }))))
    }

    /// Whether both handles are the same point.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Cesium's `makeDirty`: queue the point once and count the property.
    fn make_dirty(&self, property: usize) {
        let queue = self.0.borrow().collection.as_ref().and_then(Weak::upgrade);
        if let Some(queue) = queue {
            let mut queue = queue.borrow_mut();
            if !self.0.borrow().dirty {
                queue.to_update.push(self.clone());
            }
            queue.properties_changed[property] += 1;
            self.0.borrow_mut().dirty = true;
        }
    }

    /// A snapshot of every public property.
    pub fn options(&self) -> PointPrimitiveOptions {
        self.0.borrow().options.clone()
    }

    pub fn show(&self) -> bool {
        self.0.borrow().options.show
    }
    pub fn set_show(&self, value: bool) {
        if self.show() != value {
            self.0.borrow_mut().options.show = value;
            self.make_dirty(SHOW_INDEX);
        }
    }

    pub fn position(&self) -> Cartesian3 {
        self.0.borrow().options.position
    }
    /// Sets the position and, as in Cesium, the actual position.
    pub fn set_position(&self, value: Cartesian3) {
        if self.position() != value {
            {
                let mut s = self.0.borrow_mut();
                s.options.position = value;
                s.actual_position = value;
            }
            self.make_dirty(POSITION_INDEX);
        }
    }

    pub fn color(&self) -> Color {
        self.0.borrow().options.color
    }
    pub fn set_color(&self, value: Color) {
        if self.color() != value {
            self.0.borrow_mut().options.color = value;
            self.make_dirty(COLOR_INDEX);
        }
    }

    pub fn outline_color(&self) -> Color {
        self.0.borrow().options.outline_color
    }
    pub fn set_outline_color(&self, value: Color) {
        if self.outline_color() != value {
            self.0.borrow_mut().options.outline_color = value;
            self.make_dirty(OUTLINE_COLOR_INDEX);
        }
    }

    pub fn outline_width(&self) -> f64 {
        self.0.borrow().options.outline_width
    }
    pub fn set_outline_width(&self, value: f64) {
        if self.outline_width() != value {
            self.0.borrow_mut().options.outline_width = value;
            self.make_dirty(OUTLINE_WIDTH_INDEX);
        }
    }

    pub fn pixel_size(&self) -> f64 {
        self.0.borrow().options.pixel_size
    }
    pub fn set_pixel_size(&self, value: f64) {
        if self.pixel_size() != value {
            self.0.borrow_mut().options.pixel_size = value;
            self.make_dirty(PIXEL_SIZE_INDEX);
        }
    }

    pub fn scale_by_distance(&self) -> Option<NearFarScalar> {
        self.0.borrow().options.scale_by_distance
    }
    pub fn set_scale_by_distance(&self, value: Option<NearFarScalar>) -> Result<(), RuntimeError> {
        if value.is_some_and(|v| v.far <= v.near) {
            return Err(RuntimeError::new("far distance must be greater than near distance."));
        }
        if self.scale_by_distance() != value {
            self.0.borrow_mut().options.scale_by_distance = value;
            self.make_dirty(SCALE_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn translucency_by_distance(&self) -> Option<NearFarScalar> {
        self.0.borrow().options.translucency_by_distance
    }
    pub fn set_translucency_by_distance(&self, value: Option<NearFarScalar>) -> Result<(), RuntimeError> {
        if value.is_some_and(|v| v.far <= v.near) {
            return Err(RuntimeError::new("far distance must be greater than near distance."));
        }
        if self.translucency_by_distance() != value {
            self.0.borrow_mut().options.translucency_by_distance = value;
            self.make_dirty(TRANSLUCENCY_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.0.borrow().options.distance_display_condition
    }
    pub fn set_distance_display_condition(&self, value: Option<DistanceDisplayCondition>) -> Result<(), RuntimeError> {
        if value.is_some_and(|v| v.far <= v.near) {
            return Err(RuntimeError::new("far must be greater than near"));
        }
        if self.distance_display_condition() != value {
            self.0.borrow_mut().options.distance_display_condition = value;
            self.make_dirty(DISTANCE_DISPLAY_CONDITION_INDEX);
        }
        Ok(())
    }

    pub fn disable_depth_test_distance(&self) -> f64 {
        self.0.borrow().options.disable_depth_test_distance
    }
    /// As in Cesium, the range check runs only when the value changes.
    pub fn set_disable_depth_test_distance(&self, value: f64) -> Result<(), RuntimeError> {
        if self.disable_depth_test_distance() != value {
            if value < 0.0 || value.is_nan() {
                return Err(RuntimeError::new("disableDepthTestDistance must be greater than or equal to 0.0."));
            }
            self.0.borrow_mut().options.disable_depth_test_distance = value;
            self.make_dirty(DISABLE_DEPTH_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn id(&self) -> Option<String> {
        self.0.borrow().options.id.clone()
    }
    /// Sets the id, also on an allocated pick object.
    pub fn set_id(&self, value: Option<String>) -> Result<(), RuntimeError> {
        self.0.borrow_mut().options.id = value.clone();
        let pick = self.0.borrow().pick_id.as_ref().and_then(|p| p.object_as::<ScenePickObject>());
        if let (Some(pick_id), Some(pick)) = (self.0.borrow().pick_id.as_ref(), pick) {
            let mut replaced = ScenePickObject::new(pick.primitive.clone(), pick.primitive_show.clone());
            replaced.id = value.map(MetadataValue::String);
            pick_id.set_object(Rc::new(replaced))?;
        }
        Ok(())
    }

    pub fn split_direction(&self) -> SplitDirection {
        self.0.borrow().options.split_direction
    }
    pub fn set_split_direction(&self, value: SplitDirection) {
        if self.split_direction() != value {
            self.0.borrow_mut().options.split_direction = value;
            self.make_dirty(SPLIT_DIRECTION_INDEX);
        }
    }

    pub fn cluster_show(&self) -> bool {
        self.0.borrow().cluster_show
    }
    /// Set by clustering; marks `show` dirty.
    pub fn set_cluster_show(&self, value: bool) {
        if self.cluster_show() != value {
            self.0.borrow_mut().cluster_show = value;
            self.make_dirty(SHOW_INDEX);
        }
    }

    /// The position used for rendering: the position in 3D, its projection
    /// in 2D and Columbus view.
    pub fn actual_position(&self) -> Cartesian3 {
        self.0.borrow().actual_position
    }
    /// `_setActualPosition`: marks the position dirty.
    pub fn set_actual_position(&self, value: Cartesian3) {
        self.0.borrow_mut().actual_position = value;
        self.make_dirty(POSITION_INDEX);
    }

    /// The index in the collection's vertex buffers.
    pub fn index(&self) -> usize {
        self.0.borrow().index
    }
    /// Whether the point waits for a buffer rewrite.
    pub fn is_dirty(&self) -> bool {
        self.0.borrow().dirty
    }
    pub(crate) fn collection_id(&self) -> Option<u64> {
        self.0.borrow().collection_id
    }
    /// Whether the point still belongs to a collection.
    pub fn is_attached(&self) -> bool {
        self.0.borrow().collection.is_some()
    }

    /// The pick id, allocated by the first `getPickId`.
    pub fn pick_id(&self) -> Option<crate::core::Color> {
        self.0.borrow().pick_id.as_ref().map(|p| p.color)
    }

    /// Cesium's `getPickId`: allocates the pick id on first use. The pick
    /// object is a [`ScenePickObject`] whose `primitive` is a
    /// [`PointPrimitivePick`].
    pub fn get_pick_id<G: Gl>(&self, context: &Context<G>) -> Result<crate::core::Color, RuntimeError> {
        if let Some(color) = self.pick_id() {
            return Ok(color);
        }
        let (id, collection) = {
            let s = self.0.borrow();
            (s.options.id.clone(), s.collection_id)
        };
        let primitive: Rc<dyn std::any::Any> =
            Rc::new(PointPrimitivePick { primitive: self.clone(), collection, id: id.clone() });
        let mut object = ScenePickObject::new(primitive, Rc::new(std::cell::Cell::new(true)));
        object.id = id.map(MetadataValue::String);
        let pick_id = context.create_pick_id(Rc::new(object))?;
        let color = pick_id.color;
        self.0.borrow_mut().pick_id = Some(pick_id);
        Ok(color)
    }

    /// `_destroy`: releases the pick id and detaches from the collection.
    pub(crate) fn destroy(&self) {
        let mut s = self.0.borrow_mut();
        if let Some(pick_id) = s.pick_id.take() {
            pick_id.destroy();
        }
        s.collection = None;
        s.collection_id = None;
    }

    /// `PointPrimitive._computeActualPosition`.
    pub fn compute_actual_position(
        position: Cartesian3,
        state: &SceneTransformState<'_>,
        model_matrix: &Matrix4,
    ) -> Option<Cartesian3> {
        if state.mode == SceneMode::Scene3D {
            return Some(position);
        }
        SceneTransforms::compute_actual_ellipsoid_position(state, model_matrix.multiply_by_point(position))
    }

    /// `PointPrimitive.getScreenSpaceBoundingBox`.
    pub fn screen_space_bounding_box(&self, center: Cartesian2) -> BoundingRectangle {
        let size = self.pixel_size();
        BoundingRectangle::new(center.x - size * 0.5, center.y - size * 0.5, size, size)
    }

    /// Cesium's `equals`: every property except clusterShow, the actual
    /// position and the collection.
    pub fn equals(&self, other: &Self) -> bool {
        self.ptr_eq(other) || self.0.borrow().options == other.0.borrow().options
    }
}
