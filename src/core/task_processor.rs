//! CesiumJS task processing with a host worker instead of browser globals.
//! Task IDs, concurrency limits, completion events and destruction are native.

use super::{Event, RuntimeError};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

pub type TaskFuture<R> = Pin<Box<dyn Future<Output = Result<R, RuntimeError>>>>;

#[derive(Clone, Debug, Default)]
pub struct WebAssemblyOptions {
    pub module_path: Option<String>,
    pub wasm_binary_file: Option<String>,
    pub fallback_module_path: Option<String>,
}

/// Implementations initialize a worker lazily and transfer owned Rust buffers
/// rather than serializing JS transferable-object lists.
pub trait TaskWorker<P, R> {
    fn run(&self, id: u64, parameters: P) -> TaskFuture<R>;
    fn init_web_assembly_module(&self, options: WebAssemblyOptions) -> TaskFuture<serde_json::Value>;
    fn cancel_task(&self, id: u64);
    fn terminate(&self);
}

struct State {
    active: Cell<usize>,
    destroyed: Cell<bool>,
    wakers: RefCell<HashMap<u64, Waker>>,
}

pub struct TaskProcessor<P, R> {
    pub maximum_active_tasks: usize,
    pub task_completed_event: Rc<RefCell<Event<Option<RuntimeError>>>>,
    worker: Rc<dyn TaskWorker<P, R>>,
    state: Rc<State>,
    next_id: u64,
    initialization: Option<ModuleInitialization>,
}

impl<P: 'static, R: 'static> TaskProcessor<P, R> {
    pub fn new(worker: Rc<dyn TaskWorker<P, R>>, maximum_active_tasks: Option<usize>) -> Self {
        Self {
            maximum_active_tasks: maximum_active_tasks.unwrap_or(usize::MAX),
            task_completed_event: Rc::new(RefCell::new(Event::new())),
            worker,
            state: Rc::new(State {
                active: Cell::new(0),
                destroyed: Cell::new(false),
                wakers: RefCell::new(HashMap::new()),
            }),
            next_id: 0,
            initialization: None,
        }
    }
    pub fn active_tasks(&self) -> usize {
        self.state.active.get()
    }
    pub fn is_destroyed(&self) -> bool {
        self.state.destroyed.get()
    }
    pub fn schedule_task(&mut self, parameters: P) -> Option<ProcessedTask<P, R>> {
        if self.is_destroyed() || self.active_tasks() >= self.maximum_active_tasks {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.state.active.set(self.active_tasks() + 1);
        Some(ProcessedTask {
            id,
            future: self.worker.run(id, parameters),
            worker: self.worker.clone(),
            state: self.state.clone(),
            completed: false,
            event: self.task_completed_event.clone(),
        })
    }
    pub fn init_web_assembly_module(&mut self, options: WebAssemblyOptions) -> ModuleInitialization {
        if let Some(initialization) = &self.initialization {
            return initialization.clone();
        }
        let initialization = ModuleInitialization(Rc::new(RefCell::new(InitState {
            future: if self.is_destroyed() { None } else { Some(self.worker.init_web_assembly_module(options)) },
            result: if self.is_destroyed() {
                Some(Err(RuntimeError::new("TaskProcessor was destroyed.")))
            } else {
                None
            },
            wake: Arc::new(InitWake(Mutex::new(Vec::new()))),
        })));
        self.initialization = Some(initialization.clone());
        initialization
    }
    pub fn destroy(&mut self) {
        shutdown(self);
    }
}

fn shutdown<P, R>(processor: &mut TaskProcessor<P, R>) {
    if processor.state.destroyed.replace(true) {
        return;
    }
    processor.worker.terminate();
    let wakers: Vec<_> = processor.state.wakers.borrow_mut().drain().map(|(_, w)| w).collect();
    for waker in wakers {
        waker.wake();
    }
    if let Some(initialization) = &processor.initialization {
        initialization.reject_destroyed();
    }
}
impl<P, R> Drop for TaskProcessor<P, R> {
    fn drop(&mut self) {
        shutdown(self);
    }
}

pub struct ProcessedTask<P, R> {
    id: u64,
    future: TaskFuture<R>,
    worker: Rc<dyn TaskWorker<P, R>>,
    state: Rc<State>,
    completed: bool,
    event: Rc<RefCell<Event<Option<RuntimeError>>>>,
}

impl<P, R> Unpin for ProcessedTask<P, R> {}
impl<P, R> Future for ProcessedTask<P, R> {
    type Output = Result<R, RuntimeError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let task = self.get_mut();
        assert!(!task.completed, "completed task polled again");
        task.state.wakers.borrow_mut().insert(task.id, cx.waker().clone());
        let result = if task.state.destroyed.get() {
            Poll::Ready(Err(RuntimeError::new("TaskProcessor was destroyed.")))
        } else {
            task.future.as_mut().poll(cx)
        };
        if let Poll::Ready(result) = &result {
            task.completed = true;
            task.state.active.set(task.state.active.get().saturating_sub(1));
            task.state.wakers.borrow_mut().remove(&task.id);
            task.event.borrow_mut().raise_event(&result.as_ref().err().cloned());
        }
        result
    }
}
impl<P, R> Drop for ProcessedTask<P, R> {
    fn drop(&mut self) {
        if !self.completed {
            self.worker.cancel_task(self.id);
            self.state.active.set(self.state.active.get().saturating_sub(1));
            self.state.wakers.borrow_mut().remove(&self.id);
        }
    }
}

struct InitState {
    future: Option<TaskFuture<serde_json::Value>>,
    result: Option<Result<serde_json::Value, RuntimeError>>,
    wake: Arc<InitWake>,
}

struct InitWake(Mutex<Vec<Waker>>);
impl Wake for InitWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let wakers = std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()));
        for waker in wakers {
            waker.wake();
        }
    }
}

#[derive(Clone)]
pub struct ModuleInitialization(Rc<RefCell<InitState>>);
impl ModuleInitialization {
    fn reject_destroyed(&self) {
        let wake = {
            let mut state = self.0.borrow_mut();
            if state.result.is_some() {
                return;
            }
            state.future = None;
            state.result = Some(Err(RuntimeError::new("TaskProcessor was destroyed.")));
            state.wake.clone()
        };
        wake.wake();
    }
}
impl Future for ModuleInitialization {
    type Output = Result<serde_json::Value, RuntimeError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.borrow_mut();
        if let Some(result) = &state.result {
            return Poll::Ready(result.clone());
        }
        {
            let mut wakers = state.wake.0.lock().unwrap_or_else(|e| e.into_inner());
            if !wakers.iter().any(|w| w.will_wake(cx.waker())) {
                wakers.push(cx.waker().clone());
            }
        }
        let waker = Waker::from(state.wake.clone());
        let mut context = Context::from_waker(&waker);
        let result = state.future.as_mut().unwrap().as_mut().poll(&mut context);
        if let Poll::Ready(result) = result {
            state.result = Some(result.clone());
            state.future = None;
            let wake = state.wake.clone();
            drop(state);
            wake.wake();
            return Poll::Ready(result);
        }
        Poll::Pending
    }
}
