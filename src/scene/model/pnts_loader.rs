//! CesiumJS `Scene/Model/PntsLoader`: loads a Point Cloud (`.pnts`) tile,
//! decodes Draco point cloud attributes if present, and builds `ModelComponents`.

use crate::core::math::MersenneTwister;
use crate::core::{
    AttributeCompression, Cartesian3, Cartesian4, Color, ComponentDatatype, Matrix4, PrimitiveType, RuntimeError,
    TypedArray,
};
use crate::renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl};
use crate::scene::draco_loader::{DracoLoader, DracoTask};
use crate::scene::gltf_draco_loader::DracoDecodeWorker;
use crate::scene::metadata_table_property::ByteView;
use crate::scene::model_components::{self as mc, AttributeSemantic};
use crate::scene::parse_batch_table::{parse_batch_table, ParseBatchTableOptions};
use crate::scene::pnts_parser::{self, ParsedPnts, PntsAttribute, PntsDraco};
use crate::scene::resource_loader::{self, ResourceLoadFuture, ResourceLoader};
use crate::scene::structural_metadata::{StructuralMetadataOptions, StructuralMetadataWarning};
use crate::scene::{
    AlphaMode, AttributeType, FrameResource, FrameState, ResourceLoaderState, SceneRenderRequest, StructuralMetadata,
    VertexAttributeSemantic,
};
use futures_util::FutureExt;
use serde_json::{json, Map, Value};
use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::task::{Context as TaskContext, Poll, Waker};

fn typed_array_to_f64_vec(arr: &TypedArray) -> Vec<f64> {
    (0..arr.len()).map(|i| arr.get(i).unwrap_or(0.0)).collect()
}

#[derive(Clone, Default)]
pub struct PntsLoaderOptions {
    pub load_attributes_for_2d: bool,
    pub draco_loader: Option<Rc<DracoLoader>>,
}

fn random_values_20() -> &'static [f64; 20] {
    static RANDOM_VALUES: OnceLock<[f64; 20]> = OnceLock::new();
    RANDOM_VALUES.get_or_init(|| {
        let mut rng = MersenneTwister::new(0);
        std::array::from_fn(|_| rng.random())
    })
}

pub struct PntsLoader<G: Gl + 'static> {
    array_buffer: RefCell<Option<Arc<Vec<u8>>>>,
    byte_offset: usize,
    load_attributes_for_2d: bool,
    draco_loader: Rc<DracoLoader>,
    context: Context<G>,
    parsed_content: RefCell<Option<ParsedPnts>>,
    has_draco_batch_table: Cell<bool>,
    draco_typed_arrays: RefCell<HashMap<String, TypedArray>>,
    decode_started: Cell<bool>,
    decode_ready_without_draco: Cell<bool>,
    decode_task: RefCell<Option<DracoTask>>,
    promise: RefCell<Option<ResourceLoadFuture>>,
    error: RefCell<Option<RuntimeError>>,
    state: Cell<ResourceLoaderState>,
    destroyed: Cell<bool>,
    buffers: RefCell<Vec<Buffer<G>>>,
    components: RefCell<Option<mc::Components<G>>>,
    transform: Cell<Matrix4>,
    metadata_warnings: RefCell<Vec<StructuralMetadataWarning>>,
}

impl<G: Gl + 'static> PntsLoader<G> {
    pub fn new(
        array_buffer: Arc<Vec<u8>>,
        byte_offset: Option<usize>,
        context: Context<G>,
        options: PntsLoaderOptions,
    ) -> Self {
        let draco_loader = options.draco_loader.unwrap_or_else(|| {
            Rc::new(DracoLoader::new(Rc::new(DracoDecodeWorker), DracoLoader::max_decoding_concurrency(4)))
        });
        Self {
            array_buffer: RefCell::new(Some(array_buffer)),
            byte_offset: byte_offset.unwrap_or(0),
            load_attributes_for_2d: options.load_attributes_for_2d,
            draco_loader,
            context,
            parsed_content: RefCell::new(None),
            has_draco_batch_table: Cell::new(false),
            draco_typed_arrays: RefCell::new(HashMap::new()),
            decode_started: Cell::new(false),
            decode_ready_without_draco: Cell::new(false),
            decode_task: RefCell::new(None),
            promise: RefCell::new(None),
            error: RefCell::new(None),
            state: Cell::new(ResourceLoaderState::Unloaded),
            destroyed: Cell::new(false),
            buffers: RefCell::new(Vec::new()),
            components: RefCell::new(None),
            transform: Cell::new(Matrix4::IDENTITY),
            metadata_warnings: RefCell::new(Vec::new()),
        }
    }

    pub fn components(&self) -> Option<Ref<'_, mc::Components<G>>> {
        Ref::filter_map(self.components.borrow(), Option::as_ref).ok()
    }

    pub fn transform(&self) -> Matrix4 {
        self.transform.get()
    }

    pub fn metadata_warnings(&self) -> Vec<StructuralMetadataWarning> {
        self.metadata_warnings.borrow().clone()
    }

    fn create_vertex_buffer(&self, typed_array: &TypedArray) -> Result<Buffer<G>, RuntimeError> {
        let bytes = typed_array.to_le_bytes();
        let buffer =
            Buffer::new(&self.context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
        self.buffers.borrow_mut().push(buffer.clone());
        Ok(buffer)
    }

    fn transcode_attribute_type(components_per_attribute: u64) -> Result<&'static str, RuntimeError> {
        match components_per_attribute {
            1 => Ok("SCALAR"),
            2 => Ok("VEC2"),
            3 => Ok("VEC3"),
            4 => Ok("VEC4"),
            _ => Err(RuntimeError::new("componentsPerAttribute must be a number from 1-4")),
        }
    }

    fn transcode_component_type(value: u32) -> Result<&'static str, RuntimeError> {
        match ComponentDatatype::from_gl(value) {
            Some(ComponentDatatype::Byte) => Ok("BYTE"),
            Some(ComponentDatatype::UnsignedByte) => Ok("UNSIGNED_BYTE"),
            Some(ComponentDatatype::Short) => Ok("SHORT"),
            Some(ComponentDatatype::UnsignedShort) => Ok("UNSIGNED_SHORT"),
            Some(ComponentDatatype::Int) => Ok("INT"),
            Some(ComponentDatatype::UnsignedInt) => Ok("UNSIGNED_INT"),
            Some(ComponentDatatype::Double) => Ok("DOUBLE"),
            Some(ComponentDatatype::Float) => Ok("FLOAT"),
            None => Err(RuntimeError::new("value is not a valid WebGL constant")),
        }
    }

    fn value_to_typed_array(prop: &Value) -> Result<TypedArray, RuntimeError> {
        let gl_type = prop["data"]["componentDatatype"]
            .as_u64()
            .ok_or_else(|| RuntimeError::new("Missing Draco componentDatatype"))? as u32;
        let datatype =
            ComponentDatatype::from_gl(gl_type).ok_or_else(|| RuntimeError::new("Invalid Draco componentDatatype"))?;
        let values: Vec<f64> = prop["array"]
            .as_array()
            .ok_or_else(|| RuntimeError::new("Missing Draco attribute array"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0))
            .collect();
        Ok(TypedArray::from_values(datatype, &values))
    }

    fn process_draco_attributes(&self, draco: &PntsDraco, result: &Value) -> Result<(), RuntimeError> {
        let mut parsed_borrow = self.parsed_content.borrow_mut();
        let Some(parsed_content) = parsed_borrow.as_mut() else {
            return Ok(());
        };

        if let Some(pos) = result.get("POSITION").filter(|v| !v.is_null()) {
            let typed_array = Rc::new(Self::value_to_typed_array(pos)?);
            let mut attribute = PntsAttribute {
                name: "POSITION",
                semantic: VertexAttributeSemantic::Position,
                set_index: None,
                typed_array: Some(typed_array),
                constant_color: None,
                component_datatype: ComponentDatatype::Float,
                attribute_type: AttributeType::Vec3,
                normalized: None,
                is_quantized: Some(false),
                is_rgb565: None,
                is_translucent: None,
                oct_encoded: None,
                oct_encoded_zxy: None,
                quantized_range: None,
                quantized_volume_offset: None,
                quantized_volume_scale: None,
                quantized_component_datatype: None,
                quantized_type: None,
            };
            if let Some(q) = pos.pointer("/data/quantization").filter(|v| !v.is_null()) {
                let range = q["range"].as_f64().unwrap_or(0.0);
                let quantized_volume_scale = Cartesian3::new(range, range, range);
                let min_values = q["minValues"].as_array();
                let quantized_volume_offset = Cartesian3::new(
                    min_values.and_then(|a| a.first()).and_then(Value::as_f64).unwrap_or(0.0),
                    min_values.and_then(|a| a.get(1)).and_then(Value::as_f64).unwrap_or(0.0),
                    min_values.and_then(|a| a.get(2)).and_then(Value::as_f64).unwrap_or(0.0),
                );
                let bits = q["quantizationBits"].as_u64().unwrap_or(0) as u32;
                let quantized_range = (1u32 << bits) - 1;
                attribute.is_quantized = Some(true);
                attribute.quantized_range = Some(quantized_range);
                attribute.quantized_volume_offset = Some(quantized_volume_offset);
                attribute.quantized_volume_scale = Some(quantized_volume_scale);
                attribute.quantized_component_datatype = Some(if quantized_range <= 255 {
                    ComponentDatatype::UnsignedByte
                } else {
                    ComponentDatatype::UnsignedShort
                });
                attribute.quantized_type = Some(AttributeType::Vec3);
            }
            parsed_content.positions = Some(attribute);
        }

        if let Some(norm) = result.get("NORMAL").filter(|v| !v.is_null()) {
            let typed_array = Rc::new(Self::value_to_typed_array(norm)?);
            let mut attribute = PntsAttribute {
                name: "NORMAL",
                semantic: VertexAttributeSemantic::Normal,
                set_index: None,
                typed_array: Some(typed_array),
                constant_color: None,
                component_datatype: ComponentDatatype::Float,
                attribute_type: AttributeType::Vec3,
                normalized: None,
                is_quantized: Some(false),
                is_rgb565: None,
                is_translucent: None,
                oct_encoded: Some(false),
                oct_encoded_zxy: Some(false),
                quantized_range: None,
                quantized_volume_offset: None,
                quantized_volume_scale: None,
                quantized_component_datatype: None,
                quantized_type: None,
            };
            if let Some(q) = norm.pointer("/data/quantization").filter(|v| !v.is_null()) {
                let bits = q["quantizationBits"].as_u64().unwrap_or(0) as u32;
                let oct_encoded_range = (1u32 << bits) - 1;
                attribute.quantized_range = Some(oct_encoded_range);
                attribute.oct_encoded = Some(true);
                attribute.oct_encoded_zxy = Some(true);
                attribute.quantized_component_datatype = Some(ComponentDatatype::UnsignedByte);
                attribute.quantized_type = Some(AttributeType::Vec2);
            }
            parsed_content.normals = Some(attribute);
        }

        if let Some(rgba) = result.get("RGBA").filter(|v| !v.is_null()) {
            let typed_array = Rc::new(Self::value_to_typed_array(rgba)?);
            parsed_content.colors = Some(PntsAttribute {
                name: "COLOR",
                semantic: VertexAttributeSemantic::Color,
                set_index: Some(0),
                typed_array: Some(typed_array),
                constant_color: None,
                component_datatype: ComponentDatatype::UnsignedByte,
                attribute_type: AttributeType::Vec4,
                normalized: Some(true),
                is_quantized: None,
                is_rgb565: None,
                is_translucent: Some(true),
                oct_encoded: None,
                oct_encoded_zxy: None,
                quantized_range: None,
                quantized_volume_offset: None,
                quantized_volume_scale: None,
                quantized_component_datatype: None,
                quantized_type: None,
            });
        } else if let Some(rgb) = result.get("RGB").filter(|v| !v.is_null()) {
            let typed_array = Rc::new(Self::value_to_typed_array(rgb)?);
            parsed_content.colors = Some(PntsAttribute {
                name: "COLOR",
                semantic: VertexAttributeSemantic::Color,
                set_index: Some(0),
                typed_array: Some(typed_array),
                constant_color: None,
                component_datatype: ComponentDatatype::UnsignedByte,
                attribute_type: AttributeType::Vec3,
                normalized: Some(true),
                is_quantized: None,
                is_rgb565: None,
                is_translucent: Some(false),
                oct_encoded: None,
                oct_encoded_zxy: None,
                quantized_range: None,
                quantized_volume_offset: None,
                quantized_volume_scale: None,
                quantized_component_datatype: None,
                quantized_type: None,
            });
        }

        if let Some(batch_id) = result.get("BATCH_ID").filter(|v| !v.is_null()) {
            let typed_array = Self::value_to_typed_array(batch_id)?;
            let component_datatype = typed_array.component_datatype();
            parsed_content.batch_ids = Some(PntsAttribute {
                name: "_FEATURE_ID",
                semantic: VertexAttributeSemantic::FeatureId,
                set_index: Some(0),
                typed_array: Some(Rc::new(typed_array)),
                constant_color: None,
                component_datatype,
                attribute_type: AttributeType::Scalar,
                normalized: None,
                is_quantized: None,
                is_rgb565: None,
                is_translucent: None,
                oct_encoded: None,
                oct_encoded_zxy: None,
                quantized_range: None,
                quantized_volume_offset: None,
                quantized_volume_scale: None,
                quantized_component_datatype: None,
                quantized_type: None,
            });
        }

        if let Some(batch_props) = draco.batch_table_properties.as_ref().and_then(Value::as_object) {
            let mut batch_table_map = parsed_content
                .batch_table_json
                .take()
                .and_then(|v| if let Value::Object(m) = v { Some(m) } else { None })
                .unwrap_or_default();
            let mut typed_arrays = self.draco_typed_arrays.borrow_mut();
            for name in batch_props.keys() {
                if let Some(property) = result.get(name).filter(|v| !v.is_null()) {
                    self.has_draco_batch_table.set(true);
                    let data = &property["data"];
                    let attr_type =
                        Self::transcode_attribute_type(data["componentsPerAttribute"].as_u64().unwrap_or(0))?;
                    let comp_type =
                        Self::transcode_component_type(data["componentDatatype"].as_u64().unwrap_or(0) as u32)?;
                    let array = Self::value_to_typed_array(property)?;
                    typed_arrays.insert(name.clone(), array);
                    batch_table_map.insert(
                        name.clone(),
                        json!({
                            "byteOffset": data["byteOffset"],
                            "type": attr_type,
                            "componentType": comp_type,
                        }),
                    );
                }
            }
            if !batch_table_map.is_empty() {
                parsed_content.batch_table_json = Some(Value::Object(batch_table_map));
            }
        }

        Ok(())
    }

    fn compute_approximate_extrema(positions: &PntsAttribute) -> (Cartesian3, Cartesian3) {
        if positions.is_quantized == Some(true) {
            return (Cartesian3::ZERO, positions.quantized_volume_scale.unwrap_or(Cartesian3::ZERO));
        }
        let Some(positions_array) = &positions.typed_array else {
            return (Cartesian3::ZERO, Cartesian3::ZERO);
        };
        let values = typed_array_to_f64_vec(positions_array);
        let maximum_samples_length = 20usize;
        let points_length = values.len() / 3;
        let samples_length = points_length.min(maximum_samples_length);
        let random_values = random_values_20();
        let mut min = Cartesian3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut max = Cartesian3::new(-f64::MAX, -f64::MAX, -f64::MAX);
        for &rv in random_values.iter().take(samples_length) {
            let index = (rv * (points_length as f64)).floor() as usize;
            let base = index * 3;
            let position = Cartesian3::new(
                values.get(base).copied().unwrap_or(0.0),
                values.get(base + 1).copied().unwrap_or(0.0),
                values.get(base + 2).copied().unwrap_or(0.0),
            );
            min = min.minimum_by_component(position);
            max = max.maximum_by_component(position);
        }
        (min, max)
    }

    fn make_attribute(
        &self,
        attribute_info: &PntsAttribute,
        min_max: Option<(Cartesian3, Cartesian3)>,
    ) -> Result<mc::Attribute<G>, RuntimeError> {
        let mut quantization = None;
        if attribute_info.oct_encoded == Some(true) {
            quantization = Some(mc::Quantization {
                oct_encoded: true,
                oct_encoded_zxy: attribute_info.oct_encoded_zxy.unwrap_or(false),
                normalization_range: attribute_info.quantized_range.map(|r| mc::MathValue::Number(r as f64)),
                attribute_type: attribute_info.quantized_type,
                component_datatype: attribute_info.quantized_component_datatype,
                ..Default::default()
            });
        }
        if attribute_info.is_quantized == Some(true) {
            let normalization_range = attribute_info.quantized_range.unwrap_or(0) as f64;
            let dimensions = attribute_info.quantized_volume_scale.unwrap_or(Cartesian3::ZERO);
            let step_size = dimensions.divide_by_scalar(normalization_range);
            quantization = Some(mc::Quantization {
                oct_encoded: false,
                oct_encoded_zxy: false,
                normalization_range: Some(mc::MathValue::Number(normalization_range)),
                quantized_volume_offset: Some(mc::MathValue::Cartesian3(Cartesian3::ZERO)),
                quantized_volume_dimensions: Some(mc::MathValue::Cartesian3(dimensions)),
                quantized_volume_step_size: Some(mc::MathValue::Cartesian3(step_size)),
                component_datatype: attribute_info.quantized_component_datatype,
                attribute_type: attribute_info.quantized_type,
            });
        }

        let mut typed_array = attribute_info.typed_array.as_deref().cloned();
        if attribute_info.is_rgb565 == Some(true) {
            if let Some(arr) = &typed_array {
                let u16_vals: Vec<u16> = typed_array_to_f64_vec(arr).into_iter().map(|v| v as u16).collect();
                let decoded = AttributeCompression::decode_rgb565(&u16_vals);
                typed_array = Some(TypedArray::Float32(decoded));
            }
        }

        let (constant, buffer) = if let Some(color) = attribute_info.constant_color {
            (Some(mc::MathValue::Cartesian4(Cartesian4::new(color.red, color.green, color.blue, color.alpha))), None)
        } else if let Some(arr) = &typed_array {
            let buf = self.create_vertex_buffer(arr)?;
            (None, Some(buf))
        } else {
            (None, None)
        };

        let retained_typed_array =
            if attribute_info.semantic == VertexAttributeSemantic::Position && self.load_attributes_for_2d {
                typed_array
            } else {
                None
            };

        Ok(mc::Attribute {
            name: Some(attribute_info.name.into()),
            semantic: Some(AttributeSemantic::Vertex(attribute_info.semantic)),
            set_index: attribute_info.set_index,
            component_datatype: Some(attribute_info.component_datatype),
            attribute_type: Some(attribute_info.attribute_type),
            normalized: attribute_info.normalized.unwrap_or(false),
            count: None,
            min: min_max.map(|(min, _)| mc::MathValue::Cartesian3(min)),
            max: min_max.map(|(_, max)| mc::MathValue::Cartesian3(max)),
            constant,
            quantization,
            typed_array: retained_typed_array,
            buffer,
            ..Default::default()
        })
    }

    fn make_components(&self) -> Result<(), RuntimeError> {
        let Some(parsed_content) = self.parsed_content.borrow_mut().take() else {
            return Ok(());
        };
        let points_length = parsed_content.points_length.to_number() as usize;
        let batch_length = parsed_content.batch_length.as_ref().map(|p| p.to_number() as usize);

        let mut attributes = Vec::new();
        if let Some(positions) = &parsed_content.positions {
            let min_max = Self::compute_approximate_extrema(positions);
            let mut attr = self.make_attribute(positions, Some(min_max))?;
            attr.count = Some(points_length);
            attributes.push(attr);
        }
        if let Some(normals) = &parsed_content.normals {
            attributes.push(self.make_attribute(normals, None)?);
        }
        if let Some(colors) = &parsed_content.colors {
            attributes.push(self.make_attribute(colors, None)?);
        } else {
            let default_color = PntsAttribute {
                name: "COLOR",
                semantic: VertexAttributeSemantic::Color,
                set_index: Some(0),
                typed_array: None,
                constant_color: Some(Color::DARKGRAY),
                component_datatype: ComponentDatatype::Float,
                attribute_type: AttributeType::Vec4,
                normalized: Some(false),
                is_quantized: Some(false),
                is_rgb565: None,
                is_translucent: Some(false),
                oct_encoded: None,
                oct_encoded_zxy: None,
                quantized_range: None,
                quantized_volume_offset: None,
                quantized_volume_scale: None,
                quantized_component_datatype: None,
                quantized_type: None,
            };
            attributes.push(self.make_attribute(&default_color, None)?);
        }
        if let Some(batch_ids) = &parsed_content.batch_ids {
            attributes.push(self.make_attribute(batch_ids, None)?);
        }

        let mut material = mc::Material {
            metallic_roughness: mc::MetallicRoughness {
                metallic_factor: 0.0,
                roughness_factor: 0.9,
                ..Default::default()
            },
            unlit: parsed_content.normals.is_none(),
            ..Default::default()
        };
        if parsed_content.colors.as_ref().and_then(|c| c.is_translucent).unwrap_or(false) {
            material.alpha_mode = AlphaMode::Blend;
        }

        let mut primitive = mc::Primitive {
            attributes,
            primitive_type: Some(PrimitiveType::Points),
            material: Some(material),
            ..Default::default()
        };

        if parsed_content.batch_ids.is_some() {
            primitive.feature_ids.push(mc::FeatureId::Attribute(mc::FeatureIdAttribute {
                feature_count: None,
                null_feature_id: None,
                property_table_id: Some(0),
                set_index: Some(0),
                label: None,
                positional_label: Some("featureId_0".into()),
            }));
        }

        let parse_as_property_attributes = parsed_content.batch_ids.is_none();
        let mut custom_attribute_output = Vec::new();
        let structural_metadata = if parsed_content.batch_table_binary.is_some()
            || parsed_content.batch_table_json.is_some()
            || self.has_draco_batch_table.get()
        {
            let count = batch_length.unwrap_or(points_length);
            let empty_json = Value::Object(Map::new());
            let batch_table_json = parsed_content.batch_table_json.as_ref().unwrap_or(&empty_json);
            let binary_body =
                parsed_content.batch_table_binary.as_deref().map(|buffer| ByteView { buffer, byte_offset: 0 });
            let draco_arrays = self.draco_typed_arrays.borrow();
            let typed_arrays = if draco_arrays.is_empty() { None } else { Some(&*draco_arrays) };
            let mut warnings = Vec::new();
            let metadata = parse_batch_table(
                &ParseBatchTableOptions {
                    count,
                    batch_table: batch_table_json,
                    binary_body,
                    parse_as_property_attributes,
                    typed_arrays,
                },
                Some(&mut custom_attribute_output),
                &mut warnings,
            )
            .map_err(|e| RuntimeError::new(e.to_string()))?;
            *self.metadata_warnings.borrow_mut() = warnings;
            metadata
        } else {
            StructuralMetadata::new(StructuralMetadataOptions {
                property_tables: Some(Vec::new()),
                ..Default::default()
            })
        };

        if !custom_attribute_output.is_empty() {
            for mut custom_attribute in custom_attribute_output {
                if let Some(arr) = custom_attribute.typed_array.take() {
                    let buffer = self.create_vertex_buffer(&arr)?;
                    custom_attribute.buffer = Some(buffer);
                }
                primitive.attributes.push(custom_attribute);
            }
            primitive.property_attribute_ids = vec![0];
        }

        let node = mc::Node { index: Some(0), primitives: vec![primitive], ..Default::default() };

        let mut transform = Matrix4::IDENTITY;
        if let Some(rtc_center) = parsed_content.rtc_center {
            transform = transform.multiply_by_translation(rtc_center);
        }
        if let Some(positions) = &parsed_content.positions {
            if positions.is_quantized == Some(true) {
                if let Some(offset) = positions.quantized_volume_offset {
                    transform = transform.multiply_by_translation(offset);
                }
            }
        }
        self.transform.set(transform);

        let components = mc::Components {
            scene: Some(mc::Scene { nodes: vec![0] }),
            nodes: vec![node],
            transform,
            structural_metadata: Some(FrameResource::new(Rc::new(structural_metadata))),
            ..Default::default()
        };

        *self.components.borrow_mut() = Some(components);
        self.array_buffer.borrow_mut().take();
        Ok(())
    }
}

impl<G: Gl + 'static> ResourceLoader for PntsLoader<G> {
    fn cache_key(&self) -> Option<&str> {
        None
    }

    fn load(&self) -> ResourceLoadFuture {
        if self.is_destroyed() {
            return async { Ok(false) }.boxed_local().shared();
        }
        if let Some(promise) = self.promise.borrow().as_ref() {
            return promise.clone();
        }
        let result = (|| -> Result<ParsedPnts, RuntimeError> {
            let array_buffer =
                self.array_buffer.borrow().clone().ok_or_else(|| RuntimeError::new("pnts arrayBuffer was unloaded"))?;
            pnts_parser::parse(&array_buffer, self.byte_offset)
        })();
        let future = match result {
            Ok(parsed) => {
                *self.parsed_content.borrow_mut() = Some(parsed);
                self.state.set(ResourceLoaderState::Processing);
                async { Ok(true) }.boxed_local().shared()
            }
            Err(e) => {
                self.unload();
                self.state.set(ResourceLoaderState::Failed);
                async move { Err(e) }.boxed_local().shared()
            }
        };
        *self.promise.borrow_mut() = Some(future.clone());
        future
    }

    fn state(&self) -> ResourceLoaderState {
        self.state.get()
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }

    fn unload(&self) {
        for buffer in self.buffers.borrow_mut().drain(..) {
            if !buffer.is_destroyed() {
                let _ = buffer.destroy();
            }
        }
        self.components.borrow_mut().take();
        self.parsed_content.borrow_mut().take();
        self.array_buffer.borrow_mut().take();
        self.decode_task.borrow_mut().take();
        self.draco_typed_arrays.borrow_mut().clear();
        self.decode_started.set(false);
        self.decode_ready_without_draco.set(false);
        self.promise.borrow_mut().take();
        self.state.set(ResourceLoaderState::Unloaded);
    }

    fn destroy(&self) {
        self.unload();
        self.destroyed.set(true);
    }

    fn process(&self, frame: &FrameState) -> Result<bool, RuntimeError> {
        if let Some(error) = self.error.borrow_mut().take() {
            return Err(error);
        }
        if self.state.get() == ResourceLoaderState::Ready {
            return Ok(true);
        }
        if self.state.get() != ResourceLoaderState::Processing {
            return Ok(false);
        }

        if self.decode_started.get() {
            if self.decode_ready_without_draco.get() {
                if let Err(e) = self.make_components() {
                    self.unload();
                    self.state.set(ResourceLoaderState::Failed);
                    return Err(resource_loader::loader_error("Failed to load Draco pnts", Some(&e)));
                }
                self.state.set(ResourceLoaderState::Ready);
                return Ok(true);
            }
            let pending = self.decode_task.borrow_mut().take();
            if let Some(mut task) = pending {
                let waker = frame.render_request.as_ref().map(SceneRenderRequest::waker);
                let mut cx = TaskContext::from_waker(waker.as_ref().unwrap_or(Waker::noop()));
                match Pin::new(&mut task).poll(&mut cx) {
                    Poll::Pending => {
                        *self.decode_task.borrow_mut() = Some(task);
                        return Ok(false);
                    }
                    Poll::Ready(Ok(decoded)) => {
                        let draco = self.parsed_content.borrow().as_ref().and_then(|p| p.draco.clone());
                        let res = (|| -> Result<(), RuntimeError> {
                            if let Some(draco) = &draco {
                                self.process_draco_attributes(draco, &decoded)?;
                            }
                            self.make_components()
                        })();
                        match res {
                            Ok(()) => {
                                self.state.set(ResourceLoaderState::Ready);
                                return Ok(true);
                            }
                            Err(e) => {
                                self.unload();
                                self.state.set(ResourceLoaderState::Failed);
                                return Err(resource_loader::loader_error("Failed to load Draco pnts", Some(&e)));
                            }
                        }
                    }
                    Poll::Ready(Err(e)) => {
                        self.unload();
                        self.state.set(ResourceLoaderState::Failed);
                        return Err(resource_loader::loader_error("Failed to load Draco pnts", Some(&e)));
                    }
                }
            }
            return Ok(false);
        }

        let draco = self.parsed_content.borrow().as_ref().and_then(|p| p.draco.clone());
        match draco {
            None => {
                self.decode_started.set(true);
                self.decode_ready_without_draco.set(true);
                Ok(false)
            }
            Some(draco) => {
                let parameters = json!({
                    "buffer": draco.buffer,
                    "properties": draco.properties,
                    "featureTableProperties": draco.feature_table_properties,
                    "batchTableProperties": draco.batch_table_properties,
                    "dequantizeInShader": draco.dequantize_in_shader,
                });
                match self.draco_loader.decode_point_cloud(parameters) {
                    Ok(Some(task)) => {
                        self.decode_started.set(true);
                        *self.decode_task.borrow_mut() = Some(task);
                        Ok(false)
                    }
                    Ok(None) => Ok(false),
                    Err(e) => {
                        self.unload();
                        self.state.set(ResourceLoaderState::Failed);
                        *self.error.borrow_mut() =
                            Some(resource_loader::loader_error("Failed to load Draco pnts", Some(&e)));
                        Ok(false)
                    }
                }
            }
        }
    }
}
