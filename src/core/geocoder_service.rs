//! CesiumJS geocoder contracts, request construction and response parsing.
//! Hosts supply asynchronous JSON/JSONP transport; credentials remain explicit.

use super::{
    credit::Attribution,
    query::QueryValue,
    resource::{DerivedResourceOptions, QueryParameters},
};
use super::{Cartesian3, Cartographic, Credit, Ellipsoid, GeocodeType, Rectangle, Resource, RuntimeError};
use serde_json::Value;
use std::{future::Future, pin::Pin};

pub type GeocoderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + 'a>>;

pub trait GeocoderHost {
    fn fetch_json<'a>(&'a self, resource: Resource, jsonp_parameter: Option<&'static str>)
        -> GeocoderFuture<'a, Value>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeocoderDestination {
    Point(Cartesian3),
    Rectangle(Rectangle),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeocoderResult {
    pub display_name: String,
    pub destination: GeocoderDestination,
    pub attributions: Option<Vec<Attribution>>,
    /// Google returns a singular `attribution` field; preserve that distinction.
    pub attribution: Option<Attribution>,
}

impl GeocoderResult {
    pub fn new(display_name: String, destination: GeocoderDestination) -> Self {
        Self { display_name, destination, attributions: None, attribution: None }
    }
    pub fn get_credits(&self) -> Option<Vec<Credit>> {
        self.attributions.as_ref().map(|a| a.iter().map(Credit::get_ion_credit).collect())
    }
}

pub trait GeocoderService {
    fn credit(&self) -> Option<&Credit>;
    fn geocode<'a>(
        &'a self,
        query: &'a str,
        geocode_type: GeocodeType,
        host: &'a dyn GeocoderHost,
    ) -> GeocoderFuture<'a, Vec<GeocoderResult>>;
}

fn param(key: &str, value: impl Into<String>) -> (String, Option<QueryValue>) {
    (key.to_owned(), Some(QueryValue::Single(value.into())))
}
fn malformed() -> RuntimeError {
    RuntimeError::new("Malformed geocoder response.")
}
fn array(value: &Value) -> Result<&Vec<Value>, RuntimeError> {
    value.as_array().ok_or_else(malformed)
}
fn number(value: &Value) -> Result<f64, RuntimeError> {
    value.as_f64().ok_or_else(malformed)
}
fn text(value: &Value) -> Result<String, RuntimeError> {
    value.as_str().map(str::to_owned).ok_or_else(malformed)
}
fn bbox(value: &Value) -> Result<GeocoderDestination, RuntimeError> {
    Ok(GeocoderDestination::Rectangle(Rectangle::from_degrees(
        number(&value[0])?,
        number(&value[1])?,
        number(&value[2])?,
        number(&value[3])?,
    )))
}
fn point(longitude: f64, latitude: f64, height: f64) -> GeocoderDestination {
    GeocoderDestination::Point(
        Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(longitude, latitude, height)),
    )
}
fn derived(
    resource: &Resource,
    url: Option<&str>,
    query_parameters: QueryParameters,
) -> Result<Resource, RuntimeError> {
    resource
        .get_derived_resource(DerivedResourceOptions {
            url: url.map(str::to_owned),
            query_parameters,
            ..Default::default()
        })
        .ok_or_else(|| RuntimeError::new("Invalid geocoder URL."))
}
fn attribution(value: &Value) -> Result<Attribution, RuntimeError> {
    Ok(Attribution { html: text(&value["html"])?, collapsible: value["collapsible"].as_bool() })
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CartographicGeocoderService;

fn js_space(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
fn js_number(value: &str) -> f64 {
    use num_traits::ToPrimitive;
    let v = value.trim_matches(js_space);
    if v.is_empty() {
        return 0.0;
    }
    for (lower, upper, radix) in [("0x", "0X", 16), ("0b", "0B", 2), ("0o", "0O", 8)] {
        if let Some(digits) = v.strip_prefix(lower).or_else(|| v.strip_prefix(upper)) {
            if digits.is_empty()
                || !digits.bytes().all(|b| match radix {
                    16 => b.is_ascii_hexdigit(),
                    8 => matches!(b, b'0'..=b'7'),
                    2 => matches!(b, b'0' | b'1'),
                    _ => false,
                })
            {
                return f64::NAN;
            }
            return num_bigint::BigUint::parse_bytes(digits.as_bytes(), radix)
                .map_or(f64::NAN, |n| n.to_f64().unwrap_or(f64::INFINITY));
        }
    }
    match v {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    static DECIMAL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let grammar = DECIMAL
        .get_or_init(|| regex::Regex::new(r"^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$").unwrap());
    if grammar.is_match(v) {
        v.parse().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

impl CartographicGeocoderService {
    pub fn geocode_coordinates(&self, query: &str) -> Result<Vec<GeocoderResult>, RuntimeError> {
        let parts: Vec<_> = query.split(|c: char| js_space(c) || c == ',').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(RuntimeError::new("Cannot read properties of null (reading 'length')"));
        }
        if parts.len() != 2 && parts.len() != 3 {
            return Ok(Vec::new());
        }
        let mut lon = js_number(parts[0]);
        let mut lat = js_number(parts[1]);
        let height = if parts.len() == 3 { js_number(parts[2]) } else { 300.0 };
        if lon.is_nan() && lat.is_nan() {
            static COORD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            let pattern = COORD.get_or_init(|| regex::Regex::new(r"^([0-9]+.?[0-9]*)([nNsSeEwW])").unwrap());
            for part in &parts {
                if let Some(captures) = pattern.captures(part) {
                    let value = js_number(&captures[1]);
                    match captures[2].to_ascii_lowercase().as_str() {
                        "n" => lat = value,
                        "s" => lat = -value,
                        "e" => lon = value,
                        "w" => lon = -value,
                        _ => {}
                    }
                }
            }
        }
        if lon.is_nan() || lat.is_nan() || height.is_nan() {
            return Ok(Vec::new());
        }
        Ok(vec![GeocoderResult::new(query.into(), point(lon, lat, height))])
    }
}

impl GeocoderService for CartographicGeocoderService {
    fn credit(&self) -> Option<&Credit> {
        None
    }
    fn geocode<'a>(
        &'a self,
        query: &'a str,
        _: GeocodeType,
        _: &'a dyn GeocoderHost,
    ) -> GeocoderFuture<'a, Vec<GeocoderResult>> {
        Box::pin(async move { self.geocode_coordinates(query) })
    }
}

pub struct BingMapsGeocoderService {
    pub resource: Resource,
    pub key: String,
    credit: Credit,
}
impl BingMapsGeocoderService {
    pub const URL: &'static str = "https://dev.virtualearth.net/REST/v1/Locations";
    pub fn new(key: impl Into<String>, culture: Option<&str>) -> Self {
        let key = key.into();
        let resource = Resource::new(Self::URL).unwrap();
        let mut query = vec![param("key", &key)];
        if let Some(culture) = culture {
            query.push(param("culture", culture));
        }
        resource.set_query_parameters(&query, false);
        Self {
            resource,
            key,
            credit: Credit::new("<img src=\"http://dev.virtualearth.net/Branding/logo_powered_by.png\"/>", false),
        }
    }
    pub fn request(&self, query: &str) -> Result<Resource, RuntimeError> {
        derived(&self.resource, None, vec![param("query", query)])
    }
    pub fn parse_response(response: &Value) -> Result<Vec<GeocoderResult>, RuntimeError> {
        let sets = array(&response["resourceSets"])?;
        if sets.is_empty() {
            return Ok(Vec::new());
        }
        array(&sets[0]["resources"])?
            .iter()
            .map(|result| {
                let b = &result["bbox"];
                Ok(GeocoderResult::new(
                    text(&result["name"])?,
                    GeocoderDestination::Rectangle(Rectangle::from_degrees(
                        number(&b[1])?,
                        number(&b[0])?,
                        number(&b[3])?,
                        number(&b[2])?,
                    )),
                ))
            })
            .collect()
    }
}
impl GeocoderService for BingMapsGeocoderService {
    fn credit(&self) -> Option<&Credit> {
        Some(&self.credit)
    }
    fn geocode<'a>(
        &'a self,
        query: &'a str,
        _: GeocodeType,
        host: &'a dyn GeocoderHost,
    ) -> GeocoderFuture<'a, Vec<GeocoderResult>> {
        Box::pin(async move { Self::parse_response(&host.fetch_json(self.request(query)?, Some("jsonp")).await?) })
    }
}

pub struct PeliasGeocoderService {
    pub resource: Resource,
}
impl PeliasGeocoderService {
    pub fn new(resource: Resource) -> Self {
        resource.append_forward_slash();
        Self { resource }
    }
    pub fn request(&self, query: &str, geocode_type: GeocodeType) -> Result<Resource, RuntimeError> {
        derived(
            &self.resource,
            Some(if geocode_type == GeocodeType::Autocomplete { "autocomplete" } else { "search" }),
            vec![param("text", query)],
        )
    }
    pub fn parse_response(response: &Value) -> Result<Vec<GeocoderResult>, RuntimeError> {
        let attributions = if response["attributions"].is_null() {
            None
        } else {
            Some(array(&response["attributions"])?.iter().map(attribution).collect::<Result<Vec<_>, _>>()?)
        };
        array(&response["features"])?
            .iter()
            .map(|result| {
                let destination = if !result["bbox"].is_null() {
                    bbox(&result["bbox"])?
                } else {
                    point(
                        number(&result["geometry"]["coordinates"][0])?,
                        number(&result["geometry"]["coordinates"][1])?,
                        0.0,
                    )
                };
                let mut result = GeocoderResult::new(text(&result["properties"]["label"])?, destination);
                result.attributions = attributions.clone();
                Ok(result)
            })
            .collect()
    }
}
impl GeocoderService for PeliasGeocoderService {
    fn credit(&self) -> Option<&Credit> {
        None
    }
    fn geocode<'a>(
        &'a self,
        query: &'a str,
        kind: GeocodeType,
        host: &'a dyn GeocoderHost,
    ) -> GeocoderFuture<'a, Vec<GeocoderResult>> {
        Box::pin(async move { Self::parse_response(&host.fetch_json(self.request(query, kind)?, None).await?) })
    }
}

pub struct OpenCageGeocoderService {
    pub resource: Resource,
    pub params: QueryParameters,
    credit: Credit,
}
impl OpenCageGeocoderService {
    pub fn new(resource: Resource, api_key: &str, params: QueryParameters) -> Self {
        resource.append_forward_slash();
        resource.set_query_parameters(&vec![param("key", api_key)], false);
        Self {
            resource,
            params,
            credit: Credit::new(
                "Geodata copyright <a href=\"https://www.openstreetmap.org/\">OpenStreetMap</a> contributors",
                false,
            ),
        }
    }
    pub fn request(&self, query: &str) -> Result<Resource, RuntimeError> {
        let mut parameters = self.params.clone();
        if !parameters.iter().any(|(key, _)| key == "q") {
            parameters.push(param("q", query));
        }
        derived(&self.resource, Some("json"), parameters)
    }
    pub fn parse_response(response: &Value) -> Result<Vec<GeocoderResult>, RuntimeError> {
        array(&response["results"])?
            .iter()
            .map(|result| {
                let bounds = &result["bounds"];
                let destination = if bounds.is_null() {
                    point(number(&result["geometry"]["lng"])?, number(&result["geometry"]["lat"])?, 0.0)
                } else {
                    GeocoderDestination::Rectangle(Rectangle::from_degrees(
                        number(&bounds["southwest"]["lng"])?,
                        number(&bounds["southwest"]["lat"])?,
                        number(&bounds["northeast"]["lng"])?,
                        number(&bounds["northeast"]["lat"])?,
                    ))
                };
                Ok(GeocoderResult::new(text(&result["formatted"])?, destination))
            })
            .collect()
    }
}
impl GeocoderService for OpenCageGeocoderService {
    fn credit(&self) -> Option<&Credit> {
        Some(&self.credit)
    }
    fn geocode<'a>(
        &'a self,
        query: &'a str,
        _: GeocodeType,
        host: &'a dyn GeocoderHost,
    ) -> GeocoderFuture<'a, Vec<GeocoderResult>> {
        Box::pin(async move { Self::parse_response(&host.fetch_json(self.request(query)?, None).await?) })
    }
}

pub struct GoogleGeocoderService {
    pub resource: Resource,
    credit: Credit,
}
impl GoogleGeocoderService {
    pub const URL: &'static str = "https://maps.googleapis.com/maps/api/geocode/json";
    pub const CREDIT_HTML: &'static str =
        "<img alt=\"Google\" src=\"https://assets.ion.cesium.com/google-credit.png\" style=\"vertical-align:-5px\">";
    pub fn new(key: &str) -> Self {
        let resource = Resource::new(Self::URL).unwrap();
        resource.set_query_parameters(&vec![param("key", key)], false);
        Self { resource, credit: Credit::new(Self::CREDIT_HTML, true) }
    }
    pub fn request(&self, query: &str) -> Result<Resource, RuntimeError> {
        derived(&self.resource, None, vec![param("address", query)])
    }
    pub fn parse_response(response: &Value) -> Result<Vec<GeocoderResult>, RuntimeError> {
        if response["status"] == "ZERO_RESULTS" {
            return Ok(Vec::new());
        }
        if response["status"] != "OK" {
            return Err(RuntimeError::new(format!(
                "GoogleGeocoderService got a bad response {}: {}",
                js_display(response.get("status")),
                js_display(response.get("error_message"))
            )));
        }
        array(&response["results"])?
            .iter()
            .map(|result| {
                let viewport = &result["geometry"]["viewport"];
                let destination = GeocoderDestination::Rectangle(Rectangle::from_degrees(
                    number(&viewport["southwest"]["lng"])?,
                    number(&viewport["southwest"]["lat"])?,
                    number(&viewport["northeast"]["lng"])?,
                    number(&viewport["northeast"]["lat"])?,
                ));
                let mut result = GeocoderResult::new(text(&result["formatted_address"])?, destination);
                result.attribution = Some(Attribution { html: Self::CREDIT_HTML.into(), collapsible: Some(false) });
                Ok(result)
            })
            .collect()
    }
}
impl GeocoderService for GoogleGeocoderService {
    fn credit(&self) -> Option<&Credit> {
        Some(&self.credit)
    }
    fn geocode<'a>(
        &'a self,
        query: &'a str,
        _: GeocodeType,
        host: &'a dyn GeocoderHost,
    ) -> GeocoderFuture<'a, Vec<GeocoderResult>> {
        Box::pin(async move { Self::parse_response(&host.fetch_json(self.request(query)?, None).await?) })
    }
}

fn js_display(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(_)) => "[object Object]".into(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|v| if v.is_null() { String::new() } else { js_display(Some(v)) })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Number(n)) => n.as_f64().map_or_else(|| n.to_string(), |n| n.to_string()),
        Some(Value::Bool(b)) => b.to_string(),
    }
}
