//! CesiumJS `objectToQuery`, `queryToObject`, and `parseResponseHeaders`.
//!
//! JavaScript objects become ordered lists of entries, in the order the
//! object's keys would enumerate: integer-like keys ascending, then the rest
//! in insertion order.

use super::uri::{decode_uri_component, encode_uri_component};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryValue {
    Single(String),
    Multiple(Vec<String>),
}

/// Whether `key` is an array index, which JavaScript enumerates first.
fn is_array_index(key: &str) -> bool {
    key.parse::<u32>().is_ok_and(|n| n != u32::MAX && n.to_string() == key)
}

/// Orders entries the way JavaScript enumerates an object's keys.
pub(crate) fn js_object_order<V>(mut entries: Vec<(String, V)>) -> Vec<(String, V)> {
    let mut indices: Vec<(String, V)> = Vec::new();
    let mut rest = Vec::new();
    for e in entries.drain(..) {
        if is_array_index(&e.0) {
            indices.push(e);
        } else {
            rest.push(e);
        }
    }
    indices.sort_by_key(|e| e.0.parse::<u32>().unwrap());
    indices.extend(rest);
    indices
}

/// `name=value&…`, with array values repeated.
pub fn object_to_query(entries: &[(String, QueryValue)]) -> String {
    let mut result = String::new();
    for (name, value) in js_object_order(entries.to_vec()) {
        let part = format!("{}=", encode_uri_component(&name));
        match value {
            QueryValue::Single(v) => result.push_str(&format!("{part}{}&", encode_uri_component(&v))),
            QueryValue::Multiple(vs) => {
                for v in vs {
                    result.push_str(&format!("{part}{}&", encode_uri_component(&v)));
                }
            }
        }
    }
    result.pop();
    result
}

/// Parses `a=1&b=2;b=3`; repeated names collect their values. `None` where
/// Cesium's `decodeURIComponent` throws.
pub fn query_to_object(query_string: &str) -> Option<Vec<(String, QueryValue)>> {
    let mut result: Vec<(String, QueryValue)> = Vec::new();
    if query_string.is_empty() {
        return Some(result);
    }
    for part in query_string.replace('+', "%20").split(['&', ';']) {
        let mut sub = part.split('=');
        let name = decode_uri_component(sub.next().unwrap_or(""))?;
        let value = match sub.next() {
            Some(v) => decode_uri_component(v)?,
            None => String::new(),
        };
        match result.iter_mut().find(|(n, _)| *n == name) {
            Some((_, QueryValue::Single(first))) => {
                let first = std::mem::take(first);
                let slot = result.iter_mut().find(|(n, _)| *n == name).unwrap();
                slot.1 = QueryValue::Multiple(vec![first, value]);
            }
            Some((_, QueryValue::Multiple(values))) => values.push(value),
            None => result.push((name, QueryValue::Single(value))),
        }
    }
    Some(js_object_order(result))
}

/// Parses `getAllResponseHeaders()` text: `Name: value` lines separated by CRLF.
pub fn parse_response_headers(header_string: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for pair in header_string.split("\r\n") {
        if let Some(index) = pair.find(": ").filter(|&i| i > 0) {
            let (key, val) = (pair[..index].to_string(), pair[index + 2..].to_string());
            match headers.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = val,
                None => headers.push((key, val)),
            }
        }
    }
    js_object_order(headers)
}
