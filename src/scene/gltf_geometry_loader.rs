//! Shared implementation of frame-processed glTF GPU vertex/index resources.
use super::{
    gltf_pipeline::BufferSource,
    resource_loader::{loader_error, ManagedLoader},
    *,
};
use crate::{
    core::{ComponentDatatype, IndexDatatype, RuntimeError, TypedArray},
    renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl},
};
use futures_util::FutureExt;
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};
type Result<T> = std::result::Result<T, RuntimeError>;
#[derive(Clone)]
pub struct GltfResource {
    pub document: Arc<LoadedGltfJson>,
    pub resource: crate::core::Resource,
    pub base: crate::core::Resource,
}
impl GltfResource {
    pub fn key_context(&self) -> GltfKeyContext<'_> {
        GltfKeyContext { gltf: &self.document.json, gltf_resource: &self.resource, base_resource: &self.base }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct GpuBufferOptions {
    pub asynchronous: bool,
    pub load_buffer: bool,
    pub load_typed_array: bool,
}
impl Default for GpuBufferOptions {
    fn default() -> Self {
        Self { asynchronous: true, load_buffer: false, load_typed_array: false }
    }
}
impl GpuBufferOptions {
    fn validate(self) -> Result<()> {
        if !self.load_buffer && !self.load_typed_array {
            Err(RuntimeError::new("At least one of loadBuffer and loadTypedArray must be true."))
        } else {
            Ok(())
        }
    }
}
/// Optional host upload quota, stored in FrameState.job_scheduler. Reset it for
/// each frame; an exhausted quota leaves loaders PROCESSING for the next frame.
pub struct BufferUploadBudget {
    remaining: Cell<usize>,
}
impl BufferUploadBudget {
    pub fn new(jobs: usize) -> Self {
        Self { remaining: Cell::new(jobs) }
    }
    pub fn reset(&self, jobs: usize) {
        self.remaining.set(jobs);
    }
    pub(crate) fn claim(&self) -> bool {
        let n = self.remaining.get();
        if n == 0 {
            false
        } else {
            self.remaining.set(n - 1);
            true
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ModelQuantization {
    pub component_datatype: ComponentDatatype,
    pub oct_encoded: bool,
    pub oct_encoded_zxy: bool,
    pub attribute_type: AttributeType,
    pub normalization_range: Option<MetadataValue>,
    pub quantized_volume_offset: Option<MetadataValue>,
    pub quantized_volume_dimensions: Option<MetadataValue>,
    pub quantized_volume_step_size: Option<MetadataValue>,
}
#[derive(Clone)]
pub enum VertexBufferSource {
    BufferView(usize),
    Draco {
        primitive: Value,
        extension: Value,
        semantic: String,
        accessor_id: usize,
    },
    /// A `KHR_gaussian_splatting_compression_spz_2` attribute: `POSITION`,
    /// `COLOR_0`, a scale, rotation or spherical-harmonic semantic with the
    /// `KHR_gaussian_splatting:` or `_` prefix.
    Spz {
        primitive: Value,
        extension: Value,
        semantic: String,
    },
}
type SpzFactory = Box<dyn FnOnce() -> Result<CacheLease<GltfSpzLoader>>>;
pub(crate) enum GeometryChild {
    View(CacheLease<GltfBufferViewLoader>),
    Draco(CacheLease<GltfDracoLoader>),
    /// Requested from the cache when loading, as upstream does.
    PendingSpz(SpzFactory),
    Spz(CacheLease<GltfSpzLoader>),
}
#[derive(Clone)]
enum Payload {
    View(BufferSource),
    Draco(Rc<GltfDracoLoader>),
    Spz(Rc<GltfSpzLoader>),
}
struct GeometryLoader<G: Gl> {
    data: ManagedLoader<Payload>,
    child: Rc<RefCell<Option<GeometryChild>>>,
    context: Option<Context<G>>,
    options: GpuBufferOptions,
    accessor: Option<Value>,
    semantic: Option<String>,
    index_datatype: Cell<Option<IndexDatatype>>,
    quantization: RefCell<Option<ModelQuantization>>,
    /// Whether `bytes` holds the decoded typed array; an SPZ semantic it does
    /// not recognize decodes to none.
    decoded: Cell<bool>,
    bytes: RefCell<Option<BufferSource>>,
    buffer: RefCell<Option<Buffer<G>>>,
    statistics: Weak<RefCell<ResourceCacheStatistics>>,
    label: &'static str,
}
fn index_datatype(value: u64) -> Result<IndexDatatype> {
    match value {
        5121 => Ok(IndexDatatype::UnsignedByte),
        5123 => Ok(IndexDatatype::UnsignedShort),
        5125 => Ok(IndexDatatype::UnsignedInt),
        _ => Err(RuntimeError::new("Invalid glTF index datatype")),
    }
}
fn bytes(array: &TypedArray) -> Vec<u8> {
    macro_rules! pack {
        ($values:expr) => {
            $values.iter().flat_map(|v| v.to_le_bytes()).collect()
        };
    }
    match array {
        TypedArray::Uint8(v) => v.clone(),
        TypedArray::Int8(v) => v.iter().map(|v| *v as u8).collect(),
        TypedArray::Int16(v) => pack!(v),
        TypedArray::Uint16(v) => pack!(v),
        TypedArray::Int32(v) => pack!(v),
        TypedArray::Uint32(v) => pack!(v),
        TypedArray::Float32(v) => pack!(v),
        TypedArray::Float64(v) => pack!(v),
    }
}
fn typed_json(value: &Value, datatype: ComponentDatatype) -> Result<TypedArray> {
    let values = value
        .as_array()
        .ok_or_else(|| RuntimeError::new("Missing decoded typed array"))?
        .iter()
        .map(|v| v.as_f64().ok_or_else(|| RuntimeError::new("Invalid decoded typed array value")))
        .collect::<Result<Vec<_>>>()?;
    Ok(TypedArray::from_values(datatype, &values))
}
fn quantization(attribute: &Value, accessor: &Value) -> Result<Option<ModelQuantization>> {
    let q = &attribute["data"]["quantization"];
    if q.is_null() {
        return Ok(None);
    }
    let bits =
        q["quantizationBits"].as_u64().ok_or_else(|| RuntimeError::new("Invalid Draco quantization bits"))? as u32;
    let range = 1i32.wrapping_shl(bits).wrapping_sub(1) as f64;
    let oct = q["octEncoded"] == true;
    let component_datatype =
        ComponentDatatype::from_gl(attribute["data"]["componentDatatype"].as_u64().unwrap_or(0) as u32)
            .ok_or_else(|| RuntimeError::new("Invalid Draco component datatype"))?;
    let attribute_type = AttributeType::from_str_value(accessor["type"].as_str().unwrap_or(""))
        .ok_or_else(|| RuntimeError::new("Invalid Draco attribute type"))?;
    let mut result = ModelQuantization {
        component_datatype,
        oct_encoded: oct,
        oct_encoded_zxy: true,
        attribute_type: if oct { AttributeType::Vec2 } else { attribute_type },
        normalization_range: Some(MetadataValue::Number(range)),
        quantized_volume_offset: None,
        quantized_volume_dimensions: None,
        quantized_volume_step_size: None,
    };
    if !oct {
        let dimensions = q["range"].as_f64().ok_or_else(|| RuntimeError::new("Invalid Draco quantization range"))?;
        let count = attribute["data"]["componentsPerAttribute"].as_u64().unwrap_or(1) as usize;
        let convert = |values: Vec<MetadataValue>| -> Result<MetadataValue> {
            if attribute_type == AttributeType::Scalar {
                return Ok(values.into_iter().next().unwrap_or(MetadataValue::Null));
            }
            MetadataPicking::convert_to_object_type(
                MetadataType::from_str_value(attribute_type.as_str()).expect("matching metadata type"),
                Some(MetadataValue::Array(values)),
            )
            .map_err(|e| RuntimeError::new(e.to_string()))?
            .ok_or_else(|| RuntimeError::new("Missing quantization value"))
        };
        let repeated = |v| vec![MetadataValue::Number(v); count];
        result.normalization_range = Some(convert(repeated(range))?);
        result.quantized_volume_dimensions = Some(convert(repeated(dimensions))?);
        result.quantized_volume_step_size = Some(convert(repeated(dimensions / range))?);
        let offset = q["minValues"]
            .as_array()
            .ok_or_else(|| RuntimeError::new("Invalid Draco quantization offset"))?
            .iter()
            .map(|v| MetadataValue::Number(v.as_f64().unwrap_or(f64::NAN)))
            .collect();
        result.quantized_volume_offset = Some(convert(offset)?);
    }
    Ok(Some(result))
}
impl<G: Gl + 'static> GeometryLoader<G> {
    fn new(
        child: GeometryChild,
        context: Option<Context<G>>,
        options: GpuBufferOptions,
        key: String,
        accessor: Option<Value>,
        semantic: Option<String>,
        statistics: Rc<RefCell<ResourceCacheStatistics>>,
    ) -> Result<Self> {
        options.validate()?;
        if options.load_buffer && context.is_none() {
            return Err(RuntimeError::new("GPU loading requires a renderer context"));
        }
        let index = if semantic.is_none() {
            Some(index_datatype(accessor.as_ref().and_then(|a| a["componentType"].as_u64()).unwrap_or(0))?)
        } else {
            None
        };
        Ok(Self {
            data: ManagedLoader::new(Some(key), None),
            child: Rc::new(RefCell::new(Some(child))),
            context,
            options,
            accessor,
            semantic,
            index_datatype: Cell::new(index),
            quantization: RefCell::new(None),
            decoded: Cell::new(false),
            bytes: RefCell::new(None),
            buffer: RefCell::new(None),
            statistics: Rc::downgrade(&statistics),
            label: if index.is_some() { "Failed to load index buffer" } else { "Failed to load vertex buffer" },
        })
    }
    fn load(&self) -> ResourceLoadFuture {
        enum Child {
            View(Rc<GltfBufferViewLoader>),
            Draco(Rc<GltfDracoLoader>),
            Spz(Rc<GltfSpzLoader>),
            /// Requesting the SPZ loader failed. The source swallows its SPZ
            /// errors, leaving the loader LOADING.
            SpzFailed,
        }
        let pending =
            !self.data.is_destroyed() && matches!(self.child.borrow().as_ref(), Some(GeometryChild::PendingSpz(_)));
        let mut spz_failed = false;
        if pending {
            let child = self.child.borrow_mut().take();
            if let Some(GeometryChild::PendingSpz(factory)) = child {
                match factory() {
                    Ok(lease) => *self.child.borrow_mut() = Some(GeometryChild::Spz(lease)),
                    Err(_) => spz_failed = true,
                }
            }
        }
        let child = if spz_failed {
            Some(Child::SpzFailed)
        } else {
            self.child.borrow().as_ref().and_then(|c| match c {
                GeometryChild::View(lease) => Some(Child::View(lease.loader())),
                GeometryChild::Draco(lease) => Some(Child::Draco(lease.loader())),
                GeometryChild::Spz(lease) => Some(Child::Spz(lease.loader())),
                GeometryChild::PendingSpz(_) => None,
            })
        };
        let success = if matches!(child, Some(Child::Draco(_) | Child::Spz(_))) {
            ResourceLoaderState::Loaded
        } else {
            ResourceLoaderState::Processing
        };
        let indices = self.accessor.clone().zip(self.index_datatype.get());
        let lease = Rc::downgrade(&self.child);
        self.data.load_optional(self.label.into(), success, move || {
            let future = match child {
                Some(Child::View(loader)) => {
                    let request = loader.load();
                    let weak = Rc::downgrade(&loader);
                    async move {
                        request.await?;
                        let view = weak
                            .upgrade()
                            .and_then(|l| l.typed_array())
                            .ok_or_else(|| RuntimeError::new("Buffer view was unloaded"))?;
                        let view = if let Some((accessor, datatype)) = indices {
                            view_indices(view, &accessor, datatype)?
                        } else {
                            view
                        };
                        Ok(Some(Payload::View(view)))
                    }
                    .boxed_local()
                }
                Some(Child::Draco(loader)) => {
                    let request = loader.load();
                    let weak = Rc::downgrade(&loader);
                    async move {
                        request.await?;
                        weak.upgrade()
                            .map(|l| Some(Payload::Draco(l)))
                            .ok_or_else(|| RuntimeError::new("Draco loader was unloaded"))
                    }
                    .boxed_local()
                }
                Some(Child::Spz(loader)) => {
                    let request = loader.load();
                    let weak = Rc::downgrade(&loader);
                    async move { Ok(request.await.ok().and_then(|_| weak.upgrade()).map(Payload::Spz)) }.boxed_local()
                }
                Some(Child::SpzFailed) => async { Ok(None) }.boxed_local(),
                None => async { Err(RuntimeError::new("Geometry source was unloaded")) }.boxed_local(),
            };
            async move {
                let result = future.await;
                if result.is_err() {
                    if let Some(lease) = lease.upgrade() {
                        lease.borrow_mut().take();
                    }
                }
                result
            }
            .boxed_local()
        })
    }
    /// The typed array once decoded; an unrecognized SPZ semantic has none.
    fn decode(&self, payload: Payload, frame: &FrameState) -> Result<Option<Option<BufferSource>>> {
        match payload {
            Payload::View(view) => Ok(Some(Some(view))),
            Payload::Spz(loader) => {
                if !loader.process(frame)? {
                    return Ok(None);
                }
                self.data.set_state(ResourceLoaderState::Processing);
                let cloud = loader.decoded_data().ok_or_else(|| RuntimeError::new("Missing SPZ decoded data"))?;
                Ok(Some(spz_attribute(&cloud, self.semantic.as_deref().unwrap_or_default())))
            }
            Payload::Draco(loader) => {
                if !loader.process(frame)? {
                    return Ok(None);
                }
                if self.semantic.is_some() {
                    self.data.set_state(ResourceLoaderState::Processing);
                }
                let decoded = loader.decoded_data().ok_or_else(|| RuntimeError::new("Missing Draco decoded data"))?;
                if let Some(semantic) = &self.semantic {
                    let attribute = &decoded["vertexAttributes"][semantic];
                    let datatype =
                        ComponentDatatype::from_gl(attribute["data"]["componentDatatype"].as_u64().unwrap_or(0) as u32)
                            .ok_or_else(|| RuntimeError::new("Invalid decoded vertex type"))?;
                    *self.quantization.borrow_mut() =
                        quantization(attribute, self.accessor.as_ref().expect("Draco accessor"))?;
                    Ok(Some(Some(BufferSource::new(bytes(&typed_json(&attribute["array"], datatype)?)))))
                } else {
                    let datatype = index_datatype(decoded["indices"]["indexDatatype"].as_u64().unwrap_or(0))?;
                    self.index_datatype.set(Some(datatype));
                    let data = typed_json(
                        &decoded["indices"]["typedArray"],
                        ComponentDatatype::from_gl(datatype as u32).expect("index component"),
                    )?;
                    Ok(Some(Some(BufferSource::new(bytes(&data)))))
                }
            }
        }
    }
    fn process(&self, frame: &FrameState) -> Result<bool> {
        if self.data.is_destroyed() {
            return Err(RuntimeError::new("Geometry loader was destroyed"));
        }
        if self.data.state() == ResourceLoaderState::Ready {
            return Ok(true);
        }
        if !matches!(self.data.state(), ResourceLoaderState::Loaded | ResourceLoaderState::Processing) {
            return Ok(false);
        }
        if !self.decoded.get() {
            let Some(payload) = self.data.value() else {
                return Ok(false);
            };
            match self.decode(payload, frame) {
                Ok(Some(bytes)) => {
                    *self.bytes.borrow_mut() = bytes;
                    self.decoded.set(true);
                }
                Ok(None) => return Ok(false),
                Err(error) => {
                    self.unload();
                    self.data.set_state(ResourceLoaderState::Failed);
                    return Err(loader_error(self.label, Some(&error)));
                }
            }
        }
        if self.options.load_buffer && self.options.asynchronous {
            if let Some(scheduler) = &frame.job_scheduler {
                let allowed = if let Some(b) = scheduler.get::<Rc<BufferUploadBudget>>() {
                    b.claim()
                } else if let Some(b) = scheduler.get::<Rc<ResourceUploadBudget>>() {
                    b.buffers.claim()
                } else {
                    return Err(RuntimeError::new("Unsupported buffer upload scheduler"));
                };
                if !allowed {
                    return Ok(false);
                }
            }
        }
        let view = self.bytes.borrow().clone();
        let buffer = if self.options.load_buffer {
            // Buffer.createVertexBuffer's check, reached by an unrecognized SPZ semantic.
            let view =
                view.as_ref().ok_or_else(|| RuntimeError::new("Must specify either sizeInBytes or typedArray."))?;
            let kind = self.index_datatype.get().map_or(BufferKind::Vertex, BufferKind::Index);
            Some(Buffer::new(
                self.context.as_ref().expect("validated"),
                kind,
                BufferUsage::StaticDraw,
                BufferData::Bytes(view.bytes()),
            )?)
        } else {
            None
        };
        self.child.borrow_mut().take();
        self.data.unload();
        *self.buffer.borrow_mut() = buffer;
        self.data.set_state(ResourceLoaderState::Ready);
        if let Some(statistics) = self.statistics.upgrade() {
            statistics.borrow_mut().add_geometry_loader(
                self.data.key.as_deref().expect("key"),
                self.buffer.borrow().as_ref().map(|b| b.size_in_bytes() as u64),
                view.filter(|_| self.options.load_typed_array).map(|v| v.byte_length as u64),
            );
        }
        if !self.options.load_typed_array {
            self.bytes.borrow_mut().take();
        }
        Ok(true)
    }
    fn unload(&self) {
        if let Some(buffer) = self.buffer.borrow_mut().take() {
            let _ = buffer.destroy();
        }
        self.child.borrow_mut().take();
        self.data.unload();
        self.decoded.set(false);
        self.bytes.borrow_mut().take();
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
    fn current_bytes(&self) -> Option<BufferSource> {
        self.bytes.borrow().clone().or_else(|| match self.data.value() {
            Some(Payload::View(view)) => Some(view),
            _ => None,
        })
    }
}
pub struct GltfVertexBufferLoader<G: Gl>(GeometryLoader<G>);
pub struct GltfIndexBufferLoader<G: Gl>(GeometryLoader<G>);
macro_rules! loader_impl {
    ($type:ident) => {
        impl<G: Gl + 'static> ResourceLoader for $type<G> {
            fn cache_key(&self) -> Option<&str> {
                self.0.data.key.as_deref()
            }
            fn state(&self) -> ResourceLoaderState {
                self.0.data.state()
            }
            fn is_destroyed(&self) -> bool {
                self.0.data.is_destroyed()
            }
            fn load(&self) -> ResourceLoadFuture {
                self.0.load()
            }
            fn process(&self, frame: &FrameState) -> Result<bool> {
                self.0.process(frame)
            }
            fn unload(&self) {
                self.0.unload();
            }
            fn destroy(&self) {
                self.0.destroy();
            }
        }
        impl<G: Gl + 'static> $type<G> {
            pub fn buffer(&self) -> Option<Buffer<G>> {
                self.0.buffer.borrow().clone()
            }
        }
    };
}
loader_impl!(GltfVertexBufferLoader);
loader_impl!(GltfIndexBufferLoader);
impl<G: Gl + 'static> GltfVertexBufferLoader<G> {
    pub fn typed_array(&self) -> Option<BufferSource> {
        self.0.current_bytes()
    }
    pub fn quantization(&self) -> Option<ModelQuantization> {
        self.0.quantization.borrow().clone()
    }
}
impl<G: Gl + 'static> GltfIndexBufferLoader<G> {
    pub fn index_datatype(&self) -> IndexDatatype {
        self.0.index_datatype.get().expect("index")
    }
    pub fn typed_array(&self) -> Option<TypedArray> {
        let bytes = self.0.current_bytes()?;
        Some(match self.index_datatype() {
            IndexDatatype::UnsignedByte => TypedArray::Uint8(bytes.bytes().to_vec()),
            IndexDatatype::UnsignedShort => {
                TypedArray::Uint16(bytes.bytes().as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect())
            }
            IndexDatatype::UnsignedInt => {
                TypedArray::Uint32(bytes.bytes().as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect())
            }
        })
    }
}
impl ResourceCache {
    pub fn vertex_buffer_loader<G: Gl + 'static>(
        &self,
        request: &GltfResource,
        source: VertexBufferSource,
        context: Option<Context<G>>,
        options: GpuBufferOptions,
    ) -> Result<CacheLease<GltfVertexBufferLoader<G>>> {
        options.validate()?;
        let key_source = match &source {
            VertexBufferSource::BufferView(id) => VertexKeySource::BufferView(*id),
            VertexBufferSource::Draco { extension, semantic, .. } => VertexKeySource::Draco { extension, semantic },
            VertexBufferSource::Spz { extension, semantic, .. } => VertexKeySource::Spz { extension, semantic },
        };
        let key = ResourceCacheKey::vertex_buffer(
            request.key_context(),
            key_source,
            BufferKeyOptions {
                load_buffer: options.load_buffer,
                load_typed_array: options.load_typed_array,
                context_id: context.as_ref().map(Context::id),
                dequantize: false,
            },
        )?;
        self.get_or_insert(&key, CacheResourceKind::VertexBuffer, || {
            let (child, accessor, semantic) = match source {
                VertexBufferSource::BufferView(id) => (
                    GeometryChild::View(self.buffer_view_loader(
                        request.document.clone(),
                        id,
                        &request.resource,
                        &request.base,
                    )?),
                    None,
                    Some(String::new()),
                ),
                VertexBufferSource::Draco { primitive, extension, semantic, accessor_id } => {
                    let accessor = request.document.json["accessors"]
                        .get(accessor_id)
                        .cloned()
                        .ok_or_else(|| RuntimeError::new("Invalid Draco accessor"))?;
                    (
                        GeometryChild::Draco(self.draco_loader(
                            request.document.clone(),
                            &primitive,
                            &extension,
                            &request.resource,
                            &request.base,
                        )?),
                        Some(accessor),
                        Some(semantic),
                    )
                }
                VertexBufferSource::Spz { semantic, .. } => {
                    let cache = self.downgrade();
                    let request = request.clone();
                    let factory: SpzFactory = Box::new(move || {
                        cache
                            .upgrade()
                            .ok_or_else(|| RuntimeError::new("Resource cache was destroyed"))?
                            .spz_loader(&request)
                    });
                    (GeometryChild::PendingSpz(factory), None, Some(semantic))
                }
            };
            Ok(GltfVertexBufferLoader(GeometryLoader::new(
                child,
                context,
                options,
                key.clone(),
                accessor,
                semantic,
                self.statistics(),
            )?))
        })
    }
    pub fn index_buffer_loader<G: Gl + 'static>(
        &self,
        request: &GltfResource,
        accessor_id: usize,
        draco: Option<(&Value, &Value)>,
        context: Option<Context<G>>,
        options: GpuBufferOptions,
    ) -> Result<CacheLease<GltfIndexBufferLoader<G>>> {
        options.validate()?;
        let key = ResourceCacheKey::index_buffer(
            request.key_context(),
            accessor_id,
            draco.map(|(_, extension)| extension),
            BufferKeyOptions {
                load_buffer: options.load_buffer,
                load_typed_array: options.load_typed_array,
                context_id: context.as_ref().map(Context::id),
                dequantize: false,
            },
        )?;
        self.get_or_insert(&key, CacheResourceKind::IndexBuffer, || {
            let accessor = request.document.json["accessors"]
                .get(accessor_id)
                .cloned()
                .ok_or_else(|| RuntimeError::new("Invalid index accessor"))?;
            let child = if let Some((primitive, extension)) = draco {
                GeometryChild::Draco(self.draco_loader(
                    request.document.clone(),
                    primitive,
                    extension,
                    &request.resource,
                    &request.base,
                )?)
            } else {
                let id = accessor["bufferView"]
                    .as_u64()
                    .ok_or_else(|| RuntimeError::new("Invalid index buffer view"))? as usize;
                GeometryChild::View(self.buffer_view_loader(
                    request.document.clone(),
                    id,
                    &request.resource,
                    &request.base,
                )?)
            };
            Ok(GltfIndexBufferLoader(GeometryLoader::new(
                child,
                context,
                options,
                key.clone(),
                Some(accessor),
                None,
                self.statistics(),
            )?))
        })
    }
}

fn view_indices(view: BufferSource, accessor: &Value, datatype: IndexDatatype) -> Result<BufferSource> {
    let offset = accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = accessor["count"].as_u64().ok_or_else(|| RuntimeError::new("Invalid index count"))? as usize;
    let size = datatype.get_size_in_bytes();
    let length = count.checked_mul(size).ok_or_else(|| RuntimeError::new("Index count overflow"))?;
    let start = view.byte_offset.checked_add(offset).ok_or_else(|| RuntimeError::new("Index offset overflow"))?;
    let end = start.checked_add(length).ok_or_else(|| RuntimeError::new("Index length overflow"))?;
    if end > view.buffer.len() {
        return Err(RuntimeError::new("Index accessor is out of bounds"));
    }
    if !start.is_multiple_of(size) {
        Ok(BufferSource::new(view.buffer[start..end].to_vec()))
    } else {
        Ok(BufferSource::view(view.buffer, start, length))
    }
}

/// `processSpz`: one attribute of the decoded cloud, or none for a semantic it
/// does not recognize.
fn spz_attribute(cloud: &GaussianCloud, semantic: &str) -> Option<BufferSource> {
    let floats = |values: &[f32]| Some(BufferSource::new(values.iter().flat_map(|v| v.to_le_bytes()).collect()));
    match semantic {
        "POSITION" => floats(&cloud.positions),
        "KHR_gaussian_splatting:SCALE" | "_SCALE" => floats(&cloud.scales),
        "KHR_gaussian_splatting:ROTATION" | "_ROTATION" => floats(&cloud.rotations),
        "COLOR_0" => {
            // Uint8Array stores of CesiumMath.clamp(x * 255, 0, 255): truncated, NaN as 0.
            let byte = |v: f32| (f64::from(v) * 255.0).clamp(0.0, 255.0) as u8;
            let colors = cloud.colors.as_chunks::<3>().0;
            Some(BufferSource::new(
                colors
                    .iter()
                    .enumerate()
                    .flat_map(|(i, c)| [byte(c[0]), byte(c[1]), byte(c[2]), byte(cloud.alphas[i])])
                    .collect(),
            ))
        }
        s if s.contains("SH_DEGREE_") => {
            let (degree, coefficient) = sh_degree_and_coefficient(s);
            let stride = match cloud.sh_degree {
                1 => 9.,
                2 => 24.,
                3 => 45.,
                _ => 0.,
            };
            let base = match degree {
                1. => 0.,
                2. => 9.,
                3. => 24.,
                _ => f64::NAN,
            };
            // Out-of-range JavaScript array reads are undefined, stored as NaN.
            let read = |index: f64| {
                if index >= 0. && index.fract() == 0. && index < cloud.sh.len() as f64 {
                    cloud.sh[index as usize]
                } else {
                    f32::NAN
                }
            };
            let values = (0..cloud.num_points).flat_map(|i| {
                let index = i as f64 * stride + base + coefficient * 3.;
                [read(index), read(index + 1.), read(index + 2.)]
            });
            Some(BufferSource::new(values.flat_map(f32::to_le_bytes).collect()))
        }
        _ => None,
    }
}

/// `extractSHDegreeAndCoef`, on UTF-16 indices as JavaScript's string methods use.
fn sh_degree_and_coefficient(attribute: &str) -> (f64, f64) {
    let units = attribute.encode_utf16().collect::<Vec<_>>();
    let prefix = if attribute.starts_with("KHR_gaussian_splatting:") {
        "KHR_gaussian_splatting:SH_DEGREE_"
    } else {
        "_SH_DEGREE_"
    };
    let separator = "_COEF_".encode_utf16().collect::<Vec<_>>();
    let start = prefix.encode_utf16().count();
    let coefficient = (start..units.len()).find(|&i| units[i..].starts_with(&separator)).map_or(-1, |i| i as isize);
    // String.prototype.slice: negative indices count from the end.
    let slice = |from: isize, to: isize| {
        let length = units.len() as isize;
        let clamp = |i: isize| if i < 0 { (length + i).max(0) } else { i.min(length) } as usize;
        let (from, to) = (clamp(from), clamp(to));
        String::from_utf16_lossy(&units[from..to.max(from)])
    };
    let length = units.len() as isize;
    let degree = parse_int(&slice(start as isize, coefficient));
    let coefficient = parse_int(&slice(coefficient + separator.len() as isize, length));
    (degree, coefficient)
}

/// `parseInt(s, 10)`.
fn parse_int(s: &str) -> f64 {
    let s = s.trim_start_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}');
    let (sign, digits) = match s.as_bytes().first() {
        Some(b'-') => (-1., &s[1..]),
        Some(b'+') => (1., &s[1..]),
        _ => (1., s),
    };
    let end = digits.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return f64::NAN;
    }
    sign * digits[..end].parse::<f64>().expect("decimal digits")
}
