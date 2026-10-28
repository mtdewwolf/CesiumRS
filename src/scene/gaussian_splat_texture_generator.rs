//! CesiumJS `GaussianSplatTextureGenerator`: manages worker-based generation of
//! textures from Gaussian splat attributes (positions, scales, rotations, colors).

use crate::core::task_processor::{ModuleInitialization, ProcessedTask, TaskProcessor, TaskWorker, WebAssemblyOptions};
use crate::core::RuntimeError;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub type TextureGeneratorTask = ProcessedTask<Value, Value>;

struct TextureProcessor {
    processor: TaskProcessor<Value, Value>,
    #[allow(dead_code)]
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
    pub const WORKER_NAME: &'static str = "gaussianSplatTextureGenerator";
    pub const WASM_BINARY_FILE: &'static str = "ThirdParty/wasm_splats_bg.wasm";

    /// Upstream concurrency limit: one less than hardware concurrency, at least 1.
    pub fn max_sorting_concurrency(hardware_concurrency: usize) -> usize {
        hardware_concurrency.saturating_sub(1).max(1)
    }

    /// Creates a new texture generator with optional worker backing.
    pub fn new(
        worker: Option<Rc<dyn TaskWorker<Value, Value>>>,
        hardware_concurrency: usize,
    ) -> Self {
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

            let mut processor =
                TaskProcessor::new(worker.clone(), Some(self.maximum_sorting_concurrency));
            let init = processor.init_web_assembly_module(WebAssemblyOptions {
                wasm_binary_file: Some(Self::WASM_BINARY_FILE.into()),
                ..Default::default()
            });

            self.processor.replace(Some(TextureProcessor {
                processor,
                initialization: init,
            }));
            self.ready.set(true);
        }

        Ok(self.ready.get())
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
        assert_eq!(
            GaussianSplatTextureGenerator::WORKER_NAME,
            "gaussianSplatTextureGenerator"
        );
        assert_eq!(
            GaussianSplatTextureGenerator::WASM_BINARY_FILE,
            "ThirdParty/wasm_splats_bg.wasm"
        );
    }

    #[test]
    fn schedules_when_ready() {
        let gen = GaussianSplatTextureGenerator::default();
        assert!(!gen.is_ready());
        let res = gen.generate_from_attributes(Value::Null).unwrap();
        assert!(res.is_none());
    }
}
