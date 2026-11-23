//! Port of CesiumJS `Scene/I3SSymbology`.

use crate::core::color::Color;
use crate::core::resource_client::ResourceClient;
use crate::core::srgb_to_linear::srgb_to_linear;
use crate::scene::i3s_layer::I3SLayer;
use crate::scene::i3s_node::I3SNode;
use crate::scene::scene::Result;
use serde_json::{Map, Value};

/// Port of `I3SSymbology`: symbology parsing for I3S layers. Do not construct
/// directly; access through [`I3SLayer::symbology`].
#[derive(Clone, Debug)]
pub struct I3SSymbology {
    data: Value,
    default_symbology: Option<Value>,
    value_fields: Vec<Option<String>>,
    unique_value_hash: Option<Value>,
    class_breaks_hash: Option<Value>,
}

impl I3SSymbology {
    /// Port of `new I3SSymbology(layer)`.
    pub fn new(layer: &I3SLayer) -> Self {
        let mut symbology = Self {
            data: layer.data().clone(),
            default_symbology: None,
            value_fields: Vec::new(),
            unique_value_hash: None,
            class_breaks_hash: None,
        };
        symbology.parse_layer_symbology();
        symbology
    }

    /// Port of the `defaultSymbology` getter.
    pub fn default_symbology(&self) -> Option<&Value> {
        self.default_symbology.as_ref()
    }

    /// Port of `I3SSymbology.prototype._parseLayerSymbology`.
    fn parse_layer_symbology(&mut self) {
        let drawing_info = self.data.get("drawingInfo");
        let renderer = drawing_info.and_then(|d| d.get("renderer"));
        let Some((drawing_info, renderer)) = drawing_info.zip(renderer) else { return };
        let _ = drawing_info;
        let cached_drawing_info = self.data.get("cachedDrawingInfo");
        let is_color_captured =
            cached_drawing_info.and_then(|c| c.get("color")).and_then(Value::as_bool).unwrap_or(false);
        match renderer.get("type").and_then(Value::as_str) {
            Some("simple") => {
                self.default_symbology = Some(parse_symbol(renderer.get("symbol"), is_color_captured));
            }
            Some("uniqueValue") => {
                self.default_symbology = Some(parse_symbol(renderer.get("defaultSymbol"), is_color_captured));
                self.value_fields.push(renderer.get("field1").and_then(Value::as_str).map(str::to_owned));
                if let Some(field2) = renderer.get("field2").and_then(Value::as_str) {
                    self.value_fields.push(Some(field2.to_string()));
                }
                if let Some(field3) = renderer.get("field3").and_then(Value::as_str) {
                    self.value_fields.push(Some(field3.to_string()));
                }
                self.unique_value_hash = build_unique_value_hash(renderer, is_color_captured);
            }
            Some("classBreaks") => {
                self.default_symbology = Some(parse_symbol(renderer.get("defaultSymbol"), is_color_captured));
                if let Some(field) = renderer.get("field").and_then(Value::as_str) {
                    self.value_fields.push(Some(field.to_string()));
                }
                self.class_breaks_hash = build_class_breaks_hash(renderer, is_color_captured);
            }
            _ => {}
        }
    }

    /// Port of `I3SSymbology.prototype._getSymbology`.
    pub async fn get_symbology(&self, node: &mut I3SNode, client: &ResourceClient) -> Result<Value> {
        let mut symbology = Map::new();
        symbology.insert("default".to_string(), self.default_symbology.clone().unwrap_or(Value::Null));
        if !self.value_fields.is_empty() {
            for field in self.value_fields.iter().flatten() {
                node.load_field(field, client).await?;
            }
            let fields_values: Vec<Vec<Value>> = self
                .value_fields
                .iter()
                .map(|field| node.fields().get(field.as_deref().unwrap_or_default()).map(|f| f.values()).unwrap_or_default())
                .collect();
            let first_field_values = fields_values.first().cloned().unwrap_or_default();
            for feature_index in 0..first_field_values.len() {
                let feature_symbology = if let Some(hash) = &self.unique_value_hash {
                    find_hash_for_unique_values(hash, &fields_values, 0, feature_index)
                } else if let Some(hash) = &self.class_breaks_hash {
                    find_hash_for_class_breaks(hash, &first_field_values, feature_index)
                } else {
                    None
                };
                if let Some(feature_symbology) = feature_symbology {
                    symbology.insert(feature_index.to_string(), feature_symbology);
                }
            }
        }
        Ok(Value::Object(symbology))
    }
}

/// `convertColor`: sRGB to linear for the first three channels; appends the
/// transparency-derived alpha.
fn convert_color(color: &Value, transparency: Option<f64>) -> Value {
    let mut converted: Vec<Value> = Vec::new();
    for (i, component) in color.as_array().map(|a| a.iter()).unwrap_or_default().enumerate() {
        let byte = component.as_u64().unwrap_or_default() as f64;
        let float_color = Color::byte_to_float(byte);
        if i < 3 {
            converted.push(Value::from(srgb_to_linear(float_color)));
        } else {
            converted.push(Value::from(float_color));
        }
    }
    if converted.len() == 3 {
        let alpha = transparency.map(|t| 1.0 - t / 100.0).unwrap_or(1.0);
        converted.push(Value::from(alpha));
    }
    Value::Array(converted)
}

/// `parseSymbol`: the symbology for a `Fill` symbol layer, or an empty
/// `{edges, material}` object.
fn parse_symbol(symbol: Option<&Value>, is_color_captured: bool) -> Value {
    let mut symbology = Map::new();
    symbology.insert("edges".to_string(), Value::Null);
    symbology.insert("material".to_string(), Value::Null);
    let Some(symbol) = symbol else { return Value::Object(symbology) };
    let Some(symbol_layers) = symbol.get("symbolLayers").and_then(Value::as_array) else {
        return Value::Object(symbology);
    };
    for symbol_layer in symbol_layers {
        if symbol_layer.get("type").and_then(Value::as_str) != Some("Fill") {
            continue;
        }
        let mut edges = Map::new();
        if let Some(edge) = symbol_layer.get("edges") {
            if let Some(color) = edge.get("color") {
                edges.insert("color".to_string(), convert_color(color, edge.get("transparency").and_then(Value::as_f64)));
            }
        } else if let Some(outline) = symbol_layer.get("outline") {
            if let Some(color) = outline.get("color") {
                edges.insert("color".to_string(), convert_color(color, outline.get("transparency").and_then(Value::as_f64)));
            }
        }
        symbology.insert("edges".to_string(), Value::Object(edges));
        if !is_color_captured {
            if let Some(material) = symbol_layer.get("material") {
                let mut material_value = Map::new();
                if let Some(color_mix_mode) = material.get("colorMixMode") {
                    material_value.insert("colorMixMode".to_string(), color_mix_mode.clone());
                }
                if let Some(color) = material.get("color") {
                    material_value.insert("color".to_string(), convert_color(color, material.get("transparency").and_then(Value::as_f64)));
                }
                symbology.insert("material".to_string(), Value::Object(material_value));
            }
        }
        break;
    }
    Value::Object(symbology)
}

/// `buildUniqueValueHash`.
fn build_unique_value_hash(renderer: &Value, is_color_captured: bool) -> Option<Value> {
    if let Some(groups) = renderer.get("uniqueValueGroups").and_then(Value::as_array) {
        let mut value_hash = Map::new();
        for group in groups {
            if let Some(classes) = group.get("classes").and_then(Value::as_array) {
                for class in classes {
                    let class_symbology = parse_symbol(class.get("symbol"), is_color_captured);
                    let Some(values) = class.get("values").and_then(Value::as_array) else { continue };
                    for field_values in values {
                        let mut hash: &mut Map<String, Value> = &mut value_hash;
                        let fields: Vec<Value> = field_values.as_array().cloned().unwrap_or_default();
                        for (field_index, field_value) in fields.iter().enumerate() {
                            let key = object_key(field_value);
                            if field_index == fields.len() - 1 {
                                hash.insert(key, class_symbology.clone());
                            } else {
                                if !hash.contains_key(&key) {
                                    hash.insert(key.clone(), Value::Object(Map::new()));
                                }
                                hash = hash.get_mut(&key).expect("unique value hash entry is an object").as_object_mut().expect("unique value hash entry is an object");
                            }
                        }
                    }
                }
            }
        }
        return Some(Value::Object(value_hash));
    }
    if let Some(infos) = renderer.get("uniqueValueInfos").and_then(Value::as_array) {
        let mut value_hash = Map::new();
        for info in infos {
            if let Some(value) = info.get("value") {
                value_hash.insert(object_key(value), parse_symbol(info.get("symbol"), is_color_captured));
            }
        }
        return Some(Value::Object(value_hash));
    }
    None
}

/// `buildClassBreaksHash`.
fn build_class_breaks_hash(renderer: &Value, is_color_captured: bool) -> Option<Value> {
    let infos = renderer.get("classBreakInfos")?.as_array()?;
    let mut class_break_infos: Vec<Value> = infos.to_vec();
    class_break_infos.sort_by(|a, b| {
        let a_max = class_break_max(a);
        let b_max = class_break_max(b);
        a_max.partial_cmp(&b_max).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut ranges: Vec<Value> = Vec::new();
    let mut symbols: Vec<Value> = Vec::new();
    if let Some(min_value) = renderer.get("minValue").and_then(Value::as_f64) {
        ranges.push(Value::from(min_value));
        symbols.push(Value::Null);
    }
    for info in &class_break_infos {
        if let Some(class_min_value) = info.get("classMinValue").and_then(Value::as_f64) {
            if ranges.is_empty() || class_min_value > ranges.last().and_then(Value::as_f64).unwrap_or(f64::NEG_INFINITY) {
                ranges.push(Value::from(class_min_value));
                symbols.push(Value::Null);
            }
        }
        if let Some(class_max_value) = info.get("classMaxValue").and_then(Value::as_f64) {
            if ranges.is_empty() || class_max_value > ranges.last().and_then(Value::as_f64).unwrap_or(f64::NEG_INFINITY) {
                ranges.push(Value::from(class_max_value));
                symbols.push(parse_symbol(info.get("symbol"), is_color_captured));
            }
        }
    }
    symbols.push(Value::Null);
    let mut value_hash = Map::new();
    value_hash.insert("ranges".to_string(), Value::Array(ranges));
    value_hash.insert("symbols".to_string(), Value::Array(symbols));
    Some(Value::Object(value_hash))
}

/// `a.classMaxValue ?? a.classMinValue`.
fn class_break_max(info: &Value) -> f64 {
    info.get("classMaxValue")
        .and_then(Value::as_f64)
        .or_else(|| info.get("classMinValue").and_then(Value::as_f64))
        .unwrap_or(f64::NAN)
}

/// JS object keys compare to a feature id as strings.
fn object_key(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())
}

/// `findHashForUniqueValues`.
fn find_hash_for_unique_values(hash: &Value, values: &[Vec<Value>], hash_level: usize, value_index: usize) -> Option<Value> {
    let level_values = values.get(hash_level)?;
    let hash_value = level_values.get(value_index)?;
    let inner_hash = hash.get(object_key(hash_value));
    let next_level = hash_level + 1;
    if inner_hash.is_some() && next_level < values.len() {
        return find_hash_for_unique_values(inner_hash.unwrap(), values, next_level, value_index);
    }
    inner_hash.cloned()
}

/// `bisect`: first index with `array[i] >= value`.
fn bisect(array: &[f64], value: f64) -> usize {
    let mut low = 0;
    let mut high = array.len();
    if low < high {
        while low < high {
            let mid = (low + high) >> 1;
            if array[mid] < value {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
    }
    low
}

/// `findHashForClassBreaks`.
fn find_hash_for_class_breaks(hash: &Value, values: &[Value], value_index: usize) -> Option<Value> {
    let value = values.get(value_index).and_then(Value::as_f64)?;
    let ranges = hash.get("ranges").and_then(Value::as_array).cloned().unwrap_or_default();
    let range = bisect(&ranges.iter().filter_map(Value::as_f64).collect::<Vec<_>>(), value);
    hash.get("symbols")?.as_array()?.get(range).cloned()
}
