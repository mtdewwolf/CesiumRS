//! GeoJSON and TopoJSON entity loading. The billboard image is a pin descriptor
//! with `kind` (color, text or maki), `symbol`, RGBA `color` and pixel `size`
//! for the rendering host.
mod geometry;
mod pin;
mod topology;
pub use pin::{GeoJsonPin, PinSymbol};

use super::{DataSource, DataSourceState, PropertyInput};
use crate::core::{
    uri_helpers::get_filename_from_uri, Cartesian3, Color, Ellipsoid, Resource, ResourceClient, RuntimeError,
};
use futures_util::future::LocalBoxFuture;
use serde_json::Value;
use std::{cell::Cell, collections::HashMap, rc::Rc};

pub type CrsFunction = Rc<dyn Fn(&[f64]) -> Result<Cartesian3, RuntimeError>>;
pub type CrsLinkResolver = Rc<dyn Fn(Value) -> LocalBoxFuture<'static, Result<CrsFunction, RuntimeError>>>;
pub type DescribeProperty = Rc<dyn Fn(&Value, Option<&str>) -> Result<PropertyInput, RuntimeError>>;

fn from_degrees(p: &[f64]) -> Result<Cartesian3, RuntimeError> {
    Ok(Cartesian3::from_degrees(p[0], p[1], p.get(2).copied().unwrap_or(0.), &Ellipsoid::WGS84))
}
pub struct CrsRegistry {
    pub names: HashMap<String, CrsFunction>,
    pub link_hrefs: HashMap<String, CrsLinkResolver>,
    pub link_types: HashMap<String, CrsLinkResolver>,
}
impl Default for CrsRegistry {
    fn default() -> Self {
        Self {
            names: ["urn:ogc:def:crs:OGC:1.3:CRS84", "EPSG:4326", "urn:ogc:def:crs:EPSG::4326"]
                .into_iter()
                .map(|name| (name.to_owned(), Rc::new(from_degrees) as CrsFunction))
                .collect(),
            link_hrefs: HashMap::new(),
            link_types: HashMap::new(),
        }
    }
}
fn field(value: &Value, key: &str) -> String {
    match value.get(key) {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}
impl CrsRegistry {
    async fn resolve(&self, crs: Option<&Value>) -> Result<Option<CrsFunction>, RuntimeError> {
        let Some(crs) = crs else {
            return Ok(Some(Rc::new(from_degrees)));
        };
        if crs.is_null() {
            return Ok(None);
        }
        let p = &crs["properties"];
        if p.is_null() {
            return Err(RuntimeError::new("crs.properties is undefined."));
        }
        let projection = match field(crs, "type").as_str() {
            "name" => {
                let name = field(p, "name");
                self.names.get(&name).cloned().ok_or_else(|| RuntimeError::new(format!("Unknown crs name: {name}")))?
            }
            "EPSG" => {
                let code = field(p, "code");
                self.names
                    .get(&format!("EPSG:{code}"))
                    .cloned()
                    .ok_or_else(|| RuntimeError::new(format!("Unknown crs EPSG code: {code}")))?
            }
            "link" => {
                let resolve = self
                    .link_hrefs
                    .get(&field(p, "href"))
                    .or_else(|| self.link_types.get(&field(p, "type")))
                    .ok_or_else(|| RuntimeError::new(format!("Unable to resolve crs link: {p}")))?;
                resolve(p.clone()).await?
            }
            other => return Err(RuntimeError::new(format!("Unknown crs type: {other}"))),
        };
        Ok(Some(projection))
    }
}

#[derive(Clone)]
pub struct GeoJsonOptions {
    pub source_uri: Option<String>,
    pub resource_client: Option<ResourceClient>,
    pub crs: Rc<CrsRegistry>,
    pub describe: Option<DescribeProperty>,
    pub marker_size: f64,
    pub marker_symbol: Option<String>,
    pub marker_color: Color,
    pub stroke: Color,
    pub stroke_width: f64,
    pub fill: Color,
    pub clamp_to_ground: bool,
}
impl Default for GeoJsonOptions {
    fn default() -> Self {
        Self {
            source_uri: None,
            resource_client: None,
            crs: Rc::default(),
            describe: None,
            marker_size: GeoJsonDataSource::DEFAULT_MARKER_SIZE,
            marker_symbol: GeoJsonDataSource::DEFAULT_MARKER_SYMBOL,
            marker_color: GeoJsonDataSource::DEFAULT_MARKER_COLOR,
            stroke: GeoJsonDataSource::DEFAULT_STROKE,
            stroke_width: GeoJsonDataSource::DEFAULT_STROKE_WIDTH,
            fill: GeoJsonDataSource::DEFAULT_FILL,
            clamp_to_ground: GeoJsonDataSource::DEFAULT_CLAMP_TO_GROUND,
        }
    }
}
pub enum GeoJsonInput<'a> {
    Json(Value),
    JsonRef(&'a Value),
    Text(String),
    Resource(Resource),
}
impl From<Value> for GeoJsonInput<'static> {
    fn from(value: Value) -> Self {
        Self::Json(value)
    }
}
impl<'a> From<&'a Value> for GeoJsonInput<'a> {
    fn from(value: &'a Value) -> Self {
        Self::JsonRef(value)
    }
}
impl From<&str> for GeoJsonInput<'static> {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<Resource> for GeoJsonInput<'static> {
    fn from(value: Resource) -> Self {
        Self::Resource(value)
    }
}

pub struct GeoJsonDataSource {
    state: DataSourceState,
    active_loads: Cell<usize>,
}
impl DataSource for GeoJsonDataSource {
    fn state(&self) -> &DataSourceState {
        &self.state
    }
}
// Dropping a pending load must release its share of the collection suspension.
struct Loading<'a>(&'a GeoJsonDataSource);
impl Drop for Loading<'_> {
    fn drop(&mut self) {
        let remaining = self.0.active_loads.get() - 1;
        self.0.active_loads.set(remaining);
        if remaining == 0 {
            self.0.set_loading(false).expect("matching loading transition");
        }
    }
}
impl GeoJsonDataSource {
    pub const DEFAULT_MARKER_SIZE: f64 = 48.;
    pub const DEFAULT_MARKER_SYMBOL: Option<String> = None;
    pub const DEFAULT_MARKER_COLOR: Color = Color::ROYALBLUE;
    pub const DEFAULT_STROKE: Color = Color::YELLOW;
    pub const DEFAULT_STROKE_WIDTH: f64 = 2.;
    pub const DEFAULT_FILL: Color = Color::new(1., 1., 0., 100. / 255.);
    pub const DEFAULT_CLAMP_TO_GROUND: bool = false;

    pub fn new(name: Option<String>) -> Rc<Self> {
        Rc::new_cyclic(|owner| Self { state: DataSourceState::new(owner.clone(), name), active_loads: Cell::new(0) })
    }
    pub fn set_name(&self, name: Option<String>) {
        self.state.set_name(name);
    }

    /// Replace entities. Strings may contain a JSON document or a resource URL.
    /// Supply the host's ResourceClient in the options for URLs and Resources.
    pub async fn load<'a>(
        &self,
        input: impl Into<GeoJsonInput<'a>>,
        options: &GeoJsonOptions,
    ) -> Result<(), RuntimeError> {
        self.preload(input.into(), options, true).await
    }
    /// Append entities, preserving existing IDs and resolving collisions.
    pub async fn process<'a>(
        &self,
        input: impl Into<GeoJsonInput<'a>>,
        options: &GeoJsonOptions,
    ) -> Result<(), RuntimeError> {
        self.preload(input.into(), options, false).await
    }
    async fn preload<'a>(
        &self,
        input: GeoJsonInput<'a>,
        options: &GeoJsonOptions,
        clear: bool,
    ) -> Result<(), RuntimeError> {
        self.active_loads.set(self.active_loads.get() + 1);
        let loading = Loading(self);
        self.set_loading(true)?;
        let result = self.read(input, options, clear).await;
        drop(loading);
        if let Err(err) = &result {
            self.state.notify_error(err.clone());
        }
        result
    }
    async fn read<'a>(
        &self,
        input: GeoJsonInput<'a>,
        options: &GeoJsonOptions,
        clear: bool,
    ) -> Result<(), RuntimeError> {
        let mut source_uri = options.source_uri.clone();
        let input = match input {
            GeoJsonInput::Text(s) if s.trim_start().starts_with(['{', '[']) => {
                GeoJsonInput::Json(serde_json::from_str(&s).map_err(|e| RuntimeError::new(e.to_string()))?)
            }
            GeoJsonInput::Text(url) => {
                GeoJsonInput::Resource(Resource::new(url).ok_or_else(|| RuntimeError::new("Invalid GeoJSON URL."))?)
            }
            other => other,
        };
        let owned: Option<Value>;
        let json: &Value = match input {
            GeoJsonInput::Json(v) => {
                owned = Some(v);
                owned.as_ref().unwrap()
            }
            GeoJsonInput::JsonRef(v) => v,
            GeoJsonInput::Resource(resource) => {
                let client = options
                    .resource_client
                    .as_ref()
                    .ok_or_else(|| RuntimeError::new("Resource loading requires a ResourceClient."))?;
                source_uri.get_or_insert_with(|| resource.get_url_component(false, false));
                owned = Some(
                    resource
                        .fetch_json(client)
                        .ok_or_else(|| RuntimeError::new("GeoJSON request was not scheduled."))?
                        .await
                        .map_err(|e| RuntimeError::new(e.to_string()))?
                        .ok_or_else(|| RuntimeError::new("Empty GeoJSON response."))?,
                );
                owned.as_ref().unwrap()
            }
            GeoJsonInput::Text(_) => unreachable!(),
        };
        if let Some(url) = source_uri {
            self.set_name(Some(get_filename_from_uri(&url)));
        }
        let kind = field(json, "type");
        if !geometry::supported(&kind) {
            return Err(RuntimeError::new(format!("Unsupported GeoJSON object type: {kind}")));
        }
        let project = options.crs.resolve(json.get("crs")).await?;
        if clear {
            self.entities().remove_all();
        }
        if let Some(project) = project {
            geometry::Loader::new(self.entities(), options, &project)?.object(json)?;
        }
        Ok(())
    }
}

fn array(v: &Value) -> Result<&Vec<Value>, RuntimeError> {
    v.as_array().ok_or_else(|| RuntimeError::new("Expected an array."))
}
fn coordinate(v: &Value) -> Result<([f64; 3], usize), RuntimeError> {
    let coords = array(v)?;
    if coords.len() < 2 {
        return Err(RuntimeError::new("Expected longitude and latitude."));
    }
    let mut out = [0.0; 3];
    let len = coords.len().min(3);
    for i in 0..len {
        out[i] = coords[i].as_f64().ok_or_else(|| RuntimeError::new("Expected numeric coordinates."))?;
    }
    Ok((out, len))
}
