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
}

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
    pub statistics: RequestStatistics,
    pub request_completed_event: Event<Option<RequestErrorEvent>>,
    heap: Heap<RequestHandle<T>, fn(&RequestHandle<T>, &RequestHandle<T>) -> std::cmp::Ordering>,
    active: Vec<Active<T>>,
    by_server: HashMap<String, usize>,
    heap_length: usize,
}

impl<T> Default for RequestScheduler<T> {
    fn default() -> Self {
        let mut heap = Heap::new(compare::<T> as fn(&RequestHandle<T>, &RequestHandle<T>) -> std::cmp::Ordering);
        heap.set_maximum_length(Some(20));
        Self {
            maximum_requests: 50,
            maximum_requests_per_server: 18,
            requests_by_server: HashMap::new(),
            throttle_requests: true,
            page_url: String::new(),
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
            let r = self.heap.pop(0).unwrap();
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

    fn start(&mut self, r: RequestHandle<T>, local: bool) -> ScheduledRequest<T> {
        let response = ScheduledRequest::issue(&r);
        r.borrow_mut().state = if local { RequestState::Received } else { RequestState::Active };
        if !local {
            self.statistics.number_of_active_requests += 1;
            self.statistics.number_of_active_requests_ever += 1;
            let key = r.borrow().options.server_key.clone().unwrap();
            *self.by_server.entry(key).or_default() += 1;
        }
        let callback = r.borrow().options.request_function.clone().unwrap();
        self.active.push(Active { request: r, future: Some(callback()), local });
        response
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
            return Some(self.start(r, true));
        }
        self.statistics.number_of_attempted_requests += 1;
        if r.borrow().options.server_key.is_none() {
            let key = self.get_server_key(&url);
            r.borrow_mut().options.server_key = Some(key);
        }
        let key = r.borrow().options.server_key.clone().unwrap();
        if self.throttle_requests && r.borrow().options.throttle_by_server && !self.server_has_open_slots(&key, 1) {
            return None;
        }
        if !self.throttle_requests || !r.borrow().options.throttle {
            return Some(self.start(r, false));
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
                    r.borrow_mut().state = if result.is_ok() { RequestState::Received } else { RequestState::Failed };
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
        while filled < slots && !self.heap.is_empty() {
            let r = self.heap.pop(0).unwrap();
            let key = r.borrow().options.server_key.clone().unwrap();
            if r.borrow().cancelled || (r.borrow().options.throttle_by_server && !self.server_has_open_slots(&key, 1)) {
                self.cancel(&r, false);
                continue;
            }
            self.start(r, false);
            filled += 1;
        }
        if filled > 0 {
            cx.waker().wake_by_ref();
        }
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
