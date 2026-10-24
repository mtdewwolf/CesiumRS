//! ResourceLoader's lifecycle and shared, cancellable native load requests.
use super::{FrameState, ResourceLoaderState};
use crate::core::RuntimeError;
use futures_util::future::{AbortHandle, Abortable, FutureExt, LocalBoxFuture, Shared};
use std::{cell::RefCell, rc::Rc};
pub type ResourceLoadFuture = Shared<LocalBoxFuture<'static, Result<bool, RuntimeError>>>;
/// A successful request returns true; destruction during loading returns false,
/// corresponding to the source Promise resolving to undefined.
pub trait ResourceLoader {
    fn cache_key(&self) -> Option<&str>;
    fn load(&self) -> ResourceLoadFuture;
    fn unload(&self);
    fn is_destroyed(&self) -> bool;
    fn destroy(&self);
    fn state(&self) -> ResourceLoaderState;
    fn process(&self, _frame: &FrameState) -> Result<bool, RuntimeError> {
        Ok(false)
    }
    fn get_error(message: &str, error: Option<&RuntimeError>) -> RuntimeError
    where
        Self: Sized,
    {
        loader_error(message, error)
    }
}
pub fn loader_error(message: &str, error: Option<&RuntimeError>) -> RuntimeError {
    RuntimeError::new(match error {
        Some(error) => format!("{message}\n{}", error.message),
        None => message.into(),
    })
}
struct Data<T> {
    value: Option<T>,
    state: ResourceLoaderState,
    request: Option<ResourceLoadFuture>,
    abort: Option<AbortHandle>,
    destroyed: bool,
}
impl<T> Drop for Data<T> {
    fn drop(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
    }
}
pub(crate) struct ManagedLoader<T> {
    pub key: Option<String>,
    inner: Rc<RefCell<Data<T>>>,
}
impl<T: Clone + 'static> ManagedLoader<T> {
    pub fn new(key: Option<String>, value: Option<T>) -> Self {
        Self {
            key,
            inner: Rc::new(RefCell::new(Data {
                value,
                state: ResourceLoaderState::Unloaded,
                request: None,
                abort: None,
                destroyed: false,
            })),
        }
    }
    pub fn value(&self) -> Option<T> {
        self.inner.borrow().value.clone()
    }
    pub fn replace_value(&self, value: T) {
        let mut inner = self.inner.borrow_mut();
        if !inner.destroyed {
            inner.value = Some(value);
        }
    }
    pub fn state(&self) -> ResourceLoaderState {
        self.inner.borrow().state
    }
    pub fn is_destroyed(&self) -> bool {
        self.inner.borrow().destroyed
    }
    pub fn unload(&self) {
        self.inner.borrow_mut().value = None;
    }
    pub fn destroy(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.destroyed = true;
        inner.value = None;
        if let Some(abort) = inner.abort.take() {
            abort.abort();
        }
        inner.request = None;
    }
    pub fn set_state(&self, state: ResourceLoaderState) {
        self.inner.borrow_mut().state = state;
    }
    pub fn load(
        &self,
        error_message: String,
        create: impl FnOnce() -> LocalBoxFuture<'static, Result<T, RuntimeError>>,
    ) -> ResourceLoadFuture {
        self.load_with_state(error_message, ResourceLoaderState::Ready, create)
    }
    pub fn load_with_state(
        &self,
        error_message: String,
        success_state: ResourceLoaderState,
        create: impl FnOnce() -> LocalBoxFuture<'static, Result<T, RuntimeError>>,
    ) -> ResourceLoadFuture {
        let mut inner = self.inner.borrow_mut();
        if inner.destroyed {
            return futures_util::future::ready(Err(RuntimeError::new("Resource loader was destroyed")))
                .boxed_local()
                .shared();
        }
        if let Some(request) = &inner.request {
            return request.clone();
        }
        // Embedded BufferLoader/MetadataSchemaLoader preserve UNLOADED even
        // after their already available data's Promise resolves upstream.
        if inner.value.is_some() {
            let request = futures_util::future::ready(Ok(true)).boxed_local().shared();
            inner.request = Some(request.clone());
            return request;
        }
        inner.state = ResourceLoaderState::Loading;
        let (abort, registration) = AbortHandle::new_pair();
        inner.abort = Some(abort);
        let weak = Rc::downgrade(&self.inner);
        let work = create();
        let request = async move {
            let result = Abortable::new(work, registration).await;
            let Some(inner) = weak.upgrade() else {
                return Ok(false);
            };
            let mut inner = inner.borrow_mut();
            inner.abort = None;
            if inner.destroyed {
                return Ok(false);
            }
            match result {
                Ok(Ok(value)) => {
                    inner.value = Some(value);
                    inner.state = success_state;
                    Ok(true)
                }
                Ok(Err(error)) => {
                    inner.value = None;
                    inner.state = ResourceLoaderState::Failed;
                    Err(loader_error(&error_message, Some(&error)))
                }
                Err(_) => Ok(false),
            }
        }
        .boxed_local()
        .shared();
        inner.request = Some(request.clone());
        request
    }
}

/// ResourceClient's scheduler owns running transport futures. Mark its request
/// cancelled if a loader drops the consumer before it completes; the host's
/// next scheduler update releases the active slot and invokes transport.cancel.
pub(crate) fn guard_request<T: 'static>(
    resource: &crate::core::Resource,
    future: crate::core::ResourceFuture<T>,
) -> crate::core::ResourceFuture<T> {
    struct Cancel {
        request: crate::core::request::RequestHandle<crate::core::HttpResponse>,
        pending: bool,
    }
    impl Drop for Cancel {
        fn drop(&mut self) {
            if self.pending {
                self.request.borrow_mut().cancel();
            }
        }
    }
    let guard = Cancel { request: resource.request.clone(), pending: true };
    async move {
        let mut guard = guard;
        let result = future.await;
        guard.pending = false;
        result
    }
    .boxed_local()
}
