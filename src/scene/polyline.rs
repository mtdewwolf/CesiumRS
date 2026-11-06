//! CesiumJS `Polyline`: one line of a
//! [`PolylineCollection`](super::polyline_collection::PolylineCollection).
//!
//! A [`Polyline`] is a shared handle. Setters mark the polyline dirty in its
//! collection as Cesium's `makeDirty` does; positions drop consecutive
//! duplicates, close loops and are split at the antimeridian
//! (`PolylinePipeline.wrapLongitude`) for 2D and Columbus view.
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use crate::{
    core::{
        array_remove_duplicates::array_remove_duplicates, polyline_pipeline::wrap_longitude, BoundingSphere,
        Cartesian3, Color, DistanceDisplayCondition, Matrix4, RuntimeError,
    },
    renderer::{Context, Gl, PickId},
    scene::{
        material::{Material, MaterialValue, COLOR_TYPE},
        MetadataValue, ScenePickObject,
    },
};

pub const POSITION_INDEX: usize = 0;
pub const SHOW_INDEX: usize = 1;
pub const WIDTH_INDEX: usize = 2;
pub const MATERIAL_INDEX: usize = 3;
pub const POSITION_SIZE_INDEX: usize = 4;
pub const DISTANCE_DISPLAY_CONDITION_INDEX: usize = 5;
pub const NUMBER_OF_PROPERTIES: usize = 6;

/// A shared material: Cesium compares materials by identity.
pub type SharedMaterial = Rc<RefCell<Material>>;

/// `new Polyline(options)`; `material` defaults to a white `Color`
/// material.
#[derive(Clone, Debug, Default)]
pub struct PolylineOptions {
    pub show: Option<bool>,
    pub width: Option<f64>,
    pub r#loop: Option<bool>,
    pub material: Option<SharedMaterial>,
    pub positions: Vec<Cartesian3>,
    pub id: Option<String>,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
}

/// A white `Color` material, Cesium's default.
pub fn default_material() -> SharedMaterial {
    let material =
        Material::from_type_with_uniforms(COLOR_TYPE, vec![("color".into(), MaterialValue::from_color(Color::WHITE))])
            .expect("the Color material exists");
    Rc::new(RefCell::new(material))
}

/// The collection side of `makeDirty`: `_updatePolyline`.
pub(crate) struct PolylineShared {
    pub(crate) id: u64,
    /// The collection's current `modelMatrix`, read by `Polyline.update`.
    pub(crate) model_matrix: std::cell::Cell<Matrix4>,
    pub(crate) updated: std::cell::Cell<bool>,
    pub(crate) to_update: RefCell<Vec<Polyline>>,
    pub(crate) properties_changed: RefCell<[u32; NUMBER_OF_PROPERTIES]>,
}

/// A bucket locator: a vertex array's range for one bucket.
#[derive(Debug, Default)]
pub(crate) struct Locator {
    pub(crate) count: usize,
    pub(crate) offset: usize,
    pub(crate) bucket: u64,
}

pub(crate) struct PolylineState {
    pub(crate) show: bool,
    pub(crate) width: f64,
    pub(crate) r#loop: bool,
    pub(crate) distance_display_condition: Option<DistanceDisplayCondition>,
    pub(crate) material: SharedMaterial,
    pub(crate) positions: Vec<Cartesian3>,
    pub(crate) actual_positions: Vec<Cartesian3>,
    /// `_length`: the actual position count at the last positions change.
    pub(crate) length: usize,
    pub(crate) id: Option<String>,
    pub(crate) model_matrix: Matrix4,
    pub(crate) segments: (Vec<Cartesian3>, Vec<usize>),
    pub(crate) actual_length: Option<usize>,
    pub(crate) properties_changed: [u32; NUMBER_OF_PROPERTIES],
    pub(crate) dirty: bool,
    pub(crate) pick_id: Option<PickId>,
    pub(crate) bounding_volume: BoundingSphere,
    pub(crate) bounding_volume_wc: BoundingSphere,
    pub(crate) bounding_volume_2d: BoundingSphere,
    pub(crate) index: usize,
    pub(crate) bucket: Option<u64>,
    pub(crate) locator_buckets: Vec<(Rc<RefCell<Locator>>, usize)>,
    collection: Option<Weak<PolylineShared>>,
    collection_id: Option<u64>,
}

/// A polyline in a collection. Clones share the polyline.
#[derive(Clone)]
pub struct Polyline(pub(crate) Rc<RefCell<PolylineState>>);

impl std::fmt::Debug for Polyline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.0.borrow();
        f.debug_struct("Polyline")
            .field("index", &s.index)
            .field("positions", &s.actual_positions.len())
            .finish_non_exhaustive()
    }
}

/// The object a polyline registers with `Context.createPickId`.
pub struct PolylinePick {
    pub primitive: Polyline,
    pub collection: Option<u64>,
    pub id: Option<String>,
}

fn remove_duplicates(positions: &[Cartesian3]) -> Vec<Cartesian3> {
    array_remove_duplicates(positions, |a, b, eps| a.equals_epsilon(*b, eps), false, None)
}

impl Polyline {
    pub(crate) fn new(
        options: PolylineOptions,
        collection: &Rc<PolylineShared>,
        index: usize,
    ) -> Result<Self, RuntimeError> {
        let r#loop = options.r#loop.unwrap_or(false);
        let mut actual = remove_duplicates(&options.positions);
        if r#loop && actual.len() > 2 {
            actual.push(actual[0]);
        }
        let model_matrix = collection.model_matrix.get();
        let segments = wrap_longitude(&actual, Some(&model_matrix));
        let bounding_volume = BoundingSphere::from_points(&actual);
        Ok(Self(Rc::new(RefCell::new(PolylineState {
            show: options.show.unwrap_or(true),
            width: options.width.unwrap_or(1.0),
            r#loop,
            distance_display_condition: options.distance_display_condition,
            material: options.material.unwrap_or_else(default_material),
            length: actual.len(),
            positions: options.positions,
            actual_positions: actual,
            id: options.id,
            model_matrix,
            segments,
            actual_length: None,
            properties_changed: [0; NUMBER_OF_PROPERTIES],
            dirty: false,
            pick_id: None,
            bounding_volume_wc: bounding_volume.transform(&model_matrix),
            bounding_volume,
            bounding_volume_2d: BoundingSphere::default(),
            index,
            bucket: None,
            locator_buckets: Vec::new(),
            collection: Some(Rc::downgrade(collection)),
            collection_id: Some(collection.id),
        }))))
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    fn collection(&self) -> Option<Rc<PolylineShared>> {
        self.0.borrow().collection.as_ref().and_then(Weak::upgrade)
    }

    pub(crate) fn collection_id(&self) -> Option<u64> {
        self.0.borrow().collection_id
    }

    /// `makeDirty`: count the property on the polyline and, in a collection,
    /// queue it once and count it there too.
    fn make_dirty(&self, property: usize) {
        self.0.borrow_mut().properties_changed[property] += 1;
        if let Some(collection) = self.collection() {
            collection.updated.set(true);
            if !self.0.borrow().dirty {
                collection.to_update.borrow_mut().push(self.clone());
            }
            collection.properties_changed.borrow_mut()[property] += 1;
            self.0.borrow_mut().dirty = true;
        }
    }

    pub fn show(&self) -> bool {
        self.0.borrow().show
    }
    pub fn set_show(&self, value: bool) {
        if value != self.show() {
            self.0.borrow_mut().show = value;
            self.make_dirty(SHOW_INDEX);
        }
    }

    pub fn width(&self) -> f64 {
        self.0.borrow().width
    }
    pub fn set_width(&self, value: f64) {
        if value != self.width() {
            self.0.borrow_mut().width = value;
            self.make_dirty(WIDTH_INDEX);
        }
    }

    /// The positions as given.
    pub fn positions(&self) -> Vec<Cartesian3> {
        self.0.borrow().positions.clone()
    }
    /// Sets the positions: drops consecutive duplicates, closes a loop, and
    /// recomputes the bounding volumes and segments.
    pub fn set_positions(&self, value: Vec<Cartesian3>) {
        let mut positions = remove_duplicates(&value);
        let size_changed = {
            let s = self.0.borrow();
            if s.r#loop && positions.len() > 2 {
                positions.push(positions[0]);
            }
            s.actual_positions.len() != positions.len() || s.actual_positions.len() != s.length
        };
        if size_changed {
            self.make_dirty(POSITION_SIZE_INDEX);
        }
        {
            let mut s = self.0.borrow_mut();
            s.positions = value;
            s.length = positions.len();
            s.bounding_volume = BoundingSphere::from_points(&positions);
            s.bounding_volume_wc = s.bounding_volume.transform(&s.model_matrix);
            s.actual_positions = positions;
        }
        self.make_dirty(POSITION_INDEX);
        self.update();
    }

    /// The positions drawn: without consecutive duplicates, closed for a loop.
    pub fn actual_positions(&self) -> Vec<Cartesian3> {
        self.0.borrow().actual_positions.clone()
    }

    pub fn material(&self) -> SharedMaterial {
        self.0.borrow().material.clone()
    }
    /// Replaces the material; the same material (by identity) changes nothing.
    pub fn set_material(&self, material: SharedMaterial) {
        if !Rc::ptr_eq(&self.0.borrow().material, &material) {
            self.0.borrow_mut().material = material;
            self.make_dirty(MATERIAL_INDEX);
        }
    }

    pub fn r#loop(&self) -> bool {
        self.0.borrow().r#loop
    }
    /// Closes or opens the line. As in Cesium, the bounding volumes and the
    /// recorded length are left for the next positions change.
    pub fn set_loop(&self, value: bool) {
        if value == self.r#loop() {
            return;
        }
        {
            let mut s = self.0.borrow_mut();
            let p = &mut s.actual_positions;
            if value {
                if p.len() > 2 && p[0] != p[p.len() - 1] {
                    p.push(p[0]);
                }
            } else if p.len() > 2 && p[0] == p[p.len() - 1] {
                p.pop();
            }
            s.r#loop = value;
        }
        self.make_dirty(POSITION_SIZE_INDEX);
    }

    pub fn id(&self) -> Option<String> {
        self.0.borrow().id.clone()
    }
    /// Sets the id, also on an allocated pick object.
    pub fn set_id(&self, value: Option<String>) -> Result<(), RuntimeError> {
        self.0.borrow_mut().id = value.clone();
        let s = self.0.borrow();
        if let Some(pick_id) = &s.pick_id {
            if let Some(pick) = pick_id.object_as::<ScenePickObject>() {
                let mut replaced = ScenePickObject::new(pick.primitive.clone(), pick.primitive_show.clone());
                replaced.id = value.map(MetadataValue::String);
                pick_id.set_object(Rc::new(replaced))?;
            }
        }
        Ok(())
    }

    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.0.borrow().distance_display_condition
    }
    pub fn set_distance_display_condition(&self, value: Option<DistanceDisplayCondition>) -> Result<(), RuntimeError> {
        if value.is_some_and(|v| v.far <= v.near) {
            return Err(RuntimeError::new("far distance must be greater than near distance."));
        }
        if value != self.distance_display_condition() {
            self.0.borrow_mut().distance_display_condition = value;
            self.make_dirty(DISTANCE_DISPLAY_CONDITION_INDEX);
        }
        Ok(())
    }

    /// `isDestroyed`: removed from its collection.
    pub fn is_destroyed(&self) -> bool {
        self.0.borrow().collection.is_none()
    }
    pub fn index(&self) -> usize {
        self.0.borrow().index
    }
    pub fn is_dirty(&self) -> bool {
        self.0.borrow().dirty
    }
    pub fn properties_changed(&self) -> [u32; NUMBER_OF_PROPERTIES] {
        self.0.borrow().properties_changed
    }
    /// The antimeridian-split positions and segment lengths.
    pub fn segments(&self) -> (Vec<Cartesian3>, Vec<usize>) {
        self.0.borrow().segments.clone()
    }
    pub fn bounding_volume(&self) -> BoundingSphere {
        self.0.borrow().bounding_volume
    }
    pub fn bounding_volume_wc(&self) -> BoundingSphere {
        self.0.borrow().bounding_volume_wc
    }
    /// The 2D/Columbus view bounding volume, set when the collection writes
    /// the polyline in those modes.
    pub fn bounding_volume_2d(&self) -> BoundingSphere {
        self.0.borrow().bounding_volume_2d
    }
    /// `_actualLength`: the vertex count in the position buffer.
    pub fn actual_length(&self) -> Option<usize> {
        self.0.borrow().actual_length
    }
    /// The index counts of the polyline's ranges in its bucket's vertex arrays.
    pub fn locator_counts(&self) -> Vec<usize> {
        self.0.borrow().locator_buckets.iter().map(|(_, c)| *c).collect()
    }

    /// `update`: re-split at the antimeridian when the collection's model
    /// matrix or the positions changed, marking the size dirty when the
    /// segments changed.
    pub fn update(&self) {
        let model_matrix = self.collection().map_or(Matrix4::IDENTITY, |c| c.model_matrix.get());
        let size_changed = {
            let mut s = self.0.borrow_mut();
            let (old_count, old_lengths) = (s.segments.0.len(), s.segments.1.clone());
            let positions_changed =
                s.properties_changed[POSITION_INDEX] > 0 || s.properties_changed[POSITION_SIZE_INDEX] > 0;
            if model_matrix != s.model_matrix || positions_changed {
                s.segments = wrap_longitude(&s.actual_positions, Some(&model_matrix));
                s.bounding_volume_wc = s.bounding_volume.transform(&model_matrix);
            }
            s.model_matrix = model_matrix;
            // Cesium compares the old lengths against the new ones index by index.
            s.segments.0.len() != old_count
                || old_lengths.iter().enumerate().any(|(i, l)| s.segments.1.get(i) != Some(l))
        };
        if size_changed {
            self.make_dirty(POSITION_SIZE_INDEX);
        }
    }

    /// The pick color, once allocated.
    pub fn pick_id(&self) -> Option<Color> {
        self.0.borrow().pick_id.as_ref().map(|p| p.color)
    }
    /// `getPickId`.
    pub fn get_pick_id<G: Gl>(&self, context: &Context<G>) -> Result<Color, RuntimeError> {
        if let Some(color) = self.pick_id() {
            return Ok(color);
        }
        let (id, collection) = {
            let s = self.0.borrow();
            (s.id.clone(), s.collection_id)
        };
        let primitive: Rc<dyn std::any::Any> =
            Rc::new(PolylinePick { primitive: self.clone(), collection, id: id.clone() });
        let mut object = ScenePickObject::new(primitive, Rc::new(std::cell::Cell::new(true)));
        object.id = id.map(MetadataValue::String);
        let pick_id = context.create_pick_id(Rc::new(object))?;
        let color = pick_id.color;
        self.0.borrow_mut().pick_id = Some(pick_id);
        Ok(color)
    }

    /// `_clean`: Cesium clears every property count but the last
    /// (`DISTANCE_DISPLAY_CONDITION`).
    pub(crate) fn clean(&self) {
        let mut s = self.0.borrow_mut();
        s.dirty = false;
        for k in 0..NUMBER_OF_PROPERTIES - 1 {
            s.properties_changed[k] = 0;
        }
    }

    /// `_destroy`.
    pub(crate) fn destroy(&self) {
        let mut s = self.0.borrow_mut();
        if let Some(pick_id) = s.pick_id.take() {
            pick_id.destroy();
        }
        s.collection = None;
        s.collection_id = None;
    }
}
