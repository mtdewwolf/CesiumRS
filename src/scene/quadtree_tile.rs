//! CesiumJS `QuadtreeTile`: a single tile in a `QuadtreePrimitive`.
//!
//! Provides the quadtree tile hierarchical node structure, coordinate tracking,
//! spatial hash computation for cached surface heights, an LRU cache of sampled
//! cartographic positions, cardinal neighbor resolution (west, east, south, north),
//! lazy child subdivision, custom data routing to spatial child quadrants, and
//! load-pipeline state tracking (`QuadtreeTileLoadState`).
//!
//! Note on GPU and scheduler limits: tile mesh uploads, WebGL draw commands,
//! and background WebWorker geometry decoding belong to higher-level terrain
//! providers and rendering stages rather than this CPU tile hierarchy node.

use crate::core::{Cartesian3, Cartographic, MapProjection, Ray, Rectangle, TilingScheme};
use crate::scene::{QuadtreeTileLoadState, SceneMode, TileSelectionResult};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::rc::{Rc, Weak};
use std::sync::Arc;

pub const DEFAULT_MAX_CACHE_ENTRIES: usize = 1000;
const TILE_SIZE: f64 = 256.0;

/// A simple Least Recently Used (LRU) cache matching CesiumJS `LRUCache`.
#[derive(Clone, Debug)]
pub struct LruCache<K: Clone + Eq + Hash, V: Clone> {
    max_size: usize,
    cache: HashMap<K, V>,
    order: VecDeque<K>,
}

impl<K: Clone + Eq + Hash, V: Clone> LruCache<K, V> {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            cache: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        if !self.cache.contains_key(key) {
            return None;
        }
        // Move accessed item to the end (most recently used)
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.clone());
        self.cache.get(key)
    }

    pub fn set(&mut self, key: K, value: V) {
        if self.cache.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
            self.order.push_back(key.clone());
            self.cache.insert(key, value);
        } else {
            if self.cache.len() >= self.max_size {
                if let Some(first_key) = self.order.pop_front() {
                    self.cache.remove(&first_key);
                }
            }
            self.order.push_back(key.clone());
            self.cache.insert(key, value);
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// Callback signature for asynchronous height notifications.
pub type HeightCallback = Rc<RefCell<dyn FnMut(Cartographic)>>;

/// Custom data associated with height queries and user callbacks.
#[derive(Clone)]
pub struct QuadtreeCustomData {
    pub id: usize,
    pub position_cartographic: Cartographic,
    pub position_on_ellipsoid_surface: Option<Cartesian3>,
    pub level: i32,
    pub callback: Option<HeightCallback>,
}

impl std::fmt::Debug for QuadtreeCustomData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuadtreeCustomData")
            .field("id", &self.id)
            .field("position_cartographic", &self.position_cartographic)
            .field("position_on_ellipsoid_surface", &self.position_on_ellipsoid_surface)
            .field("level", &self.level)
            .field("has_callback", &self.callback.is_some())
            .finish()
    }
}

impl PartialEq for QuadtreeCustomData {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.position_cartographic == other.position_cartographic
            && self.position_on_ellipsoid_surface == other.position_on_ellipsoid_surface
            && self.level == other.level
    }
}

/// Provider-specific data associated with a tile.
pub trait QuadtreeTileData: std::fmt::Debug {
    fn eligible_for_unloading(&self) -> bool {
        true
    }
    fn free_resources(&mut self) {}
    fn has_mesh(&self) -> bool {
        true
    }
    fn was_created_by_upsampling(&self) -> bool {
        false
    }
    fn minimum_height(&self) -> Option<f64> {
        None
    }
    fn pick(
        &self,
        _ray: &Ray,
        _mode: SceneMode,
        _cull_back_faces: bool,
    ) -> Option<Cartesian3> {
        None
    }
    fn sample_height(
        &self,
        _position_cartographic: &Cartographic,
    ) -> Option<f64> {
        None
    }
    fn pick_with_projection(
        &self,
        ray: &Ray,
        mode: SceneMode,
        _projection: &dyn MapProjection,
        cull_back_faces: bool,
    ) -> Option<Cartesian3> {
        match mode {
            SceneMode::Scene3D => self.pick(ray, mode, cull_back_faces),
            _ => None,
        }
    }
    fn update_exaggeration(
        &mut self,
        _tile_custom_data: &mut [QuadtreeCustomData],
        _tile_added_custom_data: &mut [QuadtreeCustomData],
        _vertical_exaggeration: f64,
        _relative_height: f64,
    ) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

pub type QuadtreeTileHandle = Rc<RefCell<QuadtreeTile>>;
pub type WeakQuadtreeTileHandle = Weak<RefCell<QuadtreeTile>>;

/// Construction options for [`QuadtreeTile`].
pub struct QuadtreeTileOptions {
    pub tiling_scheme: Arc<dyn TilingScheme>,
    pub x: i32,
    pub y: i32,
    pub level: u32,
    pub parent: Option<WeakQuadtreeTileHandle>,
}

/// A single tile in a [`QuadtreePrimitive`].
pub struct QuadtreeTile {
    pub tiling_scheme: Arc<dyn TilingScheme>,
    pub x: i32,
    pub y: i32,
    pub level: u32,
    pub parent: Option<WeakQuadtreeTileHandle>,
    pub rectangle: Rectangle,

    pub southwest_child: Option<QuadtreeTileHandle>,
    pub southeast_child: Option<QuadtreeTileHandle>,
    pub northwest_child: Option<QuadtreeTileHandle>,
    pub northeast_child: Option<QuadtreeTileHandle>,

    pub distance: f64,
    pub load_priority: f64,

    pub custom_data: Vec<QuadtreeCustomData>,
    pub added_custom_data: Vec<QuadtreeCustomData>,
    pub removed_custom_data: Vec<QuadtreeCustomData>,

    pub last_selection_result: TileSelectionResult,
    pub last_selection_result_frame: Option<u32>,

    pub position_cache: LruCache<String, Cartesian3>,

    pub state: QuadtreeTileLoadState,
    pub renderable: bool,
    pub upsampled_from_parent: bool,
    pub data: Option<Box<dyn QuadtreeTileData>>,
}

impl std::fmt::Debug for QuadtreeTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuadtreeTile")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("level", &self.level)
            .field("rectangle", &self.rectangle)
            .field("state", &self.state)
            .field("renderable", &self.renderable)
            .field("upsampled_from_parent", &self.upsampled_from_parent)
            .field("distance", &self.distance)
            .finish()
    }
}

/// Creates a spatial hash key for the given longitude, latitude, and tile level.
pub fn create_spatial_hash_key(
    longitude: f64,
    latitude: f64,
    rectangle: &Rectangle,
    maximum_screen_space_error: f64,
) -> String {
    let max_error = (rectangle.width() / TILE_SIZE) * maximum_screen_space_error;
    let mut lon_grid = (longitude / max_error).floor() * max_error;
    let mut lat_grid = (latitude / max_error).floor() * max_error;
    if lon_grid == 0.0 {
        lon_grid = 0.0;
    }
    if lat_grid == 0.0 {
        lat_grid = 0.0;
    }
    format!("{:.10},{:.10}", lon_grid, lat_grid)
}

impl QuadtreeTile {
    /// Creates a new quadtree tile.
    pub fn new(options: QuadtreeTileOptions) -> Self {
        let rectangle = options
            .tiling_scheme
            .tile_xy_to_rectangle(options.x as f64, options.y as f64, options.level);

        Self {
            tiling_scheme: options.tiling_scheme,
            x: options.x,
            y: options.y,
            level: options.level,
            parent: options.parent,
            rectangle,
            southwest_child: None,
            southeast_child: None,
            northwest_child: None,
            northeast_child: None,
            distance: 0.0,
            load_priority: 0.0,
            custom_data: Vec::new(),
            added_custom_data: Vec::new(),
            removed_custom_data: Vec::new(),
            last_selection_result: TileSelectionResult::None,
            last_selection_result_frame: None,
            position_cache: LruCache::new(DEFAULT_MAX_CACHE_ENTRIES),
            state: QuadtreeTileLoadState::Start,
            renderable: false,
            upsampled_from_parent: false,
            data: None,
        }
    }

    /// Creates a rectangular set of tiles for level of detail zero.
    pub fn create_level_zero_tiles(
        tiling_scheme: Arc<dyn TilingScheme>,
    ) -> Vec<QuadtreeTileHandle> {
        let number_of_x = tiling_scheme.get_number_of_x_tiles_at_level(0);
        let number_of_y = tiling_scheme.get_number_of_y_tiles_at_level(0);
        let mut result = Vec::with_capacity((number_of_x * number_of_y) as usize);

        for y in 0..number_of_y {
            for x in 0..number_of_x {
                result.push(Rc::new(RefCell::new(QuadtreeTile::new(
                    QuadtreeTileOptions {
                        tiling_scheme: tiling_scheme.clone(),
                        x,
                        y,
                        level: 0,
                        parent: None,
                    },
                ))));
            }
        }

        result
    }

    /// Returns true if the tile needs loading (`Start` or `Loading`).
    pub fn needs_loading(&self) -> bool {
        self.state.value() < QuadtreeTileLoadState::Done.value()
    }

    /// Returns true if the tile is eligible to be unloaded.
    pub fn eligible_for_unloading(&self) -> bool {
        self.data.as_ref().is_none_or(|d| d.eligible_for_unloading())
    }

    /// Generates a unique spatial cache key for a given cartographic position.
    pub fn get_cache_key(
        &self,
        cartographic: &Cartographic,
        maximum_screen_space_error: f64,
    ) -> String {
        create_spatial_hash_key(
            cartographic.longitude,
            cartographic.latitude,
            &self.rectangle,
            maximum_screen_space_error,
        )
    }

    /// Retrieves a cached position for the specified cartographic position.
    pub fn get_position_cache_entry(
        &mut self,
        cartographic: &Cartographic,
        maximum_screen_space_error: f64,
    ) -> Option<Cartesian3> {
        let key = self.get_cache_key(cartographic, maximum_screen_space_error);
        self.position_cache.get(&key).copied()
    }

    /// Sets a position entry in the cache for this tile.
    pub fn set_position_cache_entry(
        &mut self,
        cartographic: &Cartographic,
        maximum_screen_space_error: f64,
        value: Cartesian3,
    ) {
        let key = self.get_cache_key(cartographic, maximum_screen_space_error);
        self.position_cache.set(key, value);
    }

    /// Clears the position cache for this tile.
    pub fn clear_position_cache(&mut self) {
        self.position_cache.clear();
    }

    /// Safely updates vertical exaggeration for this tile's surface data, if any.
    ///
    /// When vertical exaggeration changes:
    /// - Invalidates `self.custom_data` and `self.added_custom_data` by setting `level = -1`.
    /// - Returns `true` if exaggeration changed; `false` otherwise.
    ///
    /// Safe to call on `&mut QuadtreeTile` without causing `RefCell` reentrant borrow conflicts.
    pub fn update_exaggeration(
        &mut self,
        vertical_exaggeration: f64,
        relative_height: f64,
    ) -> bool {
        let changed = if let Some(data) = self.data.as_mut() {
            data.update_exaggeration(
                &mut self.custom_data,
                &mut self.added_custom_data,
                vertical_exaggeration,
                relative_height,
            )
        } else {
            false
        };
        if changed {
            self.clear_position_cache();
        }
        changed
    }

    /// Checks if all instantiated descendant tiles can currently be borrowed mutably.
    pub fn descendants_available(&self) -> bool {
        [&self.southwest_child, &self.southeast_child, &self.northwest_child, &self.northeast_child]
            .into_iter()
            .flatten()
            .all(|child| {
                child.try_borrow_mut().is_ok_and(|child| child.descendants_available())
            })
    }

    /// Frees this subtree. Panics before changing it if a descendant is borrowed.
    pub fn free_resources(&mut self) {
        assert!(self.descendants_available(), "cannot free a subtree while a descendant is borrowed");
        self.clear_position_cache();
        self.state = QuadtreeTileLoadState::Start;
        self.renderable = false;
        self.upsampled_from_parent = false;

        if let Some(mut data) = self.data.take() {
            data.free_resources();
        }

        if let Some(child) = self.southwest_child.take() {
            child.borrow_mut().free_resources();
        }
        if let Some(child) = self.southeast_child.take() {
            child.borrow_mut().free_resources();
        }
        if let Some(child) = self.northwest_child.take() {
            child.borrow_mut().free_resources();
        }
        if let Some(child) = self.northeast_child.take() {
            child.borrow_mut().free_resources();
        }
    }

    /// Lazily gets or instantiates the northwest child tile.
    pub fn northwest_child(this: &QuadtreeTileHandle) -> QuadtreeTileHandle {
        let needs_creation = this.borrow().northwest_child.is_none();
        if needs_creation {
            let (scheme, x, y, level) = {
                let b = this.borrow();
                (b.tiling_scheme.clone(), b.x * 2, b.y * 2, b.level + 1)
            };
            let child = Rc::new(RefCell::new(QuadtreeTile::new(QuadtreeTileOptions {
                tiling_scheme: scheme,
                x,
                y,
                level,
                parent: Some(Rc::downgrade(this)),
            })));
            this.borrow_mut().northwest_child = Some(child);
        }
        this.borrow().northwest_child.as_ref().unwrap().clone()
    }

    /// Lazily gets or instantiates the northeast child tile.
    pub fn northeast_child(this: &QuadtreeTileHandle) -> QuadtreeTileHandle {
        let needs_creation = this.borrow().northeast_child.is_none();
        if needs_creation {
            let (scheme, x, y, level) = {
                let b = this.borrow();
                (b.tiling_scheme.clone(), b.x * 2 + 1, b.y * 2, b.level + 1)
            };
            let child = Rc::new(RefCell::new(QuadtreeTile::new(QuadtreeTileOptions {
                tiling_scheme: scheme,
                x,
                y,
                level,
                parent: Some(Rc::downgrade(this)),
            })));
            this.borrow_mut().northeast_child = Some(child);
        }
        this.borrow().northeast_child.as_ref().unwrap().clone()
    }

    /// Lazily gets or instantiates the southwest child tile.
    pub fn southwest_child(this: &QuadtreeTileHandle) -> QuadtreeTileHandle {
        let needs_creation = this.borrow().southwest_child.is_none();
        if needs_creation {
            let (scheme, x, y, level) = {
                let b = this.borrow();
                (b.tiling_scheme.clone(), b.x * 2, b.y * 2 + 1, b.level + 1)
            };
            let child = Rc::new(RefCell::new(QuadtreeTile::new(QuadtreeTileOptions {
                tiling_scheme: scheme,
                x,
                y,
                level,
                parent: Some(Rc::downgrade(this)),
            })));
            this.borrow_mut().southwest_child = Some(child);
        }
        this.borrow().southwest_child.as_ref().unwrap().clone()
    }

    /// Lazily gets or instantiates the southeast child tile.
    pub fn southeast_child(this: &QuadtreeTileHandle) -> QuadtreeTileHandle {
        let needs_creation = this.borrow().southeast_child.is_none();
        if needs_creation {
            let (scheme, x, y, level) = {
                let b = this.borrow();
                (b.tiling_scheme.clone(), b.x * 2 + 1, b.y * 2 + 1, b.level + 1)
            };
            let child = Rc::new(RefCell::new(QuadtreeTile::new(QuadtreeTileOptions {
                tiling_scheme: scheme,
                x,
                y,
                level,
                parent: Some(Rc::downgrade(this)),
            })));
            this.borrow_mut().southeast_child = Some(child);
        }
        this.borrow().southeast_child.as_ref().unwrap().clone()
    }

    /// Returns all four children: `[northwest, northeast, southwest, southeast]`.
    pub fn children(this: &QuadtreeTileHandle) -> [QuadtreeTileHandle; 4] {
        [
            Self::northwest_child(this),
            Self::northeast_child(this),
            Self::southwest_child(this),
            Self::southeast_child(this),
        ]
    }

    /// Determines which child tile contains the specified cartographic position.
    pub fn child_tile_at_position(
        this: &QuadtreeTileHandle,
        position: &Cartographic,
    ) -> QuadtreeTileHandle {
        let nw = Self::northwest_child(this);
        let nw_rectangle = nw.borrow().rectangle;
        let split_point = nw_rectangle.southeast();

        let x = if position.longitude >= split_point.longitude { 1 } else { 0 };
        let y = if position.latitude < split_point.latitude { 1 } else { 0 };

        match y * 2 + x {
            0 => nw,
            1 => Self::northeast_child(this),
            2 => Self::southwest_child(this),
            _ => Self::southeast_child(this),
        }
    }

    /// Pushes newly added/removed custom data down to child spatial quadrants.
    pub fn update_custom_data(this: &QuadtreeTileHandle) {
        let (added, removed) = {
            let mut b = this.borrow_mut();
            (
                std::mem::take(&mut b.added_custom_data),
                std::mem::take(&mut b.removed_custom_data),
            )
        };
        if added.is_empty() && removed.is_empty() {
            return;
        }

        for data in added {
            let child = Self::child_tile_at_position(this, &data.position_cartographic);
            child.borrow_mut().added_custom_data.push(data.clone());
            if !this.borrow().custom_data.iter().any(|d| d.id == data.id) {
                this.borrow_mut().custom_data.push(data);
            }
        }

        for data in removed {
            this.borrow_mut().custom_data.retain(|d| d.id != data.id);
            let child = Self::child_tile_at_position(this, &data.position_cartographic);
            child.borrow_mut().removed_custom_data.push(data);
        }
    }

    /// Finds a level zero tile matching coordinates `x, y`, handling longitude wrapping.
    pub fn find_level_zero_tile(
        level_zero_tiles: &[QuadtreeTileHandle],
        x: i32,
        y: i32,
        tiling_scheme: &dyn TilingScheme,
    ) -> Option<QuadtreeTileHandle> {
        let x_tiles = tiling_scheme.get_number_of_x_tiles_at_level(0);
        let mut wrapped_x = x;
        if wrapped_x < 0 {
            wrapped_x += x_tiles;
        } else if wrapped_x >= x_tiles {
            wrapped_x -= x_tiles;
        }

        let y_tiles = tiling_scheme.get_number_of_y_tiles_at_level(0);
        if y < 0 || y >= y_tiles {
            return None;
        }

        level_zero_tiles
            .iter()
            .find(|tile| {
                let b = tile.borrow();
                b.x == wrapped_x && b.y == y
            })
            .cloned()
    }

    /// Finds the neighboring tile to the West across roots or parent hierarchy.
    pub fn find_tile_to_west(
        this: &QuadtreeTileHandle,
        level_zero_tiles: &[QuadtreeTileHandle],
    ) -> Option<QuadtreeTileHandle> {
        let parent_opt = this.borrow().parent.as_ref().and_then(|w| w.upgrade());
        let Some(parent) = parent_opt else {
            let (x, y, scheme) = {
                let b = this.borrow();
                (b.x - 1, b.y, b.tiling_scheme.clone())
            };
            return Self::find_level_zero_tile(level_zero_tiles, x, y, &*scheme);
        };

        let is_southeast = parent
            .borrow()
            .southeast_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_southeast {
            return Some(Self::southwest_child(&parent));
        }

        let is_northeast = parent
            .borrow()
            .northeast_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_northeast {
            return Some(Self::northwest_child(&parent));
        }

        let is_southwest = parent
            .borrow()
            .southwest_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));

        let west_of_parent = Self::find_tile_to_west(&parent, level_zero_tiles)?;
        if is_southwest {
            Some(Self::southeast_child(&west_of_parent))
        } else {
            Some(Self::northeast_child(&west_of_parent))
        }
    }

    /// Finds the neighboring tile to the East across roots or parent hierarchy.
    pub fn find_tile_to_east(
        this: &QuadtreeTileHandle,
        level_zero_tiles: &[QuadtreeTileHandle],
    ) -> Option<QuadtreeTileHandle> {
        let parent_opt = this.borrow().parent.as_ref().and_then(|w| w.upgrade());
        let Some(parent) = parent_opt else {
            let (x, y, scheme) = {
                let b = this.borrow();
                (b.x + 1, b.y, b.tiling_scheme.clone())
            };
            return Self::find_level_zero_tile(level_zero_tiles, x, y, &*scheme);
        };

        let is_southwest = parent
            .borrow()
            .southwest_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_southwest {
            return Some(Self::southeast_child(&parent));
        }

        let is_northwest = parent
            .borrow()
            .northwest_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_northwest {
            return Some(Self::northeast_child(&parent));
        }

        let is_southeast = parent
            .borrow()
            .southeast_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));

        let east_of_parent = Self::find_tile_to_east(&parent, level_zero_tiles)?;
        if is_southeast {
            Some(Self::southwest_child(&east_of_parent))
        } else {
            Some(Self::northwest_child(&east_of_parent))
        }
    }

    /// Finds the neighboring tile to the South across roots or parent hierarchy.
    pub fn find_tile_to_south(
        this: &QuadtreeTileHandle,
        level_zero_tiles: &[QuadtreeTileHandle],
    ) -> Option<QuadtreeTileHandle> {
        let parent_opt = this.borrow().parent.as_ref().and_then(|w| w.upgrade());
        let Some(parent) = parent_opt else {
            let (x, y, scheme) = {
                let b = this.borrow();
                (b.x, b.y + 1, b.tiling_scheme.clone())
            };
            return Self::find_level_zero_tile(level_zero_tiles, x, y, &*scheme);
        };

        let is_northwest = parent
            .borrow()
            .northwest_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_northwest {
            return Some(Self::southwest_child(&parent));
        }

        let is_northeast = parent
            .borrow()
            .northeast_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_northeast {
            return Some(Self::southeast_child(&parent));
        }

        let is_southwest = parent
            .borrow()
            .southwest_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));

        let south_of_parent = Self::find_tile_to_south(&parent, level_zero_tiles)?;
        if is_southwest {
            Some(Self::northwest_child(&south_of_parent))
        } else {
            Some(Self::northeast_child(&south_of_parent))
        }
    }

    /// Finds the neighboring tile to the North across roots or parent hierarchy.
    pub fn find_tile_to_north(
        this: &QuadtreeTileHandle,
        level_zero_tiles: &[QuadtreeTileHandle],
    ) -> Option<QuadtreeTileHandle> {
        let parent_opt = this.borrow().parent.as_ref().and_then(|w| w.upgrade());
        let Some(parent) = parent_opt else {
            let (x, y, scheme) = {
                let b = this.borrow();
                (b.x, b.y - 1, b.tiling_scheme.clone())
            };
            return Self::find_level_zero_tile(level_zero_tiles, x, y, &*scheme);
        };

        let is_southwest = parent
            .borrow()
            .southwest_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_southwest {
            return Some(Self::northwest_child(&parent));
        }

        let is_southeast = parent
            .borrow()
            .southeast_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));
        if is_southeast {
            return Some(Self::northeast_child(&parent));
        }

        let is_northwest = parent
            .borrow()
            .northwest_child
            .as_ref()
            .is_some_and(|c| Rc::ptr_eq(c, this));

        let north_of_parent = Self::find_tile_to_north(&parent, level_zero_tiles)?;
        if is_northwest {
            Some(Self::southwest_child(&north_of_parent))
        } else {
            Some(Self::southeast_child(&north_of_parent))
        }
    }
}
