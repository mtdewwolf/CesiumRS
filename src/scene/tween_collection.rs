//! Numeric property tweens with explicit seconds, shared handles and reentrant
//! completion/cancellation callbacks. Materials use the existing typed uniforms.
use crate::{
    core::{EasingFunction, RuntimeError},
    scene::{Material, MaterialValue},
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};
pub type TweenObject = BTreeMap<String, f64>;
pub type TweenCallback = Rc<dyn Fn()>;
pub type TweenUpdate = Rc<dyn Fn(&TweenObject)>;
#[derive(Clone)]
pub enum TweenEasing {
    Builtin(EasingFunction),
    Custom(Rc<dyn Fn(f64) -> f64>),
}
impl Default for TweenEasing {
    fn default() -> Self {
        Self::Builtin(EasingFunction::LinearNone)
    }
}
impl TweenEasing {
    fn evaluate(&self, t: f64) -> f64 {
        match self {
            Self::Builtin(e) => e.evaluate(t),
            Self::Custom(f) => f(t),
        }
    }
}
#[derive(Clone)]
pub struct TweenOptions {
    pub start_object: TweenObject,
    pub stop_object: TweenObject,
    pub duration: f64,
    pub delay: f64,
    pub easing_function: TweenEasing,
    pub update: Option<TweenUpdate>,
    pub complete: Option<TweenCallback>,
    pub cancel: Option<TweenCallback>,
    /// Number of repeats after the first cycle. Infinity repeats indefinitely.
    pub repeat: f64,
}
impl Default for TweenOptions {
    fn default() -> Self {
        Self {
            start_object: TweenObject::new(),
            stop_object: TweenObject::new(),
            duration: 3.,
            delay: 0.,
            easing_function: TweenEasing::default(),
            update: None,
            complete: None,
            cancel: None,
            repeat: 0.,
        }
    }
}
#[derive(Clone)]
pub struct Tween {
    inner: Rc<RefCell<TweenState>>,
    collection: Weak<RefCell<Vec<Tween>>>,
}
struct TweenState {
    options: TweenOptions,
    value: TweenObject,
    start_time: Option<f64>,
    repeat: f64,
    active: bool,
}
impl Tween {
    pub fn start_object(&self) -> TweenObject {
        self.inner.borrow().options.start_object.clone()
    }
    pub fn stop_object(&self) -> TweenObject {
        self.inner.borrow().options.stop_object.clone()
    }
    pub fn value(&self) -> TweenObject {
        self.inner.borrow().value.clone()
    }
    pub fn duration(&self) -> f64 {
        self.inner.borrow().options.duration
    }
    pub fn delay(&self) -> f64 {
        self.inner.borrow().options.delay
    }
    pub fn easing_function(&self) -> TweenEasing {
        self.inner.borrow().options.easing_function.clone()
    }
    pub fn update_callback(&self) -> Option<TweenUpdate> {
        self.inner.borrow().options.update.clone()
    }
    pub fn complete_callback(&self) -> Option<TweenCallback> {
        self.inner.borrow().options.complete.clone()
    }
    pub fn cancel_callback(&self) -> Option<TweenCallback> {
        self.inner.borrow().options.cancel.clone()
    }
    pub fn set_cancel_callback(&self, callback: Option<TweenCallback>) {
        self.inner.borrow_mut().options.cancel = callback;
    }
    pub fn needs_start(&self) -> bool {
        self.inner.borrow().start_time.is_none()
    }
    pub fn cancel_tween(&self) -> bool {
        self.collection.upgrade().is_some_and(|inner| TweenCollection { inner }.remove(self))
    }
}
#[derive(Clone, Default)]
pub struct TweenCollection {
    inner: Rc<RefCell<Vec<Tween>>>,
}
impl TweenCollection {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn length(&self) -> usize {
        self.inner.borrow().len()
    }
    pub fn is_empty(&self) -> bool {
        self.length() == 0
    }
    pub fn get(&self, index: usize) -> Option<Tween> {
        self.inner.borrow().get(index).cloned()
    }
    pub fn contains(&self, tween: &Tween) -> bool {
        self.inner.borrow().iter().any(|t| Rc::ptr_eq(&t.inner, &tween.inner))
    }
    pub fn add(&self, options: TweenOptions) -> Result<Tween, RuntimeError> {
        if options.duration < 0. {
            return Err(RuntimeError::new("options.duration is required and must be positive."));
        }
        let active = options.duration != 0.;
        let complete = options.complete.clone();
        let state = TweenState {
            value: options.start_object.clone(),
            repeat: options.repeat,
            options,
            start_time: None,
            active,
        };
        let tween = Tween { inner: Rc::new(RefCell::new(state)), collection: Rc::downgrade(&self.inner) };
        if active {
            self.inner.borrow_mut().push(tween.clone());
        } else if let Some(f) = complete {
            f();
        }
        Ok(tween)
    }
    /// The supplied object is updated in place. Like upstream `addProperty`,
    /// this helper owns the update callback rather than forwarding options.update.
    pub fn add_property(
        &self,
        object: Rc<RefCell<TweenObject>>,
        property: &str,
        start: f64,
        stop: f64,
        mut options: TweenOptions,
    ) -> Result<Tween, RuntimeError> {
        if !object.borrow().contains_key(property) {
            return Err(RuntimeError::new("options.object must have the specified property."));
        }
        let property = property.to_string();
        options.start_object = BTreeMap::from([("value".into(), start)]);
        options.stop_object = BTreeMap::from([("value".into(), stop)]);
        options.update = Some(Rc::new(move |v| {
            object.borrow_mut().insert(property.clone(), v["value"]);
        }));
        self.add(options)
    }
    pub fn add_alpha(
        &self,
        material: Rc<RefCell<Material>>,
        start: f64,
        stop: f64,
        mut options: TweenOptions,
    ) -> Result<Tween, RuntimeError> {
        let properties: Vec<String> = material
            .borrow()
            .uniforms
            .iter()
            .filter_map(|(n, v)| matches!(v, MaterialValue::Vec4(_)).then_some(n.clone()))
            .collect();
        if properties.is_empty() {
            return Err(RuntimeError::new("material has no properties with alpha components."));
        }
        options.start_object = BTreeMap::from([("alpha".into(), start)]);
        options.stop_object = BTreeMap::from([("alpha".into(), stop)]);
        options.update = Some(Rc::new(move |v| {
            let mut material = material.borrow_mut();
            for name in &properties {
                if let Some(MaterialValue::Vec4(mut c)) = material.uniform(name).cloned() {
                    c.w = v["alpha"];
                    material.set_uniform(name, MaterialValue::Vec4(c)).expect("existing color uniform");
                }
            }
        }));
        self.add(options)
    }
    pub fn add_offset_increment(
        &self,
        material: Rc<RefCell<Material>>,
        mut options: TweenOptions,
    ) -> Result<Tween, RuntimeError> {
        let start = match material.borrow().uniform("offset") {
            Some(MaterialValue::Float(v)) => *v,
            _ => return Err(RuntimeError::new("material.uniforms must have an offset property.")),
        };
        options.start_object = BTreeMap::from([("value".into(), start)]);
        options.stop_object = BTreeMap::from([("value".into(), start + 1.)]);
        options.repeat = f64::INFINITY;
        options.complete = None;
        options.update = Some(Rc::new(move |v| {
            material
                .borrow_mut()
                .set_uniform("offset", MaterialValue::Float(v["value"]))
                .expect("existing offset uniform");
        }));
        self.add(options)
    }
    pub fn remove(&self, tween: &Tween) -> bool {
        let mut list = self.inner.borrow_mut();
        let Some(index) = list.iter().position(|t| Rc::ptr_eq(&t.inner, &tween.inner)) else {
            return false;
        };
        list.remove(index);
        drop(list);
        let callback = {
            let mut t = tween.inner.borrow_mut();
            t.active = false;
            t.options.cancel.clone()
        };
        if let Some(f) = callback {
            f();
        }
        true
    }
    pub fn remove_all(&self) {
        let list = std::mem::take(&mut *self.inner.borrow_mut());
        for tween in list {
            let f = {
                let mut t = tween.inner.borrow_mut();
                t.active = false;
                t.options.cancel.clone()
            };
            if let Some(f) = f {
                f();
            }
        }
    }
    /// Advance all tweens. The first update anchors start time (including delay).
    /// Callbacks run without collection or tween borrows, so they may add/cancel.
    pub fn update(&self, time: f64) {
        let time = time / 0.001;
        let mut index = 0;
        loop {
            let Some(tween) = self.get(index) else {
                break;
            };
            let (update, complete, value, finished) = {
                let mut t = tween.inner.borrow_mut();
                if t.start_time.is_none() {
                    t.start_time = Some(time + t.options.delay / 0.001);
                }
                let start_time = t.start_time.unwrap();
                if time < start_time {
                    drop(t);
                    index += 1;
                    continue;
                }
                let elapsed_time = time - start_time;
                let duration = t.options.duration / 0.001;
                let cycle = duration + t.options.delay / 0.001;
                let total = duration + t.repeat * cycle;
                let elapsed = if elapsed_time > total {
                    1.
                } else {
                    let repeated = (elapsed_time / cycle).trunc();
                    let portion = ((elapsed_time - repeated * cycle) / duration).min(1.);
                    if portion == 0. && elapsed_time == duration {
                        1.
                    } else {
                        portion
                    }
                };
                let fraction = t.options.easing_function.evaluate(elapsed);
                let keys: Vec<_> = t.options.stop_object.iter().map(|(k, v)| (k.clone(), *v)).collect();
                for (key, end) in keys {
                    if let Some(start) = t.options.start_object.get(&key).copied() {
                        t.value.insert(key, start + (end - start) * fraction);
                    }
                }
                let mut finished = elapsed_time >= duration;
                if finished && t.repeat > 0. {
                    let count = (((elapsed_time - duration) / cycle).trunc() + 1.).min(t.repeat);
                    if t.repeat.is_finite() {
                        t.repeat -= count;
                    }
                    t.start_time = Some(start_time + cycle * count);
                    finished = false;
                }
                (
                    t.options.update.clone(),
                    if finished { t.options.complete.clone() } else { None },
                    t.value.clone(),
                    finished,
                )
            };
            if let Some(f) = update {
                f(&value);
            }
            if let Some(f) = complete {
                f();
            }
            if finished {
                tween.inner.borrow_mut().active = false;
                let mut list = self.inner.borrow_mut();
                if let Some(i) = list.iter().position(|t| Rc::ptr_eq(&t.inner, &tween.inner)) {
                    list.remove(i);
                }
            }
            if self.get(index).is_some_and(|t| Rc::ptr_eq(&t.inner, &tween.inner)) {
                index += 1;
            }
        }
    }
}
