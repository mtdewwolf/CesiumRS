//! CesiumJS `ImplicitTileset`: the shared description of an implicit tileset,
//! read from the root tile that carries `implicitTiling` (or the
//! `3DTILES_implicit_tiling` extension).

use std::sync::Arc;

use serde_json::Value;

use crate::core::Resource;

use super::has_extension::has_extension;
use super::implicit_subdivision_scheme::ImplicitSubdivisionScheme;
use super::metadata_value::{developer, MetadataError};
use super::MetadataSchema;

#[derive(Clone)]
pub struct ImplicitTileset {
    /// The resource subtree and content URIs are relative to.
    pub base_resource: Resource,
    pub geometric_error: Option<f64>,
    pub metadata_schema: Option<Arc<MetadataSchema>>,
    /// The root's bounding volume JSON: a box, a region, or the S2 or
    /// cylinder extension.
    pub bounding_volume: Value,
    pub refine: Option<String>,
    /// The subtree URI template (`{level}`, `{x}`, `{y}`, `{z}`).
    pub subtree_uri_template: Resource,
    /// One content URI template per content.
    pub content_uri_templates: Vec<Resource>,
    /// A copy of each content header.
    pub content_headers: Vec<Value>,
    pub content_count: usize,
    /// The root tile JSON without implicit tiling or content, copied to
    /// every implicit tile.
    pub tile_header: Value,
    pub subdivision_scheme: ImplicitSubdivisionScheme,
    pub branching_factor: u32,
    pub subtree_levels: u32,
    /// `availableLevels`, else `maximumLevel + 1`; NaN when neither is given.
    pub available_levels: f64,
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

impl ImplicitTileset {
    /// Errors: a `RuntimeError` for a bounding volume other than a box,
    /// region, S2 cell or cylinder; `DeveloperError`s for missing implicit
    /// tiling, an unknown subdivision scheme or URI.
    pub fn new(
        base_resource: Resource,
        tile_json: &Value,
        metadata_schema: Option<Arc<MetadataSchema>>,
    ) -> Result<Self, MetadataError> {
        let implicit_tiling = if has_extension(tile_json, "3DTILES_implicit_tiling") {
            &tile_json["extensions"]["3DTILES_implicit_tiling"]
        } else {
            &tile_json["implicitTiling"]
        };
        if !(implicit_tiling.is_object() || implicit_tiling.is_array()) {
            return Err(developer(format!(
                "Expected implicitTiling to be typeof object, actual typeof was {}",
                js_typeof(implicit_tiling)
            )));
        }

        let bounding_volume = tile_json["boundingVolume"].clone();
        if bounding_volume.get("box").is_none_or(Value::is_null)
            && bounding_volume.get("region").is_none_or(Value::is_null)
            && !has_extension(&bounding_volume, "3DTILES_bounding_volume_S2")
            && !has_extension(&bounding_volume, "3DTILES_bounding_volume_cylinder")
        {
            return Err(MetadataError::Runtime(
                "Only box, region, 3DTILES_bounding_volume_S2, and 3DTILES_bounding_volume_cylinder are supported for implicit tiling".into(),
            ));
        }

        let uri_resource = |uri: &Value| {
            uri.as_str().and_then(Resource::new).ok_or_else(|| developer("Expected a content or subtree URI string"))
        };
        let subtree_uri_template = uri_resource(&implicit_tiling["subtrees"]["uri"])?;
        let mut content_headers = Vec::new();
        let mut content_uri_templates = Vec::new();
        for header in gather_content_headers(tile_json) {
            content_headers.push(header.clone());
            content_uri_templates.push(uri_resource(&header["uri"])?);
        }

        let subdivision_scheme = implicit_tiling["subdivisionScheme"]
            .as_str()
            .and_then(ImplicitSubdivisionScheme::from_str_value)
            .ok_or_else(|| developer("subdivisionScheme is not a valid value."))?;
        let subtree_levels = number(implicit_tiling.get("subtreeLevels"))
            .filter(|l| *l >= 1.0 && l.fract() == 0.0)
            .ok_or_else(|| developer("Expected implicitTiling.subtreeLevels to be a positive integer"))?
            as u32;
        let available_levels = match number(implicit_tiling.get("availableLevels")) {
            Some(levels) => levels,
            None => number(implicit_tiling.get("maximumLevel")).map_or(f64::NAN, |l| l + 1.0),
        };

        Ok(Self {
            base_resource,
            geometric_error: number(tile_json.get("geometricError")),
            metadata_schema,
            bounding_volume,
            refine: tile_json["refine"].as_str().map(str::to_owned),
            subtree_uri_template,
            content_count: content_headers.len(),
            content_uri_templates,
            content_headers,
            tile_header: make_tile_header_template(tile_json),
            subdivision_scheme,
            branching_factor: subdivision_scheme.get_branching_factor(),
            subtree_levels,
            available_levels,
        })
    }
}

fn js_typeof(value: &Value) -> &'static str {
    match value {
        Value::Null => "undefined",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        _ => "object",
    }
}

/// The contents of `3DTILES_multiple_contents`, `contents`, or `content`.
fn gather_content_headers(tile_json: &Value) -> Vec<&Value> {
    if has_extension(tile_json, "3DTILES_multiple_contents") {
        let extension = &tile_json["extensions"]["3DTILES_multiple_contents"];
        let contents = if extension.get("contents").is_some_and(|c| !c.is_null()) {
            &extension["contents"]
        } else {
            &extension["content"]
        };
        return contents.as_array().map(|c| c.iter().collect()).unwrap_or_default();
    }
    if let Some(contents) = tile_json.get("contents").filter(|c| !c.is_null()) {
        return contents.as_array().map(|c| c.iter().collect()).unwrap_or_default();
    }
    match tile_json.get("content").filter(|c| !c.is_null()) {
        Some(content) => vec![content],
        None => Vec::new(),
    }
}

/// The tile JSON without implicit tiling and content, and without an
/// `extensions` object left empty by removing them.
fn make_tile_header_template(tile_json: &Value) -> Value {
    let mut template = tile_json.clone();
    if let Some(object) = template.as_object_mut() {
        if let Some(extensions) = object.get_mut("extensions").and_then(Value::as_object_mut) {
            extensions.shift_remove("3DTILES_implicit_tiling");
            extensions.shift_remove("3DTILES_multiple_contents");
            if extensions.is_empty() {
                object.shift_remove("extensions");
            }
        }
        object.shift_remove("implicitTiling");
        object.shift_remove("contents");
        object.shift_remove("content");
    }
    template
}
