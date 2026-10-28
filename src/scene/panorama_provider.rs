//! CesiumJS abstract provider, expressed as an asynchronous Rust factory.
use super::panorama::Panorama;
use crate::core::RuntimeError;
use std::{future::Future, pin::Pin};
pub type PanoramaFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + 'a>>;
pub trait PanoramaProvider: Sized {
    type Options;
    type LoadOptions;
    type Panorama: Panorama;
    fn from_url(options: Self::Options) -> PanoramaFuture<'static, Self>;
    fn load_panorama(&mut self, options: Self::LoadOptions) -> PanoramaFuture<'_, Self::Panorama>;
}
