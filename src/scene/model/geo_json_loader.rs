//! CesiumJS `Scene/Model/GeoJsonLoader`: loads a GeoJSON `FeatureCollection` or
//! `Feature` (`MAXAR_content_geojson`) into `ModelComponents` with `LINES` and
//! `POINTS` primitives and a `JsonMetadataTable` `PropertyTable`.

use crate::core::math::SIXTY_FOUR_KILOBYTES;
use crate::core::{
    Cartesian3, ComponentDatatype, Ellipsoid, IndexDatatype, Matrix4, PrimitiveType, RuntimeError, Transforms,
    TypedArray,
};
use crate::renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl};
use crate::scene::model_components::{self as mc, AttributeSemantic};
use crate::scene::resource_loader::{ResourceLoadFuture, ResourceLoader};
use crate::scene::structural_metadata::StructuralMetadataOptions;
use crate::scene::{
    AttributeType, FrameResource, FrameState, JsonMetadataTable, MetadataSchema, PropertyTable, PropertyTableOptions,
    ResourceLoaderState, StructuralMetadata, VertexAttributeSemantic,
};
use futures_util::FutureExt;
use serde_json::{Map, Value};
use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;
use std::sync::Arc;

struct ParsedFeature {
    lines: Option<Vec<Vec<Cartesian3>>>,
    points: Option<Vec<Cartesian3>>,
    properties: Option<Value>,
}

pub struct GeoJsonLoader<G: Gl + 'static> {
    geo_json: RefCell<Option<Value>>,
    context: Context<G>,
    components: RefCell<Option<mc::Components<G>>>,
    state: Cell<ResourceLoaderState>,
    destroyed: Cell<bool>,
}

impl<G: Gl + 'static> GeoJsonLoader<G> {
    pub fn new(geo_json: Value, context: Context<G>) -> Self {
        Self {
            geo_json: RefCell::new(Some(geo_json)),
            context,
            components: RefCell::new(None),
            state: Cell::new(ResourceLoaderState::Unloaded),
            destroyed: Cell::new(false),
        }
    }

    pub fn components(&self) -> Option<Ref<'_, mc::Components<G>>> {
        Ref::filter_map(self.components.borrow(), Option::as_ref).ok()
    }

    fn parse_position(position: &Value) -> Cartesian3 {
        let arr = position.as_array();
        let x = arr.and_then(|a| a.first()).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let y = arr.and_then(|a| a.get(1)).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let z = arr.and_then(|a| a.get(2)).and_then(Value::as_f64).unwrap_or(0.0);
        Cartesian3::new(x, y, z)
    }

    fn parse_line_string(coordinates: &Value) -> Vec<Vec<Cartesian3>> {
        let Some(coords) = coordinates.as_array() else {
            return vec![Vec::new()];
        };
        let line = coords.iter().map(Self::parse_position).collect();
        vec![line]
    }

    fn parse_multi_line_string(coordinates: &Value) -> Vec<Vec<Cartesian3>> {
        let Some(lines_arr) = coordinates.as_array() else {
            return Vec::new();
        };
        lines_arr.iter().map(|c| Self::parse_line_string(c).into_iter().next().unwrap_or_default()).collect()
    }

    fn parse_polygon(coordinates: &Value) -> Vec<Vec<Cartesian3>> {
        Self::parse_multi_line_string(coordinates)
    }

    fn parse_multi_polygon(coordinates: &Value) -> Vec<Vec<Cartesian3>> {
        let Some(polygons) = coordinates.as_array() else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for poly in polygons {
            lines.extend(Self::parse_polygon(poly));
        }
        lines
    }

    fn parse_point(coordinates: &Value) -> Vec<Cartesian3> {
        vec![Self::parse_position(coordinates)]
    }

    fn parse_multi_point(coordinates: &Value) -> Vec<Cartesian3> {
        let Some(points_arr) = coordinates.as_array() else {
            return Vec::new();
        };
        points_arr.iter().map(Self::parse_position).collect()
    }

    fn parse_feature(feature: &Value, features: &mut Vec<ParsedFeature>) {
        let Some(geometry) = feature.get("geometry").filter(|v| !v.is_null()) else {
            return;
        };
        let Some(geometry_type) = geometry.get("type").and_then(Value::as_str) else {
            return;
        };
        let Some(coordinates) = geometry.get("coordinates").filter(|v| !v.is_null()) else {
            return;
        };
        let (lines, points) = match geometry_type {
            "LineString" => (Some(Self::parse_line_string(coordinates)), None),
            "MultiLineString" => (Some(Self::parse_multi_line_string(coordinates)), None),
            "Polygon" => (Some(Self::parse_polygon(coordinates)), None),
            "MultiPolygon" => (Some(Self::parse_multi_polygon(coordinates)), None),
            "Point" => (None, Some(Self::parse_point(coordinates))),
            "MultiPoint" => (None, Some(Self::parse_multi_point(coordinates))),
            _ => return,
        };
        let properties = feature.get("properties").filter(|v| !v.is_null()).cloned();
        features.push(ParsedFeature { lines, points, properties });
    }

    fn parse_feature_collection(feature_collection: &Value, features: &mut Vec<ParsedFeature>) {
        if let Some(arr) = feature_collection.get("features").and_then(Value::as_array) {
            for feature in arr {
                Self::parse_feature(feature, features);
            }
        }
    }

    fn create_vertex_buffer(&self, typed_array: &TypedArray) -> Result<Buffer<G>, RuntimeError> {
        let bytes = typed_array.to_le_bytes();
        Buffer::new(&self.context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))
    }

    fn create_lines_primitive(
        &self,
        features: &[ParsedFeature],
        to_local: &Matrix4,
    ) -> Result<mc::Primitive<G>, RuntimeError> {
        let mut vertex_count = 0usize;
        let mut index_count = 0usize;
        let feature_count = features.len();

        for feature in features {
            if let Some(lines) = &feature.lines {
                for line in lines {
                    vertex_count += line.len();
                    index_count += line.len().saturating_sub(1) * 2;
                }
            }
        }

        let mut positions_f32 = vec![0.0f32; vertex_count * 3];
        let mut feature_ids_f32 = vec![0.0f32; vertex_count];
        let use_u32 = vertex_count >= SIXTY_FOUR_KILOBYTES as usize;
        let index_datatype = if use_u32 { IndexDatatype::UnsignedInt } else { IndexDatatype::UnsignedShort };
        let mut indices_u32 = vec![0u32; index_count];

        let mut local_min = Cartesian3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut local_max = Cartesian3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);

        let mut vertex_counter = 0usize;
        let mut segment_counter = 0usize;

        for (i, feature) in features.iter().enumerate() {
            let Some(lines) = &feature.lines else {
                continue;
            };
            for line in lines {
                let positions_length = line.len();
                for (k, cartographic) in line.iter().enumerate() {
                    let global_cartesian =
                        Cartesian3::from_degrees(cartographic.x, cartographic.y, cartographic.z, &Ellipsoid::WGS84);
                    let local_cartesian = to_local.multiply_by_point(global_cartesian);
                    local_min = local_min.minimum_by_component(local_cartesian);
                    local_max = local_max.maximum_by_component(local_cartesian);

                    positions_f32[vertex_counter * 3] = local_cartesian.x as f32;
                    positions_f32[vertex_counter * 3 + 1] = local_cartesian.y as f32;
                    positions_f32[vertex_counter * 3 + 2] = local_cartesian.z as f32;
                    feature_ids_f32[vertex_counter] = i as f32;

                    if k + 1 < positions_length {
                        indices_u32[segment_counter * 2] = vertex_counter as u32;
                        indices_u32[segment_counter * 2 + 1] = (vertex_counter + 1) as u32;
                        segment_counter += 1;
                    }
                    vertex_counter += 1;
                }
            }
        }

        let position_buffer = self.create_vertex_buffer(&TypedArray::Float32(positions_f32))?;
        let feature_id_buffer = self.create_vertex_buffer(&TypedArray::Float32(feature_ids_f32))?;
        let indices_typed_array = if use_u32 {
            TypedArray::Uint32(indices_u32)
        } else {
            TypedArray::Uint16(indices_u32.into_iter().map(|v| v as u16).collect())
        };
        let index_bytes = indices_typed_array.to_le_bytes();
        let index_buffer = Buffer::new(
            &self.context,
            BufferKind::Index(index_datatype),
            BufferUsage::StaticDraw,
            BufferData::Bytes(&index_bytes),
        )?;

        let position_attribute = mc::Attribute {
            semantic: Some(AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
            component_datatype: Some(ComponentDatatype::Float),
            attribute_type: Some(AttributeType::Vec3),
            count: Some(vertex_count),
            min: Some(mc::MathValue::Cartesian3(local_min)),
            max: Some(mc::MathValue::Cartesian3(local_max)),
            buffer: Some(position_buffer),
            ..Default::default()
        };
        let feature_id_attribute = mc::Attribute {
            semantic: Some(AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId)),
            set_index: Some(0),
            component_datatype: Some(ComponentDatatype::Float),
            attribute_type: Some(AttributeType::Scalar),
            count: Some(vertex_count),
            buffer: Some(feature_id_buffer),
            ..Default::default()
        };

        let material = mc::Material { unlit: true, ..Default::default() };
        let indices = mc::Indices {
            index_datatype: Some(index_datatype),
            count: Some(index_count),
            buffer: Some(index_buffer),
            typed_array: None,
        };
        let feature_id = mc::FeatureId::Attribute(mc::FeatureIdAttribute {
            feature_count: Some(feature_count as u32),
            null_feature_id: None,
            property_table_id: Some(0),
            set_index: Some(0),
            label: None,
            positional_label: Some("featureId_0".into()),
        });

        Ok(mc::Primitive {
            attributes: vec![position_attribute, feature_id_attribute],
            indices: Some(indices),
            feature_ids: vec![feature_id],
            primitive_type: Some(PrimitiveType::Lines),
            material: Some(material),
            ..Default::default()
        })
    }

    fn create_points_primitive(
        &self,
        features: &[ParsedFeature],
        to_local: &Matrix4,
    ) -> Result<mc::Primitive<G>, RuntimeError> {
        let mut vertex_count = 0usize;
        let feature_count = features.len();
        for feature in features {
            if let Some(points) = &feature.points {
                vertex_count += points.len();
            }
        }

        let mut positions_f32 = vec![0.0f32; vertex_count * 3];
        let mut feature_ids_f32 = vec![0.0f32; vertex_count];
        let mut local_min = Cartesian3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut local_max = Cartesian3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut vertex_counter = 0usize;

        for (i, feature) in features.iter().enumerate() {
            let Some(points) = &feature.points else {
                continue;
            };
            for cartographic in points {
                let global_cartesian =
                    Cartesian3::from_degrees(cartographic.x, cartographic.y, cartographic.z, &Ellipsoid::WGS84);
                let local_cartesian = to_local.multiply_by_point(global_cartesian);
                local_min = local_min.minimum_by_component(local_cartesian);
                local_max = local_max.maximum_by_component(local_cartesian);

                positions_f32[vertex_counter * 3] = local_cartesian.x as f32;
                positions_f32[vertex_counter * 3 + 1] = local_cartesian.y as f32;
                positions_f32[vertex_counter * 3 + 2] = local_cartesian.z as f32;
                feature_ids_f32[vertex_counter] = i as f32;
                vertex_counter += 1;
            }
        }

        let position_buffer = self.create_vertex_buffer(&TypedArray::Float32(positions_f32))?;
        let feature_id_buffer = self.create_vertex_buffer(&TypedArray::Float32(feature_ids_f32))?;

        let position_attribute = mc::Attribute {
            semantic: Some(AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
            component_datatype: Some(ComponentDatatype::Float),
            attribute_type: Some(AttributeType::Vec3),
            count: Some(vertex_count),
            min: Some(mc::MathValue::Cartesian3(local_min)),
            max: Some(mc::MathValue::Cartesian3(local_max)),
            buffer: Some(position_buffer),
            ..Default::default()
        };
        let feature_id_attribute = mc::Attribute {
            semantic: Some(AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId)),
            set_index: Some(0),
            component_datatype: Some(ComponentDatatype::Float),
            attribute_type: Some(AttributeType::Scalar),
            count: Some(vertex_count),
            buffer: Some(feature_id_buffer),
            ..Default::default()
        };

        let material = mc::Material { unlit: true, ..Default::default() };
        let feature_id = mc::FeatureId::Attribute(mc::FeatureIdAttribute {
            feature_count: Some(feature_count as u32),
            null_feature_id: None,
            property_table_id: Some(0),
            set_index: Some(0),
            label: None,
            positional_label: Some("featureId_0".into()),
        });

        Ok(mc::Primitive {
            attributes: vec![position_attribute, feature_id_attribute],
            feature_ids: vec![feature_id],
            primitive_type: Some(PrimitiveType::Points),
            material: Some(material),
            ..Default::default()
        })
    }

    fn parse_geo_json(&self, geo_json: &Value) -> Result<mc::Components<G>, RuntimeError> {
        let mut features = Vec::new();
        match geo_json.get("type").and_then(Value::as_str) {
            Some("FeatureCollection") => Self::parse_feature_collection(geo_json, &mut features),
            Some("Feature") => Self::parse_feature(geo_json, &mut features),
            _ => {}
        }

        let feature_count = features.len();
        if feature_count == 0 {
            return Err(RuntimeError::new("GeoJSON must have at least one feature"));
        }

        let mut property_keys = Vec::new();
        for feature in &features {
            if let Some(Value::Object(map)) = &feature.properties {
                for key in map.keys() {
                    if !property_keys.iter().any(|k| k == key) {
                        property_keys.push(key.clone());
                    }
                }
            }
        }

        let mut properties_map = Map::new();
        for key in &property_keys {
            let mut arr = Vec::with_capacity(feature_count);
            for feature in &features {
                let Some(Value::Object(map)) = &feature.properties else {
                    return Err(RuntimeError::new(format!("Cannot read properties of undefined (reading '{key}')")));
                };
                let val = match map.get(key) {
                    Some(v) if !v.is_null() => v.clone(),
                    _ => Value::String(String::new()),
                };
                arr.push(val);
            }
            properties_map.insert(key.clone(), Value::Array(arr));
        }

        let json_metadata_table = JsonMetadataTable::new(feature_count, &Value::Object(properties_map))
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let property_table = PropertyTable::new(PropertyTableOptions {
            id: Some(0.into()),
            count: feature_count,
            json_metadata_table: Some(json_metadata_table),
            ..Default::default()
        });
        let schema =
            MetadataSchema::from_json(&Value::Object(Map::new())).map_err(|e| RuntimeError::new(e.to_string()))?;
        let structural_metadata: StructuralMetadata<G> = StructuralMetadata::new(StructuralMetadataOptions {
            schema: Arc::new(schema),
            property_tables: Some(vec![property_table]),
            ..Default::default()
        });

        let mut cartographic_min = Cartesian3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut cartographic_max = Cartesian3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut has_lines = false;
        let mut has_points = false;

        for feature in &features {
            if let Some(lines) = &feature.lines {
                has_lines = true;
                for line in lines {
                    for pt in line {
                        cartographic_min = cartographic_min.minimum_by_component(*pt);
                        cartographic_max = cartographic_max.maximum_by_component(*pt);
                    }
                }
            }
            if let Some(points) = &feature.points {
                has_points = true;
                for pt in points {
                    cartographic_min = cartographic_min.minimum_by_component(*pt);
                    cartographic_max = cartographic_max.maximum_by_component(*pt);
                }
            }
        }

        let cartographic_center = cartographic_min.midpoint(cartographic_max);
        let ecef_center = Cartesian3::from_degrees(
            cartographic_center.x,
            cartographic_center.y,
            cartographic_center.z,
            &Ellipsoid::WGS84,
        );
        let to_global = Transforms::east_north_up_to_fixed_frame(ecef_center, &Ellipsoid::WGS84);
        let to_local = to_global.inverse_transformation();

        let mut primitives = Vec::new();
        if has_lines {
            primitives.push(self.create_lines_primitive(&features, &to_local)?);
        }
        if has_points {
            primitives.push(self.create_points_primitive(&features, &to_local)?);
        }

        let node = mc::Node { index: Some(0), primitives, ..Default::default() };

        Ok(mc::Components {
            scene: Some(mc::Scene { nodes: vec![0] }),
            nodes: vec![node],
            transform: to_global,
            structural_metadata: Some(FrameResource::new(Rc::new(structural_metadata))),
            ..Default::default()
        })
    }
}

impl<G: Gl + 'static> ResourceLoader for GeoJsonLoader<G> {
    fn cache_key(&self) -> Option<&str> {
        None
    }

    fn load(&self) -> ResourceLoadFuture {
        async { Ok(true) }.boxed_local().shared()
    }

    fn state(&self) -> ResourceLoaderState {
        self.state.get()
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }

    fn unload(&self) {
        self.geo_json.borrow_mut().take();
        self.components.borrow_mut().take();
        self.state.set(ResourceLoaderState::Unloaded);
    }

    fn destroy(&self) {
        self.unload();
        self.destroyed.set(true);
    }

    fn process(&self, _frame: &FrameState) -> Result<bool, RuntimeError> {
        if self.is_destroyed() || self.state.get() == ResourceLoaderState::Failed {
            return Ok(false);
        }
        if self.components.borrow().is_some() {
            return Ok(true);
        }
        let Some(geo_json) = self.geo_json.borrow_mut().take() else {
            return Ok(false);
        };
        let components = match self.parse_geo_json(&geo_json) {
            Ok(components) => components,
            Err(error) => {
                self.state.set(ResourceLoaderState::Failed);
                return Err(error);
            }
        };
        *self.components.borrow_mut() = Some(components);
        self.state.set(ResourceLoaderState::Ready);
        Ok(true)
    }
}
