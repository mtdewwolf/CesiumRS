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
use crate::core::{JulianDate, TimeInterval, TimeIntervalCollection};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

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

pub struct TimeDynamicImagery<T> {
    times: TimeIntervalCollection<T>,
    request_image: RequestImageFunction<T>,
    reload: ReloadFunction,
    current_interval_index: isize,
    tile_cache: RefCell<HashMap<isize, HashMap<String, CachedTile>>>,
    tiles_requested_for_interval: RefCell<Vec<TimeDynamicTileRequest>>,
}

impl<T: Clone> TimeDynamicImagery<T> {
    pub fn new(options: TimeDynamicImageryOptions<T>) -> Self {
        Self {
            times: options.times,
            request_image: options.request_image_function,
            reload: options.reload_function,
            current_interval_index: -1,
            tile_cache: RefCell::new(HashMap::new()),
            tiles_requested_for_interval: RefCell::new(Vec::new()),
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
        usize::try_from(self.current_interval_index).ok().and_then(|index| self.times.get(index))
    }

    /// The current interval index; `-1` before the first update.
    pub fn current_interval_index(&self) -> isize {
        self.current_interval_index
    }

    /// Advances to the interval containing `time` and preloads if the next
    /// interval is within five wall-clock seconds. Returns true when the
    /// interval changed (and the reload callback ran).
    ///
    /// `can_animate`, `should_animate` and `multiplier` come from the clock.
    pub fn update(&mut self, time: &JulianDate, can_animate: bool, should_animate: bool, multiplier: f64) -> bool {
        let index = self.times.index_of(time);
        if index != self.current_interval_index {
            self.tile_cache.borrow_mut().remove(&self.current_interval_index);
            self.tiles_requested_for_interval.borrow_mut().clear();
            self.current_interval_index = index;
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
        let key = tile_key(x, y, level);
        let mut cache = self.tile_cache.borrow_mut();
        let current = cache.get_mut(&self.current_interval_index)?;
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
