//! CesiumJS resource URL construction and host-supplied HTTP transport.
//!
//! HTTP futures are scheduled by `RequestScheduler`; no browser globals or
//! native runtime are embedded. Image, XML and JSONP host integration is not
//! implemented here. Clones own their configuration and reset request state.

use super::{
    proxy::Proxy,
    query::{object_to_query, query_to_object, QueryValue},
    request::{Request, RequestFuture, RequestHandle, RequestOptions, ScheduledRequest},
    request_error_event::RequestErrorEvent,
    request_scheduler::RequestScheduler,
    trusted_servers::TrustedServers,
    uri::{encode_uri_component, Uri},
    uri_helpers, RequestState,
};
use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
    sync::Arc,
};

pub type QueryParameters = Vec<(String, Option<QueryValue>)>;
pub type RetryCallback =
    Rc<dyn Fn(&Resource, &RequestErrorEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool>>>>;

#[derive(Clone, Debug, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl HttpResponse {
    pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }
    pub fn json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub with_credentials: bool,
}

/// Implementations must resolve responses on success and reject HTTP/network
/// failures with `RequestErrorEvent`, retaining the response body and headers.
pub trait ResourceTransport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse>;
    fn cancel(&self, _request: &HttpRequest) {}
}

#[derive(Clone, Default)]
pub struct ResourceOptions {
    pub url: String,
    pub query_parameters: QueryParameters,
    pub template_values: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub proxy: Option<Arc<dyn Proxy>>,
    pub retry_callback: Option<RetryCallback>,
    pub retry_attempts: usize,
}

#[derive(Clone, Default)]
pub struct DerivedResourceOptions {
    pub url: Option<String>,
    pub query_parameters: QueryParameters,
    pub template_values: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub proxy: Option<Arc<dyn Proxy>>,
    pub retry_callback: Option<RetryCallback>,
    pub retry_attempts: Option<usize>,
    pub request: Option<RequestHandle<HttpResponse>>,
    pub preserve_query_parameters: bool,
}

#[derive(Clone, Default)]
pub struct FetchOptions {
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub page_protocol: String,
}

pub struct Resource {
    options: Rc<RefCell<ResourceOptions>>,
    retry_count: Rc<RefCell<usize>>,
    pub request: RequestHandle<HttpResponse>,
}

impl Clone for Resource {
    fn clone(&self) -> Self {
        Self {
            options: Rc::new(RefCell::new(self.options.borrow().clone())),
            retry_count: Rc::new(RefCell::new(0)),
            request: self.request.borrow().clone().into_handle(),
        }
    }
}

fn combine<V: Clone>(first: &[(String, V)], second: &[(String, V)]) -> Vec<(String, V)> {
    let mut result = first.to_vec();
    for (key, value) in second {
        if !result.iter().any(|(k, _)| k == key) {
            result.push((key.clone(), value.clone()));
        }
    }
    super::query::js_object_order(result)
}

fn combine_queries(first: &QueryParameters, second: &QueryParameters, preserve: bool) -> QueryParameters {
    if !preserve {
        return combine(first, second);
    }
    let mut result = first.clone();
    for (key, value) in second {
        if let Some((_, old)) = result.iter_mut().find(|(k, _)| k == key) {
            if let Some(old_value) = old {
                let mut values = match old_value {
                    QueryValue::Single(s) => vec![s.clone()],
                    QueryValue::Multiple(v) => v.clone(),
                };
                match value {
                    Some(QueryValue::Single(s)) => values.push(s.clone()),
                    Some(QueryValue::Multiple(v)) => values.extend(v.clone()),
                    None => values.push("undefined".into()),
                }
                *old = Some(QueryValue::Multiple(values));
            } else {
                *old = value.clone();
            }
        } else {
            result.push((key.clone(), value.clone()));
        }
    }
    super::query::js_object_order(result)
}

impl Resource {
    pub fn new(url: impl Into<String>) -> Option<Self> {
        Self::from_options(ResourceOptions { url: url.into(), ..Default::default() })
    }

    pub fn from_options(options: ResourceOptions) -> Option<Self> {
        let url = options.url.clone();
        let resource = Self {
            options: Rc::new(RefCell::new(options)),
            retry_count: Rc::new(RefCell::new(0)),
            request: Request::new(RequestOptions::default()).into_handle(),
        };
        resource.parse_url(&url, true, true, None)?;
        Some(resource)
    }

    pub fn options(&self) -> Ref<'_, ResourceOptions> {
        self.options.borrow()
    }
    pub fn options_mut(&self) -> RefMut<'_, ResourceOptions> {
        self.options.borrow_mut()
    }
    pub fn url(&self) -> String {
        self.get_url_component(true, true)
    }
    pub fn is_data_uri(&self) -> bool {
        uri_helpers::is_data_uri(&self.options.borrow().url)
    }
    pub fn is_blob_uri(&self) -> bool {
        uri_helpers::is_blob_uri(&self.options.borrow().url)
    }
    pub fn has_headers(&self) -> bool {
        !self.options.borrow().headers.is_empty()
    }
    pub fn is_cross_origin_url(&self, page_url: &str) -> bool {
        uri_helpers::is_cross_origin_url(&self.options.borrow().url, page_url)
    }
    pub fn extension(&self) -> String {
        uri_helpers::get_extension_from_uri(&self.options.borrow().url)
    }
    pub fn get_base_uri(&self, include_query: bool) -> String {
        uri_helpers::get_base_uri(&self.get_url_component(include_query, false), include_query)
    }
    pub fn append_forward_slash(&self) {
        let mut o = self.options.borrow_mut();
        o.url = uri_helpers::append_forward_slash(&o.url);
    }

    // Request callbacks must not hold their own request alive through an Rc cycle.
    fn weak_context(&self) -> impl Fn() -> Self + 'static {
        let options = self.options.clone();
        let retry_count = self.retry_count.clone();
        let request = Rc::downgrade(&self.request);
        move || Self {
            options: options.clone(),
            retry_count: retry_count.clone(),
            request: request.upgrade().expect("scheduler retains the executing request"),
        }
    }

    pub fn parse_url(&self, url: &str, merge: bool, preserve_query: bool, base_url: Option<&str>) -> Option<()> {
        let mut uri = Uri::parse(url);
        let query = uri.query();
        let parameters = if query.is_empty() {
            Vec::new()
        } else if !query.contains('=') {
            vec![(query.to_owned(), None)]
        } else {
            query_to_object(query)?.into_iter().map(|(k, v)| (k, Some(v))).collect()
        };
        uri.query = None;
        uri.fragment = None;
        if let Some(base) = base_url {
            if uri.scheme().is_empty() {
                uri = uri.absolute_to(base)?;
            }
        }
        let mut o = self.options.borrow_mut();
        o.query_parameters =
            if merge { combine_queries(&parameters, &o.query_parameters, preserve_query) } else { parameters };
        o.url = uri.to_string();
        Some(())
    }

    pub fn get_url_component(&self, query: bool, proxy: bool) -> String {
        let o = self.options.borrow();
        if uri_helpers::is_data_uri(&o.url) {
            return o.url.clone();
        }
        let mut url = o.url.clone();
        if query && !o.query_parameters.is_empty() {
            url.push('?');
            if o.query_parameters.len() == 1 && o.query_parameters[0].1.is_none() {
                url.push_str(&o.query_parameters[0].0);
            } else {
                let entries: Vec<_> = o
                    .query_parameters
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone().unwrap_or(QueryValue::Single("undefined".into()))))
                    .collect();
                url.push_str(&object_to_query(&entries));
            }
        }
        url = url.replace("%7B", "{").replace("%7D", "}");
        if !o.template_values.is_empty() {
            let mut expanded = String::with_capacity(url.len());
            let mut rest = url.as_str();
            while let Some(start) = rest.find('{') {
                expanded.push_str(&rest[..start]);
                let Some(end) = rest[start + 1..].find('}').map(|i| i + start + 1) else {
                    expanded.push_str(&rest[start..]);
                    rest = "";
                    break;
                };
                let key = &rest[start + 1..end];
                if let Some((_, value)) = o.template_values.iter().find(|(k, _)| k == key) {
                    expanded.push_str(&encode_uri_component(value));
                } else {
                    expanded.push_str(&rest[start..=end]);
                }
                rest = &rest[end + 1..];
            }
            expanded.push_str(rest);
            url = expanded;
        }
        if proxy {
            if let Some(proxy) = &o.proxy {
                return proxy.get_url(&url);
            }
        }
        url
    }

    pub fn set_query_parameters(&self, parameters: &QueryParameters, use_as_default: bool) {
        let mut o = self.options.borrow_mut();
        o.query_parameters = if use_as_default {
            combine_queries(&o.query_parameters, parameters, false)
        } else {
            combine_queries(parameters, &o.query_parameters, false)
        };
    }
    pub fn append_query_parameters(&self, parameters: &QueryParameters) {
        let mut o = self.options.borrow_mut();
        o.query_parameters = combine_queries(parameters, &o.query_parameters, true);
    }
    pub fn set_template_values(&self, values: &[(String, String)], use_as_default: bool) {
        let mut o = self.options.borrow_mut();
        o.template_values =
            if use_as_default { combine(&o.template_values, values) } else { combine(values, &o.template_values) };
    }

    pub fn get_derived_resource(&self, options: DerivedResourceOptions) -> Option<Self> {
        let mut result = self.clone();
        if let Some(url) = options.url {
            result.parse_url(&url, true, options.preserve_query_parameters, Some(&self.options.borrow().url))?;
        }
        result.set_query_parameters(&options.query_parameters, false);
        result.set_template_values(&options.template_values, false);
        {
            let mut o = result.options.borrow_mut();
            o.headers = combine(&options.headers, &o.headers);
            if options.proxy.is_some() {
                o.proxy = options.proxy;
            }
            if options.retry_callback.is_some() {
                o.retry_callback = options.retry_callback;
            }
            if let Some(attempts) = options.retry_attempts {
                o.retry_attempts = attempts;
            }
        }
        if let Some(request) = options.request {
            result.request = request;
        }
        Some(result)
    }

    pub async fn retry_on_error(&self, error: &RequestErrorEvent) -> bool {
        let o = self.options.borrow();
        if *self.retry_count.borrow() >= o.retry_attempts {
            return false;
        }
        let Some(callback) = o.retry_callback.clone() else {
            return false;
        };
        drop(o);
        let retry = callback(self, error).await;
        *self.retry_count.borrow_mut() += 1;
        retry
    }

    /// All HTTP verbs share this path. Returned bytes can be read as text or JSON
    /// using `HttpResponse`; HEAD retains response headers.
    pub fn fetch(
        &self,
        scheduler: &mut RequestScheduler<HttpResponse>,
        transport: Rc<dyn ResourceTransport>,
        trusted: Rc<RefCell<TrustedServers>>,
        options: FetchOptions,
    ) -> Option<ScheduledRequest<HttpResponse>> {
        if matches!(self.request.borrow().state, RequestState::Issued | RequestState::Active) {
            return None;
        }
        {
            let mut r = self.request.borrow_mut();
            r.state = RequestState::Unissued;
            r.cancelled = false;
            r.deferred = None;
            r.options.url = self.url();
        }
        let context = self.weak_context();
        let cancel_context = self.weak_context();
        let cancel_transport = transport.clone();
        let cancel_options = options.clone();
        let cancel_trusted = trusted.clone();
        self.request.borrow_mut().options.request_function = Some(Rc::new(move || {
            let context = context();
            let transport = transport.clone();
            let trusted = trusted.clone();
            let options = options.clone();
            Box::pin(async move {
                loop {
                    let request = context.http_request(&options, &trusted.borrow());
                    match transport.execute(request).await {
                        Ok(response) => return Ok(response),
                        Err(error) => {
                            if !context.retry_on_error(&error).await {
                                return Err(error);
                            }
                        }
                    }
                }
            })
        }));
        self.request.borrow_mut().options.cancel_function = Some(Rc::new(move || {
            cancel_transport.cancel(&cancel_context().http_request(&cancel_options, &cancel_trusted.borrow()));
        }));
        scheduler.request(self.request.clone())
    }

    fn http_request(&self, options: &FetchOptions, trusted: &TrustedServers) -> HttpRequest {
        let url = self.url();
        HttpRequest {
            with_credentials: trusted.contains(&url, &options.page_protocol),
            url,
            method: if options.method.is_empty() { "GET".into() } else { options.method.clone() },
            headers: combine(&options.headers, &self.options.borrow().headers),
            body: options.body.clone(),
        }
    }
}

impl std::fmt::Display for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url())
    }
}
