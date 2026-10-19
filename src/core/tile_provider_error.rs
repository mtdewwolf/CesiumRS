//! CesiumJS tile errors. A mutable reporting callback lets listeners request a retry.

#[derive(Clone, Debug, PartialEq)]
pub struct TileProviderError<P, E = String> {
    pub provider: P,
    pub message: String,
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub level: Option<u32>,
    pub times_retried: i64,
    pub retry: bool,
    pub error: Option<E>,
}

pub struct TileProviderErrorOptions<P, E = String> {
    pub provider: P,
    pub message: String,
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub level: Option<u32>,
    pub error: Option<E>,
}

impl<P, E> TileProviderError<P, E> {
    pub fn new(options: TileProviderErrorOptions<P, E>) -> Self {
        Self {
            provider: options.provider,
            message: options.message,
            x: options.x,
            y: options.y,
            level: options.level,
            times_retried: 0,
            retry: false,
            error: options.error,
        }
    }

    /// The host dispatches listeners in registration order, or logs the error
    /// when none are registered. Its callback may change `retry`.
    pub fn report_error(
        previous: Option<Self>,
        options: TileProviderErrorOptions<P, E>,
        report: impl FnOnce(&mut Self),
    ) -> Self {
        let count = previous.map_or(0, |e| e.times_retried + 1);
        let mut error = Self::new(options);
        error.times_retried = count;
        report(&mut error);
        error
    }

    pub fn report_success(&mut self) {
        self.times_retried = -1;
    }
}
