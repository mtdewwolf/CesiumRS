//! CesiumJS resource URL construction and host-supplied HTTP transport.
//!
//! HTTP futures are scheduled by `RequestScheduler`; no browser globals or
//! native runtime are embedded. ResourceClient supplies typed fetch, retry,
//! image, XML and JSONP execution. Clones reset request state.

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
pub type FallibleRetryCallback = Rc<dyn Fn(&Resource, &RequestErrorEvent) -> super::ResourceFuture<bool>>;
pub type RequestTransform = Rc<dyn Fn(&mut HttpRequest)>;

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
    pub id: u64,
    pub kind: RequestKind,
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

#[derive(Clone, Debug, Default)]
pub enum RequestKind {
    #[default]
    Http,
    Jsonp {
        callback_parameter: String,
        function_name: String,
    },
    Image,
}

#[derive(Clone)]
pub struct ResourceOptions {
    pub url: String,
    pub query_parameters: QueryParameters,
    pub template_values: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub proxy: Option<Arc<dyn Proxy>>,
    pub retry_callback: Option<RetryCallback>,
    pub retry_attempts: usize,
    pub credits: Option<Vec<super::Credit>>,
    pub parse_url: bool,
    pub request_transform: Option<RequestTransform>,
    pub fallible_retry_callback: Option<FallibleRetryCallback>,
}

impl Default for ResourceOptions {
    fn default() -> Self {
        Self {
            url: String::new(),
            query_parameters: Vec::new(),
            template_values: Vec::new(),
            headers: Vec::new(),
            proxy: None,
            retry_callback: None,
            retry_attempts: 0,
            credits: None,
            parse_url: true,
            request_transform: None,
            fallible_retry_callback: None,
        }
    }
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
    pub kind: RequestKind,
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
    /// Native counterpart to Resource.DEFAULT: page state is supplied explicitly.
    pub fn default_for_page(page_url: &str) -> Self {
        Self::new(page_url.split('?').next().unwrap_or("")).expect("page URI")
    }

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
        if resource.options.borrow().parse_url {
            resource.parse_url(&url, true, true, None)?;
        }
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

    pub(crate) fn share(&self) -> Self {
        Self { options: self.options.clone(), retry_count: self.retry_count.clone(), request: self.request.clone() }
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
        self.retry_on_error_result(error).await.unwrap_or(false)
    }
    pub async fn retry_on_error_result(&self, error: &RequestErrorEvent) -> Result<bool, RequestErrorEvent> {
        let (callback, fallible) = {
            let o = self.options.borrow();
            if *self.retry_count.borrow() >= o.retry_attempts { return Ok(false); }
            (o.retry_callback.clone(), o.fallible_retry_callback.clone())
        };
        let retry = if let Some(callback) = fallible { callback(self, error).await? }
            else if let Some(callback) = callback { callback(self, error).await }
            else { return Ok(false); };
        *self.retry_count.borrow_mut() += 1;
        Ok(retry)
    }

    pub fn fetch(
        &self,
        client: &super::resource_client::ResourceClient,
        options: FetchOptions,
    ) -> Option<super::resource_client::ResourceFuture<Option<HttpResponse>>> {
        client.fetch_optional(self, options)
    }

    pub fn fetch_array_buffer(&self, client: &super::ResourceClient) -> Option<super::ResourceFuture<Option<Vec<u8>>>> {
        client.fetch_array_buffer_optional(self)
    }
    pub fn fetch_text(&self, client: &super::ResourceClient) -> Option<super::ResourceFuture<Option<String>>> {
        client.fetch_text_optional(self)
    }
    pub fn fetch_json(
        &self,
        client: &super::ResourceClient,
    ) -> Option<super::ResourceFuture<Option<serde_json::Value>>> {
        client.fetch_json_optional(self)
    }
    pub fn fetch_blob(
        &self,
        client: &super::ResourceClient,
    ) -> Option<super::ResourceFuture<Option<super::ResourceBlob>>> {
        client.fetch_blob_optional(self)
    }
    pub fn fetch_xml(
        &self,
        client: &super::ResourceClient,
    ) -> Option<super::ResourceFuture<Option<super::XmlDocument>>> {
        client.fetch_xml_optional(self)
    }
    pub fn fetch_jsonp(
        &self,
        client: &super::ResourceClient,
        parameter: Option<&str>,
    ) -> Option<super::ResourceFuture<Option<serde_json::Value>>> {
        client.fetch_jsonp_optional(self, parameter)
    }

    pub fn fetch_image<H: super::ImageHost + 'static>(
        &self,
        client: &super::ResourceClient,
        host: Rc<H>,
        options: super::image_helpers::FetchImageOptions,
    ) -> Option<super::ResourceFuture<Option<H::Image>>> {
        client.fetch_image(self, host, options)
    }
    pub fn head(&self, client: &super::ResourceClient) -> Option<super::ResourceFuture<Vec<(String, String)>>> {
        client.fetch_headers(self, "HEAD")
    }
    pub fn options_request(
        &self,
        client: &super::ResourceClient,
    ) -> Option<super::ResourceFuture<Vec<(String, String)>>> {
        client.fetch_headers(self, "OPTIONS")
    }
    pub fn delete(&self, client: &super::ResourceClient) -> Option<super::ResourceFuture<Option<HttpResponse>>> {
        client.fetch_optional(self, FetchOptions { method: "DELETE".into(), ..Default::default() })
    }
    pub fn post(
        &self,
        client: &super::ResourceClient,
        body: Vec<u8>,
    ) -> Option<super::ResourceFuture<Option<HttpResponse>>> {
        client.fetch_optional(self, FetchOptions { method: "POST".into(), body: Some(body), ..Default::default() })
    }
    pub fn put(
        &self,
        client: &super::ResourceClient,
        body: Vec<u8>,
    ) -> Option<super::ResourceFuture<Option<HttpResponse>>> {
        client.fetch_optional(self, FetchOptions { method: "PUT".into(), body: Some(body), ..Default::default() })
    }
    pub fn patch(
        &self,
        client: &super::ResourceClient,
        body: Vec<u8>,
    ) -> Option<super::ResourceFuture<Option<HttpResponse>>> {
        client.fetch_optional(self, FetchOptions { method: "PATCH".into(), body: Some(body), ..Default::default() })
    }

    pub fn credits(&self) -> Option<Vec<super::Credit>> {
        self.options.borrow().credits.clone()
    }

    /// Mirrors createIfNeeded: copy configuration but retain the caller's request.
    pub fn create_if_needed(&self) -> Self {
        let mut resource = self.clone();
        resource.request = self.request.clone();
        resource
    }

    pub(crate) fn fetch_once(
        &self,
        scheduler: &mut RequestScheduler<HttpResponse>,
        transport: Rc<dyn ResourceTransport>,
        trusted: Rc<RefCell<TrustedServers>>,
        options: FetchOptions,
        id: u64,
    ) -> Result<Option<ScheduledRequest<HttpResponse>>, RequestErrorEvent> {
        if matches!(self.request.borrow().state, RequestState::Issued | RequestState::Active) {
            return Err(super::resource_client::request_error("The Resource is already being fetched."));
        }
        {
            let mut r = self.request.borrow_mut();
            r.state = RequestState::Unissued;
            r.deferred = None;
            r.options.url = self.url();
        }
        let context = self.weak_context();
        let captured_url = self.url();
        let cancel_request = Rc::new(RefCell::new(None));
        let cancel_capture = cancel_request.clone();
        let cancel_transport = transport.clone();
        self.request.borrow_mut().options.request_function = Some(Rc::new(move || {
            let context = context();
            let mut request = context.http_request(&options, &trusted.borrow());
            request.id = id;
            request.url = captured_url.clone();
            *cancel_capture.borrow_mut() = Some(request.clone());
            let uri = (super::uri_helpers::is_data_uri(&request.url) && !matches!(request.kind, RequestKind::Image))
                .then(|| super::resource_client::data_uri_response(&request.url));
            if let Some(result) = uri {
                Box::pin(std::future::ready(result))
            } else {
                transport.execute(request)
            }
        }));
        self.request.borrow_mut().options.cancel_function = Some(Rc::new(move || {
            if let Some(request) = &*cancel_request.borrow() {
                cancel_transport.cancel(request);
            }
        }));
        Ok(scheduler.request(self.request.clone()))
    }

    fn http_request(&self, options: &FetchOptions, trusted: &TrustedServers) -> HttpRequest {
        let url = self.url();
        let mut request = HttpRequest {
            id: 0,
            kind: options.kind.clone(),
            with_credentials: trusted.contains(&url, &options.page_protocol),
            url,
            method: if options.method.is_empty() { "GET".into() } else { options.method.clone() },
            headers: combine(&options.headers, &self.options.borrow().headers),
            body: options.body.clone(),
        };
        if let Some(transform) = &self.options.borrow().request_transform { transform(&mut request); }
        request
    }
}

impl std::fmt::Display for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url())
    }
}

impl Default for Resource {
    fn default() -> Self {
        Self::new("").expect("empty relative URI")
    }
}
