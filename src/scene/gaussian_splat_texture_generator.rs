//! CesiumJS `GaussianSplatTextureGenerator`: manages worker-based generation of
//! textures from Gaussian splat attributes (positions, scales, rotations, colors).

use crate::core::task_processor::{ModuleInitialization, ProcessedTask, TaskProcessor, TaskWorker, WebAssemblyOptions};
use crate::core::RuntimeError;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

pub type TextureGeneratorTask = ProcessedTask<Value, Value>;

struct TextureProcessor {
    processor: TaskProcessor<Value, Value>,
    initialization: ModuleInitialization,
}

/// Texture generator coordinator for Gaussian splats that uses a task processor.
pub struct GaussianSplatTextureGenerator {
    worker: Option<Rc<dyn TaskWorker<Value, Value>>>,
    maximum_sorting_concurrency: usize,
    processor: RefCell<Option<TextureProcessor>>,
    ready: Cell<bool>,
    error: RefCell<Option<RuntimeError>>,
}

impl Default for GaussianSplatTextureGenerator {
    fn default() -> Self {
        Self::new(None, 4)
    }
}

impl GaussianSplatTextureGenerator {
    /// Uses the built-in Rust algorithms without a browser worker or Wasm module.
    /// Work is driven by polling the returned task; hosts can supply a dedicated
    /// worker to `new` when execution must occur on a background thread.
    pub fn native(hardware_concurrency: usize) -> Self {
        Self::new(Some(Rc::new(NativeWorker)), hardware_concurrency)
    }

    pub const WORKER_NAME: &'static str = "gaussianSplatTextureGenerator";
    pub const WASM_BINARY_FILE: &'static str = "ThirdParty/wasm_splats_bg.wasm";

    /// Upstream concurrency limit: one less than hardware concurrency, at least 1.
    pub fn max_sorting_concurrency(hardware_concurrency: usize) -> usize {
        hardware_concurrency.saturating_sub(1).max(1)
    }

    /// Creates a new texture generator with optional worker backing.
    pub fn new(worker: Option<Rc<dyn TaskWorker<Value, Value>>>, hardware_concurrency: usize) -> Self {
        Self {
            worker,
            maximum_sorting_concurrency: Self::max_sorting_concurrency(hardware_concurrency),
            processor: RefCell::new(None),
            ready: Cell::new(false),
            error: RefCell::new(None),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready.get()
    }

    pub fn error(&self) -> Option<RuntimeError> {
        self.error.borrow().clone()
    }

    /// Initializes the processor if worker is supplied.
    pub fn init_processor(&self) -> Result<bool, RuntimeError> {
        if let Some(err) = self.error.borrow().as_ref() {
            return Err(err.clone());
        }

        if self.processor.borrow().is_none() {
            let Some(worker) = self.worker.as_ref() else {
                return Ok(false);
            };

            let mut processor = TaskProcessor::new(worker.clone(), Some(self.maximum_sorting_concurrency));
            let init = processor.init_web_assembly_module(WebAssemblyOptions {
                wasm_binary_file: Some(Self::WASM_BINARY_FILE.into()),
                ..Default::default()
            });

            self.processor.replace(Some(TextureProcessor { processor, initialization: init }));
        }

        Ok(self.ready.get())
    }

    /// Drive worker initialization using the caller's task context.
    pub fn poll_initialization(&self, cx: &mut Context<'_>) -> Poll<Result<bool, RuntimeError>> {
        match self.init_processor() {
            Err(err) => return Poll::Ready(Err(err)),
            Ok(true) => return Poll::Ready(Ok(true)),
            Ok(false) => {}
        }
        let mut processor = self.processor.borrow_mut();
        let Some(processor) = processor.as_mut() else {
            return Poll::Ready(Ok(false));
        };
        match Pin::new(&mut processor.initialization).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(value)) if value == Value::Bool(true) => {
                self.ready.set(true);
                Poll::Ready(Ok(true))
            }
            Poll::Ready(Ok(_)) => {
                let err = RuntimeError::new("Gaussian splat sorter could not be initialized.");
                self.error.replace(Some(err.clone()));
                Poll::Ready(Err(err))
            }
            Poll::Ready(Err(err)) => {
                self.error.replace(Some(err.clone()));
                Poll::Ready(Err(err))
            }
        }
    }

    /// Schedules a texture generation task on the worker.
    pub fn generate_from_attributes(&self, parameters: Value) -> Result<Option<TextureGeneratorTask>, RuntimeError> {
        if let Some(err) = self.error.borrow().as_ref() {
            return Err(err.clone());
        }

        if !self.ready.get() {
            self.init_processor()?;
            if !self.ready.get() {
                return Ok(None);
            }
        }

        let mut proc_borrow = self.processor.borrow_mut();
        let Some(proc) = proc_borrow.as_mut() else {
            return Ok(None);
        };

        let task = proc.processor.schedule_task(parameters);
        Ok(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrency_bounds_match_upstream() {
        assert_eq!(GaussianSplatTextureGenerator::max_sorting_concurrency(1), 1);
        assert_eq!(GaussianSplatTextureGenerator::max_sorting_concurrency(0), 1);
        assert_eq!(GaussianSplatTextureGenerator::max_sorting_concurrency(8), 7);
    }

    #[test]
    fn constants_match_upstream() {
        assert_eq!(GaussianSplatTextureGenerator::WORKER_NAME, "gaussianSplatTextureGenerator");
        assert_eq!(GaussianSplatTextureGenerator::WASM_BINARY_FILE, "ThirdParty/wasm_splats_bg.wasm");
    }

    #[test]
    fn schedules_when_ready() {
        let gen = GaussianSplatTextureGenerator::default();
        assert!(!gen.is_ready());
        let res = gen.generate_from_attributes(Value::Null).unwrap();
        assert!(res.is_none());
    }
}

struct NativeWorker;
impl TaskWorker<Value, Value> for NativeWorker {
    fn init_web_assembly_module(&self, _: WebAssemblyOptions) -> crate::core::task_processor::TaskFuture<Value> {
        Box::pin(async { Ok(Value::Bool(true)) })
    }
    fn cancel_task(&self, _: u64) {}
    fn terminate(&self) {}
    fn run(&self, _: u64, parameters: Value) -> crate::core::task_processor::TaskFuture<Value> {
        Box::pin(async move {
            #[derive(serde::Deserialize)]
            struct Attributes {
                positions: Vec<f32>,
                scales: Vec<f32>,
                rotations: Vec<f32>,
                colors: Vec<u8>,
            }
            #[derive(serde::Deserialize)]
            struct Parameters {
                attributes: Attributes,
                count: usize,
            }
            let p: Parameters = serde_json::from_value(parameters).map_err(|e| RuntimeError::new(e.to_string()))?;
            let a = p.attributes;
            let result = crate::workers::gaussian_splats::gaussian_splat_texture_generator(
                &a.positions,
                &a.scales,
                &a.rotations,
                &a.colors,
                p.count,
            )?;
            Ok(serde_json::json!({"data": result.data, "width": result.width, "height":result.height}))
        })
    }
}
