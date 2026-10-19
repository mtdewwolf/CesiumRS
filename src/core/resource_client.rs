//! Execution of Resource operations through a shared, automatically driven
//! scheduler. Retry attempts release their slots and pass through scheduling
//! again; cancellation never calls the retry callback.
use super::query::QueryValue;
use super::resource::{FetchOptions, HttpResponse, RequestKind, ResourceTransport};
use super::{RequestErrorEvent, RequestScheduler, RequestState, Resource, RuntimeError, TrustedServers};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::Pin,
    rc::Rc,
    task::Poll,
};

pub type ResourceFuture<T> = Pin<Box<dyn Future<Output = Result<T, RequestErrorEvent>>>>;
pub fn request_error(message: impl Into<String>) -> RequestErrorEvent {
    RequestErrorEvent::new(None, Some(message.into().into_bytes()), None)
}
#[derive(Clone, Debug)]
pub struct ResourceBlob {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}
#[derive(Clone, Debug)]
pub struct XmlDocument {
    source: String,
}
impl XmlDocument {
    pub fn parse(source: String) -> Result<Self, RequestErrorEvent> {
        roxmltree::Document::parse_with_options(
            &source,
            roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
        )
        .map_err(|e| request_error(e.to_string()))?;
        Ok(Self { source })
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn document(&self) -> roxmltree::Document<'_> {
        roxmltree::Document::parse_with_options(
            &self.source,
            roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
        )
        .expect("validated XML")
    }
}
#[derive(Clone)]
pub struct ResourceClient {
    pub scheduler: Rc<RefCell<RequestScheduler<HttpResponse>>>,
    pub trusted_servers: Rc<RefCell<TrustedServers>>,
    pub transport: Rc<dyn ResourceTransport>,
    pub page_protocol: String,
}
impl ResourceClient {
    pub fn new(transport: Rc<dyn ResourceTransport>) -> Self {
        Self {
            scheduler: Rc::new(RefCell::new(RequestScheduler::new())),
            trusted_servers: Rc::new(RefCell::new(TrustedServers::new())),
            transport,
            page_protocol: String::new(),
        }
    }
    fn start(
        &self,
        resource: &Resource,
        options: FetchOptions,
    ) -> Result<Option<super::ScheduledRequest<HttpResponse>>, RequestErrorEvent> {
        let id = next_id();
        resource.fetch_once(
            &mut self.scheduler.borrow_mut(),
            self.transport.clone(),
            self.trusted_servers.clone(),
            options,
            id,
        )
    }
    pub fn fetch(&self, resource: &Resource, options: FetchOptions) -> Option<ResourceFuture<HttpResponse>> {
        let fetch = self.fetch_optional(resource, options)?;
        Some(Box::pin(async move { fetch.await?.ok_or_else(|| request_error("Request produced no response.")) }))
    }
    /// Exact optional-result form: a retry rejected by the scheduler resolves
    /// to None, matching Cesium's undefined result rather than failing I/O.
    pub fn fetch_optional(
        &self,
        resource: &Resource,
        mut options: FetchOptions,
    ) -> Option<ResourceFuture<Option<HttpResponse>>> {
        if options.page_protocol.is_empty() {
            options.page_protocol = self.page_protocol.clone();
        }
        let first = match self.start(resource, options.clone()) {
            Ok(None) => return None,
            Ok(Some(response)) => Ok(response),
            Err(error) => Err(error),
        };
        let client = self.clone();
        let resource = resource.share();
        Some(Box::pin(async move {
            let mut response = first?;
            loop {
                let result = poll_fn(|cx| {
                    if let Poll::Ready(result) = Pin::new(&mut response).poll(cx) {
                        return Poll::Ready(result);
                    }
                    client.scheduler.borrow_mut().poll_update(cx);
                    Pin::new(&mut response).poll(cx)
                })
                .await;
                resource.request.borrow_mut().options.cancel_function = None;
                match result {
                    Ok(value) => {
                        return Ok(if value.status == 204 && !matches!(options.method.as_str(), "HEAD" | "OPTIONS") {
                            None
                        } else {
                            Some(value)
                        })
                    }
                    Err(error) => {
                        if resource.request.borrow().state != RequestState::Failed
                            || !resource.retry_on_error(&error).await
                        {
                            return Err(error);
                        }
                        resource.request.borrow_mut().state = RequestState::Unissued;
                        resource.request.borrow_mut().deferred = None;
                        let next = client.start(&resource, options.clone())?;
                        let Some(next) = next else {
                            return Ok(None);
                        };
                        response = next;
                    }
                }
            }
        }))
    }
    fn map_optional<T: 'static>(
        &self,
        resource: &Resource,
        options: FetchOptions,
        map: impl FnOnce(HttpResponse) -> Result<T, RequestErrorEvent> + 'static,
    ) -> Option<ResourceFuture<Option<T>>> {
        let fetch = self.fetch_optional(resource, options)?;
        Some(Box::pin(async move { fetch.await?.map(map).transpose() }))
    }
    pub fn fetch_array_buffer_optional(&self, resource: &Resource) -> Option<ResourceFuture<Option<Vec<u8>>>> {
        self.map_optional(resource, FetchOptions::default(), |r| Ok(r.body))
    }
    pub fn fetch_text_optional(&self, resource: &Resource) -> Option<ResourceFuture<Option<String>>> {
        let url = resource.url();
        self.map_optional(resource, FetchOptions::default(), move |r| decode_text(&url, &r))
    }
    pub fn fetch_blob_optional(&self, resource: &Resource) -> Option<ResourceFuture<Option<ResourceBlob>>> {
        self.map_optional(resource, FetchOptions::default(), |r| {
            Ok(ResourceBlob { mime_type: blob_type(&r), bytes: r.body })
        })
    }
    pub fn fetch_json_optional(&self, resource: &Resource) -> Option<ResourceFuture<Option<serde_json::Value>>> {
        let url = resource.url();
        self.map_optional(
            resource,
            FetchOptions {
                headers: vec![("Accept".into(), "application/json,*/*;q=0.01".into())],
                ..Default::default()
            },
            move |r| serde_json::from_str(&decode_text(&url, &r)?).map_err(|e| request_error(e.to_string())),
        )
    }
    pub fn fetch_xml_optional(&self, resource: &Resource) -> Option<ResourceFuture<Option<XmlDocument>>> {
        let url = resource.url();
        self.map_optional(resource, FetchOptions::default(), move |r| XmlDocument::parse(decode_text(&url, &r)?))
    }
    pub fn fetch_array_buffer(&self, resource: &Resource) -> Option<ResourceFuture<Vec<u8>>> {
        let fetch = self.fetch(resource, FetchOptions::default())?;
        Some(Box::pin(async move { Ok(fetch.await?.body) }))
    }
    pub fn fetch_blob(&self, resource: &Resource) -> Option<ResourceFuture<ResourceBlob>> {
        let fetch = self.fetch(resource, FetchOptions::default())?;
        Some(Box::pin(async move {
            let response = fetch.await?;
            Ok(ResourceBlob { mime_type: blob_type(&response), bytes: response.body })
        }))
    }
    pub fn fetch_text(&self, resource: &Resource) -> Option<ResourceFuture<String>> {
        let url = resource.url();
        let fetch = self.fetch(resource, FetchOptions::default())?;
        Some(Box::pin(async move {
            let response = fetch.await?;
            decode_text(&url, &response)
        }))
    }
    pub fn fetch_json(&self, resource: &Resource) -> Option<ResourceFuture<serde_json::Value>> {
        let url = resource.url();
        let options = FetchOptions {
            headers: vec![("Accept".into(), "application/json,*/*;q=0.01".into())],
            ..Default::default()
        };
        let fetch = self.fetch(resource, options)?;
        Some(Box::pin(async move {
            serde_json::from_str(&decode_text(&url, &fetch.await?)?).map_err(|e| request_error(e.to_string()))
        }))
    }
    pub fn fetch_xml(&self, resource: &Resource) -> Option<ResourceFuture<XmlDocument>> {
        let fetch = self.fetch_text(resource)?;
        Some(Box::pin(async move { XmlDocument::parse(fetch.await?) }))
    }
    pub fn fetch_jsonp_optional(
        &self,
        resource: &Resource,
        callback_parameter: Option<&str>,
    ) -> Option<ResourceFuture<Option<serde_json::Value>>> {
        let parameter = callback_parameter.unwrap_or("callback");
        let function = format!("loadJsonp{}", next_id());
        resource.set_query_parameters(&vec![(parameter.into(), Some(QueryValue::Single(function.clone())))], false);
        let fetch = self.fetch_optional(
            resource,
            FetchOptions {
                kind: RequestKind::Jsonp { callback_parameter: parameter.into(), function_name: function.clone() },
                ..Default::default()
            },
        )?;
        Some(Box::pin(async move {
            let Some(response) = fetch.await? else {
                return Ok(None);
            };
            let text = String::from_utf8_lossy(&response.body);
            parse_jsonp(&text, &function).map(Some)
        }))
    }
    pub fn fetch_jsonp(
        &self,
        resource: &Resource,
        callback_parameter: Option<&str>,
    ) -> Option<ResourceFuture<serde_json::Value>> {
        let fetch = self.fetch_jsonp_optional(resource, callback_parameter)?;
        Some(Box::pin(async move { fetch.await?.ok_or_else(|| request_error("JSONP callback returned undefined.")) }))
    }
    pub fn fetch_image<H: super::ImageHost + 'static>(
        &self,
        resource: &Resource,
        host: Rc<H>,
        options: super::image_helpers::FetchImageOptions,
    ) -> Option<ResourceFuture<Option<H::Image>>> {
        let direct =
            resource.is_data_uri() || resource.is_blob_uri() || (!resource.has_headers() && !options.prefer_blob);
        if direct {
            let result = Rc::new(RefCell::new(None));
            let mut client = self.clone();
            client.transport =
                Rc::new(ImageTransport { base: self.transport.clone(), host, result: result.clone(), options });
            let fetch =
                client.fetch_optional(resource, FetchOptions { kind: RequestKind::Image, ..Default::default() })?;
            return Some(Box::pin(async move {
                if fetch.await?.is_none() {
                    return Ok(None);
                }
                Ok(result.borrow_mut().take())
            }));
        }
        let fetch = self.fetch_blob_optional(resource)?;
        Some(Box::pin(async move {
            let Some(blob) = fetch.await? else {
                return Ok(None);
            };
            let decode = host.decode_resource_image(&blob.bytes, &blob.mime_type, options, true);
            let Some(decode) = decode else {
                return Ok(None);
            };
            decode.await.map(Some).map_err(|e| request_error(e.message))
        }))
    }
    pub fn fetch_headers(&self, resource: &Resource, method: &str) -> Option<ResourceFuture<Vec<(String, String)>>> {
        let fetch = self.fetch(resource, FetchOptions { method: method.into(), ..Default::default() })?;
        Some(Box::pin(async move { Ok(fetch.await?.headers) }))
    }
    pub fn request_method(
        &self,
        resource: &Resource,
        method: &str,
        body: Option<Vec<u8>>,
    ) -> Option<ResourceFuture<HttpResponse>> {
        self.fetch(resource, FetchOptions { method: method.into(), body, ..Default::default() })
    }
}
impl super::geocoder_service::GeocoderHost for ResourceClient {
    fn fetch_json<'a>(
        &'a self,
        resource: Resource,
        jsonp: Option<&'static str>,
    ) -> super::geocoder_service::GeocoderFuture<'a, serde_json::Value> {
        let future = match jsonp {
            Some(parameter) => self.fetch_jsonp(&resource, Some(parameter)),
            None => self.fetch_json(&resource),
        };
        Box::pin(async move {
            future
                .ok_or_else(|| RuntimeError::new("Geocoder request was throttled."))?
                .await
                .map_err(|e| RuntimeError::new(e.to_string()))
        })
    }
}
impl super::ktx2_transcoder::Ktx2Loader for ResourceClient {
    fn load<'a>(&'a self, resource: &'a Resource) -> Option<super::ktx2_transcoder::Ktx2Future<'a, Vec<u8>>> {
        let fetch = self.fetch_array_buffer(resource)?;
        Some(Box::pin(async move { fetch.await.map_err(|e| RuntimeError::new(e.to_string())) }))
    }
}
fn content_type(response: &HttpResponse) -> String {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("Content-Type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}
fn blob_type(response: &HttpResponse) -> String {
    let mime = content_type(response);
    if mime.bytes().all(|b| matches!(b, 0x20..=0x7e)) {
        mime.to_ascii_lowercase()
    } else {
        String::new()
    }
}
fn decode_text(url: &str, response: &HttpResponse) -> Result<String, RequestErrorEvent> {
    if super::uri_helpers::is_data_uri(url) {
        return decode_data_uri_text(url);
    }
    let content_type = content_type(response);
    let charset = content_type.split(';').skip(1).find_map(|parameter| {
        let (key, value) = parameter.split_once('=')?;
        key.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches(['\"', '\'']).as_bytes())
    });
    let encoding = charset.and_then(encoding_rs::Encoding::for_label).unwrap_or(encoding_rs::UTF_8);
    Ok(encoding.decode(&response.body).0.into_owned())
}
pub fn parse_jsonp(text: &str, function: &str) -> Result<serde_json::Value, RequestErrorEvent> {
    let mut rest = text.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);
    while rest.starts_with("/*") {
        let end = rest.find("*/").ok_or_else(|| request_error("Invalid JSONP response."))?;
        rest = rest[end + 2..].trim_start();
    }
    let payload = rest
        .strip_prefix(function)
        .and_then(|s| s.trim_start().strip_prefix('('))
        .ok_or_else(|| request_error("Invalid JSONP callback."))?;
    let mut stream = serde_json::Deserializer::from_str(payload).into_iter::<serde_json::Value>();
    let value = stream
        .next()
        .ok_or_else(|| request_error("Missing JSONP value."))?
        .map_err(|e| request_error(e.to_string()))?;
    let tail = payload[stream.byte_offset()..].trim();
    if tail != ")" && tail != ");" {
        return Err(request_error("Invalid JSONP response suffix."));
    }
    Ok(value)
}
fn decode_data_uri(uri: &str) -> Result<(String, String, bool), RequestErrorEvent> {
    let uri = uri.get(5..).ok_or_else(|| request_error("Invalid data URI."))?;
    let (media, data) = uri.split_once(',').ok_or_else(|| request_error("Invalid data URI."))?;
    let base64 = media.ends_with(";base64");
    let media = if base64 { &media[..media.len() - 7] } else { media };
    let decoded = super::uri::decode_uri_component(data).ok_or_else(|| request_error("Invalid data URI encoding."))?;
    Ok((if media.is_empty() { "text/plain;charset=US-ASCII".into() } else { media.into() }, decoded, base64))
}
fn decode_base64(data: &str) -> Result<Vec<u8>, RequestErrorEvent> {
    use base64::{
        alphabet::STANDARD,
        engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
        Engine,
    };
    let cleaned: Vec<_> = data.bytes().filter(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)).collect();
    GeneralPurpose::new(
        &STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    )
    .decode(cleaned)
    .map_err(|e| request_error(e.to_string()))
}
pub fn decode_data_uri_text(uri: &str) -> Result<String, RequestErrorEvent> {
    let (_, data, base64) = decode_data_uri(uri)?;
    if base64 {
        Ok(decode_base64(&data)?.into_iter().map(char::from).collect())
    } else {
        Ok(data)
    }
}
pub fn data_uri_response(uri: &str) -> Result<HttpResponse, RequestErrorEvent> {
    let (media, data, base64) = decode_data_uri(uri)?;
    let body = if base64 { decode_base64(&data)? } else { data.encode_utf16().map(|u| u as u8).collect() };
    Ok(HttpResponse { status: 200, body, headers: vec![("Content-Type".into(), media)] })
}

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

struct ImageTransport<H: super::ImageHost> {
    base: Rc<dyn ResourceTransport>,
    host: Rc<H>,
    result: Rc<RefCell<Option<H::Image>>>,
    options: super::image_helpers::FetchImageOptions,
}
impl<H: super::ImageHost + 'static> ResourceTransport for ImageTransport<H> {
    fn execute(&self, request: super::resource::HttpRequest) -> super::RequestFuture<HttpResponse> {
        let host = self.host.clone();
        let result = self.result.clone();
        let options = self.options;
        let base = self.base.clone();
        Box::pin(async move {
            let url = request.url.clone();
            let image = if let Some(load) = host.load_resource_url(&url, request.with_credentials, options) {
                load.await.map_err(|e| request_error(e.message))?
            } else {
                let response = if super::uri_helpers::is_data_uri(&request.url) {
                    data_uri_response(&request.url)?
                } else {
                    base.execute(request).await?
                };
                let mime = content_type(&response);
                let decode = host
                    .decode_resource_image(&response.body, &mime, options, false)
                    .ok_or_else(|| request_error("Image decode was deferred."))?;
                decode.await.map_err(|e| request_error(e.message))?
            };
            *result.borrow_mut() = Some(image);
            Ok(HttpResponse { status: 200, body: Vec::new(), headers: Vec::new() })
        })
    }
    fn cancel(&self, request: &super::resource::HttpRequest) {
        self.base.cancel(request);
    }
}
