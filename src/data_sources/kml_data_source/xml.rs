//! XML query, namespace, color, coordinate, and URI-resolution helpers for KML.
use crate::core::{
    color::ColorRandomOptions, resource::DerivedResourceOptions, uri::Uri, Cartesian3, Color, Ellipsoid, Resource,
};
use regex::Regex;
use roxmltree::Node;
use std::{collections::HashMap, sync::OnceLock};

pub(crate) const KML_NAMESPACES: &[Option<&str>] = &[
    None,
    Some(""),
    Some("http://www.opengis.net/kml/2.2"),
    Some("http://earth.google.com/kml/2.2"),
    Some("http://earth.google.com/kml/2.1"),
    Some("http://earth.google.com/kml/2.0"),
];

pub(crate) const GX_NAMESPACES: &[Option<&str>] = &[Some("http://www.google.com/kml/ext/2.2")];

pub(crate) const ATOM_NAMESPACES: &[Option<&str>] = &[Some("http://www.w3.org/2005/Atom")];

pub(crate) const KML_GX_NAMESPACES: &[Option<&str>] = &[
    None,
    Some(""),
    Some("http://www.opengis.net/kml/2.2"),
    Some("http://earth.google.com/kml/2.2"),
    Some("http://earth.google.com/kml/2.1"),
    Some("http://earth.google.com/kml/2.0"),
    Some("http://www.google.com/kml/ext/2.2"),
];

/// In-memory map of KMZ archive entries (filename -> data URI).
#[derive(Clone, Debug, Default)]
pub(crate) struct UriResolver {
    pub(crate) entries: HashMap<String, String>,
    pub(crate) keys: Vec<String>,
}

impl UriResolver {
    pub(crate) fn insert(&mut self, key: String, data_uri: String) {
        if !self.entries.contains_key(&key) {
            self.keys.push(key.clone());
        }
        self.entries.insert(key, data_uri);
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

/// Inserts missing common XML namespace declarations (such as `xmlns:xsi`) onto `<kml`.
pub(crate) fn insert_namespaces(text: &str) -> String {
    let mut out = text.to_owned();
    let pairs = [("xsi", "http://www.w3.org/2001/XMLSchema-instance")];
    for (prefix, uri) in pairs {
        let uses_prefix = out.contains(&format!("<{prefix}:")) || out.contains(&format!(" {prefix}:"));
        let declaration = format!("xmlns:{prefix}=");
        if uses_prefix && !out.contains(&declaration) {
            if let Some(pos) = out.find("<kml") {
                let split = pos + 4;
                let mut rebuilt = String::with_capacity(out.len() + declaration.len() + uri.len() + 3);
                rebuilt.push_str(&out[..split]);
                rebuilt.push_str(&format!(" {declaration}\"{uri}\""));
                rebuilt.push_str(&out[split..]);
                out = rebuilt;
            }
        }
    }
    out
}

/// Removes duplicate `xmlns:` attribute declarations inside the root element tag.
pub(crate) fn remove_duplicate_namespaces(text: &str) -> String {
    let mut out = text.to_owned();
    let Some(first_xmlns) = out.find("xmlns:") else {
        return out;
    };
    let mut index = first_xmlns;
    while let Some(end_decl_rel) = out[index..].find('>') {
        let end_decl = index + end_decl_rel;
        if index >= end_decl {
            break;
        }
        let Some(quote_rel) = out[index..end_decl].find('"') else {
            break;
        };
        let ns_prefix = out[index..index + quote_rel].to_owned();
        let start_index = index;
        if let Some(dup_rel) = out[start_index + 1..end_decl].find(&ns_prefix) {
            let dup_start = start_index + 1 + dup_rel;
            if let Some(q1_rel) = out[dup_start..].find('"') {
                let after_q1 = dup_start + q1_rel + 1;
                if let Some(q2_rel) = out[after_q1..].find('"') {
                    let dup_end = after_q1 + q2_rel;
                    let cut_start = dup_start.saturating_sub(1);
                    let mut rebuilt = String::with_capacity(out.len());
                    rebuilt.push_str(&out[..cut_start]);
                    rebuilt.push_str(&out[dup_end + 1..]);
                    out = rebuilt;
                    if let Some(next_rel) = out[start_index.saturating_sub(1)..].find("xmlns:") {
                        index = start_index.saturating_sub(1) + next_rel;
                        continue;
                    }
                    break;
                }
            }
        }
        let next_search = start_index + 1;
        let Some(next_rel) = out[next_search..].find("xmlns:") else {
            break;
        };
        let next_idx = next_search + next_rel;
        if next_idx >= end_decl {
            break;
        }
        index = next_idx;
    }
    out
}

pub(crate) fn matches_namespace(node: Node<'_, '_>, namespaces: &[Option<&str>]) -> bool {
    let ns = node.tag_name().namespace();
    namespaces.contains(&ns)
}

pub(crate) fn is_tag(node: Node<'_, '_>, tag: &str, namespaces: &[Option<&str>]) -> bool {
    node.is_element() && node.tag_name().name() == tag && matches_namespace(node, namespaces)
}

pub(crate) fn query_first_node<'a, 'input>(
    node: Node<'a, 'input>,
    tag: &str,
    namespaces: &[Option<&str>],
) -> Option<Node<'a, 'input>> {
    node.children().find(|c| is_tag(*c, tag, namespaces))
}

pub(crate) fn query_child_nodes<'a, 'input>(
    node: Node<'a, 'input>,
    tag: &str,
    namespaces: &[Option<&str>],
) -> Vec<Node<'a, 'input>> {
    node.children().filter(|c| is_tag(*c, tag, namespaces)).collect()
}

pub(crate) fn query_nodes<'a, 'input>(
    node: Node<'a, 'input>,
    tag: &str,
    namespaces: &[Option<&str>],
) -> Vec<Node<'a, 'input>> {
    node.descendants().filter(|c| is_tag(*c, tag, namespaces)).collect()
}

pub(crate) fn node_text_content(node: Node<'_, '_>) -> String {
    let mut text = String::new();
    for desc in node.descendants() {
        if desc.is_text() {
            if let Some(t) = desc.text() {
                text.push_str(t);
            }
        }
    }
    text
}

pub(crate) fn query_string_attribute(node: Node<'_, '_>, attr: &str) -> Option<String> {
    node.attribute(attr).map(str::to_owned)
}

pub(crate) fn js_parse_float(input: &str) -> f64 {
    let s = input.trim_start();
    if s.is_empty() {
        return f64::NAN;
    }
    if s.starts_with("Infinity") || s.starts_with("+Infinity") {
        return f64::INFINITY;
    }
    if s.starts_with("-Infinity") {
        return f64::NEG_INFINITY;
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    if matches!(bytes.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut saw_digit = false;
    while matches!(bytes.get(i), Some(b'0'..=b'9')) {
        saw_digit = true;
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while matches!(bytes.get(i), Some(b'0'..=b'9')) {
            saw_digit = true;
            i += 1;
        }
    }
    if !saw_digit {
        return f64::NAN;
    }
    let exp_start = i;
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let mut saw_exp = false;
        while matches!(bytes.get(i), Some(b'0'..=b'9')) {
            saw_exp = true;
            i += 1;
        }
        if !saw_exp {
            i = exp_start;
        }
    }
    s[..i].parse::<f64>().unwrap_or(f64::NAN)
}

pub(crate) fn query_numeric_attribute(node: Node<'_, '_>, attr: &str) -> Option<f64> {
    let val = node.attribute(attr)?;
    let parsed = js_parse_float(val);
    (!parsed.is_nan()).then_some(parsed)
}

pub(crate) fn query_string_value(node: Node<'_, '_>, tag: &str, namespaces: &[Option<&str>]) -> Option<String> {
    let child = query_first_node(node, tag, namespaces)?;
    Some(node_text_content(child).trim().to_owned())
}

pub(crate) fn query_numeric_value(node: Node<'_, '_>, tag: &str, namespaces: &[Option<&str>]) -> Option<f64> {
    let child = query_first_node(node, tag, namespaces)?;
    let parsed = js_parse_float(&node_text_content(child));
    (!parsed.is_nan()).then_some(parsed)
}

pub(crate) fn query_boolean_value(node: Node<'_, '_>, tag: &str, namespaces: &[Option<&str>]) -> Option<bool> {
    let child = query_first_node(node, tag, namespaces)?;
    let value = node_text_content(child);
    let trimmed = value.trim();
    Some(trimmed == "1" || trimmed.eq_ignore_ascii_case("true"))
}

fn parse_hex_byte(slice: Option<&str>) -> f64 {
    let Some(s) = slice else {
        return f64::NAN;
    };
    let s = s.trim_start();
    let hex_len = s.bytes().take_while(u8::is_ascii_hexdigit).count();
    if hex_len == 0 {
        return f64::NAN;
    }
    u32::from_str_radix(&s[..hex_len], 16).map_or(f64::NAN, |v| f64::from(v) / 255.0)
}

/// Parses a KML `aabbggrr` color string (with optional leading `#`).
pub(crate) fn parse_color_string(value: Option<&str>, is_random: bool) -> Option<Color> {
    let value = value?;
    if value.trim().is_empty() {
        return None;
    }
    let hex = value.strip_prefix('#').unwrap_or(value);
    let sub = |start: usize, end: usize| -> Option<&str> {
        let len = hex.len();
        if start >= len {
            None
        } else {
            Some(&hex[start..end.min(len)])
        }
    };
    let alpha = parse_hex_byte(sub(0, 2));
    let blue = parse_hex_byte(sub(2, 4));
    let green = parse_hex_byte(sub(4, 6));
    let red = parse_hex_byte(sub(6, 8));

    if !is_random {
        return Some(Color::new(red, green, blue, alpha));
    }

    let mut opts = ColorRandomOptions::default();
    if red > 0.0 {
        opts.maximum_red = Some(red);
    } else {
        opts.red = Some(0.0);
    }
    if green > 0.0 {
        opts.maximum_green = Some(green);
    } else {
        opts.green = Some(0.0);
    }
    if blue > 0.0 {
        opts.maximum_blue = Some(blue);
    } else {
        opts.blue = Some(0.0);
    }
    opts.alpha = Some(alpha);
    Some(Color::from_random(&opts))
}

pub(crate) fn query_color_value(node: Node<'_, '_>, tag: &str, namespaces: &[Option<&str>]) -> Option<Color> {
    let value = query_string_value(node, tag, namespaces)?;
    let is_random = query_string_value(node, "colorMode", namespaces).as_deref() == Some("random");
    parse_color_string(Some(&value), is_random)
}

pub(crate) fn read_coordinate(value: Option<&str>, ellipsoid: &Ellipsoid) -> Cartesian3 {
    static DIGIT_RE: OnceLock<Regex> = OnceLock::new();
    let Some(value) = value else {
        return Cartesian3::from_degrees(0.0, 0.0, 0.0, ellipsoid);
    };
    let re = DIGIT_RE.get_or_init(|| Regex::new(r"[^\s,\n]+").expect("valid coordinate regex"));
    let mut parts = re.find_iter(value).map(|m| m.as_str());
    let lon_raw = parts.next().map_or(f64::NAN, js_parse_float);
    let lat_raw = parts.next().map_or(f64::NAN, js_parse_float);
    let alt_raw = parts.next().map_or(f64::NAN, js_parse_float);
    let lon = if lon_raw.is_nan() { 0.0 } else { lon_raw };
    let lat = if lat_raw.is_nan() { 0.0 } else { lat_raw };
    let alt = if alt_raw.is_nan() { 0.0 } else { alt_raw };
    Cartesian3::from_degrees(lon, lat, alt, ellipsoid)
}

pub(crate) fn read_coordinates(node: Option<Node<'_, '_>>, ellipsoid: &Ellipsoid) -> Option<Vec<Cartesian3>> {
    static TUPLE_RE: OnceLock<Regex> = OnceLock::new();
    let node = node?;
    let text = node_text_content(node);
    let re = TUPLE_RE.get_or_init(|| Regex::new(r"[^\s\n]+").expect("valid tuple regex"));
    let tuples: Vec<&str> = re.find_iter(&text).map(|m| m.as_str()).collect();
    if tuples.is_empty() {
        return None;
    }
    Some(tuples.into_iter().map(|t| read_coordinate(Some(t), ellipsoid)).collect())
}

pub(crate) fn resolve_href(
    href: Option<&str>,
    source_resource: &Resource,
    uri_resolver: Option<&UriResolver>,
) -> Option<Resource> {
    let href = href?;
    if let Some(resolver) = uri_resolver {
        let normalized = href.replace('\\', "/");
        if let Some(data_uri) = resolver.get(&normalized) {
            return Resource::new(data_uri);
        }
        let base_str = source_resource.get_url_component(false, false);
        let uri = Uri::parse(&normalized);
        if let Some(abs) = uri.absolute_to(&base_str) {
            let abs_str = abs.to_string();
            if let Some(data_uri) = resolver.get(&abs_str) {
                return Resource::new(data_uri);
            }
        }
    }
    source_resource
        .get_derived_resource(DerivedResourceOptions { url: Some(href.to_owned()), ..Default::default() })
        .or_else(|| Resource::new(href))
}
