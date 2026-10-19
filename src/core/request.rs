//! CesiumJS `Request`. Cloning resets execution state and shares callbacks.

use super::{request_error_event::RequestErrorEvent, RequestState, RequestType};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

pub type RequestFuture<T> = Pin<Box<dyn Future<Output = Result<T, RequestErrorEvent>>>>;
pub type RequestCallback<T> = Rc<dyn Fn() -> RequestFuture<T>>;
pub type RequestHandle<T> = Rc<RefCell<Request<T>>>;

#[derive(Clone)]
pub struct RequestOptions<T> {
    pub url: String,
    pub request_function: Option<RequestCallback<T>>,
    pub cancel_function: Option<Rc<dyn Fn()>>,
    pub priority_function: Option<Rc<dyn Fn() -> f64>>,
    pub priority: f64,
    pub throttle: bool,
    pub throttle_by_server: bool,
    pub request_type: RequestType,
    pub server_key: Option<String>,
}

impl<T> Default for RequestOptions<T> {
    fn default() -> Self {
        Self {
            url: String::new(),
            request_function: None,
            cancel_function: None,
            priority_function: None,
            priority: 0.0,
            throttle: false,
            throttle_by_server: false,
            request_type: RequestType::Other,
            server_key: None,
        }
    }
}

pub struct Request<T> {
    pub options: RequestOptions<T>,
    pub state: RequestState,
    pub cancelled: bool,
    pub(crate) deferred: Option<Rc<RefCell<Deferred<T>>>>,
}

impl<T> Clone for Request<T> {
    fn clone(&self) -> Self {
        let o = &self.options;
        Self::new(RequestOptions {
            url: o.url.clone(),
            request_function: o.request_function.clone(),
            cancel_function: o.cancel_function.clone(),
            priority_function: o.priority_function.clone(),
            priority: o.priority,
            throttle: o.throttle,
            throttle_by_server: o.throttle_by_server,
            request_type: o.request_type,
            server_key: o.server_key.clone(),
        })
    }
}

impl<T> Request<T> {
    pub fn new(options: RequestOptions<T>) -> Self {
        Self { options, state: RequestState::Unissued, cancelled: false, deferred: None }
    }
    pub fn into_handle(self) -> RequestHandle<T> {
        Rc::new(RefCell::new(self))
    }
    /// Cancellation is acted upon by the next scheduler update.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

pub(crate) struct Deferred<T> {
    result: Option<Result<T, RequestErrorEvent>>,
    waker: Option<Waker>,
}

impl<T> Deferred<T> {
    pub(crate) fn complete(&mut self, result: Result<T, RequestErrorEvent>) {
        self.result = Some(result);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

/// The response future. Drive the scheduler with `poll_update` to execute I/O.
pub struct ScheduledRequest<T> {
    pub(crate) deferred: Rc<RefCell<Deferred<T>>>,
}

impl<T> ScheduledRequest<T> {
    pub(crate) fn issue(request: &RequestHandle<T>) -> Self {
        let mut r = request.borrow_mut();
        if r.deferred.is_none() {
            r.state = RequestState::Issued;
            r.deferred = Some(Rc::new(RefCell::new(Deferred { result: None, waker: None })));
        }
        Self { deferred: r.deferred.as_ref().unwrap().clone() }
    }
}

impl<T> Future for ScheduledRequest<T> {
    type Output = Result<T, RequestErrorEvent>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut deferred = self.deferred.borrow_mut();
        if let Some(result) = deferred.result.take() {
            Poll::Ready(result)
        } else {
            deferred.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
