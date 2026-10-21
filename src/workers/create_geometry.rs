//! CesiumJS `createGeometry`: concurrent, ordered geometry batches and module reuse.

use super::{GeometryDescription, PackedGeometryResults};
use crate::core::{Geometry, RuntimeError};
use futures_util::future::{poll_fn, LocalBoxFuture};
use std::collections::HashMap;
use std::rc::Rc;
use std::task::Poll;

/// Owned data crossing a worker boundary. Packed descriptions retain their offset.
#[derive(Clone, Debug)]
pub enum GeometrySource {
    Description(Box<GeometryDescription>),
    Packed { values: Vec<f64>, offset: usize },
    Ready(Option<Box<Geometry>>),
}

impl GeometrySource {
    pub fn ready(geometry: Option<Geometry>) -> Self {
        Self::Ready(geometry.map(Box::new))
    }
}

#[derive(Clone, Debug)]
pub struct GeometrySubTask {
    pub geometry: GeometrySource,
    pub module_name: Option<String>,
    pub module_path: Option<String>,
}

/// An asynchronous module entry point. Custom paths are resolved by explicit
/// host registration instead of JavaScript dynamic import; factories are reused
/// across batches. Each task receives its own data, with no shared scratch state.
pub type GeometryFactory =
    Rc<dyn Fn(GeometrySource) -> LocalBoxFuture<'static, Result<Option<Geometry>, RuntimeError>>>;

#[derive(Clone)]
pub struct GeometryWorker {
    modules: HashMap<String, GeometryFactory>,
}

impl Default for GeometryWorker {
    fn default() -> Self {
        let mut worker = Self { modules: HashMap::new() };
        for &name in GeometryDescription::MODULE_NAMES {
            let factory: GeometryFactory = Rc::new(move |source| {
                Box::pin(async move {
                    let description = match source {
                        GeometrySource::Description(value) if value.module_name() == name => *value,
                        GeometrySource::Packed { values, offset } => {
                            let Some(value) = GeometryDescription::unpack(name, &values, offset) else {
                                return Ok(None);
                            };
                            value
                        }
                        _ => return Err(RuntimeError::new("Geometry description does not match its worker module")),
                    };
                    Ok(description.create_geometry())
                })
            });
            worker.register(name, factory.clone());
            worker.register(format!("./{name}.js"), factory);
        }
        worker
    }
}

impl GeometryWorker {
    pub fn register(&mut self, name_or_path: impl Into<String>, factory: GeometryFactory) {
        self.modules.insert(name_or_path.into(), factory);
    }

    /// Starts every task, collects successful results in input order, then packs one
    /// transferable Float64 buffer. A rejected task rejects the whole batch.
    pub async fn create_geometry(&self, tasks: Vec<GeometrySubTask>) -> Result<PackedGeometryResults, RuntimeError> {
        let pending = tasks.into_iter().map(|task| {
            let future: LocalBoxFuture<'_, Result<Option<Geometry>, RuntimeError>> = Box::pin(async move {
                if task.module_name.is_some() && task.module_path.is_some() {
                    return Err(RuntimeError::new("Must only set moduleName or modulePath"));
                }
                match task.module_path.or(task.module_name) {
                    Some(key) => {
                        let factory = self
                            .modules
                            .get(&key)
                            .ok_or_else(|| RuntimeError::new(format!("Unknown geometry worker module: {key}")))?;
                        factory(task.geometry).await
                    }
                    None => match task.geometry {
                        GeometrySource::Ready(geometry) => Ok(geometry.map(|g| *g)),
                        _ => Err(RuntimeError::new("A task without a module must contain an already created geometry")),
                    },
                }
            });
            Some(future)
        });
        let mut pending: Vec<_> = pending.collect();
        let mut results = vec![None; pending.len()];
        // Poll every task before reporting failure, matching Promise.all's
        // eager start. An error must not wait for an unrelated pending task.
        poll_fn(move |cx| {
            let mut error = None;
            let mut waiting = false;
            for (index, slot) in pending.iter_mut().enumerate() {
                if let Some(future) = slot {
                    match future.as_mut().poll(cx) {
                        Poll::Ready(result) => {
                            *slot = None;
                            match result {
                                Ok(geometry) => results[index] = geometry,
                                Err(e) => {
                                    error.get_or_insert(e);
                                }
                            }
                        }
                        Poll::Pending => waiting = true,
                    }
                }
            }
            if let Some(error) = error {
                Poll::Ready(Err(error))
            } else if waiting {
                Poll::Pending
            } else {
                Poll::Ready(Ok(PackedGeometryResults::pack(&results)))
            }
        })
        .await
    }
}

/// Runs a batch using the built-in geometry modules.
pub async fn create_geometry(tasks: Vec<GeometrySubTask>) -> Result<PackedGeometryResults, RuntimeError> {
    GeometryWorker::default().create_geometry(tasks).await
}
