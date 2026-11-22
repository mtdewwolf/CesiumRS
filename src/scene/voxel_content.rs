//! CesiumJS `VoxelContent`: metadata-array and glTF attribute loading for voxel tiles.
use super::{
    model_components as mc, FrameState, MetadataComponentType, MetadataType, StructuralMetadata,
};
use crate::{
    core::{ComponentDatatype, RuntimeError, TypedArray},
    renderer::Gl,
};

/// Provider metadata layout expected by `VoxelContent::process_attributes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoxelAttributeSpec {
    pub class_name: String,
    pub names: Vec<String>,
    pub types: Vec<MetadataType>,
    pub component_types: Vec<MetadataComponentType>,
}

#[derive(Clone, Debug)]
pub struct VoxelContent {
    metadata: Vec<TypedArray>,
    ready: bool,
    destroyed: bool,
}

impl VoxelContent {
    pub fn from_metadata_array(metadata: Vec<TypedArray>) -> Self {
        Self { metadata, ready: false, destroyed: false }
    }

    /// Extracts and reorders typed voxel metadata arrays from loaded glTF components
    /// (`EXT_structural_metadata` property attributes + primitive vertex attributes),
    /// matching `VoxelContent.fromGltf` / `processAttributes` in CesiumJS.
    pub fn from_gltf_components<G: Gl>(
        components: &mc::Components<G>,
        structural_metadata: &StructuralMetadata<G>,
        spec: &VoxelAttributeSpec,
    ) -> Result<Self, RuntimeError> {
        let metadata = process_gltf_components(components, structural_metadata, spec)?;
        Ok(Self { metadata, ready: false, destroyed: false })
    }

    pub fn metadata(&self) -> &[TypedArray] {
        &self.metadata
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("VoxelContent was destroyed"));
        }
        if !self.ready {
            frame.after_render.push(Box::new(|| Ok(true)));
            self.ready = true;
        }
        Ok(())
    }

    pub fn destroy(&mut self) {
        self.metadata.clear();
        self.destroyed = true;
        self.ready = false;
    }
}

/// Reorders and views glTF primitive attributes according to `EXT_structural_metadata`
/// property attribute bindings, matching CesiumJS `processAttributes`.
pub fn process_gltf_components<G: Gl>(
    components: &mc::Components<G>,
    structural_metadata: &StructuralMetadata<G>,
    spec: &VoxelAttributeSpec,
) -> Result<Vec<TypedArray>, RuntimeError> {
    let scene = components
        .scene
        .as_ref()
        .ok_or_else(|| RuntimeError::new("VoxelContent glTF is missing a scene"))?;
    let node_index = *scene
        .nodes
        .first()
        .ok_or_else(|| RuntimeError::new("VoxelContent glTF scene has no nodes"))?;
    let node = components
        .nodes
        .get(node_index)
        .ok_or_else(|| RuntimeError::new("VoxelContent glTF node index out of bounds"))?;
    let primitive = node
        .primitives
        .first()
        .ok_or_else(|| RuntimeError::new("VoxelContent glTF node has no primitives"))?;
    process_attributes(&primitive.attributes, structural_metadata, spec)
}

/// Processes glTF attributes into the provider's property order.
pub fn process_attributes<G: Gl>(
    attributes: &[mc::Attribute<G>],
    structural_metadata: &StructuralMetadata<G>,
    spec: &VoxelAttributeSpec,
) -> Result<Vec<TypedArray>, RuntimeError> {
    let property_attribute = structural_metadata
        .property_attributes()
        .iter()
        .find(|p| p.class().id == spec.class_name)
        .ok_or_else(|| RuntimeError::new(format!("No property attribute found for class '{}'", spec.class_name)))?;

    let mut data: Vec<Option<TypedArray>> = vec![None; spec.names.len()];
    for (i, slot) in data.iter_mut().enumerate() {
        let Some(prop) = property_attribute.get_property(&spec.names[i]) else {
            continue;
        };
        let Some(attr_name) = prop.attribute().and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(attribute) = attributes.iter().find(|a| a.name.as_deref() == Some(attr_name)) else {
            continue;
        };
        let Some(typed_array) = &attribute.typed_array else {
            return Err(RuntimeError::new(format!("Attribute '{attr_name}' was not loaded as a TypedArray")));
        };
        let component_datatype = spec.component_types[i]
            .to_component_datatype()
            .ok_or_else(|| RuntimeError::new("Unsupported voxel MetadataComponentType"))?;
        let component_count = spec.types[i].get_component_count();
        let count = attribute.count.unwrap_or(0);
        let total_count = count * component_count;
        let bytes = typed_array_to_bytes(typed_array);
        let start = attribute.byte_offset.unwrap_or(0);
        *slot = Some(slice_typed_array(component_datatype, &bytes, start, total_count)?);
    }

    Ok(data
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| TypedArray::Uint8(Vec::new())))
        .collect())
}

fn typed_array_to_bytes(arr: &TypedArray) -> Vec<u8> {
    match arr {
        TypedArray::Int8(v) => v.iter().map(|&x| x as u8).collect(),
        TypedArray::Uint8(v) => v.clone(),
        TypedArray::Int16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Uint16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Int32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Uint32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Float32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Float64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}

fn slice_typed_array(
    datatype: ComponentDatatype,
    bytes: &[u8],
    byte_offset: usize,
    length: usize,
) -> Result<TypedArray, RuntimeError> {
    let elem_size = datatype.get_size_in_bytes();
    let end = byte_offset
        .checked_add(length.checked_mul(elem_size).ok_or_else(|| RuntimeError::new("TypedArray length overflow"))?)
        .ok_or_else(|| RuntimeError::new("TypedArray byte offset overflow"))?;
    let slice = bytes
        .get(byte_offset..end)
        .ok_or_else(|| RuntimeError::new("Attribute byte range out of bounds"))?;
    Ok(match datatype {
        ComponentDatatype::Byte => TypedArray::Int8(slice.iter().map(|&b| b as i8).collect()),
        ComponentDatatype::UnsignedByte => TypedArray::Uint8(slice.to_vec()),
        ComponentDatatype::Short => TypedArray::Int16(
            slice.as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c)).collect(),
        ),
        ComponentDatatype::UnsignedShort => TypedArray::Uint16(
            slice.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect(),
        ),
        ComponentDatatype::Int => TypedArray::Int32(
            slice.as_chunks::<4>().0.iter().map(|&c| i32::from_le_bytes(c)).collect(),
        ),
        ComponentDatatype::UnsignedInt => TypedArray::Uint32(
            slice.as_chunks::<4>().0.iter().map(|&c| u32::from_le_bytes(c)).collect(),
        ),
        ComponentDatatype::Float => TypedArray::Float32(
            slice.as_chunks::<4>().0.iter().map(|&c| f32::from_le_bytes(c)).collect(),
        ),
        ComponentDatatype::Double => TypedArray::Float64(
            slice.as_chunks::<8>().0.iter().map(|&c| f64::from_le_bytes(c)).collect(),
        ),
    })
}
