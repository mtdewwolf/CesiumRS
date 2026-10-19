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

type ErrorListener<A> = Box<dyn FnMut(&mut A) + Send>;
/// Provider error listeners may change `retry`. Immutable `Event` listeners
/// intentionally cannot do this, so use this event for provider retry policy.
pub struct MutableErrorEvent<A> {
    listeners: Vec<(u64, ErrorListener<A>)>,
    next_id: u64,
}
impl<A> Default for MutableErrorEvent<A> {
    fn default() -> Self {
        Self { listeners: Vec::new(), next_id: 0 }
    }
}
impl<A> MutableErrorEvent<A> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn number_of_listeners(&self) -> usize {
        self.listeners.len()
    }
    pub fn add_event_listener(&mut self, listener: impl FnMut(&mut A) + Send + 'static) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.listeners.push((id, Box::new(listener)));
        id
    }
    pub fn remove_event_listener(&mut self, id: u64) -> bool {
        let old = self.listeners.len();
        self.listeners.retain(|(key, _)| *key != id);
        old != self.listeners.len()
    }
    pub fn raise_event(&mut self, value: &mut A) {
        for (_, listener) in &mut self.listeners {
            listener(value);
        }
    }
}

pub struct TileErrorReporter<'a, P, E> {
    pub event: Option<&'a mut MutableErrorEvent<TileProviderError<P, E>>>,
    pub provider_name: Option<&'a str>,
    pub log: &'a mut dyn FnMut(&str),
}
impl<P, E> TileErrorReporter<'_, P, E> {
    pub fn report(&mut self, error: &mut TileProviderError<P, E>) {
        if let Some(event) = self.event.as_deref_mut().filter(|event| event.number_of_listeners() > 0) {
            event.raise_event(error);
        } else if let Some(name) = self.provider_name {
            (self.log)(&format!("An error occurred in \"{name}\": {}", error.message));
        }
    }
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

    pub fn report_error_with(
        previous: Option<Self>,
        options: TileProviderErrorOptions<P, E>,
        reporter: &mut TileErrorReporter<'_, P, E>,
    ) -> Self {
        Self::report_error(previous, options, |error| reporter.report(error))
    }
}
