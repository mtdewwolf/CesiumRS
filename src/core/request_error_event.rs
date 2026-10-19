//! CesiumJS `RequestErrorEvent`, with a typed response body.

use super::query::parse_response_headers;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestErrorEvent<R = Vec<u8>> {
    pub status_code: Option<u16>,
    pub response: Option<R>,
    pub response_headers: Option<Vec<(String, String)>>,
}

impl<R> RequestErrorEvent<R> {
    pub fn new(status_code: Option<u16>, response: Option<R>, response_headers: Option<Vec<(String, String)>>) -> Self {
        Self { status_code, response, response_headers }
    }

    pub fn from_header_string(status_code: Option<u16>, response: Option<R>, headers: Option<&str>) -> Self {
        Self::new(status_code, response, headers.map(parse_response_headers))
    }
}

impl<R> std::fmt::Display for RequestErrorEvent<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Request has failed.")?;
        if let Some(status) = self.status_code {
            write!(f, " Status Code: {status}")?;
        }
        Ok(())
    }
}

impl<R: std::fmt::Debug> std::error::Error for RequestErrorEvent<R> {}
