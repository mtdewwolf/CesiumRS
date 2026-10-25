//! CesiumJS `Scene/Model/I3dmLoader`: loads an Instanced 3D Model (`.i3dm`) tile
//! and populates `ModelComponents::Instances` and `StructuralMetadata`.

use crate::core::get_string_from_typed_array::get_string_from_typed_array;
use crate::core::resource::DerivedResourceOptions;
use crate::core::{
    AttributeCompression, BoundingSphere, Cartesian3, ComponentDatatype, Ellipsoid, Matrix3, Matrix4, Quaternion,
    Resource, RuntimeError, Transforms, TypedArray,
};
use crate::renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl};
use crate::scene::b3dm_parser::TileFormatWarning;
use crate::scene::cesium_3d_tile_feature_table::{Cesium3DTileFeatureTable, GlobalProperty};
use crate::scene::gltf_loader::{GltfLoader, GltfLoaderOptions};
use crate::scene::gltf_texture_loader::GltfTextureOptions;
use crate::scene::i3dm_parser;
use crate::scene::metadata_table_property::ByteView;
use crate::scene::model_components::{self as mc, AttributeSemantic};
use crate::scene::parse_batch_table::{parse_batch_table, ParseBatchTableOptions};
use crate::scene::resource_loader::{self, ResourceLoadFuture, ResourceLoader};
use crate::scene::structural_metadata::{StructuralMetadataOptions, StructuralMetadataWarning};
use crate::scene::{
    AttributeType, Axis, FrameResource, FrameState, GltfJsonInput, InstanceAttributeSemantic, MetadataClass,
    ModelImageHost, PropertyTable, PropertyTableOptions, ResourceCache, ResourceLoaderState, StructuralMetadata,
};
use futures_util::FutureExt;
use serde_json::Value;
use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;
use std::sync::Arc;

fn typed_array_to_f64_vec(arr: &TypedArray) -> Vec<f64> {
    (0..arr.len()).map(|i| arr.get(i).unwrap_or(0.0)).collect()
}

#[derive(Clone, Debug)]
pub struct I3dmLoaderOptions {
    pub release_gltf_json: bool,
    pub asynchronous: bool,
    pub incrementally_load_textures: bool,
    pub up_axis: Axis,
    pub forward_axis: Axis,
    pub load_attributes_as_typed_array: bool,
    pub load_attributes_for_2d: bool,
    pub enable_pick: bool,
    pub load_indices_for_wireframe: bool,
    pub load_primitive_outline: bool,
    pub ellipsoid: Ellipsoid,
    pub texture_options: GltfTextureOptions,
}

impl Default for I3dmLoaderOptions {
    fn default() -> Self {
        Self {
            release_gltf_json: false,
            asynchronous: true,
            incrementally_load_textures: true,
            up_axis: Axis::Y,
            forward_axis: Axis::X,
            load_attributes_as_typed_array: false,
            load_attributes_for_2d: false,
            enable_pick: false,
            load_indices_for_wireframe: false,
            load_primitive_outline: true,
            ellipsoid: Ellipsoid::WGS84,
            texture_options: GltfTextureOptions::default(),
        }
    }
}

type ChildLoader<G, H> = Rc<RefCell<Option<Rc<GltfLoader<G, H>>>>>;

struct ParsedState {
    instances_length: usize,
    feature_table: Cesium3DTileFeatureTable,
    batch_table_json: Option<Value>,
    batch_table_binary: Option<Vec<u8>>,
}

pub struct I3dmLoader<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    cache: ResourceCache,
    resource: Resource,
    base_resource: Resource,
    array_buffer: Rc<RefCell<Option<Arc<Vec<u8>>>>>,
    byte_offset: usize,
    context: Context<G>,
    host: Rc<H>,
    options: I3dmLoaderOptions,
    state: Rc<Cell<ResourceLoaderState>>,
    destroyed: Rc<Cell<bool>>,
    transform: Cell<Matrix4>,
    instances_length: Cell<usize>,
    parsed: Rc<RefCell<Option<ParsedState>>>,
    gltf_loader: ChildLoader<G, H>,
    components: RefCell<Option<mc::Components<G>>>,
    buffers: RefCell<Vec<Buffer<G>>>,
    promise: RefCell<Option<ResourceLoadFuture>>,
    warnings: RefCell<Vec<TileFormatWarning>>,
    metadata_warnings: RefCell<Vec<StructuralMetadataWarning>>,
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> I3dmLoader<G, H>
where
    H::Image: 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache: ResourceCache,
        resource: Resource,
        base_resource: Option<Resource>,
        array_buffer: Arc<Vec<u8>>,
        byte_offset: Option<usize>,
        context: Context<G>,
        host: Rc<H>,
        options: I3dmLoaderOptions,
    ) -> Self {
        let base_resource = base_resource.unwrap_or_else(|| resource.clone());
        let byte_offset = byte_offset.unwrap_or(0);
        Self {
            cache,
            resource,
            base_resource,
            array_buffer: Rc::new(RefCell::new(Some(array_buffer))),
            byte_offset,
            context,
            host,
            options,
            state: Rc::new(Cell::new(ResourceLoaderState::Unloaded)),
            destroyed: Rc::new(Cell::new(false)),
            transform: Cell::new(Matrix4::IDENTITY),
            instances_length: Cell::new(0),
            parsed: Rc::new(RefCell::new(None)),
            gltf_loader: Rc::new(RefCell::new(None)),
            components: RefCell::new(None),
            buffers: RefCell::new(Vec::new()),
            promise: RefCell::new(None),
            warnings: RefCell::new(Vec::new()),
            metadata_warnings: RefCell::new(Vec::new()),
        }
    }

    pub fn components(&self) -> Option<Ref<'_, mc::Components<G>>> {
        Ref::filter_map(self.components.borrow(), Option::as_ref).ok()
    }

    pub fn textures_loaded(&self) -> bool {
        self.gltf_loader.borrow().as_ref().is_some_and(|l| l.textures_loaded())
    }

    pub fn incrementally_load_textures(&self) -> bool {
        self.options.incrementally_load_textures
    }

    pub fn transform(&self) -> Matrix4 {
        self.transform.get()
    }

    pub fn instances_length(&self) -> usize {
        self.instances_length.get()
    }

    pub fn warnings(&self) -> Vec<TileFormatWarning> {
        self.warnings.borrow().clone()
    }

    pub fn metadata_warnings(&self) -> Vec<StructuralMetadataWarning> {
        self.metadata_warnings.borrow().clone()
    }

    pub fn is_unloaded(&self) -> bool {
        self.state.get() == ResourceLoaderState::Unloaded
    }

    fn unload_buffers(&self) {
        for buffer in self.buffers.borrow_mut().drain(..) {
            if !buffer.is_destroyed() {
                let _ = buffer.destroy();
            }
        }
    }

    fn create_vertex_buffer(&self, typed_array: &TypedArray) -> Result<Buffer<G>, RuntimeError> {
        let bytes = typed_array.to_le_bytes();
        let buffer =
            Buffer::new(&self.context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
        self.buffers.borrow_mut().push(buffer.clone());
        Ok(buffer)
    }

    fn get_positions(feature_table: &Cesium3DTileFeatureTable) -> Result<Vec<f32>, RuntimeError> {
        if feature_table.has_property("POSITION") {
            let positions = feature_table
                .get_property_array("POSITION", ComponentDatatype::Float, 3)
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .ok_or_else(|| {
                    RuntimeError::new("Either POSITION or POSITION_QUANTIZED must be defined for each instance.")
                })?;
            Ok(typed_array_to_f64_vec(&positions).into_iter().map(|v| v as f32).collect())
        } else if feature_table.has_property("POSITION_QUANTIZED") {
            let quantized_positions = feature_table
                .get_property_array("POSITION_QUANTIZED", ComponentDatatype::UnsignedShort, 3)
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .ok_or_else(|| {
                    RuntimeError::new("Either POSITION or POSITION_QUANTIZED must be defined for each instance.")
                })?;
            let offset = feature_table
                .get_global_property("QUANTIZED_VOLUME_OFFSET", Some(ComponentDatatype::Float), Some(3))
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .ok_or_else(|| {
                    RuntimeError::new(
                        "Global property: QUANTIZED_VOLUME_OFFSET must be defined for quantized positions.",
                    )
                })?;
            let scale = feature_table
                .get_global_property("QUANTIZED_VOLUME_SCALE", Some(ComponentDatatype::Float), Some(3))
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .ok_or_else(|| {
                    RuntimeError::new(
                        "Global property: QUANTIZED_VOLUME_SCALE must be defined for quantized positions.",
                    )
                })?;
            let offset_arr =
                [offset.element(0).unwrap_or(0.0), offset.element(1).unwrap_or(0.0), offset.element(2).unwrap_or(0.0)];
            let scale_arr =
                [scale.element(0).unwrap_or(0.0), scale.element(1).unwrap_or(0.0), scale.element(2).unwrap_or(0.0)];
            let q_vals = typed_array_to_f64_vec(&quantized_positions);
            let mut decoded = vec![0.0f32; q_vals.len()];
            for i in 0..(q_vals.len() / 3) {
                for j in 0..3 {
                    let index = 3 * i + j;
                    decoded[index] = ((q_vals[index] / 65535.0) * scale_arr[j] + offset_arr[j]) as f32;
                }
            }
            Ok(decoded)
        } else {
            Err(RuntimeError::new("Either POSITION or POSITION_QUANTIZED must be defined for each instance."))
        }
    }

    fn process_rotation(
        feature_table: &Cesium3DTileFeatureTable,
        east_north_up: bool,
        i: usize,
        instance_position: Cartesian3,
    ) -> Result<Quaternion, RuntimeError> {
        let normal_up = feature_table
            .get_property("NORMAL_UP", ComponentDatatype::Float, 3, i)
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let normal_right = feature_table
            .get_property("NORMAL_RIGHT", ComponentDatatype::Float, 3, i)
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let mut instance_normal_up = Cartesian3::ZERO;
        let mut instance_normal_right = Cartesian3::ZERO;
        let mut instance_rotation = Matrix3::IDENTITY;
        let mut has_custom_orientation = false;

        if let Some(up) = normal_up {
            let Some(right) = normal_right else {
                return Err(RuntimeError::new(
                    "To define a custom orientation, both NORMAL_UP and NORMAL_RIGHT must be defined.",
                ));
            };
            instance_normal_up = Cartesian3::new(
                up.first().copied().flatten().unwrap_or(0.0),
                up.get(1).copied().flatten().unwrap_or(0.0),
                up.get(2).copied().flatten().unwrap_or(0.0),
            );
            instance_normal_right = Cartesian3::new(
                right.first().copied().flatten().unwrap_or(0.0),
                right.get(1).copied().flatten().unwrap_or(0.0),
                right.get(2).copied().flatten().unwrap_or(0.0),
            );
            has_custom_orientation = true;
        } else {
            let oct_normal_up = feature_table
                .get_property("NORMAL_UP_OCT32P", ComponentDatatype::UnsignedShort, 2, i)
                .map_err(|e| RuntimeError::new(e.to_string()))?;
            let oct_normal_right = feature_table
                .get_property("NORMAL_RIGHT_OCT32P", ComponentDatatype::UnsignedShort, 2, i)
                .map_err(|e| RuntimeError::new(e.to_string()))?;
            if let Some(oct_up) = oct_normal_up {
                let Some(oct_right) = oct_normal_right else {
                    return Err(RuntimeError::new(
                        "To define a custom orientation with oct-encoded vectors, both NORMAL_UP_OCT32P and NORMAL_RIGHT_OCT32P must be defined.",
                    ));
                };
                instance_normal_up = AttributeCompression::oct_decode_in_range(
                    oct_up.first().copied().flatten().unwrap_or(0.0),
                    oct_up.get(1).copied().flatten().unwrap_or(0.0),
                    65535.0,
                );
                instance_normal_right = AttributeCompression::oct_decode_in_range(
                    oct_right.first().copied().flatten().unwrap_or(0.0),
                    oct_right.get(1).copied().flatten().unwrap_or(0.0),
                    65535.0,
                );
                has_custom_orientation = true;
            } else if east_north_up {
                let instance_transform = Transforms::east_north_up_to_fixed_frame(instance_position, &Ellipsoid::WGS84);
                instance_rotation = instance_transform.get_matrix3();
            }
        }

        if has_custom_orientation {
            let instance_normal_forward = instance_normal_right.cross(instance_normal_up).normalize();
            instance_rotation = instance_rotation
                .set_column(0, instance_normal_right)
                .set_column(1, instance_normal_up)
                .set_column(2, instance_normal_forward);
        }

        Ok(Quaternion::from_rotation_matrix(&instance_rotation))
    }

    fn process_scale(feature_table: &Cesium3DTileFeatureTable, i: usize) -> Result<Cartesian3, RuntimeError> {
        let mut instance_scale = Cartesian3::new(1.0, 1.0, 1.0);
        if let Some(scale) = feature_table
            .get_property("SCALE", ComponentDatatype::Float, 1, i)
            .map_err(|e| RuntimeError::new(e.to_string()))?
        {
            if let Some(s) = scale.first().copied().flatten() {
                instance_scale = instance_scale.multiply_by_scalar(s);
            }
        }
        if let Some(non_uniform) = feature_table
            .get_property("SCALE_NON_UNIFORM", ComponentDatatype::Float, 3, i)
            .map_err(|e| RuntimeError::new(e.to_string()))?
        {
            instance_scale.x *= non_uniform.first().copied().flatten().unwrap_or(1.0);
            instance_scale.y *= non_uniform.get(1).copied().flatten().unwrap_or(1.0);
            instance_scale.z *= non_uniform.get(2).copied().flatten().unwrap_or(1.0);
        }
        Ok(instance_scale)
    }

    fn create_instances(
        &self,
        feature_table: &Cesium3DTileFeatureTable,
        instances_length: usize,
        components: &mut mc::Components<G>,
    ) -> Result<(), RuntimeError> {
        if instances_length == 0 {
            return Ok(());
        }
        let rtc_center = feature_table
            .get_global_property("RTC_CENTER", Some(ComponentDatatype::Float), Some(3))
            .map_err(|e| RuntimeError::new(e.to_string()))?
            .map(|rtc| {
                Cartesian3::new(
                    rtc.element(0).unwrap_or(0.0),
                    rtc.element(1).unwrap_or(0.0),
                    rtc.element(2).unwrap_or(0.0),
                )
            });

        let east_north_up = feature_table
            .get_global_property("EAST_NORTH_UP", None, None)
            .map_err(|e| RuntimeError::new(e.to_string()))?
            .is_some_and(|p| match p {
                GlobalProperty::Json(Value::Bool(b)) => b,
                other => other.to_number() != 0.0,
            });
        let has_rotation =
            feature_table.has_property("NORMAL_UP") || feature_table.has_property("NORMAL_UP_OCT32P") || east_north_up;
        let has_scale = feature_table.has_property("SCALE") || feature_table.has_property("SCALE_NON_UNIFORM");

        let mut translation_f32 = Self::get_positions(feature_table)?;
        let instance_positions: Vec<Cartesian3> =
            translation_f32.chunks_exact(3).map(|c| Cartesian3::new(c[0] as f64, c[1] as f64, c[2] as f64)).collect();

        if rtc_center.is_none_or(|c| c == Cartesian3::ZERO) {
            let bounding_sphere = BoundingSphere::from_points(&instance_positions);
            for (i, pos) in instance_positions.iter().enumerate() {
                let rel = *pos - bounding_sphere.center;
                translation_f32[3 * i] = rel.x as f32;
                translation_f32[3 * i + 1] = rel.y as f32;
                translation_f32[3 * i + 2] = rel.z as f32;
            }
            let center_transform = Matrix4::from_translation(bounding_sphere.center);
            components.transform = center_transform.multiply_transformation(&components.transform);
        }

        let mut rotation_f32 = if has_rotation { vec![0.0f32; 4 * instances_length] } else { Vec::new() };
        let mut scale_f32 = if has_scale { vec![0.0f32; 3 * instances_length] } else { Vec::new() };
        let mut feature_id_f32 = vec![0.0f32; instances_length];

        for i in 0..instances_length {
            let mut instance_position = instance_positions[i];
            if let Some(rtc) = rtc_center {
                instance_position += rtc;
            }
            if has_rotation {
                let q = Self::process_rotation(feature_table, east_north_up, i, instance_position)?;
                rotation_f32[4 * i] = q.x as f32;
                rotation_f32[4 * i + 1] = q.y as f32;
                rotation_f32[4 * i + 2] = q.z as f32;
                rotation_f32[4 * i + 3] = q.w as f32;
            }
            if has_scale {
                let s = Self::process_scale(feature_table, i)?;
                scale_f32[3 * i] = s.x as f32;
                scale_f32[3 * i + 1] = s.y as f32;
                scale_f32[3 * i + 2] = s.z as f32;
            }
            let batch_id = feature_table
                .get_property("BATCH_ID", ComponentDatatype::UnsignedShort, 1, i)
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .and_then(|v| v.first().copied().flatten())
                .unwrap_or(i as f64);
            feature_id_f32[i] = batch_id as f32;
        }

        let mut instances = mc::Instances { transform_in_world_space: true, ..Default::default() };

        let translation_typed_array = TypedArray::Float32(translation_f32);
        let translation_buffer =
            if !has_rotation { Some(self.create_vertex_buffer(&translation_typed_array)?) } else { None };
        instances.attributes.push(mc::Attribute {
            name: Some("Instance Translation".into()),
            semantic: Some(AttributeSemantic::Instance(InstanceAttributeSemantic::Translation)),
            component_datatype: Some(ComponentDatatype::Float),
            attribute_type: Some(AttributeType::Vec3),
            count: Some(instances_length),
            typed_array: Some(translation_typed_array),
            buffer: translation_buffer,
            ..Default::default()
        });

        if has_rotation {
            instances.attributes.push(mc::Attribute {
                name: Some("Instance Rotation".into()),
                semantic: Some(AttributeSemantic::Instance(InstanceAttributeSemantic::Rotation)),
                component_datatype: Some(ComponentDatatype::Float),
                attribute_type: Some(AttributeType::Vec4),
                count: Some(instances_length),
                typed_array: Some(TypedArray::Float32(rotation_f32)),
                ..Default::default()
            });
        }

        if has_scale {
            let scale_typed_array = TypedArray::Float32(scale_f32);
            let (typed_array, buffer) = if has_rotation {
                (Some(scale_typed_array), None)
            } else {
                let buf = self.create_vertex_buffer(&scale_typed_array)?;
                (None, Some(buf))
            };
            instances.attributes.push(mc::Attribute {
                name: Some("Instance Scale".into()),
                semantic: Some(AttributeSemantic::Instance(InstanceAttributeSemantic::Scale)),
                component_datatype: Some(ComponentDatatype::Float),
                attribute_type: Some(AttributeType::Vec3),
                count: Some(instances_length),
                typed_array,
                buffer,
                ..Default::default()
            });
        }

        let feature_id_typed_array = TypedArray::Float32(feature_id_f32);
        let feature_id_buffer = self.create_vertex_buffer(&feature_id_typed_array)?;
        instances.attributes.push(mc::Attribute {
            name: Some("Instance Feature ID".into()),
            set_index: Some(0),
            semantic: Some(AttributeSemantic::Instance(InstanceAttributeSemantic::FeatureId)),
            component_datatype: Some(ComponentDatatype::Float),
            attribute_type: Some(AttributeType::Scalar),
            count: Some(instances_length),
            buffer: Some(feature_id_buffer),
            ..Default::default()
        });

        instances.feature_ids.push(mc::FeatureId::Attribute(mc::FeatureIdAttribute {
            feature_count: None,
            null_feature_id: None,
            property_table_id: Some(0),
            set_index: Some(0),
            label: None,
            positional_label: Some("instanceFeatureId_0".into()),
        }));

        for node in &mut components.nodes {
            if !node.primitives.is_empty() {
                node.instances = Some(instances.clone());
            }
        }

        Ok(())
    }

    fn create_structural_metadata(
        &self,
        instances_length: usize,
        batch_table_json: Option<&Value>,
        batch_table_binary: Option<&[u8]>,
        components: &mut mc::Components<G>,
    ) -> Result<(), RuntimeError> {
        if instances_length == 0 {
            return Ok(());
        }
        let structural_metadata: StructuralMetadata<G> = if let Some(json) = batch_table_json {
            let mut warnings = Vec::new();
            let binary_body = batch_table_binary.map(|buffer| ByteView { buffer, byte_offset: 0 });
            let metadata: StructuralMetadata<G> = parse_batch_table(
                &ParseBatchTableOptions {
                    count: instances_length,
                    batch_table: json,
                    binary_body,
                    parse_as_property_attributes: false,
                    typed_arrays: None,
                },
                None,
                &mut warnings,
            )
            .map_err(|e| RuntimeError::new(e.to_string()))?;
            *self.metadata_warnings.borrow_mut() = warnings;
            metadata
        } else {
            let empty_property_table = PropertyTable::new(PropertyTableOptions {
                name: Some(MetadataClass::BATCH_TABLE_CLASS_NAME.into()),
                count: instances_length,
                ..Default::default()
            });
            StructuralMetadata::new(StructuralMetadataOptions {
                property_tables: Some(vec![empty_property_table]),
                ..Default::default()
            })
        };
        components.structural_metadata = Some(FrameResource::new(Rc::new(structural_metadata)));
        Ok(())
    }
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> ResourceLoader for I3dmLoader<G, H>
where
    H::Image: 'static,
{
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
        self.state.set(ResourceLoaderState::Loading);

        let parse_result = (|| -> Result<Rc<GltfLoader<G, H>>, RuntimeError> {
            let array_buffer =
                self.array_buffer.borrow().clone().ok_or_else(|| RuntimeError::new("i3dm arrayBuffer was unloaded"))?;
            let i3dm = i3dm_parser::parse(&array_buffer, self.byte_offset)?;
            *self.warnings.borrow_mut() = i3dm.warnings;
            let mut feature_table = Cesium3DTileFeatureTable::new(i3dm.feature_table_json, i3dm.feature_table_binary);
            let instances_prop = feature_table
                .get_global_property("INSTANCES_LENGTH", None, None)
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .ok_or_else(|| RuntimeError::new("Feature table global property: INSTANCES_LENGTH must be defined"))?;
            let instances_f64 = instances_prop.to_number();
            let instances_length =
                if instances_f64.is_finite() && instances_f64 >= 0.0 { instances_f64 as usize } else { 0 };
            feature_table.features_length = instances_f64;
            self.instances_length.set(instances_length);

            if let Some(rtc) = feature_table
                .get_global_property("RTC_CENTER", Some(ComponentDatatype::Float), Some(3))
                .map_err(|e| RuntimeError::new(e.to_string()))?
            {
                let x = rtc.element(0).unwrap_or(0.0);
                let y = rtc.element(1).unwrap_or(0.0);
                let z = rtc.element(2).unwrap_or(0.0);
                self.transform.set(Matrix4::from_translation(Cartesian3::new(x, y, z)));
            }

            let gltf_options = GltfLoaderOptions {
                release_gltf_json: self.options.release_gltf_json,
                asynchronous: self.options.asynchronous,
                incrementally_load_textures: self.options.incrementally_load_textures,
                up_axis: self.options.up_axis,
                forward_axis: self.options.forward_axis,
                load_attributes_as_typed_array: self.options.load_attributes_as_typed_array,
                load_attributes_for_2d: self.options.load_attributes_for_2d,
                enable_pick: self.options.enable_pick,
                load_indices_for_wireframe: self.options.load_indices_for_wireframe,
                load_primitive_outline: self.options.load_primitive_outline,
                load_for_classification: false,
                rename_batch_id_semantic: false,
                texture_options: self.options.texture_options,
            };

            let (gltf_resource, base_resource, input) = if i3dm.gltf_format == 0 {
                let gltf_url = get_string_from_typed_array(i3dm.gltf.bytes(), 0, None);
                let gltf_url = gltf_url.trim_end_matches(|c: char| c.is_whitespace() || c == '\0');
                let derived = self
                    .base_resource
                    .get_derived_resource(DerivedResourceOptions {
                        url: Some(gltf_url.to_string()),
                        ..Default::default()
                    })
                    .ok_or_else(|| RuntimeError::new(format!("Failed to derive glTF resource URL '{gltf_url}'")))?;
                (derived.clone(), Some(derived), None)
            } else {
                (self.resource.clone(), Some(self.base_resource.clone()), Some(GltfJsonInput::Bytes(i3dm.gltf)))
            };

            *self.parsed.borrow_mut() = Some(ParsedState {
                instances_length,
                feature_table,
                batch_table_json: i3dm.batch_table_json,
                batch_table_binary: i3dm.batch_table_binary,
            });

            let gltf_loader = Rc::new(GltfLoader::new(
                self.cache.clone(),
                gltf_resource,
                base_resource,
                input,
                self.context.clone(),
                self.host.clone(),
                gltf_options,
            ));
            *self.gltf_loader.borrow_mut() = Some(gltf_loader.clone());
            Ok(gltf_loader)
        })();

        let state = match parse_result {
            Ok(gltf_loader) => {
                let child_future = gltf_loader.load();
                let gltf_ref = self.gltf_loader.clone();
                let parsed_ref = self.parsed.clone();
                let array_buf_ref = self.array_buffer.clone();
                let state_cell = self.state.clone();
                let destroyed_cell = self.destroyed.clone();
                async move {
                    match child_future.await {
                        Ok(loaded) => {
                            if !loaded || destroyed_cell.get() || state_cell.get() == ResourceLoaderState::Unloaded {
                                return Ok(false);
                            }
                            state_cell.set(ResourceLoaderState::Processing);
                            Ok(true)
                        }
                        Err(e) => {
                            if destroyed_cell.get() || state_cell.get() == ResourceLoaderState::Unloaded {
                                return Ok(false);
                            }
                            if !destroyed_cell.get() {
                                if let Some(loader) = gltf_ref.borrow_mut().take() {
                                    loader.unload();
                                }
                                parsed_ref.borrow_mut().take();
                                array_buf_ref.borrow_mut().take();
                                state_cell.set(ResourceLoaderState::Failed);
                            }
                            Err(resource_loader::loader_error("Failed to load i3dm", Some(&e)))
                        }
                    }
                }
                .boxed_local()
                .shared()
            }
            Err(e) => {
                self.unload();
                self.state.set(ResourceLoaderState::Failed);
                let err = resource_loader::loader_error("Failed to load i3dm", Some(&e));
                async move { Err(err) }.boxed_local().shared()
            }
        };
        *self.promise.borrow_mut() = Some(state.clone());
        state
    }

    fn state(&self) -> ResourceLoaderState {
        self.state.get()
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }

    fn unload(&self) {
        if let Some(loader) = self.gltf_loader.borrow_mut().take() {
            if !loader.is_destroyed() {
                loader.unload();
            }
        }
        self.unload_buffers();
        self.components.borrow_mut().take();
        self.parsed.borrow_mut().take();
        self.array_buffer.borrow_mut().take();
        self.promise.borrow_mut().take();
        self.state.set(ResourceLoaderState::Unloaded);
    }

    fn destroy(&self) {
        self.unload();
        self.destroyed.set(true);
    }

    fn process(&self, frame: &FrameState) -> Result<bool, RuntimeError> {
        if self.state.get() == ResourceLoaderState::Ready {
            // Geometry may become ready before textures. Continue driving the
            // child and refresh the component snapshot as textures arrive.
            if let Some(loader) = self.gltf_loader.borrow().clone() {
                if loader.textures_loaded() {
                    return Ok(true);
                }
                if let Err(error) = loader.process(frame) {
                    return Err(resource_loader::loader_error("Failed to load i3dm", Some(&error)));
                }
                if let Some(components) = loader.components() {
                    *self.components.borrow_mut() = Some(components.clone());
                }
            }
            return Ok(true);
        }
        if self.state.get() != ResourceLoaderState::Processing && self.state.get() != ResourceLoaderState::Loaded {
            return Ok(false);
        }
        let Some(gltf_loader) = self.gltf_loader.borrow().clone() else {
            return Ok(false);
        };
        let ready = match gltf_loader.process(frame) {
            Ok(ready) => ready,
            Err(e) => {
                self.unload();
                self.state.set(ResourceLoaderState::Failed);
                return Err(resource_loader::loader_error("Failed to load i3dm", Some(&e)));
            }
        };
        if !ready {
            return Ok(false);
        }
        let Some(parsed) = self.parsed.borrow_mut().take() else {
            return Ok(false);
        };
        if let Some(mut components) = gltf_loader.components_mut() {
            components.transform = self.transform.get().multiply_transformation(&components.transform);
            if let Err(e) =
                self.create_instances(&parsed.feature_table, parsed.instances_length, &mut components).and_then(|()| {
                    self.create_structural_metadata(
                        parsed.instances_length,
                        parsed.batch_table_json.as_ref(),
                        parsed.batch_table_binary.as_deref(),
                        &mut components,
                    )
                })
            {
                drop(components);
                self.unload();
                self.state.set(ResourceLoaderState::Failed);
                return Err(resource_loader::loader_error("Failed to load i3dm", Some(&e)));
            }
            *self.components.borrow_mut() = Some(components.clone());
        }
        self.array_buffer.borrow_mut().take();
        self.state.set(ResourceLoaderState::Ready);
        Ok(true)
    }
}
