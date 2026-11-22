//! CesiumJS `ModelVisualizer`: maps `Entity.model` (`ModelGraphics`) to [`Model`] primitives.
use super::{
    visualizer_values::*, BoundingSphereState, CollectionChange, DynamicValue, EntityCollection, EntityRef,
    ModelGraphics, PropertyInput, Subscription, Visualizer,
};
use crate::{
    core::{
        BoundingSphere, Cartesian2, Cartesian3, Cartographic, Color, Ellipsoid, JulianDate, Matrix4, Resource,
        RuntimeError,
    },
    renderer::Gl,
    scene::{
        height_reference::is_height_reference_clamp,
        model::{Model, ModelAnimationOptions},
        ColorBlendMode, HeightReference, ModelAnimationLoop, ShadowMode,
    },
};
use futures_util::{future::LocalBoxFuture, FutureExt};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const DEFAULT_SCALE: f64 = 1.0;
const DEFAULT_ENABLE_VERTICAL_EXAGGERATION: bool = true;
const DEFAULT_MINIMUM_PIXEL_SIZE: f64 = 0.0;
const DEFAULT_INCREMENTALLY_LOAD_TEXTURES: bool = true;
const DEFAULT_CLAMP_ANIMATIONS: bool = true;
const DEFAULT_SHADOWS: ShadowMode = ShadowMode::Enabled;
const DEFAULT_HEIGHT_REFERENCE: HeightReference = HeightReference::None;
const DEFAULT_SILHOUETTE_COLOR: Color = Color::RED;
const DEFAULT_SILHOUETTE_SIZE: f64 = 0.0;
const DEFAULT_COLOR: Color = Color::WHITE;
const DEFAULT_COLOR_BLEND_MODE: ColorBlendMode = ColorBlendMode::Highlight;
const DEFAULT_COLOR_BLEND_AMOUNT: f64 = 0.5;
const DEFAULT_IMAGE_BASED_LIGHTING_FACTOR: Cartesian2 = Cartesian2::new(1.0, 1.0);

/// Parameters passed to a host [`ModelFactory`] when loading a model for an entity.
#[derive(Clone)]
pub struct ModelLoadRequest {
    pub entity_id: String,
    pub resource: Resource,
    pub incrementally_load_textures: bool,
    pub environment_map_options: Option<DynamicValue>,
}

/// Async factory creating a [`Model`] primitive for a given [`ModelLoadRequest`].
pub type ModelFactory<G> =
    Rc<dyn Fn(ModelLoadRequest) -> LocalBoxFuture<'static, Result<Rc<RefCell<Model<G>>>, RuntimeError>>>;

/// Optional scene height query callback used by `ModelVisualizer::get_bounding_sphere`
/// when `heightReference != HeightReference::None`.
pub type ModelHeightQuery = Rc<dyn Fn(&Cartographic, HeightReference) -> Option<f64>>;

struct ModelData<G: Gl + 'static> {
    url: String,
    pending: Option<LocalBoxFuture<'static, Result<Rc<RefCell<Model<G>>>, RuntimeError>>>,
    model_primitive: Option<Rc<RefCell<Model<G>>>>,
    animations_running: bool,
    load_failed: bool,
    model_updated: bool,
}

struct ModelRegistry<G: Gl + 'static> {
    entities_order: Vec<String>,
    entities_map: HashMap<String, EntityRef>,
    models: HashMap<String, ModelData<G>>,
    primitives: Vec<Rc<RefCell<Model<G>>>>,
}

impl<G: Gl + 'static> Default for ModelRegistry<G> {
    fn default() -> Self {
        Self {
            entities_order: Vec::new(),
            entities_map: HashMap::new(),
            models: HashMap::new(),
            primitives: Vec::new(),
        }
    }
}

impl<G: Gl + 'static> ModelRegistry<G> {
    fn insert_entity(&mut self, entity: EntityRef) {
        let id = entity.id().to_owned();
        if !self.entities_map.contains_key(&id) {
            self.entities_order.push(id.clone());
        }
        self.entities_map.insert(id, entity);
    }

    fn remove_entity(&mut self, id: &str) {
        self.remove_model(id);
        if self.entities_map.remove(id).is_some() {
            self.entities_order.retain(|k| k != id);
        }
    }

    fn remove_model(&mut self, id: &str) {
        if let Some(mut data) = self.models.remove(id) {
            data.pending = None;
            if let Some(model) = data.model_primitive.take() {
                self.primitives.retain(|p| !Rc::ptr_eq(p, &model));
                let _ = model.borrow_mut().destroy();
            }
        }
    }
}

/// Port of CesiumJS `ModelVisualizer`.
pub struct ModelVisualizer<G: Gl + 'static> {
    ellipsoid: Ellipsoid,
    entity_collection: Rc<EntityCollection>,
    factory: ModelFactory<G>,
    height_query: Option<ModelHeightQuery>,
    registry: Rc<RefCell<ModelRegistry<G>>>,
    subscription: Option<Subscription>,
    destroyed: bool,
}

impl<G: Gl + 'static> ModelVisualizer<G> {
    pub fn new(
        entity_collection: Rc<EntityCollection>,
        factory: ModelFactory<G>,
        ellipsoid: Option<Ellipsoid>,
    ) -> Self {
        let registry = Rc::new(RefCell::new(ModelRegistry::default()));
        let reg_cb = Rc::clone(&registry);
        let sub = entity_collection.collection_changed().add_listener(move |change: &CollectionChange| {
            on_collection_changed(&reg_cb, &change.added, &change.removed, &change.changed);
        });

        let initial = entity_collection.values();
        on_collection_changed(&registry, &initial, &[], &[]);

        Self {
            ellipsoid: ellipsoid.unwrap_or(Ellipsoid::WGS84),
            entity_collection,
            factory,
            height_query: None,
            registry,
            subscription: Some(sub),
            destroyed: false,
        }
    }

    pub fn set_height_query(&mut self, query: Option<ModelHeightQuery>) {
        self.height_query = query;
    }

    /// Returns the loaded [`Model`] primitive for `entity_id`, if ready.
    pub fn model_for(&self, entity_id: &str) -> Option<Rc<RefCell<Model<G>>>> {
        self.registry.borrow().models.get(entity_id).and_then(|d| d.model_primitive.clone())
    }

    /// Returns all active primitives currently managed by this visualizer.
    pub fn primitives(&self) -> Vec<Rc<RefCell<Model<G>>>> {
        self.registry.borrow().primitives.clone()
    }

    /// Non-blockingly polls any pending async model loads.
    pub fn poll_loads(&mut self) {
        let mut reg = self.registry.borrow_mut();
        let keys: Vec<String> = reg.models.keys().cloned().collect();
        for key in keys {
            let resolved = {
                let Some(data) = reg.models.get_mut(&key) else { continue };
                let Some(fut) = data.pending.as_mut() else { continue };
                fut.now_or_never()
            };
            let Some(result) = resolved else { continue };
            match result {
                Ok(model) => {
                    if let Some(data) = reg.models.get_mut(&key) {
                        data.pending = None;
                        model.borrow_mut().options.id = Some(key.clone());
                        data.model_primitive = Some(Rc::clone(&model));
                        reg.primitives.push(model);
                    }
                }
                Err(_) => {
                    if let Some(data) = reg.models.get_mut(&key) {
                        data.pending = None;
                        data.load_failed = true;
                    }
                }
            }
        }
    }

    /// Marks an entity's model as having encountered a fatal load error.
    pub fn mark_load_failed(&mut self, entity_id: &str) {
        if let Some(data) = self.registry.borrow_mut().models.get_mut(entity_id) {
            data.load_failed = true;
        }
    }
}

impl<G: Gl + 'static> Visualizer for ModelVisualizer<G> {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        self.poll_loads();

        let ids: Vec<String> = self.registry.borrow().entities_order.clone();
        for id in ids {
            let entity = match self.registry.borrow().entities_map.get(&id) {
                Some(e) => Rc::clone(e),
                None => continue,
            };
            let Some(PropertyInput::Component(comp)) = entity.get_property("model") else { continue };
            let Some(graphics) = comp.as_any().downcast_ref::<ModelGraphics>() else { continue };

            let mut show =
                entity.is_showing() && entity.is_available(time) && eval_bool(graphics.show().as_ref(), time, true)?;

            let mut model_matrix = None;
            let mut resource = None;
            if show {
                model_matrix = entity.compute_model_matrix(time, None)?;
                resource = eval_resource_opt(graphics.uri().as_ref(), time)?;
                show = model_matrix.is_some() && resource.is_some();
            }

            if !show {
                if let Some(model) = self.registry.borrow().models.get(&id).and_then(|d| d.model_primitive.clone()) {
                    model.borrow_mut().options.show = false;
                }
                continue;
            }

            let resource = resource.unwrap();
            let model_matrix = model_matrix.unwrap();
            let url = resource.url();

            let needs_create = {
                let reg = self.registry.borrow();
                reg.models.get(&id).is_none_or(|d| d.url != url)
            };

            if needs_create {
                self.registry.borrow_mut().remove_model(&id);
                let incrementally_load_textures = eval_bool(
                    graphics.incrementally_load_textures().as_ref(),
                    time,
                    DEFAULT_INCREMENTALLY_LOAD_TEXTURES,
                )?;
                let environment_map_options = eval_dynamic(graphics.environment_map_options().as_ref(), time)?;
                let req = ModelLoadRequest {
                    entity_id: id.clone(),
                    resource,
                    incrementally_load_textures,
                    environment_map_options,
                };
                let fut = (self.factory)(req);
                self.registry.borrow_mut().models.insert(
                    id.clone(),
                    ModelData {
                        url,
                        pending: Some(fut),
                        model_primitive: None,
                        animations_running: false,
                        load_failed: false,
                        model_updated: false,
                    },
                );
                self.poll_loads();
            }

            let model = self.registry.borrow().models.get(&id).and_then(|d| d.model_primitive.clone());
            let Some(model) = model else { continue };

            let scale = eval_f64(graphics.scale().as_ref(), time, DEFAULT_SCALE)?;
            let enable_vertical_exaggeration = eval_bool(
                graphics.enable_vertical_exaggeration().as_ref(),
                time,
                DEFAULT_ENABLE_VERTICAL_EXAGGERATION,
            )?;
            let minimum_pixel_size =
                eval_f64(graphics.minimum_pixel_size().as_ref(), time, DEFAULT_MINIMUM_PIXEL_SIZE)?;
            let maximum_scale = eval_f64_opt(graphics.maximum_scale().as_ref(), time)?;
            let shadows = eval_shadow_mode(graphics.shadows().as_ref(), time, DEFAULT_SHADOWS)?;
            let height_reference =
                eval_height_reference(graphics.height_reference().as_ref(), time, DEFAULT_HEIGHT_REFERENCE)?;
            let distance_display_condition =
                eval_distance_display_condition_opt(graphics.distance_display_condition().as_ref(), time)?;
            let silhouette_color = eval_color(graphics.silhouette_color().as_ref(), time, DEFAULT_SILHOUETTE_COLOR)?;
            let silhouette_size = eval_f64(graphics.silhouette_size().as_ref(), time, DEFAULT_SILHOUETTE_SIZE)?;
            let color = eval_color(graphics.color().as_ref(), time, DEFAULT_COLOR)?;
            let color_blend_mode =
                eval_color_blend_mode(graphics.color_blend_mode().as_ref(), time, DEFAULT_COLOR_BLEND_MODE)?;
            let color_blend_amount =
                eval_f64(graphics.color_blend_amount().as_ref(), time, DEFAULT_COLOR_BLEND_AMOUNT)?;
            let clamp_animations = eval_bool(graphics.clamp_animations().as_ref(), time, DEFAULT_CLAMP_ANIMATIONS)?;
            let ibl_factor = eval_cartesian2(
                graphics.image_based_lighting_factor().as_ref(),
                time,
                DEFAULT_IMAGE_BASED_LIGHTING_FACTOR,
            )?;
            let light_color =
                eval_color_opt(graphics.light_color().as_ref(), time)?.map(|c| Cartesian3::new(c.red, c.green, c.blue));

            let is_ready = {
                let mut m = model.borrow_mut();
                m.options.show = true;
                m.options.scale = scale;
                m.options.enable_vertical_exaggeration = enable_vertical_exaggeration;
                m.options.minimum_pixel_size = minimum_pixel_size;
                m.options.maximum_scale = maximum_scale;
                m.options.model_matrix = model_matrix;
                m.options.shadows = shadows;
                m.options.height_reference = height_reference;
                m.options.distance_display_condition = distance_display_condition;
                m.options.silhouette_color = silhouette_color;
                m.options.silhouette_size = silhouette_size;
                m.options.color = Some(color);
                m.options.color_blend_mode = color_blend_mode;
                m.options.color_blend_amount = color_blend_amount;
                m.options.clamp_animations = clamp_animations;
                m.image_based_lighting.image_based_lighting_factor = ibl_factor;
                m.set_light_color(light_color);
                m.ready()
            };

            if let Some(data) = self.registry.borrow_mut().models.get_mut(&id) {
                data.model_updated = true;
            }

            if is_ready {
                let run_animations = eval_bool(graphics.run_animations().as_ref(), time, true)?;
                let prev_running = self.registry.borrow().models.get(&id).map_or(false, |d| d.animations_running);
                if prev_running != run_animations {
                    let m = model.borrow();
                    if run_animations {
                        if let Some(sg) = m.scene_graph.as_ref() {
                            let mut opts = ModelAnimationOptions::default();
                            opts.loop_mode = Some(ModelAnimationLoop::Repeat);
                            let _ = m.active_animations.borrow_mut().add_all(
                                &sg.components().animations,
                                sg.runtime_nodes(),
                                &opts,
                            );
                        }
                    } else {
                        m.active_animations.borrow_mut().remove_all();
                    }
                    if let Some(data) = self.registry.borrow_mut().models.get_mut(&id) {
                        data.animations_running = run_animations;
                    }
                }

                // Node transformations
                if let Some(DynamicValue::Record(entries)) =
                    eval_dynamic(graphics.node_transformations().as_ref(), time)?
                {
                    let mut m = model.borrow_mut();
                    for (node_name, val) in entries {
                        let Some(val) = val else { continue };
                        let Some(trs) = dynamic_to_trs(&val) else { continue };
                        if let Some(sg) = m.scene_graph.as_mut() {
                            if let Some(node) = sg.get_node(&node_name).cloned() {
                                if let Some(original) = node.original_matrix(sg).copied() {
                                    let t = Matrix4::from_translation_rotation_scale(&trs);
                                    let combined = original.multiply(&t);
                                    node.set_matrix(sg, Some(&combined));
                                }
                            }
                        }
                    }
                }

                // Articulations
                if let Some(DynamicValue::Record(entries)) = eval_dynamic(graphics.articulations().as_ref(), time)? {
                    let mut m = model.borrow_mut();
                    let mut any_updated = false;
                    for (key, val) in entries {
                        let Some(DynamicValue::Number(v)) = val else { continue };
                        any_updated = true;
                        let _ = m.set_articulation_stage(&key, v);
                    }
                    if any_updated {
                        let _ = m.apply_articulations();
                    }
                }
            }
        }
        Ok(true)
    }

    fn get_bounding_sphere(&self, entity: &EntityRef, result: &mut BoundingSphere) -> BoundingSphereState {
        let reg = self.registry.borrow();
        let Some(data) = reg.models.get(entity.id()) else {
            return BoundingSphereState::Failed;
        };
        if data.load_failed {
            return BoundingSphereState::Failed;
        }
        let Some(model) = data.model_primitive.as_ref() else {
            return BoundingSphereState::Pending;
        };
        let m = model.borrow();
        if !m.options.show || !m.ready() || !data.model_updated {
            return BoundingSphereState::Pending;
        }
        let Ok(sphere) = m.bounding_sphere() else {
            return BoundingSphereState::Pending;
        };
        let hr = m.options.height_reference;
        if hr != HeightReference::None {
            let mm = m.options.model_matrix;
            let pos = Cartesian3::new(mm.0[12], mm.0[13], mm.0[14]);
            if let Some(mut carto) = self.ellipsoid.cartesian_to_cartographic(pos) {
                if let Some(height) = self.height_query.as_ref().and_then(|q| q(&carto, hr)) {
                    if is_height_reference_clamp(hr) {
                        carto.height = height;
                    } else {
                        carto.height += height;
                    }
                }
                *result = sphere;
                result.center = self.ellipsoid.cartographic_to_cartesian(&carto);
                return BoundingSphereState::Done;
            }
        }
        *result = sphere;
        BoundingSphereState::Done
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            self.destroyed = true;
            self.subscription = None;
            let mut reg = self.registry.borrow_mut();
            let ids: Vec<String> = reg.entities_order.clone();
            for id in ids.into_iter().rev() {
                reg.remove_model(&id);
            }
            reg.entities_order.clear();
            reg.entities_map.clear();
            let _ = &self.entity_collection;
        }
    }
}

fn on_collection_changed<G: Gl + 'static>(
    registry: &RefCell<ModelRegistry<G>>,
    added: &[EntityRef],
    removed: &[EntityRef],
    changed: &[EntityRef],
) {
    let mut reg = registry.borrow_mut();
    for entity in added.iter().rev() {
        if entity.get_property("model").is_some() && entity.get_property("position").is_some() {
            reg.insert_entity(Rc::clone(entity));
        }
    }
    for entity in changed.iter().rev() {
        if entity.get_property("model").is_some() && entity.get_property("position").is_some() {
            reg.insert_entity(Rc::clone(entity));
        } else {
            reg.remove_entity(&entity.id());
        }
    }
    for entity in removed.iter().rev() {
        reg.remove_entity(&entity.id());
    }
}
