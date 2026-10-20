//! Cesium Sync fences. The host supplies the polling cadence as a future.
use super::context::*;
use crate::core::{webgl_constants as gl, RuntimeError};
use std::future::Future;

pub struct Sync<G: Gl> {
    resource: Resource<G>,
}
impl<G: Gl> Sync<G> {
    pub fn new(context: &Context<G>) -> Result<Self> {
        Ok(Self {
            resource: Resource::create(
                context,
                "fenceSync",
                &[gl::SYNC_GPU_COMMANDS_COMPLETE.into(), 0.into()],
                "deleteSync",
            )?,
        })
    }
    pub fn status(&self) -> Result<u32> {
        match self.resource.context.call("getSyncParameter", &[self.resource.value()?, gl::SYNC_STATUS.into()])? {
            GlValue::Number(n) if n == gl::SIGNALED as f64 || n == gl::UNSIGNALED as f64 => Ok(n as u32),
            _ => Err(RuntimeError::new("Invalid sync status from host")),
        }
    }
    /// Schedule before every poll, including the first. As in Cesium, ttl=N
    /// permits N retries after the initial poll (N+1 scheduled polls).
    pub async fn wait_for_signal<S, F>(&self, mut schedule: S, ttl: Option<u32>) -> Result<()>
    where
        S: FnMut() -> F,
        F: Future<Output = Result<()>>,
    {
        for _ in 0..=ttl.unwrap_or(10) {
            schedule().await?;
            if self.status()? == gl::SIGNALED {
                return Ok(());
            }
        }
        Err(RuntimeError::new("Wait for signal timeout"))
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
}
