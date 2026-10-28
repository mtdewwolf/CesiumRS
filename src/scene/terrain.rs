//! CesiumJS `Terrain` async readiness and error events. Call `poll` from the
//! executor/frame loop; construction never blocks. Dropping cancels pending work.
use crate::core::{Event, RuntimeError};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
type ProviderFuture<P> = Pin<Box<dyn Future<Output = Result<P, RuntimeError>>>>;
pub struct Terrain<P> {
    pending: Option<ProviderFuture<P>>,
    provider: Option<P>,
    pub ready_event: Event<P>,
    pub error_event: Event<RuntimeError>,
}
impl<P> Terrain<P> {
    pub fn new(future: impl Future<Output = Result<P, RuntimeError>> + 'static) -> Self {
        Self { pending: Some(Box::pin(future)), provider: None, ready_event: Event::new(), error_event: Event::new() }
    }
    pub fn ready(&self) -> bool {
        self.provider.is_some()
    }
    pub fn provider(&self) -> Option<&P> {
        self.provider.as_ref()
    }
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(future) = &mut self.pending else {
            return Poll::Ready(());
        };
        let result = match future.as_mut().poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(v) => v,
        };
        self.pending = None;
        match result {
            Ok(provider) => {
                self.provider = Some(provider);
                self.ready_event.raise_event(self.provider.as_ref().unwrap());
            }
            Err(error) => {
                if self.error_event.number_of_listeners() == 0 {
                    eprintln!("{error}");
                } else {
                    self.error_event.raise_event(&error);
                }
            }
        }
        Poll::Ready(())
    }
}
impl Terrain<crate::core::CesiumTerrainProvider> {
    pub fn from_world_terrain(
        ion: crate::core::Ion,
        client: crate::core::ResourceClient,
        normals: bool,
        water: bool,
    ) -> Self {
        Self::new(
            async move { crate::core::world_terrain::create_world_terrain_async(&ion, client, normals, water).await },
        )
    }
    pub fn from_world_bathymetry(ion: crate::core::Ion, client: crate::core::ResourceClient, normals: bool) -> Self {
        Self::new(async move { crate::core::world_terrain::create_world_bathymetry_async(&ion, client, normals).await })
    }
}
