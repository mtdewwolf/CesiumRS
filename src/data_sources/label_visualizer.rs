//! CesiumJS `LabelVisualizer`: maps `Entity.label` (`LabelGraphics`) to [`Label`](crate::scene::Label) primitives.
use super::{
    visualizer_values::*, BoundingSphereState, CollectionChange, EntityCluster, EntityCollection, EntityRef,
    LabelGraphics, PropertyInput, Subscription, Visualizer,
};
use crate::{
    core::{BoundingSphere, Cartesian2, Cartesian3, Color, JulianDate, RuntimeError},
    renderer::Gl,
    scene::{HeightReference, HorizontalOrigin, LabelHandle, LabelStyle, VerticalOrigin},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const DEFAULT_SCALE: f64 = 1.0;
const DEFAULT_FONT: &str = "30px sans-serif";
const DEFAULT_STYLE: LabelStyle = LabelStyle::Fill;
const DEFAULT_FILL_COLOR: Color = Color::WHITE;
const DEFAULT_OUTLINE_COLOR: Color = Color::BLACK;
const DEFAULT_OUTLINE_WIDTH: f64 = 1.0;
const DEFAULT_SHOW_BACKGROUND: bool = false;
const DEFAULT_BACKGROUND_COLOR: Color = Color { red: 0.165, green: 0.165, blue: 0.165, alpha: 0.8 };
const DEFAULT_BACKGROUND_PADDING: Cartesian2 = Cartesian2::new(7.0, 5.0);
const DEFAULT_PIXEL_OFFSET: Cartesian2 = Cartesian2::ZERO;
const DEFAULT_EYE_OFFSET: Cartesian3 = Cartesian3::ZERO;
const DEFAULT_HEIGHT_REFERENCE: HeightReference = HeightReference::None;
const DEFAULT_HORIZONTAL_ORIGIN: HorizontalOrigin = HorizontalOrigin::Center;
const DEFAULT_VERTICAL_ORIGIN: VerticalOrigin = VerticalOrigin::Center;

struct EntityData {
    entity: EntityRef,
    label: Option<LabelHandle>,
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

/// Port of CesiumJS `LabelVisualizer`.
pub struct LabelVisualizer<G: Gl + 'static> {
    cluster: Rc<EntityCluster<G>>,
    entity_collection: Rc<EntityCollection>,
    items: Rc<RefCell<Items>>,
    subscription: Option<Subscription>,
    destroyed: bool,
}

impl<G: Gl + 'static> LabelVisualizer<G> {
    pub fn new(cluster: Rc<EntityCluster<G>>, entity_collection: Rc<EntityCollection>) -> Self {
        let items = Rc::new(RefCell::new(Items::default()));
        let items_cb = Rc::clone(&items);
        let cluster_cb = Rc::clone(&cluster);
        let sub = entity_collection.collection_changed().add_listener(move |change: &CollectionChange| {
            on_collection_changed(&items_cb, &cluster_cb, &change.added, &change.removed, &change.changed);
        });

        let initial = entity_collection.values();
        on_collection_changed(&items, &cluster, &initial, &[], &[]);

        Self { cluster, entity_collection, items, subscription: Some(sub), destroyed: false }
    }

    /// Returns the currently active [`LabelHandle`] for `entity_id`, if any.
    pub fn label_for(&self, entity_id: &str) -> Option<LabelHandle> {
        self.items.borrow().map.get(entity_id).and_then(|d| d.label.clone())
    }
}

impl<G: Gl + 'static> Visualizer for LabelVisualizer<G> {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let ids: Vec<String> = self.items.borrow().order.clone();
        for id in ids {
            let entity = match self.items.borrow().map.get(&id) {
                Some(item) => Rc::clone(&item.entity),
                None => continue,
            };
            let Some(PropertyInput::Component(comp)) = entity.get_property("label") else { continue };
            let Some(graphics) = comp.as_any().downcast_ref::<LabelGraphics>() else { continue };

            let mut show =
                entity.is_showing() && entity.is_available(time) && eval_bool(graphics.show().as_ref(), time, true)?;

            let mut position = None;
            let mut text = None;
            if show {
                position = entity.position_at(time)?;
                text = eval_string_opt(graphics.text().as_ref(), time)?;
                show = position.is_some() && text.is_some();
            }

            if !show {
                let mut items = self.items.borrow_mut();
                if let Some(item) = items.map.get_mut(&id) {
                    return_primitive(item, &entity, &self.cluster);
                }
                continue;
            }

            let pos_slot = entity.get_property("position");
            if !is_slot_constant(pos_slot.as_ref()) {
                self.cluster.mark_cluster_dirty();
            }

            let height_reference =
                eval_height_reference(graphics.height_reference().as_ref(), time, DEFAULT_HEIGHT_REFERENCE)?;
            let pos = position.unwrap();
            let mut update_clamping = false;

            let label = {
                let mut items = self.items.borrow_mut();
                let item = items.map.get_mut(&id).unwrap();
                if item.label.is_none() {
                    let l = self.cluster.get_label(&entity)?;
                    {
                        let mut lb = l.borrow_mut();
                        lb.set_id(Some(entity.id().to_owned()));
                        update_clamping = lb.position() == pos && lb.height_reference() == height_reference;
                    }
                    item.label = Some(l);
                }
                item.label.clone().unwrap()
            };

            let mut lb = label.borrow_mut();
            lb.set_show(true);
            lb.set_position(pos);
            lb.set_text(text.unwrap());
            lb.set_scale(eval_f64(graphics.scale().as_ref(), time, DEFAULT_SCALE)?);
            lb.set_font(eval_string(graphics.font().as_ref(), time, DEFAULT_FONT)?);
            lb.set_style(eval_label_style(graphics.style().as_ref(), time, DEFAULT_STYLE)?);
            lb.set_fill_color(eval_color(graphics.fill_color().as_ref(), time, DEFAULT_FILL_COLOR)?);
            lb.set_outline_color(eval_color(graphics.outline_color().as_ref(), time, DEFAULT_OUTLINE_COLOR)?);
            lb.set_outline_width(eval_f64(graphics.outline_width().as_ref(), time, DEFAULT_OUTLINE_WIDTH)?);
            lb.set_show_background(eval_bool(graphics.show_background().as_ref(), time, DEFAULT_SHOW_BACKGROUND)?);
            lb.set_background_color(eval_color(graphics.background_color().as_ref(), time, DEFAULT_BACKGROUND_COLOR)?);
            lb.set_background_padding(eval_cartesian2(
                graphics.background_padding().as_ref(),
                time,
                DEFAULT_BACKGROUND_PADDING,
            )?);
            lb.set_pixel_offset(eval_cartesian2(graphics.pixel_offset().as_ref(), time, DEFAULT_PIXEL_OFFSET)?);
            lb.set_eye_offset(eval_cartesian3(graphics.eye_offset().as_ref(), time, DEFAULT_EYE_OFFSET)?);
            lb.set_height_reference(height_reference);
            lb.set_horizontal_origin(eval_horizontal_origin(
                graphics.horizontal_origin().as_ref(),
                time,
                DEFAULT_HORIZONTAL_ORIGIN,
            )?);
            lb.set_vertical_origin(eval_vertical_origin(
                graphics.vertical_origin().as_ref(),
                time,
                DEFAULT_VERTICAL_ORIGIN,
            )?);
            lb.set_translucency_by_distance(eval_near_far_scalar_opt(
                graphics.translucency_by_distance().as_ref(),
                time,
            )?)
            .map_err(RuntimeError::new)?;
            lb.set_pixel_offset_scale_by_distance(eval_near_far_scalar_opt(
                graphics.pixel_offset_scale_by_distance().as_ref(),
                time,
            )?)
            .map_err(RuntimeError::new)?;
            lb.set_scale_by_distance(eval_near_far_scalar_opt(graphics.scale_by_distance().as_ref(), time)?)
                .map_err(RuntimeError::new)?;
            lb.set_distance_display_condition(eval_distance_display_condition_opt(
                graphics.distance_display_condition().as_ref(),
                time,
            )?)
            .map_err(RuntimeError::new)?;
            lb.set_disable_depth_test_distance(eval_f64_opt(graphics.disable_depth_test_distance().as_ref(), time)?)
                .map_err(RuntimeError::new)?;
            if update_clamping {
                lb.set_clamped_position(None);
            }
        }
        Ok(true)
    }

    fn get_bounding_sphere(&self, entity: &EntityRef, result: &mut BoundingSphere) -> BoundingSphereState {
        let items = self.items.borrow();
        let Some(label) = items.map.get(entity.id()).and_then(|d| d.label.as_ref()) else {
            return BoundingSphereState::Failed;
        };
        let lb = label.borrow();
        result.center = lb.clamped_position().unwrap_or_else(|| lb.position());
        result.radius = 0.0;
        BoundingSphereState::Done
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            self.destroyed = true;
            self.subscription = None;
            for entity in self.entity_collection.values() {
                self.cluster.remove_label(&entity);
            }
            self.items.borrow_mut().map.clear();
            self.items.borrow_mut().order.clear();
        }
    }
}

fn on_collection_changed<G: Gl + 'static>(
    items: &RefCell<Items>,
    cluster: &EntityCluster<G>,
    added: &[EntityRef],
    removed: &[EntityRef],
    changed: &[EntityRef],
) {
    let mut items = items.borrow_mut();
    for entity in added.iter().rev() {
        if entity.get_property("label").is_some() && entity.get_property("position").is_some() {
            items.insert(entity.id().to_owned(), EntityData { entity: Rc::clone(entity), label: None });
        }
    }
    for entity in changed.iter().rev() {
        let id = entity.id().to_owned();
        if entity.get_property("label").is_some() && entity.get_property("position").is_some() {
            if !items.contains(&id) {
                items.insert(id, EntityData { entity: Rc::clone(entity), label: None });
            }
        } else if let Some(mut data) = items.remove(&id) {
            return_primitive(&mut data, entity, cluster);
        }
    }
    for entity in removed.iter().rev() {
        if let Some(mut data) = items.remove(&entity.id()) {
            return_primitive(&mut data, entity, cluster);
        }
    }
}

fn return_primitive<G: Gl + 'static>(item: &mut EntityData, entity: &EntityRef, cluster: &EntityCluster<G>) {
    item.label = None;
    cluster.remove_label(entity);
}
