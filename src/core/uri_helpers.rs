//! CesiumJS URI helpers: `getExtensionFromUri`, `getFilenameFromUri`,
//! `getBaseUri`, `getAbsoluteUri`, `appendForwardSlash`, `isDataUri`,
//! `isBlobUri`, and `isCrossOriginUrl`.

use super::uri::Uri;

/// The file extension of the URI's path, without the dot.
pub fn get_extension_from_uri(uri: &str) -> String {
    let mut u = Uri::parse(uri);
    u.normalize();
    let path = u.path();
    let name = &path[path.rfind('/').map_or(0, |i| i + 1)..];
    name.rfind('.').map_or_else(String::new, |i| name[i + 1..].to_string())
}

/// The last segment of the URI's path.
pub fn get_filename_from_uri(uri: &str) -> String {
    let mut u = Uri::parse(uri);
    u.normalize();
    let path = u.path();
    path[path.rfind('/').map_or(0, |i| i + 1)..].to_string()
}

/// Everything up to and including the last slash, optionally followed by
/// the query and fragment.
pub fn get_base_uri(uri: &str, include_query: bool) -> String {
    let mut base_path = uri.rfind('/').map_or_else(String::new, |i| uri[..=i].to_string());
    if !include_query {
        return base_path;
    }
    let u = Uri::parse(uri);
    if !u.query().is_empty() {
        base_path.push('?');
        base_path.push_str(u.query());
    }
    if !u.fragment().is_empty() {
        base_path.push('#');
        base_path.push_str(u.fragment());
    }
    base_path
}

/// `relative` resolved against `base` (Cesium uses the page URL when there is
/// no base; without one, `relative` is returned unchanged).
pub fn get_absolute_uri(relative: &str, base: Option<&str>) -> String {
    let Some(base) = base else {
        return relative.to_string();
    };
    let relative_uri = Uri::parse(relative);
    if !relative_uri.scheme().is_empty() {
        return relative_uri.to_string();
    }
    relative_uri.absolute_to(base).map_or_else(|| relative.to_string(), |u| u.to_string())
}

pub fn append_forward_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    }
}

pub fn is_data_uri(uri: &str) -> bool {
    uri.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:"))
}

pub fn is_blob_uri(uri: &str) -> bool {
    uri.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("blob:"))
}

/// Whether `url`, resolved against the page at `page_url`, has a different
/// protocol or host (with port) than the page. Cesium reads the page from
/// `window.location`.
pub fn is_cross_origin_url(url: &str, page_url: &str) -> bool {
    let origin = |u: &Uri| {
        let mut u = u.clone();
        u.normalize();
        (u.protocol.unwrap_or_default(), u.hostname.unwrap_or_default(), u.port.unwrap_or_default())
    };
    let page = Uri::parse(page_url);
    let target = Uri::parse(&get_absolute_uri(url, Some(page_url)));
    origin(&page) != origin(&target)
}
