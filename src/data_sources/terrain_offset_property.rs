//! CesiumJS `TerrainOffsetProperty`: computes a surface-normal offset vector
//! matching the terrain height at a position for clamped/relative height references.
use super::{
    property::{system_clock, write_result},
    DefinitionChanged, Property, PropertyClock, PropertyRef, Subscription,
};
use crate::{
    core::{iso8601, math, Cartesian3, Cartographic, Ellipsoid, JulianDate, RuntimeError},
    scene::{height_reference::is_height_reference_relative, HeightReference},
};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// Callback invoked when a scene refines a clamped position's terrain height.
pub type TerrainHeightCallback = Rc<dyn Fn(&Cartographic)>;

/// Handle that unregisters a pending height-update callback when dropped or cancelled.
pub struct TerrainHeightHandle(Option<Box<dyn FnOnce()>>);
impl TerrainHeightHandle {
    pub fn new(remove: impl FnOnce() + 'static) -> Self {
        Self(Some(Box::new(remove)))
    }
    pub fn cancel(&mut self) {
        if let Some(remove) = self.0.take() {
            remove();
        }
    }
}
impl Drop for TerrainHeightHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Host scene surface contract consumed by [`TerrainOffsetProperty`].
pub trait TerrainOffsetScene {
    fn has_globe(&self) -> bool;
    fn ellipsoid(&self) -> Ellipsoid {
        Ellipsoid::WGS84
    }
    fn get_height(&self, cartographic: &Cartographic, height_reference: Option<HeightReference>) -> Option<f64>;
    fn update_height(
        &self,
        cartographic: &Cartographic,
        callback: TerrainHeightCallback,
        height_reference: Option<HeightReference>,
    ) -> Option<TerrainHeightHandle>;
    fn terrain_provider_changed(&self) -> Option<&DefinitionChanged> {
        None
    }
    fn morph_complete(&self) -> Option<&DefinitionChanged> {
        None
    }
}

struct State {
    position: Cartesian3,
    cartographic_position: Cartographic,
    normal: Cartesian3,
    terrain_height: f64,
    remove_height_callback: Option<TerrainHeightHandle>,
    subscriptions: Vec<Subscription>,
}

/// Port of CesiumJS `TerrainOffsetProperty`.
pub struct TerrainOffsetProperty {
    scene: Rc<dyn TerrainOffsetScene>,
    position_property: PropertyRef<Cartesian3>,
    height_reference: Option<PropertyRef<HeightReference>>,
    extruded_height_reference: Option<PropertyRef<HeightReference>>,
    definition_changed: DefinitionChanged,
    clock: PropertyClock,
    destroyed: Cell<bool>,
    state: RefCell<State>,
    weak: Weak<Self>,
}

impl TerrainOffsetProperty {
    pub fn new(
        scene: Rc<dyn TerrainOffsetScene>,
        position_property: PropertyRef<Cartesian3>,
        height_reference: Option<PropertyRef<HeightReference>>,
        extruded_height_reference: Option<PropertyRef<HeightReference>>,
    ) -> Rc<Self> {
        Self::with_clock(scene, position_property, height_reference, extruded_height_reference, system_clock())
    }

    pub fn with_clock(
        scene: Rc<dyn TerrainOffsetScene>,
        position_property: PropertyRef<Cartesian3>,
        height_reference: Option<PropertyRef<HeightReference>>,
        extruded_height_reference: Option<PropertyRef<HeightReference>>,
        clock: PropertyClock,
    ) -> Rc<Self> {
        let prop = Rc::new_cyclic(|weak: &Weak<Self>| Self {
            scene: Rc::clone(&scene),
            position_property: Rc::clone(&position_property),
            height_reference,
            extruded_height_reference,
            definition_changed: DefinitionChanged::new(),
            clock,
            destroyed: Cell::new(false),
            state: RefCell::new(State {
                position: Cartesian3::ZERO,
                cartographic_position: Cartographic::default(),
                normal: Cartesian3::ZERO,
                terrain_height: 0.0,
                remove_height_callback: None,
                subscriptions: Vec::new(),
            }),
            weak: weak.clone(),
        });

        if scene.has_globe() {
            let mut subs = Vec::new();
            if let Some(event) = scene.terrain_provider_changed() {
                let weak = Rc::downgrade(&prop);
                subs.push(event.add_listener(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.update_clamping();
                    }
                }));
            }
            if let Some(event) = scene.morph_complete() {
                let weak = Rc::downgrade(&prop);
                subs.push(event.add_listener(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.update_clamping();
                    }
                }));
            }
            prop.state.borrow_mut().subscriptions = subs;
        }

        if position_property.is_constant() {
            let min_time = iso8601::minimum_value();
            if let Ok(Some(position)) = position_property.get_value(Some(&min_time), None) {
                if position != Cartesian3::ZERO && scene.has_globe() {
                    prop.state.borrow_mut().position = position;
                    prop.update_clamping();
                    let normal = scene.ellipsoid().geodetic_surface_normal(position).unwrap_or(Cartesian3::ZERO);
                    prop.state.borrow_mut().normal = normal;
                }
            }
        }

        prop
    }

    fn current_height_reference_for_scene(&self) -> Option<HeightReference> {
        // Cesium passes the heightReference property (evaluated when constant) to scene.getHeight / updateHeight.
        let prop = self.height_reference.as_ref()?;
        prop.get_value(Some(&iso8601::minimum_value()), None).ok().flatten()
    }

    pub fn update_clamping(&self) {
        if self.destroyed.get() {
            return;
        }
        let previous = self.state.borrow_mut().remove_height_callback.take();
        drop(previous);

        let position = self.state.borrow().position;
        if position == Cartesian3::ZERO {
            self.state.borrow_mut().terrain_height = 0.0;
            return;
        }

        let ellipsoid = self.scene.ellipsoid();
        let Some(cartographic) = ellipsoid.cartesian_to_cartographic(position) else {
            self.state.borrow_mut().terrain_height = 0.0;
            return;
        };
        self.state.borrow_mut().cartographic_position = cartographic;

        let hr = self.current_height_reference_for_scene();
        let height = self.scene.get_height(&cartographic, hr).unwrap_or(0.0);
        self.state.borrow_mut().terrain_height = height;

        let weak = self.weak.clone();
        let callback: TerrainHeightCallback = Rc::new(move |clamped: &Cartographic| {
            if let Some(this) = weak.upgrade() {
                if !this.destroyed.get() {
                    this.state.borrow_mut().terrain_height = clamped.height;
                    this.definition_changed.raise();
                }
            }
        });

        let handle = self.scene.update_height(&cartographic, callback, hr);
        if !self.destroyed.get() {
            self.state.borrow_mut().remove_height_callback = handle;
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }

    pub fn destroy(&self) {
        if !self.destroyed.replace(true) {
            let mut state = self.state.borrow_mut();
            let subs = std::mem::take(&mut state.subscriptions);
            let cb = state.remove_height_callback.take();
            drop(state);
            drop(subs);
            drop(cb);
        }
    }
}

impl Property<Cartesian3> for TerrainOffsetProperty {
    fn is_constant(&self) -> bool {
        false
    }

    fn definition_changed(&self) -> &DefinitionChanged {
        &self.definition_changed
    }

    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        let height_reference = self
            .height_reference
            .as_ref()
            .map(|p| p.get_value(Some(&time), None))
            .transpose()?
            .flatten()
            .unwrap_or(HeightReference::None);
        let extruded_height_reference = self
            .extruded_height_reference
            .as_ref()
            .map(|p| p.get_value(Some(&time), None))
            .transpose()?
            .flatten()
            .unwrap_or(HeightReference::None);

        if height_reference == HeightReference::None && !is_height_reference_relative(extruded_height_reference) {
            self.state.borrow_mut().position = Cartesian3::ZERO;
            return Ok(write_result(Some(Cartesian3::ZERO), result));
        }

        if self.position_property.is_constant() {
            let state = self.state.borrow();
            let offset = state.normal.multiply_by_scalar(state.terrain_height);
            return Ok(write_result(Some(offset), result));
        }

        let position = self.position_property.get_value(Some(&time), None)?;
        let Some(position) = position else {
            return Ok(write_result(Some(Cartesian3::ZERO), result));
        };
        if position == Cartesian3::ZERO || !self.scene.has_globe() {
            return Ok(write_result(Some(Cartesian3::ZERO), result));
        }

        {
            let state = self.state.borrow();
            if state.position.equals_epsilon(position, math::EPSILON10) {
                let offset = state.normal.multiply_by_scalar(state.terrain_height);
                return Ok(write_result(Some(offset), result));
            }
        }

        self.state.borrow_mut().position = position;
        self.update_clamping();

        let normal = self.scene.ellipsoid().geodetic_surface_normal(position).unwrap_or(Cartesian3::ZERO);
        let terrain_height = {
            let mut state = self.state.borrow_mut();
            state.normal = normal;
            state.terrain_height
        };
        let offset = normal.multiply_by_scalar(terrain_height);
        Ok(write_result(Some(offset), result))
    }

    fn equals(&self, other: &dyn Property<Cartesian3>) -> Result<bool, RuntimeError> {
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| std::ptr::eq(self, o)))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
