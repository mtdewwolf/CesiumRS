//! CesiumJS request scheduling with explicit per-instance state.
//!
//! Call `poll_update` from the host event loop with its task context. Network
//! callbacks supply futures, so native and WebAssembly hosts choose their I/O
//! runtime. Dropping a running future aborts its work where the host supports it.

use super::request::{RequestFuture, RequestHandle, ScheduledRequest};
use super::request_error_event::RequestErrorEvent;
use super::{uri::Uri, Event, Heap, RequestState};
use std::{
    collections::HashMap,
    rc::Rc,
    task::{Context, Poll},
};

#[derive(Clone, Debug, Default)]
pub struct RequestStatistics {
    pub number_of_attempted_requests: usize,
    pub number_of_active_requests: usize,
    pub number_of_cancelled_requests: usize,
    pub number_of_cancelled_active_requests: usize,
    pub number_of_failed_requests: usize,
    pub number_of_active_requests_ever: usize,
    pub last_number_of_active_requests: usize,
}

type RequestOrdering<T> = fn(&RequestHandle<T>, &RequestHandle<T>) -> std::cmp::Ordering;

fn compare<T>(a: &RequestHandle<T>, b: &RequestHandle<T>) -> std::cmp::Ordering {
    a.borrow().options.priority.partial_cmp(&b.borrow().options.priority).unwrap_or(std::cmp::Ordering::Equal)
}

struct Active<T> {
    request: RequestHandle<T>,
    future: Option<RequestFuture<T>>,
    local: bool,
}

pub struct RequestScheduler<T> {
    pub maximum_requests: usize,
    pub maximum_requests_per_server: usize,
    pub requests_by_server: HashMap<String, usize>,
    pub throttle_requests: bool,
    pub page_url: String,
    pub debug_show_statistics: bool,
    // Cesium's statistics callback. Naming this in the public API would add a type
    // that no caller mentions; the field stays the function trait object directly.
    #[allow(clippy::type_complexity)]
    pub statistics_log: Option<Box<dyn FnMut(&str)>>,
    pub statistics: RequestStatistics,
    pub request_completed_event: Event<Option<RequestErrorEvent>>,
    heap: Heap<RequestHandle<T>, RequestOrdering<T>>,
    active: Vec<Active<T>>,
    by_server: HashMap<String, usize>,
    heap_length: usize,
}

impl<T> Default for RequestScheduler<T> {
    fn default() -> Self {
        let mut heap = Heap::new(compare::<T> as RequestOrdering<T>);
        heap.set_maximum_length(Some(20));
        Self {
            maximum_requests: 50,
            maximum_requests_per_server: 18,
            requests_by_server: HashMap::new(),
            throttle_requests: true,
            page_url: String::new(),
            debug_show_statistics: false,
            statistics_log: Some(Box::new(default_statistics_log)),
            statistics: RequestStatistics::default(),
            request_completed_event: Event::new(),
            heap,
            active: Vec::new(),
            by_server: HashMap::new(),
            heap_length: 20,
        }
    }
}

impl<T> RequestScheduler<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn priority_heap_length(&self) -> usize {
        self.heap_length
    }
    pub fn set_priority_heap_length(&mut self, length: usize) {
        while self.heap.len() > length {
            let Some(r) = self.heap.pop(0) else { break };
            self.cancel(&r, false);
        }
        self.heap_length = length;
        self.heap.set_maximum_length(Some(length));
        self.heap.reserve(length);
    }

    pub fn get_server_key(&mut self, url: &str) -> String {
        let mut uri = Uri::parse(url);
        if uri.scheme().is_empty() {
            uri = uri.absolute_to(&self.page_url).unwrap_or(uri);
            uri.normalize();
        }
        let mut key = authority(&uri);
        if !key.contains(':') {
            key.push_str(if uri.scheme() == "https" { ":443" } else { ":80" });
        }
        self.by_server.entry(key.clone()).or_insert(0);
        key
    }

    pub fn number_of_active_requests_by_server(&self, key: &str) -> Option<usize> {
        self.by_server.get(key).copied()
    }
    pub fn server_has_open_slots(&self, key: &str, desired: usize) -> bool {
        self.by_server.get(key).is_some_and(|active| {
            active.saturating_add(desired)
                <= *self.requests_by_server.get(key).unwrap_or(&self.maximum_requests_per_server)
        })
    }
    pub fn heap_has_open_slots(&self, desired: usize) -> bool {
        self.heap.len().saturating_add(desired) <= self.heap_length
    }

    fn update_priority(r: &RequestHandle<T>) {
        let callback = r.borrow().options.priority_function.clone();
        if let Some(callback) = callback {
            let priority = callback();
            r.borrow_mut().options.priority = priority;
        }
    }

    fn start(&mut self, r: RequestHandle<T>, local: bool) -> Option<ScheduledRequest<T>> {
        // Both fields are public, so a caller can clear them after `request` queues the handle.
        let callback = r.borrow().options.request_function.clone()?;
        if !local && r.borrow().options.server_key.is_none() {
            let url = r.borrow().options.url.clone();
            let key = self.get_server_key(&url);
            r.borrow_mut().options.server_key = Some(key);
        }
        let server_key = if local { None } else { r.borrow().options.server_key.clone() };
        if !local && server_key.is_none() {
            return None;
        }
        let response = ScheduledRequest::issue(&r);
        r.borrow_mut().state = if local { RequestState::Received } else { RequestState::Active };
        if let Some(key) = server_key {
            self.statistics.number_of_active_requests += 1;
            self.statistics.number_of_active_requests_ever += 1;
            *self.by_server.entry(key).or_default() += 1;
        }
        self.active.push(Active { request: r, future: Some(callback()), local });
        Some(response)
    }

    fn release_slot(&mut self, r: &RequestHandle<T>) {
        self.statistics.number_of_active_requests = self.statistics.number_of_active_requests.saturating_sub(1);
        if let Some(key) = &r.borrow().options.server_key {
            if let Some(count) = self.by_server.get_mut(key) {
                *count = count.saturating_sub(1);
            }
        }
    }

    fn cancel(&mut self, r: &RequestHandle<T>, local: bool) {
        let active = r.borrow().state == RequestState::Active && !local;
        if r.borrow().state == RequestState::Cancelled {
            return;
        }
        r.borrow_mut().state = RequestState::Cancelled;
        self.statistics.number_of_cancelled_requests += 1;
        if active {
            self.release_slot(r);
            self.statistics.number_of_cancelled_active_requests += 1;
        }
        let deferred = r.borrow_mut().deferred.take();
        if let Some(d) = deferred {
            d.borrow_mut().complete(Err(RequestErrorEvent::new(None, None, None)));
        }
        let callback = r.borrow().options.cancel_function.clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    /// `None` means the caller should retry scheduling on a later frame.
    pub fn request(&mut self, r: RequestHandle<T>) -> Option<ScheduledRequest<T>> {
        if r.borrow().state != RequestState::Unissued || r.borrow().options.request_function.is_none() {
            return None;
        }
        let url = r.borrow().options.url.clone();
        if url.to_ascii_lowercase().starts_with("data:") || url.to_ascii_lowercase().starts_with("blob:") {
            self.request_completed_event.raise_event(&None);
            return self.start(r, true);
        }
        self.statistics.number_of_attempted_requests += 1;
        let existing_key = r.borrow().options.server_key.clone();
        let key = if let Some(key) = existing_key {
            key
        } else {
            let key = self.get_server_key(&url);
            r.borrow_mut().options.server_key = Some(key.clone());
            key
        };
        if self.throttle_requests && r.borrow().options.throttle_by_server && !self.server_has_open_slots(&key, 1) {
            return None;
        }
        if !self.throttle_requests || !r.borrow().options.throttle {
            return self.start(r, false);
        }
        if self.active.iter().filter(|a| !a.local).count() >= self.maximum_requests {
            return None;
        }
        Self::update_priority(&r);
        if let Some(removed) = self.heap.insert(r.clone()) {
            if Rc::ptr_eq(&removed, &r) {
                return None;
            }
            self.cancel(&removed, false);
        }
        Some(ScheduledRequest::issue(&r))
    }

    /// Polls in-flight I/O, processes cancellation, then starts queued work.
    /// The host should call this again each frame and whenever its waker fires.
    pub fn poll_update(&mut self, cx: &mut Context<'_>) {
        let old = std::mem::take(&mut self.active);
        for mut active in old {
            let r = &active.request;
            if r.borrow().cancelled {
                self.cancel(r, active.local);
            }
            if r.borrow().state == RequestState::Cancelled {
                continue;
            }
            let Some(future) = &mut active.future else {
                continue;
            };
            match future.as_mut().poll(cx) {
                Poll::Pending => self.active.push(active),
                Poll::Ready(result) => {
                    if !active.local {
                        self.release_slot(r);
                        match &result {
                            Ok(_) => self.request_completed_event.raise_event(&None),
                            Err(error) => {
                                self.statistics.number_of_failed_requests += 1;
                                self.request_completed_event.raise_event(&Some(error.clone()));
                            }
                        }
                    }
                    if !active.local {
                        r.borrow_mut().state =
                            if result.is_ok() { RequestState::Received } else { RequestState::Failed };
                    }
                    let deferred = r.borrow_mut().deferred.take();
                    if let Some(d) = deferred {
                        d.borrow_mut().complete(result);
                    }
                }
            }
        }
        for r in self.heap.internal_array() {
            Self::update_priority(r);
        }
        self.heap.resort();
        let slots = self.maximum_requests.saturating_sub(self.active.iter().filter(|a| !a.local).count());
        let mut filled = 0;
        while filled < slots {
            let Some(r) = self.heap.pop(0) else { break };
            let Some(key) = r.borrow().options.server_key.clone() else {
                self.cancel(&r, false);
                continue;
            };
            if r.borrow().cancelled || (r.borrow().options.throttle_by_server && !self.server_has_open_slots(&key, 1)) {
                self.cancel(&r, false);
                continue;
            }
            if self.start(Rc::clone(&r), false).is_some() {
                filled += 1;
            } else {
                self.cancel(&r, false);
            }
        }
        self.update_statistics();
        if filled > 0 {
            cx.waker().wake_by_ref();
        }
    }

    fn update_statistics(&mut self) {
        if !self.debug_show_statistics {
            return;
        }
        let s = &mut self.statistics;
        if s.number_of_active_requests == 0 && s.last_number_of_active_requests > 0 {
            for (name, count) in [
                ("Number of attempted requests", &mut s.number_of_attempted_requests),
                ("Number of cancelled requests", &mut s.number_of_cancelled_requests),
                ("Number of cancelled active requests", &mut s.number_of_cancelled_active_requests),
                ("Number of failed requests", &mut s.number_of_failed_requests),
            ] {
                if *count > 0 {
                    if let Some(log) = &mut self.statistics_log {
                        log(&format!("{name}: {count}"));
                    }
                    *count = 0;
                }
            }
        }
        s.last_number_of_active_requests = s.number_of_active_requests;
    }

    pub fn clear(&mut self) {
        while let Some(r) = self.heap.pop(0) {
            self.cancel(&r, false);
        }
        for a in std::mem::take(&mut self.active) {
            self.cancel(&a.request, a.local);
        }
        self.by_server.clear();
        self.statistics = RequestStatistics::default();
    }
}

pub(crate) fn authority(uri: &Uri) -> String {
    let mut result = String::new();
    if let Some(user) = &uri.username {
        result.push_str(&super::uri::encode_uri_component(user));
        if let Some(password) = &uri.password {
            result.push(':');
            result.push_str(&super::uri::encode_uri_component(password));
        }
        result.push('@');
    }
    if let Some(host) = &uri.hostname {
        if host.contains(':') {
            result.push('[');
            result.push_str(host);
            result.push(']');
        } else {
            result.push_str(host);
        }
        if let Some(port) = &uri.port {
            result.push(':');
            result.push_str(port);
        }
    }
    result
}

fn default_statistics_log(message: &str) {
    #[cfg(all(feature = "browser", target_arch = "wasm32"))]
    {
        use wasm_bindgen::JsCast;
        if let Ok(console) = js_sys::Reflect::get(&js_sys::global(), &"console".into()) {
            if let Ok(log) =
                js_sys::Reflect::get(&console, &"log".into()).and_then(|value| value.dyn_into::<js_sys::Function>())
            {
                let _ = log.call1(&console, &message.into());
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    println!("{message}");
    #[cfg(all(target_arch = "wasm32", not(feature = "browser")))]
    let _ = message; // raw C-ABI hosts supply statistics_log explicitly.
}

#[cfg(test)]
mod tests {
    use super::RequestScheduler;
    use crate::core::{Request, RequestOptions, RequestState};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::task::{Context, Waker};

    fn throttled(url: &str, started: &Rc<RefCell<usize>>) -> crate::core::request::RequestHandle<u8> {
        let started = started.clone();
        Request::new(RequestOptions {
            url: url.into(),
            throttle: true,
            request_function: Some(Rc::new(move || {
                *started.borrow_mut() += 1;
                Box::pin(async { Ok(1) })
            })),
            ..Default::default()
        })
        .into_handle()
    }

    #[test]
    fn shrinking_the_heap_cancels_without_panicking() {
        let mut scheduler = RequestScheduler::<u8>::new();
        scheduler.set_priority_heap_length(4);
        let started = Rc::new(RefCell::new(0));
        let mut queued = Vec::new();
        for i in 0..3 {
            let request = throttled(&format!("https://example.test/{i}"), &started);
            assert!(scheduler.request(request.clone()).is_some());
            queued.push(request);
        }
        scheduler.set_priority_heap_length(0);
        assert!(queued.iter().all(|r| r.borrow().state == RequestState::Cancelled));
        assert_eq!(*started.borrow(), 0);
        assert_eq!(scheduler.statistics.number_of_cancelled_requests, 3);
    }

    #[test]
    fn cleared_server_key_or_callback_is_cancelled() {
        let mut scheduler = RequestScheduler::<u8>::new();
        let started = Rc::new(RefCell::new(0));
        let missing_key = throttled("https://example.test/key", &started);
        assert!(scheduler.request(missing_key.clone()).is_some());
        missing_key.borrow_mut().options.server_key = None;
        let missing_callback = throttled("https://example.test/callback", &started);
        assert!(scheduler.request(missing_callback.clone()).is_some());
        missing_callback.borrow_mut().options.request_function = None;
        let mut cx = Context::from_waker(Waker::noop());
        scheduler.poll_update(&mut cx);
        assert_eq!(missing_key.borrow().state, RequestState::Cancelled);
        assert_eq!(missing_callback.borrow().state, RequestState::Cancelled);
        assert_eq!(*started.borrow(), 0);
    }

    #[test]
    fn a_queued_request_still_starts() {
        let mut scheduler = RequestScheduler::<u8>::new();
        let started = Rc::new(RefCell::new(0));
        let request = throttled("https://example.test/ok", &started);
        assert!(scheduler.request(request.clone()).is_some());
        let mut cx = Context::from_waker(Waker::noop());
        scheduler.poll_update(&mut cx);
        assert_eq!(request.borrow().state, RequestState::Active);
        assert_eq!(*started.borrow(), 1);
    }
}
