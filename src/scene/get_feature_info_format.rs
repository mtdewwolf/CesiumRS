//! CesiumJS `GetFeatureInfoFormat`: how a WMS provider interprets a
//! GetFeatureInfo response, and the default JSON, XML and text parsers.
//!
//! Callbacks are `Rc` closures. The defaults are exposed as functions so they
//! can be validated directly and reused by custom formats.

use super::ImageryLayerFeatureInfo;
use crate::core::{Cartographic, RuntimeError, XmlDocument};
use roxmltree::Node;
use serde_json::Value;
use std::rc::Rc;

pub const MAPINFO_MXP_NAMESPACE: &str = "http://www.mapinfo.com/mxp";
pub const ESRI_WMS_NAMESPACE: &str = "http://www.esri.com/wms";
pub const WFS_NAMESPACE: &str = "http://www.opengis.net/wfs";
pub const GML_NAMESPACE: &str = "http://www.opengis.net/gml";

/// Cesium's response type. Custom values are passed to the server as the
/// format string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeatureInfoType {
    Json,
    Xml,
    Html,
    Text,
    Other(String),
}

impl FeatureInfoType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Json => "json",
            Self::Xml => "xml",
            Self::Html => "html",
            Self::Text => "text",
            Self::Other(value) => value,
        }
    }

    /// Cesium's default request format for the built-in types.
    pub fn default_format(&self) -> Option<&'static str> {
        match self {
            Self::Json => Some("application/json"),
            Self::Xml => Some("text/xml"),
            Self::Html => Some("text/html"),
            Self::Text => Some("text/plain"),
            Self::Other(_) => None,
        }
    }
}

/// A parsed GetFeatureInfo response.
#[derive(Clone, Debug)]
pub enum FeatureInfoResponse {
    Json(Value),
    Xml(XmlDocument),
    Text(String),
    /// A custom response type, returned as the raw response body.
    Bytes(Vec<u8>),
}

impl PartialEq for FeatureInfoResponse {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Json(a), Self::Json(b)) => a == b,
            (Self::Xml(a), Self::Xml(b)) => a.source() == b.source(),
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            _ => false,
        }
    }
}

pub type FeatureInfoCallback =
    Rc<dyn Fn(FeatureInfoResponse) -> Result<Option<Vec<ImageryLayerFeatureInfo>>, RuntimeError>>;

/// CesiumJS `GetFeatureInfoFormat`.
#[derive(Clone)]
pub struct GetFeatureInfoFormat {
    pub response_type: FeatureInfoType,
    pub format: String,
    pub callback: FeatureInfoCallback,
}

impl std::fmt::Debug for GetFeatureInfoFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetFeatureInfoFormat")
            .field("response_type", &self.response_type)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl GetFeatureInfoFormat {
    /// Cesium's constructor. `format` and `callback` must be supplied for
    /// custom types; the built-ins have defaults.
    pub fn new(
        response_type: FeatureInfoType,
        format: Option<&str>,
        callback: Option<FeatureInfoCallback>,
    ) -> Result<Self, RuntimeError> {
        let format = match (format, response_type.default_format()) {
            (Some(format), _) => format.to_owned(),
            (None, Some(default)) => default.to_owned(),
            (None, None) => {
                return Err(RuntimeError::new(
                    "format is required when type is not \"json\", \"xml\", \"html\", or \"text\".",
                ))
            }
        };
        let callback = match (callback, &response_type) {
            (Some(callback), _) => callback,
            (None, FeatureInfoType::Json) => Rc::new(|data| match data {
                FeatureInfoResponse::Json(value) => geo_json_to_feature_info(&value),
                _ => Err(RuntimeError::new("Expected a JSON GetFeatureInfo response.")),
            }),
            (None, FeatureInfoType::Xml) => Rc::new(|data| match data {
                FeatureInfoResponse::Xml(value) => xml_to_feature_info(&value),
                _ => Err(RuntimeError::new("Expected an XML GetFeatureInfo response.")),
            }),
            (None, FeatureInfoType::Html) | (None, FeatureInfoType::Text) => Rc::new(|data| match data {
                FeatureInfoResponse::Text(text) => Ok(text_to_feature_info(&text)),
                _ => Err(RuntimeError::new("Expected a text GetFeatureInfo response.")),
            }),
            (None, FeatureInfoType::Other(_)) => {
                return Err(RuntimeError::new(
                    "callback is required when type is not \"json\", \"xml\", \"html\", or \"text\".",
                ))
            }
        };
        Ok(Self { response_type, format, callback })
    }

    pub fn json() -> Self {
        Self::new(FeatureInfoType::Json, None, None).unwrap()
    }
    pub fn xml() -> Self {
        Self::new(FeatureInfoType::Xml, None, None).unwrap()
    }
    pub fn html() -> Self {
        Self::new(FeatureInfoType::Html, None, None).unwrap()
    }
    pub fn text() -> Self {
        Self::new(FeatureInfoType::Text, None, None).unwrap()
    }

    /// Runs the format's callback.
    pub fn callback(
        &self,
        response: FeatureInfoResponse,
    ) -> Result<Option<Vec<ImageryLayerFeatureInfo>>, RuntimeError> {
        (self.callback)(response)
    }
}

/// Cesium's default GeoJSON parser.
pub fn geo_json_to_feature_info(json: &Value) -> Result<Option<Vec<ImageryLayerFeatureInfo>>, RuntimeError> {
    let features = json
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| RuntimeError::new("TypeError: Cannot read properties of undefined (reading 'length')"))?;
    let mut result = Vec::with_capacity(features.len());
    for feature in features {
        let mut feature_info = ImageryLayerFeatureInfo::new();
        feature_info.data = Some(feature.clone());
        let properties = feature.get("properties").cloned().unwrap_or(Value::Null);
        feature_info.properties = Some(properties.clone());
        feature_info.configure_name_from_properties(&properties);
        feature_info.configure_description_from_properties(&properties);
        if let Some(geometry) = feature.get("geometry") {
            if geometry.get("type").and_then(Value::as_str) == Some("Point") {
                if let Some(coordinates) = geometry.get("coordinates").and_then(Value::as_array) {
                    let longitude = coordinates.first().and_then(Value::as_f64).unwrap_or(f64::NAN);
                    let latitude = coordinates.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN);
                    feature_info.position = Some(Cartographic::from_degrees(longitude, latitude, 0.0));
                }
            }
        }
        result.push(feature_info);
    }
    Ok(Some(result))
}

fn local_name<'a, 'i>(node: &Node<'a, 'i>) -> &'a str {
    node.tag_name().name()
}
fn namespace<'a, 'i>(node: &Node<'a, 'i>) -> Option<&'a str> {
    node.tag_name().namespace()
}

/// The element's `textContent`: all descendant text, in document order.
fn text_content(node: &Node) -> String {
    let mut text = String::new();
    fn walk(node: Node, text: &mut String) {
        for child in node.children() {
            if child.is_text() {
                text.push_str(child.text().unwrap_or(""));
            } else if child.is_element() {
                walk(child, text);
            }
        }
    }
    walk(*node, &mut text);
    text
}

fn element_children<'a, 'i>(node: &Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    node.children().filter(Node::is_element).collect()
}

/// All descendant elements with the given local name. `namespace` uses `None`
/// for the DOM `*` wildcard.
fn get_elements_by_tag_name_ns<'a, 'i>(node: Node<'a, 'i>, namespace: Option<&str>, name: &str) -> Vec<Node<'a, 'i>> {
    node.descendants()
        .skip(1)
        .filter(|candidate| candidate.is_element())
        .filter(|candidate| candidate.tag_name().name() == name)
        .filter(|candidate| namespace.is_none_or(|ns| candidate.tag_name().namespace() == Some(ns)))
        .collect()
}

/// Cesium's default XML parser.
pub fn xml_to_feature_info(document: &XmlDocument) -> Result<Option<Vec<ImageryLayerFeatureInfo>>, RuntimeError> {
    let doc = document.document();
    let root = doc.root_element();
    let name = local_name(&root);
    let ns = namespace(&root);
    if name == "MultiFeatureCollection" && ns == Some(MAPINFO_MXP_NAMESPACE) {
        return Ok(Some(map_info_xml_to_feature_info(&root)));
    }
    if name == "FeatureInfoResponse" && ns == Some(ESRI_WMS_NAMESPACE) {
        return Ok(Some(esri_xml_to_feature_info(&root)));
    }
    if name == "FeatureCollection" && ns == Some(WFS_NAMESPACE) {
        return Ok(Some(gml_to_feature_info(&root)));
    }
    if name == "ServiceExceptionReport" {
        return Err(RuntimeError::new(serialize_xml(&root)));
    }
    if name == "msGMLOutput" {
        return Ok(Some(ms_gml_to_feature_info(&root)?));
    }
    Ok(Some(vec![unknown_xml_to_feature_info(document, &root)]))
}

fn feature_info_from_properties(data: Option<Value>, properties: Value) -> ImageryLayerFeatureInfo {
    let mut feature_info = ImageryLayerFeatureInfo::new();
    feature_info.data = data;
    feature_info.properties = Some(properties.clone());
    feature_info.configure_name_from_properties(&properties);
    feature_info.configure_description_from_properties(&properties);
    feature_info
}

fn map_info_xml_to_feature_info(root: &Node) -> Vec<ImageryLayerFeatureInfo> {
    let mut result = Vec::new();
    for feature in get_elements_by_tag_name_ns(*root, Some(MAPINFO_MXP_NAMESPACE), "Feature") {
        let mut properties = serde_json::Map::new();
        for property_element in get_elements_by_tag_name_ns(feature, Some(MAPINFO_MXP_NAMESPACE), "Val") {
            if let Some(name) = property_element.attribute("ref") {
                let value = text_content(&property_element).trim().to_owned();
                properties.insert(name.to_owned(), Value::String(value));
            }
        }
        let properties = Value::Object(properties);
        result.push(feature_info_from_properties(None, properties));
    }
    result
}

fn esri_xml_to_feature_info(root: &Node) -> Vec<ImageryLayerFeatureInfo> {
    let mut result = Vec::new();
    let fields = get_elements_by_tag_name_ns(*root, None, "FIELDS");
    if !fields.is_empty() {
        for feature in fields {
            let mut properties = serde_json::Map::new();
            for attribute in feature.attributes() {
                properties.insert(attribute.name().to_owned(), Value::String(attribute.value().to_owned()));
            }
            let properties = Value::Object(properties);
            result.push(feature_info_from_properties(None, properties));
        }
    } else {
        for element in get_elements_by_tag_name_ns(*root, None, "FeatureInfo") {
            let mut properties = serde_json::Map::new();
            for child in element_children(&element) {
                properties.insert(local_name(&child).to_owned(), Value::String(text_content(&child)));
            }
            let properties = Value::Object(properties);
            result.push(feature_info_from_properties(None, properties));
        }
    }
    result
}

fn gml_to_feature_info(root: &Node) -> Vec<ImageryLayerFeatureInfo> {
    let mut result = Vec::new();
    for member in get_elements_by_tag_name_ns(*root, Some(GML_NAMESPACE), "featureMember") {
        let mut properties = serde_json::Map::new();
        get_gml_properties_recursively(&member, &mut properties);
        result.push(feature_info_from_properties(None, Value::Object(properties)));
    }
    result
}

fn ms_gml_to_feature_info(root: &Node) -> Result<Vec<ImageryLayerFeatureInfo>, RuntimeError> {
    let Some(layer) = element_children(root).into_iter().next() else {
        return Err(RuntimeError::new("Unable to find first child of the feature info xml document"));
    };
    let mut result = Vec::new();
    for member in element_children(&layer) {
        let mut properties = serde_json::Map::new();
        get_gml_properties_recursively(&member, &mut properties);
        result.push(feature_info_from_properties(None, Value::Object(properties)));
    }
    Ok(result)
}

/// Cesium's recursive GML property gatherer. Returns whether the node held a
/// single value, which decides whether the caller records its text.
fn get_gml_properties_recursively(node: &Node, properties: &mut serde_json::Map<String, Value>) -> bool {
    let mut is_single_value = true;
    for child in node.children() {
        if child.is_element() {
            is_single_value = false;
        }
        let name = child.tag_name().name();
        if matches!(name, "Point" | "LineString" | "Polygon" | "boundedBy") {
            continue;
        }
        if child.children().next().is_some() && get_gml_properties_recursively(&child, properties) {
            properties.insert(name.to_owned(), Value::String(text_content(&child)));
        }
    }
    is_single_value
}

fn unknown_xml_to_feature_info(document: &XmlDocument, _root: &Node) -> ImageryLayerFeatureInfo {
    // Cesium serializes the response document and dumps it into a <pre>. The
    // host DOM serializer decides the exact markup; the parsed source is the
    // closest host-independent equivalent.
    let mut feature_info = ImageryLayerFeatureInfo::new();
    feature_info.data = None;
    feature_info.description = Some(format!("<pre>{}</pre>", escape_html(document.source())));
    feature_info
}

/// Cesium's default text/HTML parser.
pub fn text_to_feature_info(text: &str) -> Option<Vec<ImageryLayerFeatureInfo>> {
    // /<body>\s*<\/body>/im
    if contains_ignore_ascii_case_with(text, "<body>", "</body>", true) {
        return None;
    }
    // /<ServiceExceptionReport([\s\S]*)<\/ServiceExceptionReport>/im
    if contains_ignore_ascii_case(text, "<serviceexceptionreport")
        && contains_ignore_ascii_case(text, "</serviceexceptionreport>")
    {
        return None;
    }
    let name = find_title(text);
    let mut feature_info = ImageryLayerFeatureInfo::new();
    feature_info.name = name.map(Value::String);
    feature_info.description = Some(text.to_owned());
    feature_info.data = Some(Value::String(text.to_owned()));
    Some(vec![feature_info])
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

/// `true` when `<start>` and `</end>` occur with only whitespace between them.
fn contains_ignore_ascii_case_with(text: &str, start: &str, end: &str, whitespace_between: bool) -> bool {
    let lower = text.to_ascii_lowercase();
    let start = start.to_ascii_lowercase();
    let end = end.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(index) = lower[offset..].find(&start) {
        let after = offset + index + start.len();
        let rest = &lower[after..];
        let rest_trimmed = if whitespace_between { rest.trim_start() } else { rest };
        if rest_trimmed.starts_with(&end) {
            return true;
        }
        offset = after;
    }
    false
}

fn find_title(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let open = lower.find("<title>")? + "<title>".len();
    let close = lower.rfind("</title>")?;
    (close >= open).then(|| text[open..close].to_owned())
}

/// HTML escaping as a browser's `textContent` → `innerHTML` round trip does.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            other => out.push(other),
        }
    }
    out
}

/// A minimal XML serializer for the response fragments Cesium embeds in the
/// feature description. It preserves the parsed tree, not source whitespace.
pub fn serialize_xml(node: &Node) -> String {
    fn write(node: &Node, out: &mut String) {
        if node.is_text() {
            let text = node.text().unwrap_or("");
            out.push_str(&text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
            return;
        }
        if !node.is_element() {
            return;
        }
        out.push('<');
        out.push_str(node.tag_name().name());
        for attribute in node.attributes() {
            out.push(' ');
            out.push_str(attribute.name());
            out.push_str("=\"");
            out.push_str(&attribute.value().replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;"));
            out.push('"');
        }
        if node.children().next().is_none() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for child in node.children() {
            write(&child, out);
        }
        out.push_str("</");
        out.push_str(node.tag_name().name());
        out.push('>');
    }
    let mut out = String::new();
    write(node, &mut out);
    out
}
