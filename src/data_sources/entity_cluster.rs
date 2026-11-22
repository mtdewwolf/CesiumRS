//! CesiumJS `EntityCluster`: spatial screen-space clustering for billboards, labels and points.
use super::{ChangeEvent, EntityRef};
use crate::{
    core::{BoundingRectangle, Cartesian2, Cartesian3, Ellipsoid, EllipsoidalOccluder, Matrix4, RuntimeError},
    renderer::{Context, Gl},
    scene::{
        billboard::Billboard,
        billboard_collection::{BillboardCollection, BillboardCollectionOptions},
        label::LabelOptions,
        label_collection::LabelCollection,
        point_primitive::{PointPrimitive, PointPrimitiveOptions},
        point_primitive_collection::{PointPrimitiveCollection, PointPrimitiveCollectionOptions},
        Camera, FrameState, HeightSampler, LabelHandle, SceneMode, ScenePrimitive, SceneTransformState,
        SceneTransforms,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
};

/// Constructor options for [`EntityCluster`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityClusterOptions {
    pub enabled: bool,
    pub pixel_range: f64,
    pub minimum_cluster_size: usize,
    pub cluster_billboards: bool,
    pub cluster_labels: bool,
    pub cluster_points: bool,
    pub show: bool,
}

impl Default for EntityClusterOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            pixel_range: 80.0,
            minimum_cluster_size: 2,
            cluster_billboards: true,
            cluster_labels: true,
            cluster_points: true,
            show: true,
        }
    }
}

/// The primitives representing one generated cluster, passed to [`EntityCluster::cluster_event`].
#[derive(Clone)]
pub struct ClusterPrimitives<G: Gl + 'static> {
    pub billboard: Billboard<G>,
    pub label: LabelHandle,
    pub point: PointPrimitive,
}

/// Payload raised by [`EntityCluster::cluster_event`] for each cluster.
#[derive(Clone)]
pub struct ClusterEventPayload<G: Gl + 'static> {
    pub entities: Vec<EntityRef>,
    pub cluster: ClusterPrimitives<G>,
}

#[derive(Default, Clone, Copy)]
struct EntityIndices {
    billboard_index: Option<usize>,
    label_index: Option<usize>,
    point_index: Option<usize>,
}

#[derive(Clone)]
struct PreviousCluster {
    position: Cartesian3,
    width: f64,
    height: f64,
    minimum_width: f64,
    minimum_height: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Label,
    Billboard,
    Point,
}

struct ScreenPoint {
    index: usize,
    kind: SourceKind,
    entity: Option<EntityRef>,
    clustered: bool,
    coord: Cartesian2,
}

struct ClusterState<G: Gl + 'static> {
    label_collection: Option<LabelCollection>,
    billboard_collection: Option<BillboardCollection<G>>,
    point_collection: Option<PointPrimitiveCollection<G>>,
    cluster_label_collection: Option<LabelCollection>,
    cluster_billboard_collection: Option<BillboardCollection<G>>,
    cluster_point_collection: Option<PointPrimitiveCollection<G>>,
    collection_indices_by_entity: HashMap<String, EntityIndices>,
    entities_by_id: HashMap<String, EntityRef>,
    unused_label_indices: VecDeque<usize>,
    unused_billboard_indices: VecDeque<usize>,
    unused_point_indices: VecDeque<usize>,
    previous_clusters: Vec<PreviousCluster>,
    previous_height: Option<f64>,
    height_sampler: Option<HeightSampler>,
}

impl<G: Gl + 'static> Default for ClusterState<G> {
    fn default() -> Self {
        Self {
            label_collection: None,
            billboard_collection: None,
            point_collection: None,
            cluster_label_collection: None,
            cluster_billboard_collection: None,
            cluster_point_collection: None,
            collection_indices_by_entity: HashMap::new(),
            entities_by_id: HashMap::new(),
            unused_label_indices: VecDeque::new(),
            unused_billboard_indices: VecDeque::new(),
            unused_point_indices: VecDeque::new(),
            previous_clusters: Vec::new(),
            previous_height: None,
            height_sampler: None,
        }
    }
}

/// Port of CesiumJS `EntityCluster`.
pub struct EntityCluster<G: Gl + 'static> {
    enabled: Cell<bool>,
    pixel_range: Cell<f64>,
    minimum_cluster_size: Cell<usize>,
    cluster_billboards: Cell<bool>,
    cluster_labels: Cell<bool>,
    cluster_points: Cell<bool>,
    show: Cell<bool>,
    enabled_dirty: Cell<bool>,
    cluster_dirty: Cell<bool>,
    cluster_event: ChangeEvent<ClusterEventPayload<G>>,
    state: RefCell<ClusterState<G>>,
}

impl<G: Gl + 'static> EntityCluster<G> {
    pub fn new(options: EntityClusterOptions) -> Rc<Self> {
        Rc::new(Self {
            enabled: Cell::new(options.enabled),
            pixel_range: Cell::new(options.pixel_range),
            minimum_cluster_size: Cell::new(options.minimum_cluster_size),
            cluster_billboards: Cell::new(options.cluster_billboards),
            cluster_labels: Cell::new(options.cluster_labels),
            cluster_points: Cell::new(options.cluster_points),
            show: Cell::new(options.show),
            enabled_dirty: Cell::new(false),
            cluster_dirty: Cell::new(false),
            cluster_event: ChangeEvent::new(),
            state: RefCell::new(ClusterState::default()),
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }
    pub fn set_enabled(&self, value: bool) {
        self.enabled_dirty.set(value != self.enabled.get());
        self.enabled.set(value);
    }

    pub fn pixel_range(&self) -> f64 {
        self.pixel_range.get()
    }
    pub fn set_pixel_range(&self, value: f64) {
        if value != self.pixel_range.get() {
            self.cluster_dirty.set(true);
            self.pixel_range.set(value);
        }
    }

    pub fn minimum_cluster_size(&self) -> usize {
        self.minimum_cluster_size.get()
    }
    pub fn set_minimum_cluster_size(&self, value: usize) {
        if value != self.minimum_cluster_size.get() {
            self.cluster_dirty.set(true);
            self.minimum_cluster_size.set(value);
        }
    }

    pub fn cluster_billboards(&self) -> bool {
        self.cluster_billboards.get()
    }
    pub fn set_cluster_billboards(&self, value: bool) {
        if value != self.cluster_billboards.get() {
            self.cluster_dirty.set(true);
            self.cluster_billboards.set(value);
        }
    }

    pub fn cluster_labels(&self) -> bool {
        self.cluster_labels.get()
    }
    pub fn set_cluster_labels(&self, value: bool) {
        if value != self.cluster_labels.get() {
            self.cluster_dirty.set(true);
            self.cluster_labels.set(value);
        }
    }

    pub fn cluster_points(&self) -> bool {
        self.cluster_points.get()
    }
    pub fn set_cluster_points(&self, value: bool) {
        if value != self.cluster_points.get() {
            self.cluster_dirty.set(true);
            self.cluster_points.set(value);
        }
    }

    pub fn show(&self) -> bool {
        self.show.get()
    }
    pub fn set_show(&self, value: bool) {
        self.show.set(value);
    }

    pub fn cluster_event(&self) -> &ChangeEvent<ClusterEventPayload<G>> {
        &self.cluster_event
    }

    /// Marks clustering dirty (called by visualizers when an entity's position is non-constant).
    pub fn mark_cluster_dirty(&self) {
        self.cluster_dirty.set(true);
    }

    /// Attaches a terrain/3D-tile height sampler to owned billboard and label collections.
    pub fn set_height_sampler(&self, sampler: Option<HeightSampler>) {
        let mut s = self.state.borrow_mut();
        s.height_sampler = sampler.clone();
        if let Some(bc) = s.billboard_collection.as_mut() {
            bc.set_height_sampler(sampler.clone());
        }
        if let Some(lc) = s.label_collection.as_mut() {
            lc.set_height_sampler(sampler);
        }
    }

    /// Returns true when all clustered data has been updated and underlying collections are ready.
    pub fn ready(&self) -> bool {
        let s = self.state.borrow();
        !self.enabled_dirty.get()
            && !self.cluster_dirty.get()
            && s.billboard_collection.as_ref().is_none_or(BillboardCollection::ready)
    }

    pub fn billboard_count(&self) -> usize {
        self.state.borrow_mut().billboard_collection.as_mut().map_or(0, BillboardCollection::length)
    }

    pub fn label_count(&self) -> usize {
        self.state.borrow().label_collection.as_ref().map_or(0, LabelCollection::len)
    }

    pub fn point_count(&self) -> usize {
        self.state.borrow_mut().point_collection.as_mut().map_or(0, PointPrimitiveCollection::length)
    }

    pub fn cluster_billboard_count(&self) -> usize {
        self.state.borrow_mut().cluster_billboard_collection.as_mut().map_or(0, BillboardCollection::length)
    }

    pub fn cluster_label_count(&self) -> usize {
        self.state.borrow().cluster_label_collection.as_ref().map_or(0, LabelCollection::len)
    }

    pub fn cluster_point_count(&self) -> usize {
        self.state.borrow_mut().cluster_point_collection.as_mut().map_or(0, PointPrimitiveCollection::length)
    }

    pub fn get_cluster_billboard(&self, index: usize) -> Option<Billboard<G>> {
        self.state.borrow_mut().cluster_billboard_collection.as_mut().and_then(|c| c.get(index))
    }

    pub fn get_cluster_label(&self, index: usize) -> Option<LabelHandle> {
        self.state.borrow().cluster_label_collection.as_ref().and_then(|c| c.get(index))
    }

    pub fn get_cluster_point(&self, index: usize) -> Option<PointPrimitive> {
        self.state.borrow_mut().cluster_point_collection.as_mut().and_then(|c| c.get(index))
    }

    /// Returns or allocates the [`Billboard`] associated with `entity`.
    pub fn get_billboard(&self, entity: &EntityRef) -> Result<Billboard<G>, RuntimeError> {
        let mut s = self.state.borrow_mut();
        let entity_id = entity.id().to_owned();
        s.entities_by_id.insert(entity_id.clone(), Rc::clone(entity));
        let existing = s.collection_indices_by_entity.get(&entity_id).and_then(|idx| idx.billboard_index);
        if let Some(index) = existing {
            if let Some(bc) = s.billboard_collection.as_mut() {
                if let Some(b) = bc.get(index) {
                    return Ok(b);
                }
            }
        }
        let reused = s.unused_billboard_indices.pop_front();
        let sampler = s.height_sampler.clone();
        let bc = s.billboard_collection.get_or_insert_with(|| {
            let mut c = BillboardCollection::new(BillboardCollectionOptions::default());
            c.set_height_sampler(sampler);
            c
        });
        let (index, item) = if let Some(reused) = reused {
            let b = bc.get(reused).ok_or_else(|| RuntimeError::new("Invalid reused billboard index."))?;
            (reused, b)
        } else {
            let b = bc.add(Default::default(), None)?;
            (bc.length() - 1, b)
        };
        s.collection_indices_by_entity.entry(entity_id).or_default().billboard_index = Some(index);
        self.cluster_dirty.set(true);
        Ok(item)
    }

    /// Releases the [`Billboard`] associated with `entity` back into the reuse pool.
    pub fn remove_billboard(&self, entity: &EntityRef) {
        let mut s = self.state.borrow_mut();
        let entity_id = entity.id().to_owned();
        let index = match s.collection_indices_by_entity.get_mut(&entity_id) {
            Some(indices) => indices.billboard_index.take(),
            None => None,
        };
        let Some(index) = index else { return };
        remove_entity_indices_if_unused(&mut s, &entity_id);
        if let Some(bc) = s.billboard_collection.as_mut() {
            if let Some(b) = bc.get(index) {
                let _ = b.set_id(None);
                b.set_show(false);
                b.set_image(None);
            }
        }
        s.unused_billboard_indices.push_back(index);
        self.cluster_dirty.set(true);
    }

    /// Returns or allocates the [`LabelHandle`] associated with `entity`.
    pub fn get_label(&self, entity: &EntityRef) -> Result<LabelHandle, RuntimeError> {
        let mut s = self.state.borrow_mut();
        let entity_id = entity.id().to_owned();
        s.entities_by_id.insert(entity_id.clone(), Rc::clone(entity));
        let existing = s.collection_indices_by_entity.get(&entity_id).and_then(|idx| idx.label_index);
        if let Some(index) = existing {
            if let Some(lc) = s.label_collection.as_ref() {
                if let Some(l) = lc.get(index) {
                    return Ok(l);
                }
            }
        }
        let reused = s.unused_label_indices.pop_front();
        let sampler = s.height_sampler.clone();
        let lc = s.label_collection.get_or_insert_with(|| {
            let mut c = LabelCollection::new();
            c.set_height_sampler(sampler);
            c
        });
        let (index, item) = if let Some(reused) = reused {
            let l = lc.get(reused).ok_or_else(|| RuntimeError::new("Invalid reused label index."))?;
            (reused, l)
        } else {
            let l = lc.add(LabelOptions::default()).map_err(RuntimeError::new)?;
            (lc.len() - 1, l)
        };
        s.collection_indices_by_entity.entry(entity_id).or_default().label_index = Some(index);
        self.cluster_dirty.set(true);
        Ok(item)
    }

    /// Releases the [`LabelHandle`] associated with `entity` back into the reuse pool.
    pub fn remove_label(&self, entity: &EntityRef) {
        let mut s = self.state.borrow_mut();
        let entity_id = entity.id().to_owned();
        let index = match s.collection_indices_by_entity.get_mut(&entity_id) {
            Some(indices) => indices.label_index.take(),
            None => None,
        };
        let Some(index) = index else { return };
        remove_entity_indices_if_unused(&mut s, &entity_id);
        if let Some(lc) = s.label_collection.as_ref() {
            if let Some(l) = lc.get(index) {
                let mut l = l.borrow_mut();
                l.set_show(false);
                l.set_text(String::new());
                l.set_id(None);
            }
        }
        s.unused_label_indices.push_back(index);
        self.cluster_dirty.set(true);
    }

    /// Returns or allocates the [`PointPrimitive`] associated with `entity`.
    pub fn get_point(&self, entity: &EntityRef) -> Result<PointPrimitive, RuntimeError> {
        let mut s = self.state.borrow_mut();
        let entity_id = entity.id().to_owned();
        s.entities_by_id.insert(entity_id.clone(), Rc::clone(entity));
        let existing = s.collection_indices_by_entity.get(&entity_id).and_then(|idx| idx.point_index);
        if let Some(index) = existing {
            if let Some(pc) = s.point_collection.as_mut() {
                if let Some(p) = pc.get(index) {
                    return Ok(p);
                }
            }
        }
        let reused = s.unused_point_indices.pop_front();
        let pc = s
            .point_collection
            .get_or_insert_with(|| PointPrimitiveCollection::new(PointPrimitiveCollectionOptions::default()));
        let (index, item) = if let Some(reused) = reused {
            let p = pc.get(reused).ok_or_else(|| RuntimeError::new("Invalid reused point index."))?;
            (reused, p)
        } else {
            let p = pc.add(PointPrimitiveOptions::default())?;
            (pc.length() - 1, p)
        };
        s.collection_indices_by_entity.entry(entity_id).or_default().point_index = Some(index);
        self.cluster_dirty.set(true);
        Ok(item)
    }

    /// Releases the [`PointPrimitive`] associated with `entity` back into the reuse pool.
    pub fn remove_point(&self, entity: &EntityRef) {
        let mut s = self.state.borrow_mut();
        let entity_id = entity.id().to_owned();
        let index = match s.collection_indices_by_entity.get_mut(&entity_id) {
            Some(indices) => indices.point_index.take(),
            None => None,
        };
        let Some(index) = index else { return };
        remove_entity_indices_if_unused(&mut s, &entity_id);
        if let Some(pc) = s.point_collection.as_mut() {
            if let Some(p) = pc.get(index) {
                p.set_show(false);
                let _ = p.set_id(None);
            }
        }
        s.unused_point_indices.push_back(index);
        self.cluster_dirty.set(true);
    }

    /// Runs the screen-space clustering pass (CesiumJS's `createDeclutterCallback`).
    /// Pass `amount = Some(camera_changed_amount)` when triggered from `camera.changed`.
    pub fn run_clustering(
        &self,
        ellipsoid: Ellipsoid,
        camera: &mut Camera,
        transform_state: &SceneTransformState<'_>,
        amount: Option<f64>,
    ) -> Result<(), RuntimeError> {
        if amount.is_some_and(|a| a < 0.05) || !self.enabled.get() {
            return Ok(());
        }

        let cluster_billboards = self.cluster_billboards.get();
        let cluster_labels = self.cluster_labels.get();
        let cluster_points = self.cluster_points.get();
        let pixel_range = self.pixel_range.get();
        let minimum_cluster_size = self.minimum_cluster_size.get();

        let pending_events: Vec<ClusterEventPayload<G>> = {
            let mut s = self.state.borrow_mut();
            if (s.label_collection.is_none() && s.billboard_collection.is_none() && s.point_collection.is_none())
                || (!cluster_billboards && !cluster_labels && !cluster_points)
            {
                return Ok(());
            }

            if let Some(c) = s.cluster_label_collection.as_mut() {
                c.remove_all();
            } else {
                s.cluster_label_collection = Some(LabelCollection::new());
            }
            if let Some(c) = s.cluster_billboard_collection.as_mut() {
                c.remove_all()?;
            } else {
                s.cluster_billboard_collection = Some(BillboardCollection::new(BillboardCollectionOptions::default()));
            }
            if let Some(c) = s.cluster_point_collection.as_mut() {
                c.remove_all()?;
            } else {
                s.cluster_point_collection =
                    Some(PointPrimitiveCollection::new(PointPrimitiveCollectionOptions::default()));
            }

            let previous_clusters = std::mem::take(&mut s.previous_clusters);
            let previous_height = s.previous_height;
            let current_height = camera.position_cartographic().map(|c| c.height).unwrap_or(0.0);
            let occluder = EllipsoidalOccluder::new(&ellipsoid, Some(camera.position_wc()));
            let mut points = Vec::new();

            if cluster_labels {
                collect_label_points(
                    &s,
                    transform_state,
                    camera,
                    &occluder,
                    cluster_billboards,
                    cluster_points,
                    &mut points,
                )?;
            }
            if cluster_billboards {
                collect_billboard_points(&mut s, transform_state, camera, &occluder, &mut points)?;
            }
            if cluster_points {
                collect_point_points(&mut s, transform_state, camera, &occluder, &mut points)?;
            }

            let mut new_clusters = Vec::new();
            let mut events = Vec::new();

            if !points.is_empty() {
                let mut index = KdBush::new(points.len());
                for p in &points {
                    index.add(p.coord.x, p.coord.y);
                }
                index.finish();

                if previous_height.is_some_and(|prev| current_height < prev) {
                    let prev_h = previous_height.unwrap();
                    for mut cluster in previous_clusters {
                        if !occluder.is_point_visible(cluster.position) {
                            continue;
                        }
                        let Some(coord) = SceneTransforms::world_with_eye_offset_to_window_coordinates(
                            transform_state,
                            camera,
                            cluster.position,
                            Cartesian3::ZERO,
                        )?
                        else {
                            continue;
                        };
                        let factor = 1.0 - current_height / prev_h;
                        cluster.width *= factor;
                        cluster.height *= factor;
                        let width = cluster.width.max(cluster.minimum_width);
                        let height = cluster.height.max(cluster.minimum_height);
                        let min_x = coord.x - width * 0.5;
                        let min_y = coord.y - height * 0.5;
                        let max_x = coord.x + width;
                        let max_y = coord.y + height;

                        let neighbors = index.range(min_x, min_y, max_x, max_y);
                        let mut num_points = 0usize;
                        let mut entities = Vec::new();
                        for &ni in &neighbors {
                            if !points[ni].clustered {
                                num_points += 1;
                                if let Some(e) = points[ni].entity.as_ref() {
                                    entities.push(Rc::clone(e));
                                }
                            }
                        }
                        if num_points >= minimum_cluster_size {
                            events.push(add_cluster(&mut s, cluster.position, num_points, entities)?);
                            new_clusters.push(cluster);
                            for &ni in &neighbors {
                                points[ni].clustered = true;
                            }
                        }
                    }
                }

                for i in 0..points.len() {
                    if points[i].clustered {
                        continue;
                    }
                    points[i].clustered = true;

                    let bbox = get_bounding_box(
                        &mut s,
                        &points[i],
                        pixel_range,
                        cluster_billboards,
                        cluster_labels,
                        cluster_points,
                    );
                    let mut total_bbox = bbox;
                    let neighbors = index.range(bbox.x, bbox.y, bbox.x + bbox.width, bbox.y + bbox.height);

                    let mut cluster_pos = get_point_position(&mut s, &points[i]);
                    let mut num_points = 1usize;
                    let mut entities = Vec::new();
                    if let Some(e) = points[i].entity.as_ref() {
                        entities.push(Rc::clone(e));
                    }

                    for &ni in &neighbors {
                        if !points[ni].clustered {
                            let n_bbox = get_bounding_box(
                                &mut s,
                                &points[ni],
                                pixel_range,
                                cluster_billboards,
                                cluster_labels,
                                cluster_points,
                            );
                            cluster_pos = cluster_pos + get_point_position(&mut s, &points[ni]);
                            total_bbox = total_bbox.union(&n_bbox);
                            num_points += 1;
                            if let Some(e) = points[ni].entity.as_ref() {
                                entities.push(Rc::clone(e));
                            }
                        }
                    }

                    if num_points >= minimum_cluster_size {
                        let position = cluster_pos.multiply_by_scalar(1.0 / num_points as f64);
                        events.push(add_cluster(&mut s, position, num_points, entities)?);
                        new_clusters.push(PreviousCluster {
                            position,
                            width: total_bbox.width,
                            height: total_bbox.height,
                            minimum_width: bbox.width,
                            minimum_height: bbox.height,
                        });
                        for &ni in &neighbors {
                            points[ni].clustered = true;
                        }
                    } else {
                        add_non_clustered_item(&mut s, &points[i]);
                    }
                }
            }

            if s.cluster_label_collection.as_ref().is_some_and(LabelCollection::is_empty) {
                if let Some(mut c) = s.cluster_label_collection.take() {
                    let _ = c.destroy();
                }
            }
            if s.cluster_billboard_collection.as_mut().is_some_and(|c| c.length() == 0) {
                if let Some(mut c) = s.cluster_billboard_collection.take() {
                    let _ = c.destroy();
                }
            }
            if s.cluster_point_collection.as_mut().is_some_and(|c| c.length() == 0) {
                if let Some(mut c) = s.cluster_point_collection.take() {
                    let _ = c.destroy();
                }
            }

            s.previous_clusters = new_clusters;
            s.previous_height = Some(current_height);
            events
        };

        for payload in pending_events {
            self.cluster_event.raise(payload);
        }
        Ok(())
    }

    /// Updates clustering state (enable/disable transitions and dirty recomputation)
    /// without needing a GPU context.
    pub fn update_clustering(
        &self,
        ellipsoid: Ellipsoid,
        camera: &mut Camera,
        transform_state: &SceneTransformState<'_>,
    ) -> Result<(), RuntimeError> {
        if !self.show.get() {
            return Ok(());
        }

        if self.enabled_dirty.replace(false) {
            self.update_enable();
            self.cluster_dirty.set(true);
        }

        if self.cluster_dirty.get() {
            self.run_clustering(ellipsoid, camera, transform_state, None)?;
            let still_dirty = self.state.borrow().billboard_collection.as_ref().is_some_and(|b| !b.ready());
            self.cluster_dirty.set(still_dirty);
        }

        Ok(())
    }

    /// Full frame update including underlying collection GPU updates.
    pub fn update(
        &self,
        frame_state: &mut FrameState,
        camera: &mut Camera,
        _context: &Context<G>,
    ) -> Result<(), RuntimeError> {
        if !self.show.get() {
            return Ok(());
        }

        let client_dims = Cartesian2::new(camera.client_width, camera.client_height);
        let ellipsoid = camera.ellipsoid;

        {
            let mut s = self.state.borrow_mut();
            if let Some(bc) = s.billboard_collection.as_mut() {
                if bc.length() > 0 && !bc.ready() {
                    let saved = std::mem::take(&mut frame_state.command_list);
                    bc.update(frame_state)?;
                    frame_state.command_list = saved;
                }
            }
        }

        let transform_state = SceneTransformState::from_frame(frame_state, client_dims);
        self.update_clustering(ellipsoid, camera, &transform_state)?;

        let mut s = self.state.borrow_mut();
        if let Some(c) = s.cluster_label_collection.as_mut() {
            c.update_with_context::<G>(frame_state)?;
        }
        if let Some(c) = s.cluster_billboard_collection.as_mut() {
            c.update(frame_state)?;
        }
        if let Some(c) = s.cluster_point_collection.as_mut() {
            c.update(frame_state)?;
        }
        if let Some(c) = s.label_collection.as_mut() {
            c.update_with_context::<G>(frame_state)?;
        }
        if let Some(c) = s.billboard_collection.as_mut() {
            c.update(frame_state)?;
        }
        if let Some(c) = s.point_collection.as_mut() {
            c.update(frame_state)?;
        }
        Ok(())
    }

    fn update_enable(&self) {
        if self.enabled.get() {
            return;
        }
        let mut s = self.state.borrow_mut();
        if let Some(mut c) = s.cluster_label_collection.take() {
            let _ = c.destroy();
        }
        if let Some(mut c) = s.cluster_billboard_collection.take() {
            let _ = c.destroy();
        }
        if let Some(mut c) = s.cluster_point_collection.take() {
            let _ = c.destroy();
        }
        if let Some(lc) = s.label_collection.as_ref() {
            for i in 0..lc.len() {
                if let Some(l) = lc.get(i) {
                    l.borrow_mut().set_cluster_show(true);
                }
            }
        }
        if let Some(bc) = s.billboard_collection.as_mut() {
            let len = bc.length();
            for i in 0..len {
                if let Some(b) = bc.get(i) {
                    b.set_cluster_show(true);
                }
            }
        }
        if let Some(pc) = s.point_collection.as_mut() {
            let len = pc.length();
            for i in 0..len {
                if let Some(p) = pc.get(i) {
                    p.set_cluster_show(true);
                }
            }
        }
    }

    /// Releases all owned primitive collections and resets clustering state so the
    /// instance can be reused (matching CesiumJS `EntityCluster.prototype.destroy`).
    pub fn destroy(&self) {
        let mut s = self.state.borrow_mut();
        if let Some(mut c) = s.label_collection.take() {
            let _ = c.destroy();
        }
        if let Some(mut c) = s.billboard_collection.take() {
            let _ = c.destroy();
        }
        if let Some(mut c) = s.point_collection.take() {
            let _ = c.destroy();
        }
        if let Some(mut c) = s.cluster_label_collection.take() {
            let _ = c.destroy();
        }
        if let Some(mut c) = s.cluster_billboard_collection.take() {
            let _ = c.destroy();
        }
        if let Some(mut c) = s.cluster_point_collection.take() {
            let _ = c.destroy();
        }
        s.collection_indices_by_entity.clear();
        s.entities_by_id.clear();
        s.unused_label_indices.clear();
        s.unused_billboard_indices.clear();
        s.unused_point_indices.clear();
        s.previous_clusters.clear();
        s.previous_height = None;
        self.enabled_dirty.set(false);
        self.cluster_dirty.set(false);
    }
}

fn remove_entity_indices_if_unused<G: Gl + 'static>(s: &mut ClusterState<G>, entity_id: &str) {
    let empty = s
        .collection_indices_by_entity
        .get(entity_id)
        .is_some_and(|i| i.billboard_index.is_none() && i.label_index.is_none() && i.point_index.is_none());
    if empty {
        s.collection_indices_by_entity.remove(entity_id);
        s.entities_by_id.remove(entity_id);
    }
}

fn expand_bounding_box(bbox: &mut BoundingRectangle, pixel_range: f64) {
    bbox.x -= pixel_range;
    bbox.y -= pixel_range;
    bbox.width += pixel_range * 2.0;
    bbox.height += pixel_range * 2.0;
}

fn finite_bbox(mut bbox: BoundingRectangle) -> BoundingRectangle {
    if !bbox.x.is_finite() {
        bbox.x = 0.0;
    }
    if !bbox.y.is_finite() {
        bbox.y = 0.0;
    }
    if !bbox.width.is_finite() {
        bbox.width = 0.0;
    }
    if !bbox.height.is_finite() {
        bbox.height = 0.0;
    }
    bbox
}

fn collect_label_points<G: Gl + 'static>(
    s: &ClusterState<G>,
    transform_state: &SceneTransformState<'_>,
    camera: &mut Camera,
    occluder: &EllipsoidalOccluder,
    cluster_billboards: bool,
    cluster_points: bool,
    out: &mut Vec<ScreenPoint>,
) -> Result<(), RuntimeError> {
    let Some(lc) = s.label_collection.as_ref() else { return Ok(()) };
    let model_matrix = *lc.model_matrix();
    for i in 0..lc.len() {
        let Some(handle) = lc.get(i) else { continue };
        let (show, position, eye_offset, pixel_offset, entity_id) = {
            let mut l = handle.borrow_mut();
            l.set_cluster_show(false);
            (
                l.show(),
                l.clamped_position().unwrap_or_else(|| l.position()),
                l.eye_offset(),
                l.pixel_offset(),
                l.id().map(str::to_owned),
            )
        };
        if !show || (transform_state.mode == SceneMode::Scene3D && !occluder.is_point_visible(position)) {
            continue;
        }
        let entity = entity_id.as_deref().and_then(|id| s.entities_by_id.get(id)).cloned();
        if let Some(e) = entity.as_ref() {
            let can_cluster_billboards = cluster_billboards && e.get_property("billboard").is_some();
            let can_cluster_points = cluster_points && e.get_property("point").is_some();
            if can_cluster_billboards || can_cluster_points {
                continue;
            }
        }
        let world = model_matrix.multiply_by_point(position);
        let Some(coord) =
            SceneTransforms::world_with_eye_offset_to_window_coordinates(transform_state, camera, world, eye_offset)?
        else {
            continue;
        };
        out.push(ScreenPoint {
            index: i,
            kind: SourceKind::Label,
            entity,
            clustered: false,
            coord: coord + pixel_offset,
        });
    }
    Ok(())
}

fn collect_billboard_points<G: Gl + 'static>(
    s: &mut ClusterState<G>,
    transform_state: &SceneTransformState<'_>,
    camera: &mut Camera,
    occluder: &EllipsoidalOccluder,
    out: &mut Vec<ScreenPoint>,
) -> Result<(), RuntimeError> {
    let Some(bc) = s.billboard_collection.as_mut() else { return Ok(()) };
    let len = bc.length();
    let mut items = Vec::with_capacity(len);
    for i in 0..len {
        if let Some(b) = bc.get(i) {
            items.push((i, b));
        }
    }
    let model_matrix = Matrix4::IDENTITY;
    for (i, b) in items {
        b.set_cluster_show(false);
        let pos = b.position();
        if !b.show() || (transform_state.mode == SceneMode::Scene3D && !occluder.is_point_visible(pos)) {
            continue;
        }
        let Some(coord) = b.compute_screen_space_position(&model_matrix, transform_state, camera)? else {
            continue;
        };
        let entity = b.id().and_then(|id| s.entities_by_id.get(&id)).cloned();
        out.push(ScreenPoint { index: i, kind: SourceKind::Billboard, entity, clustered: false, coord });
    }
    Ok(())
}

fn collect_point_points<G: Gl + 'static>(
    s: &mut ClusterState<G>,
    transform_state: &SceneTransformState<'_>,
    camera: &mut Camera,
    occluder: &EllipsoidalOccluder,
    out: &mut Vec<ScreenPoint>,
) -> Result<(), RuntimeError> {
    let Some(pc) = s.point_collection.as_mut() else { return Ok(()) };
    let len = pc.length();
    let mut items = Vec::with_capacity(len);
    for i in 0..len {
        if let Some(p) = pc.get(i) {
            items.push((i, p));
        }
    }
    for (i, p) in items {
        p.set_cluster_show(false);
        let pos = p.position();
        if !p.show() || (transform_state.mode == SceneMode::Scene3D && !occluder.is_point_visible(pos)) {
            continue;
        }
        let Some(coord) = SceneTransforms::world_to_window_coordinates(transform_state, camera, pos)? else {
            continue;
        };
        let entity = p.id().and_then(|id| s.entities_by_id.get(&id)).cloned();
        out.push(ScreenPoint { index: i, kind: SourceKind::Point, entity, clustered: false, coord });
    }
    Ok(())
}

fn get_point_position<G: Gl + 'static>(s: &mut ClusterState<G>, p: &ScreenPoint) -> Cartesian3 {
    match p.kind {
        SourceKind::Label => {
            s.label_collection.as_ref().and_then(|c| c.get(p.index)).map_or(Cartesian3::ZERO, |l| l.borrow().position())
        }
        SourceKind::Billboard => {
            s.billboard_collection.as_mut().and_then(|c| c.get(p.index)).map_or(Cartesian3::ZERO, |b| b.position())
        }
        SourceKind::Point => {
            s.point_collection.as_mut().and_then(|c| c.get(p.index)).map_or(Cartesian3::ZERO, |pt| pt.position())
        }
    }
}

fn get_bounding_box<G: Gl + 'static>(
    s: &mut ClusterState<G>,
    p: &ScreenPoint,
    pixel_range: f64,
    cluster_billboards: bool,
    cluster_labels: bool,
    cluster_points: bool,
) -> BoundingRectangle {
    let mut result = match p.kind {
        SourceKind::Label if cluster_labels => s
            .label_collection
            .as_ref()
            .and_then(|c| c.get(p.index))
            .map_or(BoundingRectangle::new(p.coord.x, p.coord.y, 0.0, 0.0), |l| {
                finite_bbox(l.borrow().screen_space_bounding_box(p.coord))
            }),
        SourceKind::Billboard if cluster_billboards => s
            .billboard_collection
            .as_mut()
            .and_then(|c| c.get(p.index))
            .map_or(BoundingRectangle::new(p.coord.x, p.coord.y, 0.0, 0.0), |b| {
                let mut bb = b.screen_space_bounding_box(p.coord);
                if !bb.width.is_finite() || !bb.height.is_finite() {
                    bb = BoundingRectangle::new(p.coord.x, p.coord.y, 0.0, 0.0);
                }
                bb
            }),
        SourceKind::Point if cluster_points => s
            .point_collection
            .as_mut()
            .and_then(|c| c.get(p.index))
            .map_or(BoundingRectangle::new(p.coord.x, p.coord.y, 0.0, 0.0), |pt| {
                finite_bbox(pt.screen_space_bounding_box(p.coord))
            }),
        _ => BoundingRectangle::new(p.coord.x, p.coord.y, 0.0, 0.0),
    };

    expand_bounding_box(&mut result, pixel_range);

    if cluster_labels && p.kind != SourceKind::Label {
        if let Some(entity) = p.entity.as_ref() {
            if entity.get_property("label").is_some() {
                if let Some(label_idx) = s.collection_indices_by_entity.get(entity.id()).and_then(|idx| idx.label_index)
                {
                    if let Some(label) = s.label_collection.as_ref().and_then(|c| c.get(label_idx)) {
                        let mut label_bbox = finite_bbox(label.borrow().screen_space_bounding_box(p.coord));
                        expand_bounding_box(&mut label_bbox, pixel_range);
                        result = result.union(&label_bbox);
                    }
                }
            }
        }
    }

    result
}

fn add_non_clustered_item<G: Gl + 'static>(s: &mut ClusterState<G>, p: &ScreenPoint) {
    match p.kind {
        SourceKind::Label => {
            if let Some(l) = s.label_collection.as_ref().and_then(|c| c.get(p.index)) {
                l.borrow_mut().set_cluster_show(true);
            }
        }
        SourceKind::Billboard => {
            if let Some(b) = s.billboard_collection.as_mut().and_then(|c| c.get(p.index)) {
                b.set_cluster_show(true);
            }
        }
        SourceKind::Point => {
            if let Some(pt) = s.point_collection.as_mut().and_then(|c| c.get(p.index)) {
                pt.set_cluster_show(true);
            }
        }
    }

    if p.kind != SourceKind::Label {
        if let Some(entity) = p.entity.as_ref() {
            if entity.get_property("label").is_some() {
                if let Some(label_idx) = s.collection_indices_by_entity.get(entity.id()).and_then(|idx| idx.label_index)
                {
                    if let Some(label) = s.label_collection.as_ref().and_then(|c| c.get(label_idx)) {
                        label.borrow_mut().set_cluster_show(true);
                    }
                }
            }
        }
    }
}

fn add_cluster<G: Gl + 'static>(
    s: &mut ClusterState<G>,
    position: Cartesian3,
    num_points: usize,
    entities: Vec<EntityRef>,
) -> Result<ClusterEventPayload<G>, RuntimeError> {
    let billboard = s.cluster_billboard_collection.as_mut().unwrap().add(Default::default(), None)?;
    let label = s.cluster_label_collection.as_mut().unwrap().add(LabelOptions::default()).map_err(RuntimeError::new)?;
    let point = s.cluster_point_collection.as_mut().unwrap().add(PointPrimitiveOptions::default())?;

    billboard.set_show(false);
    point.set_show(false);
    billboard.set_position(position);
    point.set_position(position);
    {
        let mut l = label.borrow_mut();
        l.set_show(true);
        l.set_text(format_cluster_count(num_points));
        l.set_position(position);
    }

    Ok(ClusterEventPayload { entities, cluster: ClusterPrimitives { billboard, label, point } })
}

fn format_cluster_count(n: usize) -> String {
    // Matches `numPoints.toLocaleString()` for en-US / standard grouping.
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (idx, ch) in s.chars().enumerate() {
        if idx > 0 && (s.len() - idx) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Static 2D KD-tree matching `kdbush` (nodeSize = 64) used by CesiumJS `EntityCluster`.
struct KdBush {
    node_size: usize,
    ids: Vec<usize>,
    coords: Vec<f64>,
}

impl KdBush {
    fn new(capacity: usize) -> Self {
        Self { node_size: 64, ids: Vec::with_capacity(capacity), coords: Vec::with_capacity(capacity * 2) }
    }

    fn add(&mut self, x: f64, y: f64) {
        let idx = self.ids.len();
        self.ids.push(idx);
        self.coords.push(x);
        self.coords.push(y);
    }

    fn finish(&mut self) {
        if !self.ids.is_empty() {
            let right = self.ids.len() - 1;
            self.sort(0, right, 0);
        }
    }

    fn sort(&mut self, left: usize, right: usize, axis: usize) {
        if right.saturating_sub(left) <= self.node_size {
            return;
        }
        let m = (left + right) >> 1;
        self.select(m, left, right, axis);
        if m > 0 {
            self.sort(left, m - 1, 1 - axis);
        }
        self.sort(m + 1, right, 1 - axis);
    }

    fn select(&mut self, k: usize, mut left: usize, mut right: usize, axis: usize) {
        while right > left {
            if right - left > 600 {
                let n = (right - left + 1) as f64;
                let m = (k - left + 1) as f64;
                let z = n.ln();
                let s = 0.5 * (2.0 * z / 3.0).exp();
                let sd = 0.5 * (z * s * (n - s) / n).sqrt() * if m - n / 2.0 < 0.0 { -1.0 } else { 1.0 };
                let new_left = left.max((k as f64 - m * s / n + sd).floor() as usize);
                let new_right = right.min((k as f64 + (n - m) * s / n + sd).floor() as usize);
                self.select(k, new_left, new_right, axis);
            }
            let t = self.coords[2 * k + axis];
            let mut i = left;
            let mut j = right;
            self.swap_item(left, k);
            if self.coords[2 * right + axis] > t {
                self.swap_item(left, right);
            }
            while i < j {
                self.swap_item(i, j);
                i += 1;
                j -= 1;
                while self.coords[2 * i + axis] < t {
                    i += 1;
                }
                while self.coords[2 * j + axis] > t {
                    j -= 1;
                }
            }
            if self.coords[2 * left + axis] == t {
                self.swap_item(left, j);
            } else {
                j += 1;
                self.swap_item(j, right);
            }
            if j <= k {
                left = j + 1;
            }
            if k <= j {
                right = j.saturating_sub(1);
            }
        }
    }

    fn swap_item(&mut self, i: usize, j: usize) {
        self.ids.swap(i, j);
        self.coords.swap(2 * i, 2 * j);
        self.coords.swap(2 * i + 1, 2 * j + 1);
    }

    fn range(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<usize> {
        let mut result = Vec::new();
        if self.ids.is_empty() {
            return result;
        }
        let mut stack = vec![(0usize, self.ids.len() - 1, 0usize)];
        while let Some((left, right, axis)) = stack.pop() {
            if right - left <= self.node_size {
                for i in left..=right {
                    let x = self.coords[2 * i];
                    let y = self.coords[2 * i + 1];
                    if x >= min_x && x <= max_x && y >= min_y && y <= max_y {
                        result.push(self.ids[i]);
                    }
                }
                continue;
            }
            let m = (left + right) >> 1;
            let x = self.coords[2 * m];
            let y = self.coords[2 * m + 1];
            if x >= min_x && x <= max_x && y >= min_y && y <= max_y {
                result.push(self.ids[m]);
            }
            if (axis == 0 && min_x <= x) || (axis == 1 && min_y <= y) {
                if m > left {
                    stack.push((left, m - 1, 1 - axis));
                }
            }
            if (axis == 0 && max_x >= x) || (axis == 1 && max_y >= y) {
                if m < right {
                    stack.push((m + 1, right, 1 - axis));
                }
            }
        }
        result
    }
}
