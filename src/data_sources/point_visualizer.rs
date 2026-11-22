//! CesiumJS `PointVisualizer`: maps `Entity.point` (`PointGraphics`) to
//! [`PointPrimitive`] (unclamped) or a rasterized circle [`Billboard`] (height-referenced).
use super::{
    visualizer_values::*, BoundingSphereState, CollectionChange, EntityCluster, EntityCollection, EntityRef,
    PointGraphics, PropertyInput, Subscription, Visualizer,
};
use crate::{
    core::{BoundingSphere, Color, JulianDate, RuntimeError},
    renderer::Gl,
    scene::{
        billboard::{Billboard, BillboardImage},
        point_primitive::PointPrimitive,
        HeightReference, SplitDirection,
    },
};
use serde_json::json;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const DEFAULT_COLOR: Color = Color::WHITE;
const DEFAULT_OUTLINE_COLOR: Color = Color::BLACK;
const DEFAULT_OUTLINE_WIDTH: f64 = 0.0;
const DEFAULT_PIXEL_SIZE: f64 = 1.0;
const DEFAULT_DISABLE_DEPTH_TEST_DISTANCE: f64 = 0.0;
const DEFAULT_SPLIT_DIRECTION: SplitDirection = SplitDirection::None;

struct EntityData<G: Gl + 'static> {
    entity: EntityRef,
    point_primitive: Option<PointPrimitive>,
    billboard: Option<Billboard<G>>,
    color: Option<Color>,
    outline_color: Option<Color>,
    pixel_size: Option<f64>,
    outline_width: Option<f64>,
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

/// Port of CesiumJS `PointVisualizer`.
pub struct PointVisualizer<G: Gl + 'static> {
    cluster: Rc<EntityCluster<G>>,
    entity_collection: Rc<EntityCollection>,
    items: Rc<RefCell<Items<G>>>,
    subscription: Option<Subscription>,
    destroyed: bool,
}

impl<G: Gl + 'static> PointVisualizer<G> {
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

    pub fn point_for(&self, entity_id: &str) -> Option<PointPrimitive> {
        self.items.borrow().map.get(entity_id).and_then(|d| d.point_primitive.clone())
    }

    pub fn billboard_for(&self, entity_id: &str) -> Option<Billboard<G>> {
        self.items.borrow().map.get(entity_id).and_then(|d| d.billboard.clone())
    }
}

impl<G: Gl + 'static> Visualizer for PointVisualizer<G> {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let ids: Vec<String> = self.items.borrow().order.clone();
        for id in ids {
            let entity = match self.items.borrow().map.get(&id) {
                Some(item) => Rc::clone(&item.entity),
                None => continue,
            };
            let Some(PropertyInput::Component(comp)) = entity.get_property("point") else { continue };
            let Some(graphics) = comp.as_any().downcast_ref::<PointGraphics>() else { continue };

            let height_reference =
                eval_height_reference(graphics.height_reference().as_ref(), time, HeightReference::None)?;
            let mut show =
                entity.is_showing() && entity.is_available(time) && eval_bool(graphics.show().as_ref(), time, true)?;
            let mut position = None;
            if show {
                position = entity.position_at(time)?;
                show = position.is_some();
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

            let pos = position.unwrap();
            let mut needs_redraw = false;

            let (point_primitive, billboard) = {
                let mut items = self.items.borrow_mut();
                let item = items.map.get_mut(&id).unwrap();
                if height_reference != HeightReference::None && item.billboard.is_none() {
                    if item.point_primitive.is_some() {
                        return_primitive(item, &entity, &self.cluster);
                    }
                    let b = self.cluster.get_billboard(&entity)?;
                    b.set_id(Some(entity.id().to_owned()))?;
                    b.set_image(None);
                    item.billboard = Some(b);
                    needs_redraw = true;
                } else if height_reference == HeightReference::None && item.point_primitive.is_none() {
                    if item.billboard.is_some() {
                        return_primitive(item, &entity, &self.cluster);
                    }
                    let p = self.cluster.get_point(&entity)?;
                    p.set_id(Some(entity.id().to_owned()))?;
                    item.point_primitive = Some(p);
                }
                (item.point_primitive.clone(), item.billboard.clone())
            };

            if let Some(point) = point_primitive {
                point.set_show(true);
                point.set_position(pos);
                point.set_scale_by_distance(eval_near_far_scalar_opt(graphics.scale_by_distance().as_ref(), time)?)?;
                point.set_translucency_by_distance(eval_near_far_scalar_opt(
                    graphics.translucency_by_distance().as_ref(),
                    time,
                )?)?;
                point.set_color(eval_color(graphics.color().as_ref(), time, DEFAULT_COLOR)?);
                point.set_outline_color(eval_color(graphics.outline_color().as_ref(), time, DEFAULT_OUTLINE_COLOR)?);
                point.set_outline_width(eval_f64(graphics.outline_width().as_ref(), time, DEFAULT_OUTLINE_WIDTH)?);
                point.set_pixel_size(eval_f64(graphics.pixel_size().as_ref(), time, DEFAULT_PIXEL_SIZE)?);
                point.set_distance_display_condition(eval_distance_display_condition_opt(
                    graphics.distance_display_condition().as_ref(),
                    time,
                )?)?;
                point.set_disable_depth_test_distance(eval_f64(
                    graphics.disable_depth_test_distance().as_ref(),
                    time,
                    DEFAULT_DISABLE_DEPTH_TEST_DISTANCE,
                )?)?;
                point.set_split_direction(eval_split_direction(
                    graphics.split_direction().as_ref(),
                    time,
                    DEFAULT_SPLIT_DIRECTION,
                )?);
            } else if let Some(billboard) = billboard {
                billboard.set_show(true);
                billboard.set_position(pos);
                billboard
                    .set_scale_by_distance(eval_near_far_scalar_opt(graphics.scale_by_distance().as_ref(), time)?)?;
                billboard.set_translucency_by_distance(eval_near_far_scalar_opt(
                    graphics.translucency_by_distance().as_ref(),
                    time,
                )?)?;
                billboard.set_distance_display_condition(eval_distance_display_condition_opt(
                    graphics.distance_display_condition().as_ref(),
                    time,
                )?)?;
                billboard.set_disable_depth_test_distance(Some(eval_f64(
                    graphics.disable_depth_test_distance().as_ref(),
                    time,
                    DEFAULT_DISABLE_DEPTH_TEST_DISTANCE,
                )?))?;
                billboard.set_split_direction(eval_split_direction(
                    graphics.split_direction().as_ref(),
                    time,
                    DEFAULT_SPLIT_DIRECTION,
                )?);
                billboard.set_height_reference(height_reference)?;

                let new_color = eval_color(graphics.color().as_ref(), time, DEFAULT_COLOR)?;
                let new_outline_color = eval_color(graphics.outline_color().as_ref(), time, DEFAULT_OUTLINE_COLOR)?;
                let new_outline_width =
                    js_round(eval_f64(graphics.outline_width().as_ref(), time, DEFAULT_OUTLINE_WIDTH)?);
                let mut new_pixel_size =
                    js_round(eval_f64(graphics.pixel_size().as_ref(), time, DEFAULT_PIXEL_SIZE)?).max(1.0);

                let mut items = self.items.borrow_mut();
                let item = items.map.get_mut(&id).unwrap();

                if new_outline_width > 0.0 {
                    billboard.set_scale(1.0);
                    needs_redraw = needs_redraw
                        || item.outline_width != Some(new_outline_width)
                        || item.pixel_size != Some(new_pixel_size)
                        || item.color != Some(new_color)
                        || item.outline_color != Some(new_outline_color);
                } else {
                    billboard.set_scale(new_pixel_size / 50.0);
                    new_pixel_size = 50.0;
                    needs_redraw = needs_redraw
                        || item.outline_width != Some(new_outline_width)
                        || item.color != Some(new_color)
                        || item.outline_color != Some(new_outline_color);
                }

                if needs_redraw {
                    item.color = Some(new_color);
                    item.outline_color = Some(new_outline_color);
                    item.pixel_size = Some(new_pixel_size);
                    item.outline_width = Some(new_outline_width);

                    let css_color = new_color.to_css_color_string();
                    let css_outline_color = new_outline_color.to_css_color_string();
                    let texture_id =
                        json!([css_color, new_pixel_size as i64, css_outline_color, new_outline_width as i64])
                            .to_string();
                    let image =
                        rasterize_billboard_point(new_color, new_outline_color, new_outline_width, new_pixel_size);
                    billboard.set_image_with_id(texture_id, image);
                }
            }
        }
        Ok(true)
    }

    fn get_bounding_sphere(&self, entity: &EntityRef, result: &mut BoundingSphere) -> BoundingSphereState {
        let items = self.items.borrow();
        let Some(item) = items.map.get(entity.id()) else {
            return BoundingSphereState::Failed;
        };
        if let Some(point) = item.point_primitive.as_ref() {
            result.center = point.position();
        } else if let Some(billboard) = item.billboard.as_ref() {
            let Some(clamped) = billboard.clamped_position() else {
                return BoundingSphereState::Pending;
            };
            result.center = clamped;
        } else {
            return BoundingSphereState::Failed;
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
                self.cluster.remove_point(&entity);
                self.cluster.remove_billboard(&entity);
            }
            self.items.borrow_mut().map.clear();
            self.items.borrow_mut().order.clear();
        }
    }
}

fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Pure-Rust equivalent of CesiumJS `createBillboardPointCallback` rasterizing an RGBA circle + outline.
fn rasterize_billboard_point<G: Gl + 'static>(
    color: Color,
    outline_color: Color,
    outline_width: f64,
    pixel_size: f64,
) -> BillboardImage<G> {
    let length = (pixel_size + 2.0 * outline_width).max(1.0).ceil() as u32;
    let mut bytes = vec![0u8; (length * length * 4) as usize];
    let center = length as f64 * 0.5;
    let outer_r = center;
    let inner_r = pixel_size * 0.5;
    let fill = [
        (color.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ];
    let outline = [
        (outline_color.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (outline_color.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (outline_color.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        (outline_color.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ];
    for y in 0..length {
        for x in 0..length {
            let dx = x as f64 + 0.5 - center;
            let dy = y as f64 + 0.5 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let px = if dist <= inner_r {
                fill
            } else if outline_width > 0.0 && dist <= outer_r {
                outline
            } else {
                [0, 0, 0, 0]
            };
            let idx = ((y * length + x) * 4) as usize;
            bytes[idx..idx + 4].copy_from_slice(&px);
        }
    }
    BillboardImage::pixels(length, length, bytes)
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
        if entity.get_property("point").is_some() && entity.get_property("position").is_some() {
            items.insert(
                entity.id().to_owned(),
                EntityData {
                    entity: Rc::clone(entity),
                    point_primitive: None,
                    billboard: None,
                    color: None,
                    outline_color: None,
                    pixel_size: None,
                    outline_width: None,
                },
            );
        }
    }
    for entity in changed.iter().rev() {
        let id = entity.id().to_owned();
        if entity.get_property("point").is_some() && entity.get_property("position").is_some() {
            if !items.contains(&id) {
                items.insert(
                    id,
                    EntityData {
                        entity: Rc::clone(entity),
                        point_primitive: None,
                        billboard: None,
                        color: None,
                        outline_color: None,
                        pixel_size: None,
                        outline_width: None,
                    },
                );
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

fn return_primitive<G: Gl + 'static>(item: &mut EntityData<G>, entity: &EntityRef, cluster: &EntityCluster<G>) {
    if item.point_primitive.take().is_some() {
        cluster.remove_point(entity);
        return;
    }
    if item.billboard.take().is_some() {
        cluster.remove_billboard(entity);
    }
}
