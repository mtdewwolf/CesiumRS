//! CesiumJS `BillboardVisualizer`: maps `Entity.billboard` (`BillboardGraphics`) to [`Billboard`] primitives.
use super::{
    visualizer_values::*, BillboardGraphics, BoundingSphereState, CollectionChange, DynamicValue, EntityCluster,
    EntityCollection, EntityRef, PropertyInput, PropertyValue, Subscription, Visualizer,
};
use crate::{
    core::{BoundingSphere, Cartesian2, Cartesian3, Color, JulianDate, RuntimeError},
    renderer::Gl,
    scene::{
        billboard::{Billboard, BillboardImage},
        HeightReference, HorizontalOrigin, SplitDirection, VerticalOrigin,
    },
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const DEFAULT_COLOR: Color = Color::WHITE;
const DEFAULT_EYE_OFFSET: Cartesian3 = Cartesian3::ZERO;
const DEFAULT_HEIGHT_REFERENCE: HeightReference = HeightReference::None;
const DEFAULT_PIXEL_OFFSET: Cartesian2 = Cartesian2::ZERO;
const DEFAULT_SCALE: f64 = 1.0;
const DEFAULT_ROTATION: f64 = 0.0;
const DEFAULT_ALIGNED_AXIS: Cartesian3 = Cartesian3::ZERO;
const DEFAULT_HORIZONTAL_ORIGIN: HorizontalOrigin = HorizontalOrigin::Center;
const DEFAULT_VERTICAL_ORIGIN: VerticalOrigin = VerticalOrigin::Center;
const DEFAULT_SIZE_IN_METERS: bool = false;
const DEFAULT_SPLIT_DIRECTION: SplitDirection = SplitDirection::None;

/// Host resolver translating a billboard graphics `image` value into `(image_id, BillboardImage)`.
pub type BillboardImageResolver<G> = Rc<dyn Fn(&DynamicValue) -> Option<(String, BillboardImage<G>)>>;

struct EntityData<G: Gl + 'static> {
    entity: EntityRef,
    billboard: Option<Billboard<G>>,
    texture_value: Option<DynamicValue>,
}

struct Items<G: Gl + 'static> {
    order: Vec<String>,
    map: HashMap<String, EntityData<G>>,
}

impl<G: Gl + 'static> Default for Items<G> {
    fn default() -> Self {
        Self { order: Vec::new(), map: HashMap::new() }
    }
}

impl<G: Gl + 'static> Items<G> {
    fn insert(&mut self, id: String, data: EntityData<G>) {
        if !self.map.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.map.insert(id, data);
    }
    fn remove(&mut self, id: &str) -> Option<EntityData<G>> {
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

/// Port of CesiumJS `BillboardVisualizer`.
pub struct BillboardVisualizer<G: Gl + 'static> {
    cluster: Rc<EntityCluster<G>>,
    entity_collection: Rc<EntityCollection>,
    items: Rc<RefCell<Items<G>>>,
    image_resolver: BillboardImageResolver<G>,
    subscription: Option<Subscription>,
    destroyed: bool,
}

impl<G: Gl + 'static> BillboardVisualizer<G> {
    pub fn new(cluster: Rc<EntityCluster<G>>, entity_collection: Rc<EntityCollection>) -> Self {
        Self::with_image_resolver(cluster, entity_collection, Rc::new(default_image_resolver))
    }

    pub fn with_image_resolver(
        cluster: Rc<EntityCluster<G>>,
        entity_collection: Rc<EntityCollection>,
        image_resolver: BillboardImageResolver<G>,
    ) -> Self {
        let items = Rc::new(RefCell::new(Items::default()));
        let items_cb = Rc::clone(&items);
        let cluster_cb = Rc::clone(&cluster);
        let sub = entity_collection.collection_changed().add_listener(move |change: &CollectionChange| {
            on_collection_changed(&items_cb, &cluster_cb, &change.added, &change.removed, &change.changed);
        });

        let initial = entity_collection.values();
        on_collection_changed(&items, &cluster, &initial, &[], &[]);

        Self { cluster, entity_collection, items, image_resolver, subscription: Some(sub), destroyed: false }
    }

    /// Returns the currently active [`Billboard`] for `entity_id`, if any.
    pub fn billboard_for(&self, entity_id: &str) -> Option<Billboard<G>> {
        self.items.borrow().map.get(entity_id).and_then(|d| d.billboard.clone())
    }
}

impl<G: Gl + 'static> Visualizer for BillboardVisualizer<G> {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let ids: Vec<String> = self.items.borrow().order.clone();
        for id in ids {
            let entity = match self.items.borrow().map.get(&id) {
                Some(item) => Rc::clone(&item.entity),
                None => continue,
            };
            let graphics_slot = entity.get_property("billboard");
            let Some(PropertyInput::Component(comp)) = graphics_slot else { continue };
            let Some(graphics) = comp.as_any().downcast_ref::<BillboardGraphics>() else { continue };

            let mut show =
                entity.is_showing() && entity.is_available(time) && eval_bool(graphics.show().as_ref(), time, true)?;

            let mut position = None;
            let mut texture_value = None;
            if show {
                position = entity.position_at(time)?;
                texture_value = eval_dynamic(graphics.image().as_ref(), time)?;
                show = position.is_some() && texture_value.is_some();
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

            let (billboard, prev_texture) = {
                let mut items = self.items.borrow_mut();
                let item = items.map.get_mut(&id).unwrap();
                if item.billboard.is_none() {
                    let b = self.cluster.get_billboard(&entity)?;
                    b.set_id(Some(entity.id().to_owned()))?;
                    item.billboard = Some(b);
                    item.texture_value = None;
                }
                (item.billboard.clone().unwrap(), item.texture_value.clone())
            };

            billboard.set_show(true);
            billboard.set_position(position.unwrap());
            billboard.set_color(eval_color(graphics.color().as_ref(), time, DEFAULT_COLOR)?);
            billboard.set_eye_offset(eval_cartesian3(graphics.eye_offset().as_ref(), time, DEFAULT_EYE_OFFSET)?);
            billboard.set_height_reference(eval_height_reference(
                graphics.height_reference().as_ref(),
                time,
                DEFAULT_HEIGHT_REFERENCE,
            )?)?;
            billboard.set_pixel_offset(eval_cartesian2(graphics.pixel_offset().as_ref(), time, DEFAULT_PIXEL_OFFSET)?);
            billboard.set_scale(eval_f64(graphics.scale().as_ref(), time, DEFAULT_SCALE)?);
            billboard.set_rotation(eval_f64(graphics.rotation().as_ref(), time, DEFAULT_ROTATION)?);
            billboard.set_aligned_axis(eval_cartesian3(graphics.aligned_axis().as_ref(), time, DEFAULT_ALIGNED_AXIS)?);
            billboard.set_horizontal_origin(eval_horizontal_origin(
                graphics.horizontal_origin().as_ref(),
                time,
                DEFAULT_HORIZONTAL_ORIGIN,
            )?);
            billboard.set_vertical_origin(eval_vertical_origin(
                graphics.vertical_origin().as_ref(),
                time,
                DEFAULT_VERTICAL_ORIGIN,
            )?);
            billboard.set_width(eval_f64_opt(graphics.width().as_ref(), time)?);
            billboard.set_height(eval_f64_opt(graphics.height().as_ref(), time)?);
            billboard.set_scale_by_distance(eval_near_far_scalar_opt(graphics.scale_by_distance().as_ref(), time)?)?;
            billboard.set_translucency_by_distance(eval_near_far_scalar_opt(
                graphics.translucency_by_distance().as_ref(),
                time,
            )?)?;
            billboard.set_pixel_offset_scale_by_distance(eval_near_far_scalar_opt(
                graphics.pixel_offset_scale_by_distance().as_ref(),
                time,
            )?)?;
            billboard.set_size_in_meters(eval_bool(graphics.size_in_meters().as_ref(), time, DEFAULT_SIZE_IN_METERS)?);
            billboard.set_distance_display_condition(eval_distance_display_condition_opt(
                graphics.distance_display_condition().as_ref(),
                time,
            )?)?;
            billboard.set_disable_depth_test_distance(eval_f64_opt(
                graphics.disable_depth_test_distance().as_ref(),
                time,
            )?)?;
            billboard.set_split_direction(eval_split_direction(
                graphics.split_direction().as_ref(),
                time,
                DEFAULT_SPLIT_DIRECTION,
            )?);

            let tex = texture_value.unwrap();
            let same_texture = prev_texture.as_ref().is_some_and(|p| p.property_equals(&tex));
            if !same_texture {
                if let Some((tex_id, image)) = (self.image_resolver)(&tex) {
                    billboard.set_image_with_id(tex_id, image);
                }
                if let Some(item) = self.items.borrow_mut().map.get_mut(&id) {
                    item.texture_value = Some(tex);
                }
            }

            if let Some(sub_region) = eval_bounding_rectangle_opt(graphics.image_sub_region().as_ref(), time)? {
                if let Some(img_id) = billboard.image() {
                    billboard.set_image_sub_region(img_id, sub_region)?;
                }
            }
        }
        Ok(true)
    }

    fn get_bounding_sphere(&self, entity: &EntityRef, result: &mut BoundingSphere) -> BoundingSphereState {
        let items = self.items.borrow();
        let Some(billboard) = items.map.get(entity.id()).and_then(|d| d.billboard.as_ref()) else {
            return BoundingSphereState::Failed;
        };
        if billboard.height_reference() == HeightReference::None {
            result.center = billboard.position();
        } else {
            let Some(clamped) = billboard.clamped_position() else {
                return BoundingSphereState::Pending;
            };
            result.center = clamped;
        }
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
                self.cluster.remove_billboard(&entity);
            }
            self.items.borrow_mut().map.clear();
            self.items.borrow_mut().order.clear();
        }
    }
}

fn on_collection_changed<G: Gl + 'static>(
    items: &RefCell<Items<G>>,
    cluster: &EntityCluster<G>,
    added: &[EntityRef],
    removed: &[EntityRef],
    changed: &[EntityRef],
) {
    let mut items = items.borrow_mut();
    for entity in added.iter().rev() {
        if entity.get_property("billboard").is_some() && entity.get_property("position").is_some() {
            items.insert(
                entity.id().to_owned(),
                EntityData { entity: Rc::clone(entity), billboard: None, texture_value: None },
            );
        }
    }
    for entity in changed.iter().rev() {
        let id = entity.id().to_owned();
        if entity.get_property("billboard").is_some() && entity.get_property("position").is_some() {
            if !items.contains(&id) {
                items.insert(id, EntityData { entity: Rc::clone(entity), billboard: None, texture_value: None });
            }
        } else {
            if let Some(mut data) = items.remove(&id) {
                return_primitive(&mut data, entity, cluster);
            }
        }
    }
    for entity in removed.iter().rev() {
        if let Some(mut data) = items.remove(&entity.id()) {
            return_primitive(&mut data, entity, cluster);
        }
    }
}

fn return_primitive<G: Gl + 'static>(item: &mut EntityData<G>, entity: &EntityRef, cluster: &EntityCluster<G>) {
    item.billboard = None;
    cluster.remove_billboard(entity);
}

fn default_image_resolver<G: Gl + 'static>(value: &DynamicValue) -> Option<(String, BillboardImage<G>)> {
    let id = match value {
        DynamicValue::String(s) => s.clone(),
        DynamicValue::Resource(r) => r.url(),
        DynamicValue::Pin(p) => p.to_json().to_string(),
        _ => value.property_string(),
    };
    Some((id, BillboardImage::pixels(1, 1, vec![255, 255, 255, 255])))
}
