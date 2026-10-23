//! CesiumJS `ImplicitSubtree`: one subtree of an implicit tileset — tile,
//! content and child subtree availability, and the tile and content
//! metadata tables.
//!
//! Cesium loads external buffers through `ResourceCache`; the port asks the
//! caller's `load_buffer` for the bytes of each active external buffer, in
//! buffer order, and builds the subtree synchronously.
//!
//! Buffer views follow `Uint8Array.subarray`: ranges are clamped to the
//! buffer, and metadata typed arrays read from the view's offset in the
//! whole underlying buffer, as Cesium's do.

use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::core::get_string_from_typed_array::get_string_from_typed_array;
use crate::core::resource::DerivedResourceOptions;
use crate::core::Resource;

use super::has_extension::has_extension;
use super::implicit_availability_bitstream::{ImplicitAvailabilityBitstream, ImplicitAvailabilityBitstreamOptions};
use super::implicit_subdivision_scheme::ImplicitSubdivisionScheme;
use super::implicit_tile_coordinates::ImplicitTileCoordinates;
use super::implicit_tileset::ImplicitTileset;
use super::metadata_table_property::ByteView;
use super::metadata_value::{developer, json_entries, JsMap, MetadataError};
use super::{ImplicitMetadataView, ImplicitSubtreeMetadata, MetadataTable};

type Result<T> = std::result::Result<T, MetadataError>;

pub struct ImplicitSubtree {
    resource: Resource,
    subtree_json: Value,
    tile_availability: ImplicitAvailabilityBitstream,
    content_availability_bitstreams: Vec<ImplicitAvailabilityBitstream>,
    child_subtree_availability: ImplicitAvailabilityBitstream,
    implicit_coordinates: ImplicitTileCoordinates,
    subtree_levels: u32,
    subdivision_scheme: ImplicitSubdivisionScheme,
    branching_factor: u32,
    metadata: Option<ImplicitSubtreeMetadata>,
    tile_metadata_table: Option<Arc<Mutex<MetadataTable>>>,
    tile_property_table_json: Option<Value>,
    content_metadata_tables: Vec<Arc<Mutex<MetadataTable>>>,
    content_property_table_jsons: Vec<Option<Value>>,
    tile_jump_buffer: Option<Vec<u32>>,
    content_jump_buffers: Vec<Vec<u32>>,
}

/// Where a subtree comes from: its JSON, or a binary `.subtree` file.
pub enum SubtreeSource<'a> {
    Json(Value),
    Binary(&'a [u8]),
}

/// A `Uint8Array` view: `len` bytes at `offset` in `data`.
#[derive(Clone)]
struct U8View {
    data: Arc<[u8]>,
    offset: usize,
    len: usize,
}

impl U8View {
    fn whole(data: Arc<[u8]>) -> Self {
        let len = data.len();
        Self { data, offset: 0, len }
    }

    /// `subarray(start, end)` with non-negative bounds, clamped.
    fn subarray(&self, start: usize, end: usize) -> Self {
        let start = start.min(self.len);
        let end = end.min(self.len).max(start);
        Self { data: self.data.clone(), offset: self.offset + start, len: end - start }
    }

    fn bytes(&self) -> &[u8] {
        &self.data[self.offset..self.offset + self.len]
    }
}

fn type_error(message: &str) -> MetadataError {
    MetadataError::Type(message.into())
}

/// A JSON number as an array index, or `None` when undefined.
fn index(value: Option<&Value>) -> Option<usize> {
    value.filter(|v| !v.is_null()).and_then(Value::as_f64).filter(|v| *v >= 0.0 && v.fract() == 0.0).map(|v| v as usize)
}

fn defined(value: Option<&Value>) -> bool {
    value.is_some_and(|v| !v.is_null())
}

/// JavaScript `Boolean(value)`.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

struct BufferHeader {
    uri: Option<String>,
    is_active: bool,
}

struct BufferViewHeader {
    buffer: Option<usize>,
    byte_offset: Option<f64>,
    byte_length: Option<f64>,
    is_active: bool,
}

/// Marks a buffer view and its buffer active, as Cesium's assignments do;
/// a missing buffer view or buffer is a `TypeError`.
fn activate(views: &mut [BufferViewHeader], buffers: &mut [BufferHeader], view: usize) -> Result<()> {
    let header =
        views.get_mut(view).ok_or_else(|| type_error("Cannot set properties of undefined (setting 'isActive')"))?;
    header.is_active = true;
    let buffer = header
        .buffer
        .and_then(|b| buffers.get_mut(b))
        .ok_or_else(|| type_error("Cannot set properties of undefined (setting 'isActive')"))?;
    buffer.is_active = true;
    Ok(())
}

/// The availability's bitstream or bufferView index.
fn availability_view(json: &Value) -> Option<Option<usize>> {
    if defined(json.get("bitstream")) {
        Some(index(json.get("bitstream")))
    } else if defined(json.get("bufferView")) {
        Some(index(json.get("bufferView")))
    } else {
        None
    }
}

impl ImplicitSubtree {
    /// `ImplicitSubtree.fromSubtreeJson`. `load_buffer` fetches an external
    /// buffer's resource, derived from `resource` by the buffer URI.
    pub fn from_subtree_json(
        resource: &Resource,
        source: SubtreeSource<'_>,
        implicit_tileset: &ImplicitTileset,
        implicit_coordinates: ImplicitTileCoordinates,
        load_buffer: &mut dyn FnMut(&Resource) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        let (mut subtree_json, binary) = match source {
            SubtreeSource::Json(json) => (json, None),
            SubtreeSource::Binary(bytes) => {
                let (json, binary) = parse_subtree_chunks(U8View::whole(bytes.into()))?;
                (json, Some(binary))
            }
        };

        let tile_property_table_json = if has_extension(&subtree_json, "3DTILES_metadata") {
            Some(subtree_json["extensions"]["3DTILES_metadata"].clone())
        } else if defined(subtree_json.get("tileMetadata")) {
            index(subtree_json.get("tileMetadata"))
                .and_then(|i| subtree_json["propertyTables"].get(i))
                .filter(|v| !v.is_null())
                .cloned()
        } else {
            None
        };

        let mut content_property_table_jsons = Vec::new();
        if let Some(content_metadata) = subtree_json.get("contentMetadata").and_then(Value::as_array) {
            for table in content_metadata {
                let json =
                    index(Some(table)).and_then(|i| subtree_json["propertyTables"].get(i)).filter(|v| !v.is_null());
                content_property_table_jsons.push(json.cloned());
            }
        }

        let metadata = match subtree_json.get("subtreeMetadata").filter(|v| !v.is_null()) {
            Some(subtree_metadata) => {
                let schema = implicit_tileset
                    .metadata_schema
                    .as_ref()
                    .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'classes')"))?;
                let class =
                    subtree_metadata["class"].as_str().and_then(|name| schema.classes.get(name).cloned()).ok_or_else(
                        || developer("Expected options.class to be typeof object, actual typeof was undefined"),
                    )?;
                Some(ImplicitSubtreeMetadata::new(subtree_metadata, class)?)
            }
            None => None,
        };

        let content_availability_headers: Vec<Value> = if has_extension(&subtree_json, "3DTILES_multiple_contents") {
            match &subtree_json["extensions"]["3DTILES_multiple_contents"]["contentAvailability"] {
                Value::Array(headers) => headers.clone(),
                _ => Vec::new(),
            }
        } else if let Some(Value::Array(headers)) = subtree_json.get("contentAvailability") {
            headers.clone()
        } else {
            vec![subtree_json
                .get("contentAvailability")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "constant": 0 }))]
        };
        subtree_json["contentAvailabilityHeaders"] = Value::Array(content_availability_headers.clone());

        let mut buffer_headers: Vec<BufferHeader> = subtree_json["buffers"]
            .as_array()
            .map(|buffers| {
                buffers
                    .iter()
                    .map(|b| BufferHeader {
                        uri: b.get("uri").filter(|u| !u.is_null()).map(|u| u.as_str().unwrap_or_default().to_owned()),
                        is_active: false,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut view_headers: Vec<BufferViewHeader> = subtree_json["bufferViews"]
            .as_array()
            .map(|views| {
                views
                    .iter()
                    .map(|v| BufferViewHeader {
                        buffer: index(v.get("buffer")),
                        byte_offset: v.get("byteOffset").and_then(Value::as_f64),
                        byte_length: v.get("byteLength").and_then(Value::as_f64),
                        is_active: false,
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Mark the buffer views availability uses. Cesium skips an index
        // with no buffer view here and fails when parsing the bitstream.
        let tile_availability_json = subtree_json
            .get("tileAvailability")
            .filter(|v| !v.is_null())
            .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'bitstream')"))?;
        let mut availability_jsons = vec![tile_availability_json];
        availability_jsons.extend(content_availability_headers.iter());
        let child_availability_json = subtree_json
            .get("childSubtreeAvailability")
            .filter(|v| !v.is_null())
            .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'bitstream')"))?;
        availability_jsons.push(child_availability_json);
        for json in &availability_jsons {
            if let Some(Some(view)) = availability_view(json) {
                if view < view_headers.len() {
                    activate(&mut view_headers, &mut buffer_headers, view)?;
                }
            }
        }

        let mut metadata_tables_json: Vec<&Value> = Vec::new();
        if let Some(json) = &tile_property_table_json {
            metadata_tables_json.push(json);
        }
        for json in &content_property_table_jsons {
            metadata_tables_json.push(
                json.as_ref()
                    .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'properties')"))?,
            );
        }
        for table in &metadata_tables_json {
            for (_, property) in json_entries(table.get("properties")) {
                let views = [
                    property.get("values").filter(|v| !v.is_null()).or(property.get("bufferView")),
                    property.get("stringOffsets").filter(|v| !v.is_null()).or(property.get("stringOffsetBufferView")),
                    property.get("arrayOffsets").filter(|v| !v.is_null()).or(property.get("arrayOffsetBufferView")),
                ];
                for (i, view) in views.into_iter().enumerate() {
                    // The values view is required; the offsets only when defined.
                    if i > 0 && !defined(view) {
                        continue;
                    }
                    let view = index(view)
                        .ok_or_else(|| type_error("Cannot set properties of undefined (setting 'isActive')"))?;
                    activate(&mut view_headers, &mut buffer_headers, view)?;
                }
            }
        }

        // Fetch the active buffers.
        let mut buffers: Vec<Option<U8View>> = Vec::with_capacity(buffer_headers.len());
        for header in &buffer_headers {
            buffers.push(match (header.is_active, &header.uri) {
                (false, _) => None,
                (true, Some(uri)) => {
                    let derived = resource
                        .get_derived_resource(DerivedResourceOptions { url: Some(uri.clone()), ..Default::default() })
                        .ok_or_else(|| MetadataError::Runtime(format!("Invalid buffer URI: {uri}")))?;
                    Some(U8View::whole(load_buffer(&derived)?.into()))
                }
                (true, None) => binary.clone(),
            });
        }

        // Slice the active buffer views.
        let mut views: Vec<Option<U8View>> = vec![None; view_headers.len()];
        for (i, header) in view_headers.iter().enumerate() {
            if !header.is_active {
                continue;
            }
            let buffer = header
                .buffer
                .and_then(|b| buffers.get(b).cloned().flatten())
                .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'subarray')"))?;
            let start = header.byte_offset.unwrap_or(0.0);
            let end = match (header.byte_offset, header.byte_length) {
                (Some(o), Some(l)) => o + l,
                _ => 0.0,
            };
            views[i] = Some(buffer.subarray(start.max(0.0) as usize, end.max(0.0) as usize));
        }

        // Availability.
        let branching_factor = implicit_tileset.branching_factor as f64;
        let subtree_levels = implicit_tileset.subtree_levels as f64;
        let tile_availability_bits =
            ((branching_factor.powf(subtree_levels) - 1.0) / (branching_factor - 1.0)) as usize;
        let child_subtree_bits = branching_factor.powf(subtree_levels) as usize;
        let has_metadata_extension = has_extension(&subtree_json, "3DTILES_metadata");
        let mut compute_available_count = has_metadata_extension || tile_property_table_json.is_some();
        let tile_availability = parse_availability_bitstream(
            tile_availability_json,
            &views,
            tile_availability_bits,
            compute_available_count,
        )?;
        compute_available_count = compute_available_count || !content_property_table_jsons.is_empty();
        let content_availability_bitstreams = content_availability_headers
            .iter()
            .map(|json| parse_availability_bitstream(json, &views, tile_availability_bits, compute_available_count))
            .collect::<Result<Vec<_>>>()?;
        let child_subtree_availability =
            parse_availability_bitstream(child_availability_json, &views, child_subtree_bits, false)?;

        // Buffer views by index, as metadata tables read them.
        let byte_views: JsMap<ByteView<'_>> = views
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.as_ref().map(|v| (i.to_string(), ByteView { buffer: &v.data, byte_offset: v.offset }))
            })
            .collect();
        let table = |json: &Value, availability: &ImplicitAvailabilityBitstream| -> Result<Arc<Mutex<MetadataTable>>> {
            let schema = implicit_tileset
                .metadata_schema
                .as_ref()
                .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'classes')"))?;
            let class = json["class"]
                .as_str()
                .and_then(|name| schema.classes.get(name).cloned())
                .ok_or_else(|| developer("Expected options.class to be typeof object, actual typeof was undefined"))?;
            let count = availability
                .available_count()
                .ok_or_else(|| developer("Expected options.count to be typeof number, actual typeof was undefined"))?;
            Ok(Arc::new(Mutex::new(MetadataTable::new(count, json.get("properties"), class, &byte_views)?)))
        };

        let mut tile_metadata_table = None;
        let mut tile_jump_buffer = None;
        if let Some(json) = &tile_property_table_json {
            tile_metadata_table = Some(table(json, &tile_availability)?);
            tile_jump_buffer = Some(make_jump_buffer(&tile_availability));
        }

        let mut content_metadata_tables = Vec::new();
        for (i, json) in content_property_table_jsons.iter().enumerate() {
            let json = json.as_ref().expect("checked when marking buffer views");
            let availability = content_availability_bitstreams
                .get(i)
                .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'availableCount')"))?;
            content_metadata_tables.push(table(json, availability)?);
        }
        let content_jump_buffers = content_availability_bitstreams.iter().map(make_jump_buffer).collect();
        drop(byte_views);

        Ok(Self {
            resource: resource.clone(),
            subtree_json,
            tile_availability,
            content_availability_bitstreams,
            child_subtree_availability,
            implicit_coordinates,
            subtree_levels: implicit_tileset.subtree_levels,
            subdivision_scheme: implicit_tileset.subdivision_scheme,
            branching_factor: implicit_tileset.branching_factor,
            metadata,
            tile_metadata_table,
            tile_property_table_json,
            content_metadata_tables,
            content_property_table_jsons,
            tile_jump_buffer,
            content_jump_buffers,
        })
    }

    /// Always true: the port builds the subtree before returning it.
    pub fn ready(&self) -> bool {
        true
    }

    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    /// The subtree JSON, with the `contentAvailabilityHeaders` Cesium adds.
    pub fn subtree_json(&self) -> &Value {
        &self.subtree_json
    }

    pub fn metadata(&self) -> Option<&ImplicitSubtreeMetadata> {
        self.metadata.as_ref()
    }

    pub fn tile_metadata_table(&self) -> Option<&Arc<Mutex<MetadataTable>>> {
        self.tile_metadata_table.as_ref()
    }

    pub fn tile_property_table_json(&self) -> Option<&Value> {
        self.tile_property_table_json.as_ref()
    }

    pub fn content_metadata_tables(&self) -> &[Arc<Mutex<MetadataTable>>] {
        &self.content_metadata_tables
    }

    /// One entry per `contentMetadata` index; `None` where the property
    /// table is missing.
    pub fn content_property_table_jsons(&self) -> &[Option<Value>] {
        &self.content_property_table_jsons
    }

    pub fn implicit_coordinates(&self) -> &ImplicitTileCoordinates {
        &self.implicit_coordinates
    }

    pub fn tile_availability(&self) -> &ImplicitAvailabilityBitstream {
        &self.tile_availability
    }

    pub fn content_availability_bitstreams(&self) -> &[ImplicitAvailabilityBitstream] {
        &self.content_availability_bitstreams
    }

    pub fn child_subtree_availability(&self) -> &ImplicitAvailabilityBitstream {
        &self.child_subtree_availability
    }

    /// The entity id of each available tile, by tile index; `None` without
    /// tile metadata.
    pub fn tile_jump_buffer(&self) -> Option<&[u32]> {
        self.tile_jump_buffer.as_deref()
    }

    pub fn content_jump_buffers(&self) -> &[Vec<u32>] {
        &self.content_jump_buffers
    }

    pub fn tile_is_available_at_index(&self, index: usize) -> Result<bool> {
        get_bit(&self.tile_availability, index)
    }

    pub fn tile_is_available_at_coordinates(&self, coordinates: &ImplicitTileCoordinates) -> Result<bool> {
        self.tile_is_available_at_index(self.get_tile_index(coordinates)?)
    }

    /// `content_index` defaults to 0.
    pub fn content_is_available_at_index(&self, index: usize, content_index: Option<usize>) -> Result<bool> {
        let content_index = content_index.unwrap_or(0);
        let bitstream = self
            .content_availability_bitstreams
            .get(content_index)
            .ok_or_else(|| developer("contentIndex out of bounds."))?;
        get_bit(bitstream, index)
    }

    pub fn content_is_available_at_coordinates(
        &self,
        coordinates: &ImplicitTileCoordinates,
        content_index: Option<usize>,
    ) -> Result<bool> {
        self.content_is_available_at_index(self.get_tile_index(coordinates)?, content_index)
    }

    pub fn child_subtree_is_available_at_index(&self, index: usize) -> Result<bool> {
        get_bit(&self.child_subtree_availability, index)
    }

    pub fn child_subtree_is_available_at_coordinates(&self, coordinates: &ImplicitTileCoordinates) -> Result<bool> {
        self.child_subtree_is_available_at_index(self.get_child_subtree_index(coordinates)?)
    }

    /// The number of tiles above `level` in the subtree.
    pub fn get_level_offset(&self, level: u32) -> f64 {
        let b = self.branching_factor as f64;
        (b.powf(level as f64) - 1.0) / (b - 1.0)
    }

    pub fn get_parent_morton_index(&self, morton_index: u32) -> u32 {
        let bits_per_level = if self.subdivision_scheme == ImplicitSubdivisionScheme::Octree { 3 } else { 2 };
        ((morton_index as i32) >> bits_per_level) as u32
    }

    /// The tile's index in this subtree's tile availability.
    pub fn get_tile_index(&self, coordinates: &ImplicitTileCoordinates) -> Result<usize> {
        let local_level = coordinates.level as i64 - self.implicit_coordinates.level as i64;
        if local_level < 0 || self.subtree_levels as i64 <= local_level {
            return Err(MetadataError::Runtime("level is out of bounds for this subtree".into()));
        }
        let subtree_coordinates = coordinates.get_subtree_coordinates()?;
        let offset = subtree_coordinates.get_offset_coordinates(coordinates)?;
        offset
            .tile_index()
            .map(|i| i as usize)
            .ok_or_else(|| developer("tile coordinates exceed the Morton index range"))
    }

    /// The child subtree's index in the child subtree availability.
    pub fn get_child_subtree_index(&self, coordinates: &ImplicitTileCoordinates) -> Result<usize> {
        let local_level = coordinates.level as i64 - self.implicit_coordinates.level as i64;
        if local_level != self.implicit_coordinates.subtree_levels as i64 {
            return Err(MetadataError::Runtime("level is out of bounds for this subtree".into()));
        }
        let parent = coordinates.get_parent_subtree_coordinates()?;
        let offset = parent.get_offset_coordinates(coordinates)?;
        offset
            .morton_index()
            .map(|i| i as usize)
            .ok_or_else(|| developer("tile coordinates exceed the Morton index range"))
    }

    fn get_tile_entity_id(&self, coordinates: &ImplicitTileCoordinates) -> Result<Option<u32>> {
        let (Some(_), Some(jump_buffer)) = (&self.tile_metadata_table, &self.tile_jump_buffer) else {
            return Ok(None);
        };
        let tile_index = self.get_tile_index(coordinates)?;
        Ok(get_bit(&self.tile_availability, tile_index)?.then(|| jump_buffer[tile_index]))
    }

    fn get_content_entity_id(
        &self,
        coordinates: &ImplicitTileCoordinates,
        content_index: usize,
    ) -> Result<Option<u32>> {
        if content_index >= self.content_metadata_tables.len() {
            return Ok(None);
        }
        let availability = &self.content_availability_bitstreams[content_index];
        let tile_index = self.get_tile_index(coordinates)?;
        Ok(get_bit(availability, tile_index)?.then(|| self.content_jump_buffers[content_index][tile_index]))
    }

    /// The tile's row of the tile metadata table; `None` without tile
    /// metadata or when the tile is unavailable.
    pub fn get_tile_metadata_view(
        &self,
        coordinates: &ImplicitTileCoordinates,
    ) -> Result<Option<ImplicitMetadataView>> {
        let Some(entity_id) = self.get_tile_entity_id(coordinates)? else {
            return Ok(None);
        };
        let table = self.tile_metadata_table.as_ref().expect("entity ids need a table");
        let class = table.lock().expect("metadata table lock").class().clone();
        let json = self.tile_property_table_json.as_ref().expect("a tile table has its JSON");
        Ok(Some(ImplicitMetadataView::new(table.clone(), class, entity_id as usize, json)?))
    }

    /// The content's row of its content metadata table.
    pub fn get_content_metadata_view(
        &self,
        coordinates: &ImplicitTileCoordinates,
        content_index: usize,
    ) -> Result<Option<ImplicitMetadataView>> {
        let Some(entity_id) = self.get_content_entity_id(coordinates, content_index)? else {
            return Ok(None);
        };
        let table = &self.content_metadata_tables[content_index];
        let class = table.lock().expect("metadata table lock").class().clone();
        let json = self.content_property_table_jsons[content_index].as_ref().expect("a content table has its JSON");
        Ok(Some(ImplicitMetadataView::new(table.clone(), class, entity_id as usize, json)?))
    }
}

fn get_bit(bitstream: &ImplicitAvailabilityBitstream, index: usize) -> Result<bool> {
    bitstream.get_bit(index).ok_or_else(|| developer("Bit index out of bounds."))
}

/// The JSON and binary chunks of a binary subtree: a 24-byte header with
/// the two chunk lengths as 64-bit little-endian integers (Cesium reads the
/// low 32 bits), then the chunks.
fn parse_subtree_chunks(view: U8View) -> Result<(Value, U8View)> {
    let bytes = view.bytes();
    let u32_at = |offset: usize| -> Result<usize> {
        bytes
            .get(offset..offset + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| MetadataError::Range("Offset is outside the bounds of the DataView".into()))
    };
    let json_byte_length = u32_at(8)?;
    let binary_byte_length = u32_at(16)?;
    let text = get_string_from_typed_array(bytes, 24, Some(json_byte_length));
    let json: Value = serde_json::from_str(&text).map_err(|e| MetadataError::Syntax(e.to_string()))?;
    let start = 24 + json_byte_length;
    Ok((json, view.subarray(start, start + binary_byte_length)))
}

fn parse_availability_bitstream(
    json: &Value,
    views: &[Option<U8View>],
    length_bits: usize,
    compute_available_count_enabled: bool,
) -> Result<ImplicitAvailabilityBitstream> {
    let available_count = json.get("availableCount").and_then(Value::as_u64).map(|c| c as usize);
    if let Some(constant) = json.get("constant").filter(|c| !c.is_null()) {
        return ImplicitAvailabilityBitstream::new(ImplicitAvailabilityBitstreamOptions {
            length_bits,
            available_count,
            constant: Some(truthy(constant)),
            ..Default::default()
        })
        .map_err(|e| MetadataError::Runtime(e.message));
    }
    let bitstream = availability_view(json)
        .flatten()
        .and_then(|i| views.get(i).cloned().flatten())
        .ok_or_else(|| type_error("Cannot read properties of undefined (reading 'length')"))?;
    ImplicitAvailabilityBitstream::new(ImplicitAvailabilityBitstreamOptions {
        length_bits,
        available_count,
        constant: None,
        bitstream: Some(bitstream.bytes().to_vec()),
        compute_available_count_enabled,
    })
    .map_err(|e| MetadataError::Runtime(e.message))
}

/// The entity id of each available bit, stored in a `Uint8Array`,
/// `Uint16Array` or `Uint32Array` chosen by the available count; ids wrap
/// at the array's width as Cesium's do.
fn make_jump_buffer(availability: &ImplicitAvailabilityBitstream) -> Vec<u32> {
    let mask: u32 = match availability.available_count() {
        Some(count) if count < 256 => 0xff,
        Some(count) if count < 65536 => 0xffff,
        _ => u32::MAX,
    };
    let mut entity_id: u32 = 0;
    let mut jump_buffer = vec![0; availability.length_bits()];
    for (i, slot) in jump_buffer.iter_mut().enumerate() {
        if availability.get_bit(i) == Some(true) {
            *slot = entity_id & mask;
            entity_id = entity_id.wrapping_add(1);
        }
    }
    jump_buffer
}
