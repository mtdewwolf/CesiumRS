//! CesiumJS `ImageryLayerFeatureInfo`: what an imagery provider reports
//! about a picked feature, with helpers that pick a name and build an HTML
//! description from the feature's properties.

use super::gltf_pipeline::js;
use super::FrameResource;
use crate::core::Cartographic;
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageryLayerFeatureInfo {
    /// Usually a string, but it is the property value as found.
    pub name: Option<Value>,
    /// HTML.
    pub description: Option<String>,
    pub position: Option<Cartographic>,
    /// The raw feature data.
    pub data: Option<Value>,
    /// The feature's properties, as the provider reported them.
    pub properties: Option<Value>,
    /// The imagery layer the feature came from.
    pub imagery_layer: Option<FrameResource>,
}

/// JavaScript truthiness of a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `for (const key in value)` over own properties, with each value. A
/// string's keys are its UTF-16 code unit indices.
fn entries(value: &Value) -> Vec<(String, Value)> {
    match value {
        Value::String(s) => s
            .encode_utf16()
            .enumerate()
            .map(|(i, unit)| (i.to_string(), Value::String(String::from_utf16_lossy(&[unit]))))
            .collect(),
        _ => js::for_in(Some(value))
            .into_iter()
            .map(|k| {
                let v = js::get(Some(value), &k).cloned().unwrap_or(Value::Null);
                (k, v)
            })
            .collect(),
    }
}

impl ImageryLayerFeatureInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Names the feature from the first truthy property called `name`, else
    /// `title`, else one containing "name", else one containing "title",
    /// ignoring case.
    pub fn configure_name_from_properties(&mut self, properties: &Value) {
        let mut precedence = 10;
        let mut name_property = None;
        for (key, value) in entries(properties) {
            if !truthy(&value) {
                continue;
            }
            let lower = key.to_lowercase();
            // The regular expressions are case-insensitive without the u
            // flag, which folds ASCII letters only.
            let ascii = key.to_ascii_lowercase();
            let rank = if precedence > 1 && lower == "name" {
                1
            } else if precedence > 2 && lower == "title" {
                2
            } else if precedence > 3 && ascii.contains("name") {
                3
            } else if precedence > 4 && ascii.contains("title") {
                4
            } else {
                continue;
            };
            precedence = rank;
            name_property = Some(value);
        }
        if let Some(name) = name_property {
            self.name = Some(name);
        }
    }

    /// An HTML table of the properties; object values become nested tables
    /// and `null` values are left out.
    pub fn configure_description_from_properties(&mut self, properties: &Value) {
        fn describe(properties: &Value) -> String {
            let mut html = String::from("<table class=\"cesium-infoBox-defaultTable\">");
            for (key, value) in entries(properties) {
                match &value {
                    Value::Null => {}
                    Value::Array(_) | Value::Object(_) => {
                        html += &format!("<tr><td>{key}</td><td>{}</td></tr>", describe(&value));
                    }
                    other => html += &format!("<tr><td>{key}</td><td>{}</td></tr>", js::key(other)),
                }
            }
            html + "</table>"
        }
        self.description = Some(describe(properties));
    }
}
