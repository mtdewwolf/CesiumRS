//! CesiumJS `Vector3DTileContent`: parses a `vctr` tile and composes the
//! polygon, polyline, clamped-polyline and point primitives.
//!
//! The 3D Tiles core port owns `Cesium3DTileBatchTable`; this module keeps the
//! boundary it needs through [`Vector3DTileBatchTable`]. Batch-table JSON and
//! binary sections are validated and retained for byte accounting, but feature
//! properties are not decoded (the vector features here carry style state only).
//! The byte layout, validation, error strings, batch-id defaults and primitive
//! composition follow the pinned source.

use super::{
    batch_table_hierarchy::{BatchTableHierarchy, BinaryColumn},
    cesium_3d_tile_style::Cesium3DTileStyle,
    json_metadata_table::JsonColumn,
    metadata_table_property::ByteView,
    metadata_value::{json_entries, JsMap, MetadataValue},
    scene::Result,
    vector_3d_tile_clamped_polylines::{Vector3DTileClampedPolylines, Vector3DTileClampedPolylinesOptions},
    vector_3d_tile_points::{Vector3DTilePoints, Vector3DTilePointsOptions},
    vector_3d_tile_polygons::{Vector3DTilePolygons, Vector3DTilePolygonsOptions},
    vector_3d_tile_polylines::{Vector3DTilePolylines, Vector3DTilePolylinesOptions},
    vector_3d_tile_primitive::{
        BatchTableRef, FeatureList, SimpleVector3DTileBatchTable, Vector3DTileBatchTable,
    },
    Cesium3DTileFeatureTable, ClassificationType, FrameState, HeightReference, TileContentMetrics,
    TilesetContent,
};
use crate::{
    core::{
        math, Cartesian3, Color, ComponentDatatype, Ellipsoid, Matrix4, Ray, Rectangle, RuntimeError, TypedArray,
    },
    core::decode_vector_polyline_positions::{decode_vector_polyline_positions, DecodeVectorPolylineOptions},
    core::get_string_from_typed_array::get_json_from_typed_array,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// Callback type for `tileset.examineVectorLinesFunction`.
pub type ExamineVectorLines =
    Rc<dyn Fn(&[f64], u16, &str, &BatchTableRef)>;

/// Tile-level options that Cesium reads from the owning tileset.
#[derive(Clone)]
pub struct Vector3DTileContentOptions {
    pub url: String,
    pub model_matrix: Matrix4,
    pub bounding_volume: crate::core::BoundingSphere,
    /// `None` selects floating polylines; `Some` selects clamped polylines.
    pub classification_type: Option<ClassificationType>,
    pub height_reference: HeightReference,
    pub vector_keep_decoded_positions: bool,
    pub debug_wireframe: bool,
    pub ellipsoid: Ellipsoid,
    pub examine_vector_lines: Option<ExamineVectorLines>,
}

impl Default for Vector3DTileContentOptions {
    fn default() -> Self {
        Self {
            url: String::new(),
            model_matrix: Matrix4::IDENTITY,
            bounding_volume: crate::core::BoundingSphere::new(Cartesian3::ZERO, 0.0),
            classification_type: None,
            height_reference: HeightReference::None,
            vector_keep_decoded_positions: false,
            debug_wireframe: false,
            ellipsoid: Ellipsoid::WGS84,
            examine_vector_lines: None,
        }
    }
}

fn range_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(message)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let slice = bytes.get(offset..offset + 4).ok_or_else(|| range_error("Offset is outside the bounds of the DataView"))?;
    Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
}

fn view_u16(bytes: &Arc<Vec<u8>>, offset: usize, count: usize) -> Result<Vec<u16>> {
    if !offset.is_multiple_of(2) {
        return Err(range_error("start offset of Uint16Array should be a multiple of 2"));
    }
    let length = count
        .checked_mul(2)
        .ok_or_else(|| range_error("Invalid typed array length"))?;
    let slice = bytes
        .get(offset..offset + length)
        .ok_or_else(|| range_error("Invalid typed array length"))?;
    Ok(slice
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect())
}

fn view_u32(bytes: &Arc<Vec<u8>>, offset: usize, count: usize) -> Result<Vec<u32>> {
    if !offset.is_multiple_of(4) {
        return Err(range_error("start offset of Uint32Array should be a multiple of 4"));
    }
    let length = count
        .checked_mul(4)
        .ok_or_else(|| range_error("Invalid typed array length"))?;
    let slice = bytes
        .get(offset..offset + length)
        .ok_or_else(|| range_error("Invalid typed array length"))?;
    Ok(slice
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect())
}

/// The three optional batch-id arrays of a vector tile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VectorTileBatchIds {
    pub polygons: Vec<u16>,
    pub polylines: Vec<u16>,
    pub points: Vec<u16>,
}

fn json_count(json: &serde_json::Value, key: &str) -> usize {
    match json.get(key) {
        None | Some(serde_json::Value::Null) => 0,
        Some(value) => {
            let number = super::gltf_pipeline::js::to_number(Some(value));
            if !number.is_finite() || number <= 0.0 {
                0
            } else {
                number as usize
            }
        }
    }
}

fn get_batch_ids(
    feature_table_json: &serde_json::Value,
    feature_table_binary: &super::gltf_pipeline::BufferSource,
    buffer: &Arc<Vec<u8>>,
) -> Result<VectorTileBatchIds> {
    let number_of_polygons = json_count(feature_table_json, "POLYGONS_LENGTH");
    let number_of_polylines = json_count(feature_table_json, "POLYLINES_LENGTH");
    let number_of_points = json_count(feature_table_json, "POINTS_LENGTH");

    let read = |name: &str, length: usize| -> Result<Option<Vec<u16>>> {
        let Some(property) = feature_table_json.get(name) else {
            return Ok(None);
        };
        if property.is_null() {
            return Ok(None);
        }
        let offset = super::gltf_pipeline::js::to_number(property.get("byteOffset"));
        let absolute = feature_table_binary.byte_offset as f64 + offset;
        if !absolute.is_finite() || absolute < 0.0 {
            return Err(range_error("Invalid typed array length"));
        }
        Ok(Some(view_u16(buffer, absolute as usize, length)?))
    };

    let mut polygon_batch_ids = read("POLYGON_BATCH_IDS", number_of_polygons)?;
    let mut polyline_batch_ids = read("POLYLINE_BATCH_IDS", number_of_polylines)?;
    let mut point_batch_ids = read("POINT_BATCH_IDS", number_of_points)?;

    let at_least_one_defined =
        polygon_batch_ids.is_some() || polyline_batch_ids.is_some() || point_batch_ids.is_some();
    let at_least_one_undefined = (number_of_polygons > 0 && polygon_batch_ids.is_none())
        || (number_of_polylines > 0 && polyline_batch_ids.is_none())
        || (number_of_points > 0 && point_batch_ids.is_none());
    if at_least_one_defined && at_least_one_undefined {
        return Err(range_error(
            "If one group of batch ids is defined, then all batch ids must be defined",
        ));
    }

    let all_undefined = polygon_batch_ids.is_none() && polyline_batch_ids.is_none() && point_batch_ids.is_none();
    if all_undefined {
        let mut id = 0u16;
        if number_of_polygons > 0 {
            polygon_batch_ids = Some((0..number_of_polygons).map(|_| {
                let value = id;
                id = id.wrapping_add(1);
                value
            }).collect());
        }
        if number_of_polylines > 0 {
            polyline_batch_ids = Some((0..number_of_polylines).map(|_| {
                let value = id;
                id = id.wrapping_add(1);
                value
            }).collect());
        }
        if number_of_points > 0 {
            point_batch_ids = Some((0..number_of_points).map(|_| {
                let value = id;
                id = id.wrapping_add(1);
                value
            }).collect());
        }
    }

    Ok(VectorTileBatchIds {
        polygons: polygon_batch_ids.unwrap_or_default(),
        polylines: polyline_batch_ids.unwrap_or_default(),
        points: point_batch_ids.unwrap_or_default(),
    })
}

fn property_array(
    feature_table: &Cesium3DTileFeatureTable,
    names: &[&str],
    component_type: ComponentDatatype,
    component_length: usize,
) -> Result<Option<Rc<TypedArray>>> {
    for name in names {
        if let Some(values) = feature_table
            .get_property_array(name, component_type, component_length)
            .map_err(|e| range_error(e.to_string()))?
        {
            return Ok(Some(values));
        }
    }
    Ok(None)
}

fn typed_to_u32(values: &TypedArray) -> Vec<u32> {
    values.to_f64_vec().iter().map(|&v| v as u32).collect()
}

fn typed_to_f32(values: &TypedArray) -> Vec<f32> {
    values.to_f64_vec().iter().map(|&v| v as f32).collect()
}

/// The contents of a vector (`.vctr`) tile.
pub struct Vector3DTileContent<G: crate::renderer::Gl + 'static> {
    url: String,
    polygons: Option<Vector3DTilePolygons<G>>,
    polylines: Option<Vector3DTilePolylines<G>>,
    clamped_polylines: Option<Vector3DTileClampedPolylines<G>>,
    points: Option<Vector3DTilePoints>,
    batch_table: BatchTableRef,
    features: Option<FeatureList>,
    ready: bool,
    pub feature_properties_dirty: bool,
    metadata: Option<serde_json::Value>,
    group: Option<serde_json::Value>,
    debug_wireframe: bool,
    classification_type: Option<ClassificationType>,
    destroyed: bool,
}

impl<G: crate::renderer::Gl + 'static> Vector3DTileContent<G> {
    /// Parses a vector tile. `byte_offset` is the start of the tile inside
    /// `array_buffer`, as for a composite tile.
    pub fn from_bytes(
        options: Vector3DTileContentOptions,
        array_buffer: Arc<Vec<u8>>,
        byte_offset: usize,
    ) -> Result<Self> {
        let mut byte_offset = byte_offset;
        let buffer = array_buffer;

        // Skip the magic number.
        byte_offset += 4;
        let version = read_u32(&buffer, byte_offset)?;
        if version != 1 {
            return Err(range_error(format!(
                "Only Vector tile version 1 is supported.  Version {version} is not."
            )));
        }
        byte_offset += 4;
        let byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;

        if byte_length == 0 {
            let batch_table: BatchTableRef = Rc::new(RefCell::new(SimpleVector3DTileBatchTable::new(0)));
            return Ok(Self {
                url: options.url,
                polygons: None,
                polylines: None,
                clamped_polylines: None,
                points: None,
                batch_table,
                features: None,
                ready: true,
                feature_properties_dirty: false,
                metadata: None,
                group: None,
                debug_wireframe: options.debug_wireframe,
                classification_type: options.classification_type,
                destroyed: false,
            });
        }

        let feature_table_json_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;
        if feature_table_json_byte_length == 0 {
            return Err(range_error("Feature table must have a byte length greater than zero"));
        }
        let feature_table_binary_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;
        let batch_table_json_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;
        let batch_table_binary_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;
        let indices_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;
        let position_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;
        let polyline_position_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;
        let points_position_byte_length = read_u32(&buffer, byte_offset)? as usize;
        byte_offset += 4;

        let feature_table_json =
            get_json_from_typed_array(&buffer, byte_offset, Some(feature_table_json_byte_length))?;
        byte_offset += feature_table_json_byte_length;

        let feature_table_binary = super::gltf_pipeline::BufferSource::view(
            buffer.clone(),
            byte_offset,
            feature_table_binary_byte_length,
        );
        byte_offset += feature_table_binary_byte_length;

        let mut batch_table_json = None;
        let mut batch_table_binary_copied: Option<Vec<u8>> = None;
        if batch_table_json_byte_length > 0 {
            batch_table_json =
                Some(get_json_from_typed_array(&buffer, byte_offset, Some(batch_table_json_byte_length))?);
            byte_offset += batch_table_json_byte_length;
            if batch_table_binary_byte_length > 0 {
                // Copied so the tile buffer can be released, as upstream does.
                let start = byte_offset.min(buffer.len());
                let end = (byte_offset + batch_table_binary_byte_length).min(buffer.len());
                batch_table_binary_copied = Some(buffer[start..end].to_vec());
                byte_offset += batch_table_binary_byte_length;
            }
        }

        let number_of_polygons = json_count(&feature_table_json, "POLYGONS_LENGTH");
        let number_of_polylines = json_count(&feature_table_json, "POLYLINES_LENGTH");
        let number_of_points = json_count(&feature_table_json, "POINTS_LENGTH");
        let total_primitives = number_of_polygons + number_of_polylines + number_of_points;

        let batch_table: BatchTableRef = Rc::new(RefCell::new(VectorBatchTable::new(
            total_primitives,
            batch_table_json.as_ref(),
            batch_table_binary_copied.as_deref(),
        )?));

        if total_primitives == 0 {
            return Ok(Self {
                url: options.url,
                polygons: None,
                polylines: None,
                clamped_polylines: None,
                points: None,
                batch_table,
                features: None,
                ready: false,
                feature_properties_dirty: false,
                metadata: None,
                group: None,
                debug_wireframe: options.debug_wireframe,
                classification_type: options.classification_type,
                destroyed: false,
            });
        }

        let feature_table = Cesium3DTileFeatureTable::new(feature_table_json.clone(), feature_table_binary.clone());
        let region = feature_table
            .get_global_property("REGION", None, None)
            .map_err(|e| range_error(e.to_string()))?
            .ok_or_else(|| range_error("Feature table global property: REGION must be defined"))?;
        let mut region_values = [0.0f64; 6];
        for (i, value) in region_values.iter_mut().enumerate() {
            *value = region.element(i).unwrap_or(f64::NAN);
        }
        let rectangle = Rectangle::unpack(&region_values, 0);
        let min_height = region_values[4];
        let max_height = region_values[5];

        let model_matrix = options.model_matrix;
        let mut center = match feature_table
            .get_global_property("RTC_CENTER", Some(ComponentDatatype::Float), Some(3))
            .map_err(|e| range_error(e.to_string()))?
        {
            Some(value) => {
                let mut center = Cartesian3::ZERO;
                center.x = value.element(0).unwrap_or(f64::NAN);
                center.y = value.element(1).unwrap_or(f64::NAN);
                center.z = value.element(2).unwrap_or(f64::NAN);
                model_matrix.multiply_by_point(center)
            }
            None => {
                let mut c = rectangle.center();
                c.height = math::lerp(min_height, max_height, 0.5);
                Ellipsoid::WGS84.cartographic_to_cartesian(&c)
            }
        };

        let batch_ids = get_batch_ids(&feature_table_json, &feature_table_binary, &buffer)?;
        byte_offset += (4 - (byte_offset % 4)) % 4;

        let mut polygons = None;
        let mut polylines = None;
        let mut clamped_polylines = None;
        let mut points = None;

        if number_of_polygons > 0 {
            let mut feature_table = Cesium3DTileFeatureTable::new(feature_table_json.clone(), feature_table_binary.clone());
            feature_table.features_length = number_of_polygons as f64;
            let polygon_counts = property_array(
                &feature_table,
                &["POLYGON_COUNTS", "POLYGON_COUNT"],
                ComponentDatatype::UnsignedInt,
                1,
            )?
            .ok_or_else(|| {
                range_error("Feature table property: POLYGON_COUNTS must be defined when POLYGONS_LENGTH is greater than 0")
            })?;
            let polygon_index_counts = property_array(
                &feature_table,
                &["POLYGON_INDEX_COUNTS", "POLYGON_INDEX_COUNT"],
                ComponentDatatype::UnsignedInt,
                1,
            )?
            .ok_or_else(|| {
                range_error("Feature table property: POLYGON_INDEX_COUNTS must be defined when POLYGONS_LENGTH is greater than 0")
            })?;

            let counts = typed_to_u32(&polygon_counts);
            let index_counts = typed_to_u32(&polygon_index_counts);
            let num_polygon_positions: usize = counts.iter().map(|&c| c as usize * 2).sum();
            let num_polygon_indices: usize = index_counts.iter().map(|&c| c as usize).sum();

            let indices = view_u32(&buffer, byte_offset, num_polygon_indices)?;
            byte_offset += indices_byte_length;
            let positions = view_u16(&buffer, byte_offset, num_polygon_positions)?;
            byte_offset += position_byte_length;

            let (polygon_minimum_heights, polygon_maximum_heights) = if feature_table_json
                .get("POLYGON_MINIMUM_HEIGHTS")
                .is_some()
                && feature_table_json.get("POLYGON_MAXIMUM_HEIGHTS").is_some()
            {
                let min = feature_table
                    .get_property_array("POLYGON_MINIMUM_HEIGHTS", ComponentDatatype::Float, 1)
                    .map_err(|e| range_error(e.to_string()))?;
                let max = feature_table
                    .get_property_array("POLYGON_MAXIMUM_HEIGHTS", ComponentDatatype::Float, 1)
                    .map_err(|e| range_error(e.to_string()))?;
                (min.map(|v| typed_to_f32(&v)), max.map(|v| typed_to_f32(&v)))
            } else {
                (None, None)
            };

            polygons = Some(Vector3DTilePolygons::new(Vector3DTilePolygonsOptions {
                positions,
                counts,
                indices: TypedArray::Uint32(indices),
                index_counts,
                minimum_height: min_height,
                maximum_height: max_height,
                polygon_minimum_heights,
                polygon_maximum_heights,
                rectangle,
                ellipsoid: Some(Ellipsoid::WGS84),
                center: Some(center),
                batch_table: Some(batch_table.clone()),
                batch_ids: batch_ids.polygons.clone(),
                bounding_volume: options.bounding_volume,
            }));
        }

        if number_of_polylines > 0 {
            let mut feature_table = Cesium3DTileFeatureTable::new(feature_table_json.clone(), feature_table_binary.clone());
            feature_table.features_length = number_of_polylines as f64;
            let polyline_counts = property_array(
                &feature_table,
                &["POLYLINE_COUNTS", "POLYLINE_COUNT"],
                ComponentDatatype::UnsignedInt,
                1,
            )?
            .ok_or_else(|| {
                range_error("Feature table property: POLYLINE_COUNTS must be defined when POLYLINES_LENGTH is greater than 0")
            })?;
            let counts = typed_to_u32(&polyline_counts);
            let mut widths = match feature_table
                .get_property_array("POLYLINE_WIDTHS", ComponentDatatype::UnsignedShort, 1)
                .map_err(|e| range_error(e.to_string()))?
            {
                Some(values) => values.to_f64_vec().iter().map(|&v| v as u16).collect(),
                None => vec![2u16; number_of_polylines],
            };
            let num_polyline_positions: usize = counts.iter().map(|&c| c as usize * 3).sum();
            let positions = view_u16(&buffer, byte_offset, num_polyline_positions)?;
            byte_offset += polyline_position_byte_length;

            if let Some(examine) = &options.examine_vector_lines {
                let decoded = decode_vector_polyline_positions(
                    &mut positions.clone(),
                    DecodeVectorPolylineOptions {
                        rectangle: &rectangle,
                        minimum_height: min_height,
                        maximum_height: max_height,
                        ellipsoid: &Ellipsoid::WGS84,
                    },
                );
                if let Some(decoded) = decoded {
                    let mut start = 0usize;
                    for (i, &count) in counts.iter().enumerate() {
                        let count = count as usize * 3;
                        let end = (start + count).min(decoded.len());
                        examine(
                            &decoded[start..end],
                            batch_ids.polylines.get(i).copied().unwrap_or(0),
                            &options.url,
                            &batch_table,
                        );
                        start = end;
                    }
                }
            }

            if options.classification_type.is_some() {
                clamped_polylines = Some(Vector3DTileClampedPolylines::new(Vector3DTileClampedPolylinesOptions {
                    positions,
                    counts: counts.clone(),
                    widths,
                    minimum_height: min_height,
                    maximum_height: max_height,
                    rectangle,
                    ellipsoid: Some(Ellipsoid::WGS84),
                    center: Some(center),
                    batch_table: Some(batch_table.clone()),
                    batch_ids: batch_ids.polylines.clone(),
                    classification_type: options.classification_type.unwrap_or(ClassificationType::Both),
                    keep_decoded_positions: options.vector_keep_decoded_positions,
                }));
            } else {
                polylines = Some(Vector3DTilePolylines::new(Vector3DTilePolylinesOptions {
                    positions,
                    counts,
                    widths: std::mem::take(&mut widths),
                    minimum_height: min_height,
                    maximum_height: max_height,
                    rectangle,
                    ellipsoid: Some(Ellipsoid::WGS84),
                    center: Some(center),
                    batch_table: Some(batch_table.clone()),
                    batch_ids: batch_ids.polylines.clone(),
                    bounding_volume: options.bounding_volume,
                    keep_decoded_positions: options.vector_keep_decoded_positions,
                }));
            }
        }

        if number_of_points > 0 {
            let positions = view_u16(&buffer, byte_offset, number_of_points * 3)?;
            byte_offset += points_position_byte_length;
            let _ = byte_offset;
            points = Some(Vector3DTilePoints::new(Vector3DTilePointsOptions {
                positions,
                batch_ids: batch_ids.points.clone(),
                minimum_height: min_height,
                maximum_height: max_height,
                rectangle,
                height_reference: options.height_reference,
                batch_table: Some(batch_table.clone()),
                ellipsoid: options.ellipsoid,
            }));
        }

        let _ = &mut center;
        let _ = model_matrix;
        Ok(Self {
            url: options.url,
            polygons,
            polylines,
            clamped_polylines,
            points,
            batch_table,
            features: None,
            ready: false,
            feature_properties_dirty: false,
            metadata: None,
            group: None,
            debug_wireframe: options.debug_wireframe,
            classification_type: options.classification_type,
            destroyed: false,
        })
    }

    pub fn features_length(&self) -> usize {
        self.batch_table.borrow().features_length()
    }
    pub fn points_length(&self) -> usize {
        self.points.as_ref().map_or(0, |points| points.points_length())
    }
    pub fn triangles_length(&self) -> f64 {
        let mut total = 0.0;
        if let Some(polygons) = &self.polygons {
            total += polygons.triangles_length();
        }
        if let Some(polylines) = &self.polylines {
            total += polylines.triangles_length();
        }
        total
    }
    pub fn geometry_byte_length(&self) -> usize {
        let mut total = 0;
        if let Some(polygons) = &self.polygons {
            total += polygons.geometry_byte_length();
        }
        if let Some(polylines) = &self.polylines {
            total += polylines.geometry_byte_length();
        }
        total
    }
    pub fn textures_byte_length(&self) -> usize {
        self.points.as_ref().map_or(0, |points| points.textures_byte_length())
    }
    pub fn batch_table_byte_length(&self) -> usize {
        self.batch_table.borrow().batch_table_byte_length()
    }
    pub fn inner_contents(&self) -> Option<&[()]> {
        None
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_ref()
    }
    pub fn set_metadata(&mut self, value: Option<serde_json::Value>) {
        self.metadata = value;
    }
    pub fn batch_table(&self) -> &BatchTableRef {
        &self.batch_table
    }
    pub fn group(&self) -> Option<&serde_json::Value> {
        self.group.as_ref()
    }
    pub fn set_group(&mut self, value: Option<serde_json::Value>) {
        self.group = value;
    }
    pub fn polygons(&self) -> Option<&Vector3DTilePolygons<G>> {
        self.polygons.as_ref()
    }
    pub fn polylines(&self) -> Option<&Vector3DTilePolylines<G>> {
        self.polylines.as_ref()
    }
    pub fn clamped_polylines(&self) -> Option<&Vector3DTileClampedPolylines<G>> {
        self.clamped_polylines.as_ref()
    }
    pub fn points(&self) -> Option<&Vector3DTilePoints> {
        self.points.as_ref()
    }
    pub fn feature_list(&self) -> Option<&FeatureList> {
        self.features.as_ref()
    }
    pub fn get_feature(&self, batch_id: usize) -> Option<&super::vector_3d_tile_primitive::Vector3DTileFeature> {
        self.features.as_ref()?.get(batch_id)?.as_ref()
    }
    pub fn has_property(&self, batch_id: usize, name: &str) -> bool {
        self.batch_table.borrow().has_property(batch_id, name)
    }

    fn create_features(&mut self) {
        if self.features.is_some() {
            return;
        }
        let length = self.batch_table.borrow().features_length();
        if length == 0 {
            return;
        }
        let mut features: FeatureList = vec![None; length];
        if let Some(polygons) = &self.polygons {
            polygons.create_features(&mut features);
        }
        if let Some(polylines) = &self.polylines {
            polylines.create_features(&mut features);
        }
        if let Some(polylines) = &self.clamped_polylines {
            polylines.create_features(&mut features);
        }
        if let Some(points) = &self.points {
            points.create_features(&mut features);
        }
        // Batch-table JSON arrays become per-feature properties for styling.
        let table = self.batch_table.borrow();
        let names = table.property_names();
        for (batch_id, feature) in features.iter_mut().enumerate() {
            let Some(feature) = feature.as_mut() else {
                continue;
            };
            for name in &names {
                if let Some(value) = table.property(batch_id, name) {
                    feature
                        .properties
                        .insert(name.clone(), super::expression::StyleValue::from_json(&value));
                }
            }
        }
        drop(table);
        self.features = Some(features);
    }

    pub fn apply_debug_settings(&mut self, enabled: bool, color: Color) {
        if let Some(polygons) = &mut self.polygons {
            polygons.apply_debug_settings(enabled, color);
        }
        if let Some(polylines) = &mut self.polylines {
            polylines.apply_debug_settings(enabled, color);
        }
        if let Some(polylines) = &mut self.clamped_polylines {
            polylines.apply_debug_settings(enabled, color);
        }
        if let Some(points) = &mut self.points {
            points.apply_debug_settings(enabled, color);
        }
    }

    pub fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>) -> Result<()> {
        self.create_features();
        let Some(mut features) = self.features.take() else {
            return Ok(());
        };
        if let Some(polygons) = &mut self.polygons {
            polygons.apply_style(style, &mut features)?;
        }
        if let Some(polylines) = &mut self.polylines {
            polylines.apply_style(style, &mut features)?;
        }
        if let Some(polylines) = &mut self.clamped_polylines {
            polylines.apply_style(style, &mut features)?;
        }
        if let Some(points) = &mut self.points {
            points.apply_style(style, &mut features)?;
        }
        self.features = Some(features);
        Ok(())
    }

    /// Cesium's `update`; returns whether every primitive is ready.
    pub fn update_content(&mut self, frame: &mut FrameState) -> Result<bool> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        let mut ready = true;
        if let Some(polygons) = &mut self.polygons {
            polygons.classification_type = self.classification_type.unwrap_or(ClassificationType::Both);
            polygons.debug_wireframe = self.debug_wireframe;
            polygons.update(frame)?;
            ready &= polygons.ready();
        }
        if let Some(polylines) = &mut self.polylines {
            polylines.update(frame)?;
            ready &= polylines.ready();
        }
        if let Some(polylines) = &mut self.clamped_polylines {
            polylines.update(frame)?;
            ready &= polylines.ready();
        }
        if let Some(points) = &mut self.points {
            points.update_with_context::<G>(frame)?;
            ready &= points.ready();
        }
        if ready {
            self.create_features();
            self.batch_table.borrow_mut().update(frame)?;
            self.ready = true;
        }
        Ok(ready)
    }

    pub fn get_polyline_positions(&self, batch_id: u16) -> Option<Vec<f64>> {
        if let Some(polylines) = &self.polylines {
            return polylines.get_positions(batch_id);
        }
        if let Some(polylines) = &self.clamped_polylines {
            return polylines.get_positions(batch_id);
        }
        None
    }

    pub fn pick(&self, _ray: &Ray, _frame: &FrameState) -> Result<Option<Cartesian3>> {
        Ok(None)
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy_content(&mut self) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if let Some(mut polygons) = self.polygons.take() {
            polygons.destroy()?;
        }
        if let Some(mut polylines) = self.polylines.take() {
            polylines.destroy()?;
        }
        if let Some(mut polylines) = self.clamped_polylines.take() {
            polylines.destroy()?;
        }
        if let Some(mut points) = self.points.take() {
            points.destroy()?;
        }
        self.features = None;
        self.destroyed = true;
        Ok(())
    }
}

enum VectorBatchColumn {
    Json(JsonColumn),
    Binary(BinaryColumn),
}

fn metadata_value_to_json(value: MetadataValue) -> serde_json::Value {
    match value {
        MetadataValue::Null => serde_json::Value::Null,
        MetadataValue::Boolean(b) => serde_json::Value::Bool(b),
        MetadataValue::Number(n) => serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        MetadataValue::BigInt(i) => serde_json::Value::String(i.to_string()),
        MetadataValue::String(s) => serde_json::Value::String(s),
        MetadataValue::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(metadata_value_to_json).collect())
        }
        MetadataValue::Cartesian2(v) => serde_json::json!([v.x, v.y]),
        MetadataValue::Cartesian3(v) => serde_json::json!([v.x, v.y, v.z]),
        MetadataValue::Cartesian4(v) => serde_json::json!([v.x, v.y, v.z, v.w]),
        MetadataValue::Matrix2(m) => serde_json::json!(m.0),
        MetadataValue::Matrix3(m) => serde_json::json!(m.0),
        MetadataValue::Matrix4(m) => serde_json::json!(m.0),
        MetadataValue::Object(v) => v,
    }
}

struct VectorBatchTable {
    colors: Vec<Color>,
    shows: Vec<bool>,
    columns: JsMap<VectorBatchColumn>,
    hierarchy: Option<BatchTableHierarchy>,
    property_bytes: usize,
}

impl VectorBatchTable {
    fn new(count: usize, json: Option<&serde_json::Value>, binary: Option<&[u8]>) -> Result<Self> {
        let body = binary.map(|buffer| ByteView { buffer, byte_offset: 0 });
        let hierarchy = json
            .and_then(|j| {
                j.get("HIERARCHY")
                    .filter(|v| !v.is_null())
                    .or_else(|| j.pointer("/extensions/3DTILES_batch_table_hierarchy"))
            })
            .filter(|v| !v.is_null())
            .map(|v| BatchTableHierarchy::new(v, body))
            .transpose()
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let mut property_bytes = hierarchy.as_ref().map_or(0, BatchTableHierarchy::byte_length);
        let mut columns = JsMap::new();
        for (name, value) in json_entries(json) {
            if matches!(name.as_str(), "HIERARCHY" | "extensions" | "extras") {
                continue;
            }
            let col = if value.get("byteOffset").is_some_and(|v| !v.is_null()) {
                let binary = BinaryColumn::read(&name, value, body.as_ref(), count as f64)
                    .map_err(|e| RuntimeError::new(e.to_string()))?;
                property_bytes += binary.byte_length();
                VectorBatchColumn::Binary(binary)
            } else {
                VectorBatchColumn::Json(JsonColumn::from_json(value))
            };
            columns.insert(name, col);
        }
        Ok(Self {
            colors: vec![Color::WHITE; count],
            shows: vec![true; count],
            columns,
            hierarchy,
            property_bytes,
        })
    }
}

impl Vector3DTileBatchTable for VectorBatchTable {
    fn features_length(&self) -> usize {
        self.colors.len()
    }
    fn batch_table_byte_length(&self) -> usize {
        self.property_bytes
    }
    fn get_color(&self, batch_id: usize) -> Color {
        self.colors.get(batch_id).copied().unwrap_or(Color::WHITE)
    }
    fn get_show(&self, batch_id: usize) -> bool {
        self.shows.get(batch_id).copied().unwrap_or(true)
    }
    fn set_color(&mut self, batch_id: usize, color: Color) {
        if let Some(slot) = self.colors.get_mut(batch_id) {
            *slot = color;
        }
    }
    fn set_show(&mut self, batch_id: usize, show: bool) {
        if let Some(slot) = self.shows.get_mut(batch_id) {
            *slot = show;
        }
    }
    fn has_property(&self, batch_id: usize, name: &str) -> bool {
        if batch_id >= self.colors.len() {
            return false;
        }
        if self
            .columns
            .get(name)
            .is_some_and(|c| !matches!(c, VectorBatchColumn::Json(JsonColumn::Null)))
        {
            return true;
        }
        self.hierarchy
            .as_ref()
            .and_then(|h| h.has_property(batch_id, name).ok())
            .unwrap_or(false)
    }
    fn property(&self, batch_id: usize, name: &str) -> Option<serde_json::Value> {
        if batch_id >= self.colors.len() {
            return None;
        }
        let value = match self.columns.get(name) {
            Some(VectorBatchColumn::Binary(c)) => c.get(batch_id).ok().flatten(),
            Some(VectorBatchColumn::Json(c)) if c.is_defined() => c.get(batch_id),
            _ => self.hierarchy.as_ref().and_then(|h| h.get_property(batch_id, name).ok().flatten()),
        };
        value.map(metadata_value_to_json)
    }
    fn property_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.columns.keys().map(str::to_string).collect();
        if let Some(hierarchy) = &self.hierarchy {
            for batch_id in 0..self.colors.len() {
                if let Ok(ids) = hierarchy.get_property_ids(batch_id) {
                    for id in ids {
                        if !names.contains(&id) {
                            names.push(id);
                        }
                    }
                }
            }
        }
        names
    }
}

impl<G: crate::renderer::Gl + 'static> TilesetContent for Vector3DTileContent<G> {
    fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>) -> Result<()> {
        Vector3DTileContent::apply_style(self, style)
    }
    fn apply_debug_settings(&mut self, enabled: bool, color: Color) -> Result<()> {
        Vector3DTileContent::apply_debug_settings(self, enabled, color);
        Ok(())
    }
    fn process(&mut self, _frame: &FrameState) -> Result<bool> {
        Ok(self.ready)
    }
    fn process_with_frame(&mut self, frame: &mut FrameState) -> Result<bool> {
        self.update_content(frame)
    }
    fn metrics(&self) -> TileContentMetrics {
        TileContentMetrics {
            features: self.features_length() as u64,
            points: self.points_length() as u64,
            triangles: self.triangles_length() as u64,
            geometry_bytes: self.geometry_byte_length() as u64,
            texture_bytes: self.textures_byte_length() as u64,
            batch_table_bytes: self.batch_table_byte_length() as u64,
        }
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.update_content(frame)?;
        Ok(())
    }
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>> {
        Vector3DTileContent::pick(self, ray, frame)
    }
    fn destroy(&mut self) -> Result<()> {
        self.destroy_content()
    }
}
