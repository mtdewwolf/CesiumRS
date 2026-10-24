//! glTF buffer views, including EXT_meshopt_compression, with shared byte views.
use super::{
    gltf_json_loader::LoadedGltfJson, gltf_pipeline::BufferSource, resource_loader::ManagedLoader, BufferLoader,
    ResourceLoadFuture, ResourceLoader, ResourceLoaderState,
};
use crate::core::{resource::DerivedResourceOptions, Resource, ResourceClient, RuntimeError};
use futures_util::FutureExt;
use serde_json::Value;
use std::{cell::RefCell, rc::Rc, sync::Arc};
#[derive(Clone)]
struct Meshopt {
    count: usize,
    stride: usize,
    mode: String,
    filter: String,
}
pub struct GltfBufferViewLoader {
    data: ManagedLoader<BufferSource>,
    child: Rc<RefCell<Option<Rc<BufferLoader>>>>,
    offset: usize,
    length: usize,
    meshopt: Option<Meshopt>,
}
fn integer(value: &Value, key: &str, default: Option<usize>) -> Result<usize, RuntimeError> {
    let Some(value) = value.get(key).filter(|v| !v.is_null()) else {
        return default.ok_or_else(|| RuntimeError::new(format!("Invalid buffer view {key}")));
    };
    value
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| RuntimeError::new(format!("Invalid buffer view {key}")))
}
impl GltfBufferViewLoader {
    pub fn new(
        gltf: Arc<LoadedGltfJson>,
        id: usize,
        base: Resource,
        client: ResourceClient,
        key: Option<String>,
    ) -> Result<Self, RuntimeError> {
        let view = gltf.json["bufferViews"].get(id).ok_or_else(|| RuntimeError::new("Invalid buffer view ID"))?;
        let layout = view.pointer("/extensions/EXT_meshopt_compression").filter(|v| !v.is_null()).unwrap_or(view);
        let buffer_id = integer(layout, "buffer", None)?;
        let buffer = gltf.json["buffers"].get(buffer_id).ok_or_else(|| RuntimeError::new("Invalid buffer ID"))?;
        let bytes = gltf.buffer_sources.get(&buffer_id).cloned();
        let resource = buffer["uri"]
            .as_str()
            .filter(|_| bytes.is_none())
            .map(|uri| {
                base.get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
                    .ok_or_else(|| RuntimeError::new("Invalid buffer URI"))
            })
            .transpose()?;
        let loader = Rc::new(BufferLoader::new(bytes, resource, Some(client), None)?);
        Self::from_buffer_loader(view, loader, key)
    }
    /// Hosts with a resource cache can supply a shared BufferLoader lease.
    /// Unloading releases this loader's lease; other views keep theirs alive.
    pub fn from_buffer_loader(
        view: &Value,
        loader: Rc<BufferLoader>,
        key: Option<String>,
    ) -> Result<Self, RuntimeError> {
        let ext = view.pointer("/extensions/EXT_meshopt_compression").filter(|v| !v.is_null());
        let layout = ext.unwrap_or(view);
        let meshopt = ext
            .map(|ext| {
                Ok::<_, RuntimeError>(Meshopt {
                    count: integer(ext, "count", None)?,
                    stride: integer(ext, "byteStride", None)?,
                    mode: ext["mode"].as_str().unwrap_or("").into(),
                    filter: ext["filter"].as_str().unwrap_or("NONE").into(),
                })
            })
            .transpose()?;
        Ok(Self {
            data: ManagedLoader::new(key, None),
            child: Rc::new(RefCell::new(Some(loader))),
            offset: integer(layout, "byteOffset", Some(0))?,
            length: integer(layout, "byteLength", None)?,
            meshopt,
        })
    }
    pub fn typed_array(&self) -> Option<BufferSource> {
        self.data.value()
    }
}
impl ResourceLoader for GltfBufferViewLoader {
    fn cache_key(&self) -> Option<&str> {
        self.data.key.as_deref()
    }
    fn state(&self) -> ResourceLoaderState {
        self.data.state()
    }
    fn is_destroyed(&self) -> bool {
        self.data.is_destroyed()
    }
    fn unload(&self) {
        self.child.borrow_mut().take();
        self.data.unload();
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
    fn load(&self) -> ResourceLoadFuture {
        let child = self.child.borrow().clone();
        let (offset, length, meshopt) = (self.offset, self.length, self.meshopt.clone());
        let lease = Rc::downgrade(&self.child);
        self.data.load("Failed to load buffer view".into(), move || {
            let request = child.as_ref().map(|child| child.load());
            let weak = child.as_ref().map(Rc::downgrade);
            async move {
                let result = async move {
                    request.ok_or_else(|| RuntimeError::new("Buffer loader was unloaded"))?.await?;
                    let source = weak
                        .and_then(|weak| weak.upgrade())
                        .and_then(|child| child.typed_array())
                        .ok_or_else(|| RuntimeError::new("Buffer loader was unloaded"))?;
                    let start = source
                        .byte_offset
                        .checked_add(offset)
                        .ok_or_else(|| RuntimeError::new("Buffer view offset overflow"))?;
                    let end =
                        start.checked_add(length).ok_or_else(|| RuntimeError::new("Buffer view length overflow"))?;
                    if end > source.buffer.len() {
                        return Err(RuntimeError::new(format!("Invalid typed array length: {length}")));
                    }
                    let view = BufferSource::view(source.buffer, start, length);
                    match meshopt {
                        Some(meshopt) => decode_meshopt(view.bytes(), meshopt).map(BufferSource::new),
                        None => Ok(view),
                    }
                }
                .await;
                if let Some(lease) = lease.upgrade() {
                    lease.borrow_mut().take();
                }
                result
            }
            .boxed_local()
        })
    }
}
fn decode_meshopt(bytes: &[u8], m: Meshopt) -> Result<Vec<u8>, RuntimeError> {
    let size = m.count.checked_mul(m.stride).ok_or_else(|| RuntimeError::new("Meshopt output size overflow"))?;
    let mut output = match m.mode.as_str() {
        "ATTRIBUTES" => crate::core::decode_meshopt_vertices(bytes, m.count, m.stride)?,
        "TRIANGLES" | "INDICES" => {
            if m.mode == "TRIANGLES" && !m.count.is_multiple_of(3) {
                return Err(RuntimeError::new("Invalid meshopt triangle count"));
            }
            if !matches!(m.stride, 2 | 4) {
                return Err(RuntimeError::new("Invalid meshopt index stride"));
            }
            let mut values = vec![0u32; m.count];
            let result = if m.mode == "TRIANGLES" {
                meshopt_rs::index::buffer::decode_index_buffer(&mut values, bytes)
            } else {
                meshopt_rs::index::sequence::decode_index_sequence(&mut values, bytes)
            };
            result.map_err(|e| RuntimeError::new(format!("Invalid meshopt indices: {e:?}")))?;
            if m.stride == 2 {
                values.into_iter().flat_map(|v| (v as u16).to_le_bytes()).collect()
            } else {
                values.into_iter().flat_map(u32::to_le_bytes).collect()
            }
        }
        _ => return Err(RuntimeError::new("Invalid meshopt mode")),
    };
    if output.len() != size {
        return Err(RuntimeError::new("Invalid meshopt output size"));
    }
    use meshopt_rs::vertex::filter;
    match (m.filter.as_str(), m.stride) {
        ("NONE", _) => {}
        ("OCTAHEDRAL", 4) => {
            let mut values =
                output.chunks_exact(4).map(|v| <[u8; 4]>::try_from(v).expect("stride")).collect::<Vec<_>>();
            filter::decode_filter_oct_8(&mut values);
            output = values.into_iter().flatten().collect();
        }
        ("OCTAHEDRAL", 8) | ("QUATERNION", 8) => {
            let mut values = output
                .chunks_exact(8)
                .map(|v| std::array::from_fn(|i| u16::from_le_bytes([v[2 * i], v[2 * i + 1]])))
                .collect::<Vec<[u16; 4]>>();
            if m.filter == "OCTAHEDRAL" {
                filter::decode_filter_oct_16(&mut values);
            } else {
                // meshopt-rs 0.1.2 treats signed quaternion components as
                // unsigned. Keep the pinned codec's signed interpretation.
                for q in &mut values {
                    let input = q.map(|v| v as i16);
                    let scale = std::f32::consts::FRAC_1_SQRT_2 / (input[3] | 3) as f32;
                    let x = input[0] as f32 * scale;
                    let y = input[1] as f32 * scale;
                    let z = input[2] as f32 * scale;
                    let w = (1. - x * x - y * y - z * z).max(0.).sqrt();
                    let c = (input[3] & 3) as usize;
                    for (offset, v) in [(1, x), (2, y), (3, z), (0, w)] {
                        q[(c + offset) & 3] = ((v * 32767.).round() as i16) as u16;
                    }
                }
            }
            output = values.into_iter().flatten().flat_map(u16::to_le_bytes).collect();
        }
        ("EXPONENTIAL", stride) if stride % 4 == 0 => {
            let mut values =
                output.chunks_exact(4).map(|v| u32::from_le_bytes(v.try_into().expect("word"))).collect::<Vec<_>>();
            filter::decode_filter_exp(&mut values);
            output = values.into_iter().flat_map(u32::to_le_bytes).collect();
        }
        _ => return Err(RuntimeError::new("Invalid meshopt filter or stride")),
    }
    Ok(output)
}
