//! CesiumJS `DracoLoader`: schedules Draco decodes on the `decodeDraco` task
//! processor once its decoder module has initialized.
//!
//! Upstream keeps one task processor in static state. Here each `DracoLoader`
//! owns one, created on the first decode request with the host's
//! `decodeDraco` [`TaskWorker`] (for example a `NativeTaskWorker` running
//! `workers::decode_draco`). Parameters and results are the worker's JSON
//! messages; Rust transfers owned buffers instead of transfer lists.
//!
//! As upstream, the request that creates the processor starts the decoder
//! module and returns `None`; the module's result is observed on later
//! requests, where JavaScript's promise callbacks would have run. A falsy
//! result, or a failed initialization, is returned as the error of every
//! later request.

use crate::core::task_processor::{ModuleInitialization, ProcessedTask, TaskProcessor, TaskWorker, WebAssemblyOptions};
use crate::core::RuntimeError;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

pub type DracoTask = ProcessedTask<Value, Value>;

struct Processor {
    processor: TaskProcessor<Value, Value>,
    initialization: ModuleInitialization,
}

pub struct DracoLoader {
    worker: Rc<dyn TaskWorker<Value, Value>>,
    maximum_decoding_concurrency: usize,
    processor: RefCell<Option<Processor>>,
    ready: Cell<bool>,
    error: RefCell<Option<RuntimeError>>,
}

/// JavaScript truthiness of the module initialization result.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

impl DracoLoader {
    /// The worker the task processor runs (`TaskProcessor("decodeDraco")` upstream).
    pub const WORKER_NAME: &'static str = "decodeDraco";
    /// The decoder module upstream asks the worker to load.
    pub const WASM_BINARY_FILE: &'static str = "ThirdParty/draco_decoder.wasm";

    /// `DracoLoader._maxDecodingConcurrency`: one less than the hardware
    /// concurrency, and at least one.
    pub fn max_decoding_concurrency(hardware_concurrency: usize) -> usize {
        hardware_concurrency.saturating_sub(1).max(1)
    }

    pub fn new(worker: Rc<dyn TaskWorker<Value, Value>>, maximum_decoding_concurrency: usize) -> Self {
        Self {
            worker,
            maximum_decoding_concurrency,
            processor: RefCell::new(None),
            ready: Cell::new(false),
            error: RefCell::new(None),
        }
    }

    /// Whether the decoder module has initialized.
    pub fn is_ready(&self) -> bool {
        self.ready.get()
    }

    /// `DracoLoader.decodePointCloud`: `None` when no task can be scheduled yet.
    pub fn decode_point_cloud(&self, parameters: Value) -> Result<Option<DracoTask>, RuntimeError> {
        self.schedule(parameters)
    }

    /// `DracoLoader.decodeBufferView`: `None` when no task can be scheduled yet.
    pub fn decode_buffer_view(&self, options: Value) -> Result<Option<DracoTask>, RuntimeError> {
        self.schedule(options)
    }

    fn schedule(&self, parameters: Value) -> Result<Option<DracoTask>, RuntimeError> {
        let mut slot = self.processor.borrow_mut();
        match slot.as_mut() {
            None => {
                let mut processor = TaskProcessor::new(self.worker.clone(), Some(self.maximum_decoding_concurrency));
                let initialization = processor.init_web_assembly_module(WebAssemblyOptions {
                    wasm_binary_file: Some(Self::WASM_BINARY_FILE.into()),
                    ..Default::default()
                });
                *slot = Some(Processor { processor, initialization });
            }
            Some(p) if !self.ready.get() && self.error.borrow().is_none() => self.observe(&mut p.initialization),
            Some(_) => {}
        }
        if let Some(error) = self.error.borrow().clone() {
            return Err(error);
        }
        if !self.ready.get() {
            return Ok(None);
        }
        Ok(slot.as_mut().and_then(|p| p.processor.schedule_task(parameters)))
    }

    /// Applies the module's result if it has settled, as the promise callbacks would.
    fn observe(&self, initialization: &mut ModuleInitialization) {
        match Pin::new(initialization).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(Ok(result)) if truthy(&result) => self.ready.set(true),
            Poll::Ready(Ok(_)) => {
                *self.error.borrow_mut() = Some(RuntimeError::new("Draco decoder could not be initialized."));
            }
            Poll::Ready(Err(error)) => *self.error.borrow_mut() = Some(error),
            Poll::Pending => {}
        }
    }
}
