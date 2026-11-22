//! CesiumJS `TimeDynamicImagery`: time-dependent tile dimensions and the
//! interval cache used by WMS and WMTS providers.
//!
//! Cesium subscribes to `Clock.onTick`. A Rust clock owns its listeners, so
//! this port keeps the same cache/interval machinery but is advanced
//! explicitly: providers call [`TimeDynamicImagery::update`] with the clock's
//! current time before serving a request. The observable semantics match
//! Cesium's `_clockOnTick`: an interval change drops the previous interval's
//! cache, clears preload candidates and signals a reload; an approaching
//! interval preloads recently requested tiles.

use super::{ImageryFuture, ImageryProviderError};
use crate::core::{event::ListenerId, Clock, ClockTick, JulianDate, TimeInterval, TimeIntervalCollection};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context as TaskContext, Poll, Waker},
};

/// A tile request that may be preloaded for the approaching interval.
#[derive(Clone)]
pub struct TimeDynamicTileRequest {
    pub x: i32,
    pub y: i32,
    pub level: u32,
    /// Camera-distance priority supplied by the layer. Kept for parity and
    /// for hosts that schedule the preload themselves.
    pub priority: Option<Rc<dyn Fn() -> f64>>,
}

impl std::fmt::Debug for TimeDynamicTileRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeDynamicTileRequest")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

struct CachedTile {
    future: ImageryFuture,
    request: Option<ImageryProviderError>,
}

/// Requests one tile for `interval`'s dynamic data. `None` means the request
/// scheduler was at capacity.
pub type RequestImageFunction<T> = Rc<dyn Fn(i32, i32, u32, Option<&TimeInterval<T>>) -> Option<ImageryFuture>>;

/// Called when the current interval changes or `times` is replaced.
pub type ReloadFunction = Rc<dyn Fn()>;

/// Options for [`TimeDynamicImagery::new`]. `times` and the two callbacks are
/// required, as they are in Cesium.
pub struct TimeDynamicImageryOptions<T> {
    pub times: TimeIntervalCollection<T>,
    pub request_image_function: RequestImageFunction<T>,
    pub reload_function: ReloadFunction,
}

type PendingClockTicks = (Arc<Mutex<Vec<ClockTick>>>, bool);

pub struct TimeDynamicImagery<T> {
    times: TimeIntervalCollection<T>,
    request_image: RequestImageFunction<T>,
    reload: ReloadFunction,
    current_interval_index: Cell<isize>,
    tile_cache: RefCell<HashMap<isize, HashMap<String, CachedTile>>>,
    tiles_requested_for_interval: RefCell<Vec<TimeDynamicTileRequest>>,
    pending_clock_ticks: RefCell<Option<PendingClockTicks>>,
}

impl<T: Clone> TimeDynamicImagery<T> {
    pub fn new(options: TimeDynamicImageryOptions<T>) -> Self {
        Self {
            times: options.times,
            request_image: options.request_image_function,
            reload: options.reload_function,
            current_interval_index: Cell::new(-1),
            tile_cache: RefCell::new(HashMap::new()),
            tiles_requested_for_interval: RefCell::new(Vec::new()),
            pending_clock_ticks: RefCell::new(None),
        }
    }

    pub fn times(&self) -> &TimeIntervalCollection<T> {
        &self.times
    }

    /// Replaces the collection, as Cesium's `times` setter does.
    pub fn set_times(&mut self, times: TimeIntervalCollection<T>) {
        self.times = times;
        self.reload.clone()();
    }

    /// Cesium's `currentInterval`: the interval containing the last update.
    pub fn current_interval(&self) -> Option<&TimeInterval<T>> {
        self.sync_clock();
        usize::try_from(self.current_interval_index.get()).ok().and_then(|index| self.times.get(index))
    }

    /// The current interval index; `-1` before the first update.
    pub fn current_interval_index(&self) -> isize {
        self.sync_clock();
        self.current_interval_index.get()
    }

    /// Advances to the interval containing `time` and preloads if the next
    /// interval is within five wall-clock seconds. Returns true when the
    /// interval changed (and the reload callback ran).
    ///
    /// `can_animate`, `should_animate` and `multiplier` come from the clock.
    pub fn update(&mut self, time: &JulianDate, can_animate: bool, should_animate: bool, multiplier: f64) -> bool {
        self.sync_clock();
        self.update_inner(time, can_animate, should_animate, multiplier)
    }

    fn update_inner(&self, time: &JulianDate, can_animate: bool, should_animate: bool, multiplier: f64) -> bool {
        let index = self.times.index_of(time);
        let prev = self.current_interval_index.get();
        if index != prev {
            self.tile_cache.borrow_mut().remove(&prev);
            self.tiles_requested_for_interval.borrow_mut().clear();
            self.current_interval_index.set(index);
            self.reload.clone()();
            return true;
        }

        if let Some((interval_index, interval)) =
            self.approaching_interval(time, can_animate, should_animate, multiplier)
        {
            let mut requested = self.tiles_requested_for_interval.borrow_mut();
            while let Some(tile) = requested.pop() {
                if !self.add_to_cache(&tile, interval_index, &interval) {
                    requested.push(tile);
                    break;
                }
            }
        }
        false
    }

    /// The interval within five wall-clock seconds in the direction of the
    /// clock multiplier, if any.
    fn approaching_interval(
        &self,
        time: &JulianDate,
        can_animate: bool,
        should_animate: bool,
        multiplier: f64,
    ) -> Option<(isize, TimeInterval<T>)> {
        let is_animating = can_animate && should_animate;
        if !is_animating && multiplier != 0.0 {
            return None;
        }
        let mut index = self.times.index_of(time);
        if index < 0 {
            return None;
        }
        let interval = self.times.get(index as usize)?;
        let seconds = if multiplier > 0.0 {
            let seconds = JulianDate::seconds_difference(&interval.stop, time);
            index += 1;
            seconds
        } else {
            let seconds = JulianDate::seconds_difference(&interval.start, time);
            index -= 1;
            seconds
        };
        // JavaScript division by zero yields ±Infinity; Rust f64 does too.
        let seconds = seconds / multiplier;
        if index >= 0 && seconds <= 5.0 {
            let interval = self.times.get(index as usize)?.clone();
            Some((index, interval))
        } else {
            None
        }
    }

    /// Returns a cached tile and removes it from the cache, as Cesium does.
    pub fn get_from_cache(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        self.sync_clock();
        let key = tile_key(x, y, level);
        let mut cache = self.tile_cache.borrow_mut();
        let current = cache.get_mut(&self.current_interval_index.get())?;
        let item = current.remove(&key)?;
        let _ = item.request;
        Some(item.future)
    }

    /// Records or immediately preloads a requested tile for the approaching
    /// interval; Cesium caps the list at 512 entries by dropping the oldest
    /// 256.
    #[allow(clippy::too_many_arguments)]
    pub fn check_approaching_interval(
        &mut self,
        x: i32,
        y: i32,
        level: u32,
        time: &JulianDate,
        can_animate: bool,
        should_animate: bool,
        multiplier: f64,
        priority: Option<Rc<dyn Fn() -> f64>>,
    ) {
        let tile = TimeDynamicTileRequest { x, y, level, priority };
        let approaching = self.approaching_interval(time, can_animate, should_animate, multiplier);
        if let Some((index, interval)) = approaching {
            if self.add_to_cache(&tile, index, &interval) {
                return;
            }
        }
        let mut requested = self.tiles_requested_for_interval.borrow_mut();
        requested.push(tile);
        if requested.len() >= 512 {
            requested.drain(0..256);
        }
    }

    fn add_to_cache(&self, tile: &TimeDynamicTileRequest, index: isize, interval: &TimeInterval<T>) -> bool {
        let mut caches = self.tile_cache.borrow_mut();
        let cache = caches.entry(index).or_default();
        let key = tile_key(tile.x, tile.y, tile.level);
        if cache.contains_key(&key) {
            return true;
        }
        let Some(future) = (self.request_image)(tile.x, tile.y, tile.level, Some(interval)) else {
            return false;
        };
        cache.insert(key, CachedTile { future, request: None });
        true
    }

    /// Number of cached intervals. Exposed for lifecycle checks.
    pub fn cached_interval_count(&self) -> usize {
        self.tile_cache.borrow().len()
    }

    /// Number of pending preload candidates.
    pub fn pending_preload_count(&self) -> usize {
        self.tiles_requested_for_interval.borrow().len()
    }

    /// Updates directly from a [`Clock`]'s current time and animation state.
    pub fn update_from_clock(&mut self, clock: &Clock) -> bool {
        self.update(&clock.current_time(), clock.can_animate, clock.should_animate(), clock.multiplier())
    }

    /// Subscribes to `clock.on_tick_state` so interval transitions and preloading
    /// track every `clock.tick()`, mirroring CesiumJS's `clock.onTick` listener.
    pub fn subscribe_to_clock(&self, clock: &mut Clock) -> ListenerId {
        let can_animate = clock.can_animate;
        let _ = self.update_inner(&clock.current_time(), can_animate, clock.should_animate(), clock.multiplier());
        let queue = Arc::new(Mutex::new(Vec::<ClockTick>::new()));
        *self.pending_clock_ticks.borrow_mut() = Some((Arc::clone(&queue), can_animate));
        clock.on_tick_state.add_event_listener(move |tick| {
            queue.lock().unwrap_or_else(|e| e.into_inner()).push(*tick);
        })
    }

    /// Applies any clock ticks recorded since the last check. Returns true if
    /// any tick caused an interval transition.
    pub fn sync_clock(&self) -> bool {
        let ticks_and_can = {
            let guard = self.pending_clock_ticks.borrow();
            guard.as_ref().map(|(q, can)| {
                let mut locked = q.lock().unwrap_or_else(|e| e.into_inner());
                (std::mem::take(&mut *locked), *can)
            })
        };
        let Some((ticks, can_animate)) = ticks_and_can else {
            return false;
        };
        let mut reloaded = false;
        for tick in ticks {
            if self.update_inner(&tick.current_time, can_animate, tick.should_animate, tick.multiplier) {
                reloaded = true;
            }
        }
        reloaded
    }

    /// Polls all currently cached preload futures once so eager/ready futures
    /// advance and remain cached as ready results. Returns how many finished.
    pub fn poll_cached_futures(&self) -> usize {
        self.sync_clock();
        let mut ready_count = 0usize;
        let waker = Waker::noop();
        let mut cx = TaskContext::from_waker(waker);
        let mut caches = self.tile_cache.borrow_mut();
        for cache in caches.values_mut() {
            for cached in cache.values_mut() {
                let mut fut = std::mem::replace(&mut cached.future, Box::pin(std::future::pending()));
                match fut.as_mut().poll(&mut cx) {
                    Poll::Ready(result) => {
                        ready_count += 1;
                        cached.future = Box::pin(std::future::ready(result));
                    }
                    Poll::Pending => {
                        cached.future = fut;
                    }
                }
            }
        }
        ready_count
    }
}

fn tile_key(x: i32, y: i32, level: u32) -> String {
    format!("{x}-{y}-{level}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(seconds: f64) -> JulianDate {
        let mut d = JulianDate::default();
        d = d.add_seconds(seconds);
        d
    }

    fn collection() -> TimeIntervalCollection<&'static str> {
        let mut times = TimeIntervalCollection::new();
        times.add_interval(TimeInterval::new(date(0.0), date(10.0), true, false, Some("a")));
        times.add_interval(TimeInterval::new(date(10.0), date(20.0), true, false, Some("b")));
        times
    }

    #[test]
    fn interval_change_clears_cache_and_reloads() {
        let reloads = Rc::new(RefCell::new(0));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let r = reloads.clone();
        let q = requests.clone();
        let mut dynamic = TimeDynamicImagery::new(TimeDynamicImageryOptions {
            times: collection(),
            request_image_function: Rc::new(move |x, y, level, interval| {
                q.borrow_mut().push((x, y, level, interval.and_then(|i| i.data)));
                Some(Box::pin(std::future::ready(Err(crate::core::RuntimeError::new("test")))))
            }),
            reload_function: Rc::new(move || *r.borrow_mut() += 1),
        });
        assert_eq!(dynamic.current_interval_index(), -1);
        assert!(dynamic.update(&date(5.0), false, false, 0.0));
        assert_eq!(dynamic.current_interval_index(), 0);
        assert_eq!(*reloads.borrow(), 1);
        dynamic.check_approaching_interval(1, 2, 3, &date(2.0), true, true, 1.0, None);
        assert_eq!(dynamic.pending_preload_count(), 1);
        assert!(dynamic.update(&date(15.0), false, false, 0.0));
        assert_eq!(dynamic.current_interval_index(), 1);
        assert_eq!(dynamic.pending_preload_count(), 0);
        assert_eq!(*reloads.borrow(), 2);
    }

    #[test]
    fn set_times_triggers_reload() {
        let reloads = Rc::new(RefCell::new(0));
        let r = reloads.clone();
        let mut dynamic = TimeDynamicImagery::new(TimeDynamicImageryOptions {
            times: collection(),
            request_image_function: Rc::new(|_, _, _, _| None),
            reload_function: Rc::new(move || *r.borrow_mut() += 1),
        });
        dynamic.set_times(collection());
        assert_eq!(*reloads.borrow(), 1);
    }

    #[test]
    fn approaching_interval_preloads() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let q = requests.clone();
        let mut dynamic = TimeDynamicImagery::new(TimeDynamicImageryOptions {
            times: collection(),
            request_image_function: Rc::new(move |x, y, level, interval| {
                q.borrow_mut().push((x, y, level, interval.and_then(|i| i.data)));
                Some(Box::pin(std::future::ready(Err(crate::core::RuntimeError::new("test")))))
            }),
            reload_function: Rc::new(|| {}),
        });
        dynamic.update(&date(8.0), true, true, 1.0);
        dynamic.check_approaching_interval(4, 5, 6, &date(8.0), true, true, 1.0, None);
        assert_eq!(dynamic.pending_preload_count(), 0);
        assert_eq!(requests.borrow().len(), 1);
        assert_eq!(requests.borrow()[0], (4, 5, 6, Some("b")));
    }
}
