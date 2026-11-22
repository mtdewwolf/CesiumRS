//! CesiumJS `PathVisualizer`: maps `Entity.path` (`PathGraphics`) to [`Polyline`] primitives,
//! including sub-sampling of all position property types across fixed and inertial frames.
use super::{
    reference_property::ReferencePosition, visualizer_values::*, CallbackPositionProperty, CollectionChange,
    CompositePositionProperty, ConstantPositionProperty, EntityCollection, EntityRef, MaterialProperty, PathGraphics,
    PositionContext, PositionProperty, PositionPropertyRef, PropertyInput, SampledPositionProperty,
    ScaledPositionProperty, Subscription, TimeIntervalCollectionPositionProperty, Visualizer,
};
use crate::{
    core::{Cartesian3, JulianDate, Matrix4, ReferenceFrame, RuntimeError, TimeInterval},
    renderer::Gl,
    scene::{
        polyline::{Polyline, PolylineOptions},
        polyline_collection::{PolylineCollection, PolylineCollectionOptions},
        FrameState, SceneMode, ScenePrimitive,
    },
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const DEFAULT_RESOLUTION: f64 = 60.0;
const DEFAULT_WIDTH: f64 = 1.0;

struct EntityData {
    entity: EntityRef,
    polyline: Option<Polyline>,
    index: Option<usize>,
    frame: Option<ReferenceFrame>,
}

#[derive(Default)]
struct Items {
    order: Vec<String>,
    map: HashMap<String, EntityData>,
}

impl Items {
    fn insert(&mut self, id: String, data: EntityData) {
        if !self.map.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.map.insert(id, data);
    }
    fn remove(&mut self, id: &str) -> Option<EntityData> {
        if let Some(removed) = self.map.remove(id) {
            self.order.retain(|k| k != id);
            Some(removed)
        } else {
            None
        }
    }
    fn contains(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }
}

struct PolylineUpdater<G: Gl + 'static> {
    unused_indexes: Vec<usize>,
    polyline_collection: PolylineCollection<G>,
    reference_frame: ReferenceFrame,
}

impl<G: Gl + 'static> PolylineUpdater<G> {
    fn new(reference_frame: ReferenceFrame) -> Self {
        Self {
            unused_indexes: Vec::new(),
            polyline_collection: PolylineCollection::new(PolylineCollectionOptions::default()),
            reference_frame,
        }
    }

    fn update(&mut self, time: &JulianDate, context: &PositionContext) -> Result<(), RuntimeError> {
        if self.reference_frame == ReferenceFrame::Inertial {
            let to_fixed = (context.frame_transform)(time)?;
            self.polyline_collection.set_model_matrix(Matrix4::from_rotation_translation(&to_fixed, Cartesian3::ZERO));
        }
        Ok(())
    }

    fn update_object(&mut self, time: &JulianDate, item: &mut EntityData) -> Result<(), RuntimeError> {
        let entity = Rc::clone(&item.entity);
        let Some(PropertyInput::Component(comp)) = entity.get_property("path") else { return Ok(()) };
        let Some(graphics) = comp.as_any().downcast_ref::<PathGraphics>() else { return Ok(()) };
        let Some(position_property) = entity.position() else { return Ok(()) };

        let mut show =
            entity.is_showing() && entity.is_available(time) && eval_bool(graphics.show().as_ref(), time, true)?;

        let mut sample_start = *time;
        let mut sample_stop = *time;

        if show {
            let lead_time = eval_f64_opt(graphics.lead_time().as_ref(), time)?;
            let trail_time = eval_f64_opt(graphics.trail_time().as_ref(), time)?;
            let availability = entity.availability();
            let avail_bounds = availability.as_ref().and_then(|a| {
                let r = a.read();
                Some((r.start()?, r.stop()?))
            });
            let has_availability = avail_bounds.is_some();
            let has_lead_time = lead_time.is_some();
            let has_trail_time = trail_time.is_some();

            show = has_availability || (has_lead_time && has_trail_time);
            if show {
                if let Some(trail) = trail_time {
                    sample_start = time.add_seconds(-trail);
                }
                if let Some(lead) = lead_time {
                    sample_stop = time.add_seconds(lead);
                }
                if let Some((start, stop)) = avail_bounds {
                    if !has_trail_time || JulianDate::greater_than(&start, &sample_start) {
                        sample_start = start;
                    }
                    if !has_lead_time || JulianDate::less_than(&stop, &sample_stop) {
                        sample_stop = stop;
                    }
                }
                show = JulianDate::less_than(&sample_start, &sample_stop);
            }
        }

        if !show {
            if let Some(polyline) = item.polyline.take() {
                if let Some(idx) = item.index.take() {
                    self.unused_indexes.push(idx);
                }
                polyline.set_show(false);
            }
            return Ok(());
        }

        if item.polyline.is_none() {
            let (idx, polyline) = if let Some(idx) = self.unused_indexes.pop() {
                let p = self
                    .polyline_collection
                    .get(idx)
                    .ok_or_else(|| RuntimeError::new("Invalid reused polyline index."))?;
                (idx, p)
            } else {
                let idx = self.polyline_collection.length();
                let p = self.polyline_collection.add(PolylineOptions::default())?;
                (idx, p)
            };
            polyline.set_id(Some(entity.id().to_owned()))?;
            item.index = Some(idx);
            item.polyline = Some(polyline);
        }

        let polyline = item.polyline.as_ref().unwrap();
        let resolution = eval_f64(graphics.resolution().as_ref(), time, DEFAULT_RESOLUTION)?;
        let positions =
            sub_sample(&position_property, &sample_start, &sample_stop, Some(time), self.reference_frame, resolution)?;
        polyline.set_show(true);
        polyline.set_positions(positions);

        let mat_comp = slot_component(graphics.material());
        let existing_mat = polyline.material().borrow().clone();
        let new_mat = MaterialProperty::get_value(Some(time), mat_comp.as_deref(), Some(existing_mat))?;
        polyline.set_material(Rc::new(RefCell::new(new_mat)));

        polyline.set_width(eval_f64(graphics.width().as_ref(), time, DEFAULT_WIDTH)?);
        polyline.set_distance_display_condition(eval_distance_display_condition_opt(
            graphics.distance_display_condition().as_ref(),
            time,
        )?)?;
        Ok(())
    }

    fn remove_object(&mut self, item: &mut EntityData) {
        if let Some(polyline) = item.polyline.take() {
            if let Some(idx) = item.index.take() {
                self.unused_indexes.push(idx);
            }
            polyline.set_show(false);
            let _ = polyline.set_id(None);
        }
    }

    fn destroy(&mut self) {
        let _ = self.polyline_collection.destroy();
    }
}

/// Port of CesiumJS `PathVisualizer`.
pub struct PathVisualizer<G: Gl + 'static> {
    scene_mode: SceneMode,
    context: PositionContext,
    updaters: Rc<RefCell<HashMap<ReferenceFrame, PolylineUpdater<G>>>>,
    entity_collection: Rc<EntityCollection>,
    items: Rc<RefCell<Items>>,
    subscription: Option<Subscription>,
    destroyed: bool,
}

impl<G: Gl + 'static> PathVisualizer<G> {
    pub fn new(scene_mode: SceneMode, entity_collection: Rc<EntityCollection>) -> Self {
        Self::with_context(scene_mode, entity_collection, PositionContext::default())
    }

    pub fn with_context(
        scene_mode: SceneMode,
        entity_collection: Rc<EntityCollection>,
        context: PositionContext,
    ) -> Self {
        let items = Rc::new(RefCell::new(Items::default()));
        let updaters: Rc<RefCell<HashMap<ReferenceFrame, PolylineUpdater<G>>>> = Rc::new(RefCell::new(HashMap::new()));
        let items_cb = Rc::clone(&items);
        let updaters_cb = Rc::clone(&updaters);
        let sub = entity_collection.collection_changed().add_listener(move |change: &CollectionChange| {
            on_collection_changed(&items_cb, &updaters_cb, &change.added, &change.removed, &change.changed);
        });

        let initial = entity_collection.values();
        on_collection_changed(&items, &updaters, &initial, &[], &[]);

        Self { scene_mode, context, updaters, entity_collection, items, subscription: Some(sub), destroyed: false }
    }

    pub fn set_scene_mode(&mut self, mode: SceneMode) {
        self.scene_mode = mode;
    }

    /// Submits the sampled paths from the most recent update to the renderer.
    pub fn update_frame(&self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let mut updaters = self.updaters.borrow_mut();
        for reference_frame in [ReferenceFrame::Fixed, ReferenceFrame::Inertial] {
            if let Some(updater) = updaters.get_mut(&reference_frame) {
                updater.polyline_collection.update(frame)?;
            }
        }
        Ok(())
    }

    pub fn polyline_for(&self, entity_id: &str) -> Option<Polyline> {
        self.items.borrow().map.get(entity_id).and_then(|d| d.polyline.clone())
    }

    pub fn updater_count(&self) -> usize {
        self.updaters.borrow().len()
    }

    pub fn model_matrix_for_frame(&self, frame: ReferenceFrame) -> Option<Matrix4> {
        self.updaters.borrow().get(&frame).map(|u| u.polyline_collection.model_matrix())
    }

    /// CesiumJS `PathVisualizer._subSample`: subsamples `property` over `[start, stop]`.
    pub fn sub_sample(
        property: &PositionPropertyRef,
        start: &JulianDate,
        stop: &JulianDate,
        update_time: Option<&JulianDate>,
        reference_frame: ReferenceFrame,
        maximum_step: f64,
    ) -> Result<Vec<Cartesian3>, RuntimeError> {
        sub_sample(property, start, stop, update_time, reference_frame, maximum_step)
    }
}

impl<G: Gl + 'static> Visualizer for PathVisualizer<G> {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        for updater in self.updaters.borrow_mut().values_mut() {
            updater.update(time, &self.context)?;
        }

        let ids: Vec<String> = self.items.borrow().order.clone();
        if ids.is_empty() && !self.updaters.borrow().is_empty() {
            for updater in self.updaters.borrow_mut().values_mut() {
                updater.destroy();
            }
            self.updaters.borrow_mut().clear();
        }

        for id in ids {
            let mut items = self.items.borrow_mut();
            let Some(item) = items.map.get_mut(&id) else { continue };
            let Some(position_property) = item.entity.position() else { continue };

            let frame_to_visualize = if self.scene_mode == SceneMode::Scene3D {
                position_property.reference_frame()
            } else {
                ReferenceFrame::Fixed
            };

            let last_frame = item.frame;
            let mut updaters = self.updaters.borrow_mut();

            if last_frame == Some(frame_to_visualize) {
                if let Some(updater) = updaters.get_mut(&frame_to_visualize) {
                    updater.update_object(time, item)?;
                    continue;
                }
            }

            if let Some(prev) = last_frame {
                if let Some(prev_updater) = updaters.get_mut(&prev) {
                    prev_updater.remove_object(item);
                }
            }

            if let std::collections::hash_map::Entry::Vacant(e) = updaters.entry(frame_to_visualize) {
                let mut created = PolylineUpdater::new(frame_to_visualize);
                created.update(time, &self.context)?;
                e.insert(created);
            }

            item.frame = Some(frame_to_visualize);
            if let Some(updater) = updaters.get_mut(&frame_to_visualize) {
                updater.update_object(time, item)?;
            }
        }
        Ok(true)
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            self.destroyed = true;
            self.subscription = None;
            for updater in self.updaters.borrow_mut().values_mut() {
                updater.destroy();
            }
            self.updaters.borrow_mut().clear();
            self.items.borrow_mut().map.clear();
            self.items.borrow_mut().order.clear();
            let _ = &self.entity_collection;
        }
    }
}

fn on_collection_changed<G: Gl + 'static>(
    items: &RefCell<Items>,
    updaters: &RefCell<HashMap<ReferenceFrame, PolylineUpdater<G>>>,
    added: &[EntityRef],
    removed: &[EntityRef],
    changed: &[EntityRef],
) {
    let mut items = items.borrow_mut();
    for entity in added.iter().rev() {
        if entity.get_property("path").is_some() && entity.get_property("position").is_some() {
            items.insert(
                entity.id().to_owned(),
                EntityData { entity: Rc::clone(entity), polyline: None, index: None, frame: None },
            );
        }
    }
    for entity in changed.iter().rev() {
        let id = entity.id().to_owned();
        if entity.get_property("path").is_some() && entity.get_property("position").is_some() {
            if !items.contains(&id) {
                items.insert(id, EntityData { entity: Rc::clone(entity), polyline: None, index: None, frame: None });
            }
        } else if let Some(mut item) = items.remove(&id) {
            if let Some(frame) = item.frame {
                if let Some(updater) = updaters.borrow_mut().get_mut(&frame) {
                    updater.remove_object(&mut item);
                }
            }
        }
    }
    for entity in removed.iter().rev() {
        if let Some(mut item) = items.remove(entity.id()) {
            if let Some(frame) = item.frame {
                if let Some(updater) = updaters.borrow_mut().get_mut(&frame) {
                    updater.remove_object(&mut item);
                }
            }
        }
    }
}

/// Sub-samples a [`PositionProperty`] over `[start, stop]` in `reference_frame`,
/// matching CesiumJS `PathVisualizer._subSample`.
pub fn sub_sample(
    property: &PositionPropertyRef,
    start: &JulianDate,
    stop: &JulianDate,
    update_time: Option<&JulianDate>,
    reference_frame: ReferenceFrame,
    maximum_step: f64,
) -> Result<Vec<Cartesian3>, RuntimeError> {
    let mut result = Vec::new();
    really_sub_sample(property, start, stop, update_time, reference_frame, maximum_step, &mut result)?;
    Ok(result)
}

fn unwrap_reference(property: &PositionPropertyRef) -> Option<PositionPropertyRef> {
    let mut current = Rc::clone(property);
    for _ in 0..64 {
        let next = if let Some(ref_pos) = current.as_any().downcast_ref::<ReferencePosition>() {
            match ref_pos.0.resolved_property() {
                Some(PropertyInput::Component(comp)) => comp.position_property(),
                _ => return None,
            }
        } else {
            return Some(current);
        };
        current = next?;
    }
    Some(current)
}

fn really_sub_sample(
    property: &PositionPropertyRef,
    start: &JulianDate,
    stop: &JulianDate,
    update_time: Option<&JulianDate>,
    reference_frame: ReferenceFrame,
    maximum_step: f64,
    result: &mut Vec<Cartesian3>,
) -> Result<(), RuntimeError> {
    let Some(property) = unwrap_reference(property) else {
        return Ok(());
    };
    let any = property.as_any();
    if let Some(sampled) = any.downcast_ref::<SampledPositionProperty>() {
        sub_sample_sampled_property(sampled, start, stop, update_time, reference_frame, maximum_step, result)
    } else if let Some(cb) = any.downcast_ref::<CallbackPositionProperty>() {
        sub_sample_callback_property(cb, start, stop, update_time, reference_frame, maximum_step, result)
    } else if let Some(comp) = any.downcast_ref::<CompositePositionProperty>() {
        sub_sample_composite_property(comp, start, stop, update_time, reference_frame, maximum_step, result)
    } else if let Some(ti) = any.downcast_ref::<TimeIntervalCollectionPositionProperty>() {
        sub_sample_interval_property(ti, start, stop, reference_frame, result)
    } else if any.downcast_ref::<ConstantPositionProperty>().is_some()
        || (any.downcast_ref::<ScaledPositionProperty>().is_some() && property.is_constant())
    {
        if let Some(v) = property.get_value_in_reference_frame(start, reference_frame, None)? {
            result.push(v);
        }
        Ok(())
    } else {
        sub_sample_generic_property(&*property, start, stop, update_time, reference_frame, maximum_step, result)
    }
}

fn sub_sample_sampled_property(
    property: &SampledPositionProperty,
    start: &JulianDate,
    stop: &JulianDate,
    update_time: Option<&JulianDate>,
    reference_frame: ReferenceFrame,
    maximum_step: f64,
    result: &mut Vec<Cartesian3>,
) -> Result<(), RuntimeError> {
    if let Some(v) = property.get_value_in_reference_frame(start, reference_frame, None)? {
        result.push(v);
    }

    let mut stepped_on_now = match update_time {
        None => true,
        Some(u) => JulianDate::less_than_or_equals(u, start) || JulianDate::greater_than_or_equals(u, stop),
    };

    let times = property.times();
    let len = times.len();
    let mut t = 0usize;
    let mut sampling = false;
    let mut sample_steps_to_take = 0usize;
    let mut sample_steps_taken = 0usize;
    let mut sample_step_size = 0.0;

    if len > 0 {
        let mut current = times[0];
        while t < len {
            if !stepped_on_now {
                if let Some(u) = update_time {
                    if JulianDate::greater_than_or_equals(&current, u) {
                        if let Some(v) = property.get_value_in_reference_frame(u, reference_frame, None)? {
                            result.push(v);
                        }
                        stepped_on_now = true;
                    }
                }
            }

            let eq_update = update_time.is_some_and(|u| current == *u);
            if JulianDate::greater_than(&current, start) && JulianDate::less_than(&current, stop) && !eq_update {
                if let Some(v) = property.get_value_in_reference_frame(&current, reference_frame, None)? {
                    result.push(v);
                }
            }

            if t < len - 1 {
                if maximum_step > 0.0 && !sampling {
                    let next = times[t + 1];
                    let seconds_until_next = JulianDate::seconds_difference(&next, &current);
                    sampling = seconds_until_next > maximum_step;
                    if sampling {
                        let steps = (seconds_until_next / maximum_step).ceil();
                        sample_steps_taken = 0;
                        sample_step_size = seconds_until_next / steps.max(2.0);
                        sample_steps_to_take = (steps - 1.0).max(1.0) as usize;
                    }
                }
                if sampling && sample_steps_taken < sample_steps_to_take {
                    current = current.add_seconds(sample_step_size);
                    sample_steps_taken += 1;
                    continue;
                }
            }
            sampling = false;
            t += 1;
            if t < len {
                current = times[t];
            }
        }
    }

    if let Some(v) = property.get_value_in_reference_frame(stop, reference_frame, None)? {
        result.push(v);
    }
    Ok(())
}

fn sub_sample_callback_property(
    property: &CallbackPositionProperty,
    start: &JulianDate,
    stop: &JulianDate,
    update_time: Option<&JulianDate>,
    reference_frame: ReferenceFrame,
    maximum_step: f64,
    result: &mut Vec<Cartesian3>,
) -> Result<(), RuntimeError> {
    let mut i = 0usize;
    let mut time = *start;
    let mut stepped_on_now = match update_time {
        None => true,
        Some(u) => JulianDate::less_than_or_equals(u, start) || JulianDate::greater_than_or_equals(u, stop),
    };
    while JulianDate::less_than(&time, stop) {
        if !stepped_on_now {
            if let Some(u) = update_time {
                if JulianDate::greater_than_or_equals(&time, u) {
                    stepped_on_now = true;
                    if let Some(v) = property.get_value_in_reference_frame(u, reference_frame, None)? {
                        result.push(v);
                    }
                }
            }
        }
        if let Some(v) = property.get_value_in_reference_frame(&time, reference_frame, None)? {
            result.push(v);
        }
        i += 1;
        time = start.add_seconds(maximum_step * i as f64);
    }
    if let Some(v) = property.get_value_in_reference_frame(stop, reference_frame, None)? {
        result.push(v);
    }
    Ok(())
}

fn sub_sample_generic_property(
    property: &dyn PositionProperty,
    start: &JulianDate,
    stop: &JulianDate,
    update_time: Option<&JulianDate>,
    reference_frame: ReferenceFrame,
    maximum_step: f64,
    result: &mut Vec<Cartesian3>,
) -> Result<(), RuntimeError> {
    let step_size = if maximum_step.is_finite() && maximum_step > 0.0 { maximum_step } else { DEFAULT_RESOLUTION };
    let mut i = 0usize;
    let mut time = *start;
    let mut stepped_on_now = match update_time {
        None => true,
        Some(u) => JulianDate::less_than_or_equals(u, start) || JulianDate::greater_than_or_equals(u, stop),
    };
    while JulianDate::less_than(&time, stop) {
        if !stepped_on_now {
            if let Some(u) = update_time {
                if JulianDate::greater_than_or_equals(&time, u) {
                    stepped_on_now = true;
                    if let Some(v) = property.get_value_in_reference_frame(u, reference_frame, None)? {
                        result.push(v);
                    }
                }
            }
        }
        if let Some(v) = property.get_value_in_reference_frame(&time, reference_frame, None)? {
            result.push(v);
        }
        i += 1;
        time = start.add_seconds(step_size * i as f64);
    }
    if let Some(v) = property.get_value_in_reference_frame(stop, reference_frame, None)? {
        result.push(v);
    }
    Ok(())
}

fn sub_sample_interval_property(
    property: &TimeIntervalCollectionPositionProperty,
    start: &JulianDate,
    stop: &JulianDate,
    reference_frame: ReferenceFrame,
    result: &mut Vec<Cartesian3>,
) -> Result<(), RuntimeError> {
    let window = TimeInterval::<()>::new(*start, *stop, true, true, None);
    let intervals = property.intervals().read();
    for i in 0..intervals.len() {
        let Some(interval) = intervals.get(i) else { continue };
        let range = TimeInterval::new(
            interval.start,
            interval.stop,
            interval.is_start_included,
            interval.is_stop_included,
            None,
        );
        if !range.intersect(&window, None).is_empty() {
            let time = if interval.is_start_included {
                interval.start
            } else if interval.is_stop_included {
                interval.stop
            } else {
                interval.start.add_seconds(JulianDate::seconds_difference(&interval.stop, &interval.start) / 2.0)
            };
            if let Some(v) = property.get_value_in_reference_frame(&time, reference_frame, None)? {
                result.push(v);
            }
        }
    }
    Ok(())
}

fn sub_sample_composite_property(
    property: &CompositePositionProperty,
    start: &JulianDate,
    stop: &JulianDate,
    update_time: Option<&JulianDate>,
    reference_frame: ReferenceFrame,
    maximum_step: f64,
    result: &mut Vec<Cartesian3>,
) -> Result<(), RuntimeError> {
    let window = TimeInterval::<()>::new(*start, *stop, true, true, None);
    let entries: Vec<(JulianDate, JulianDate, PositionPropertyRef)> = {
        let intervals = property.intervals().read();
        let mut out = Vec::new();
        for i in 0..intervals.len() {
            let Some(interval) = intervals.get(i) else { continue };
            let range = TimeInterval::new(
                interval.start,
                interval.stop,
                interval.is_start_included,
                interval.is_stop_included,
                None,
            );
            if !range.intersect(&window, None).is_empty() {
                if let Some(data) = interval.data.as_ref() {
                    out.push((interval.start, interval.stop, Rc::clone(data)));
                }
            }
        }
        out
    };
    for (istart, istop, child) in entries {
        let sample_start = if JulianDate::greater_than(&istart, start) { istart } else { *start };
        let sample_stop = if JulianDate::less_than(&istop, stop) { istop } else { *stop };
        really_sub_sample(&child, &sample_start, &sample_stop, update_time, reference_frame, maximum_step, result)?;
    }
    Ok(())
}
