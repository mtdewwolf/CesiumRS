//! CesiumJS `Scene/Model/ModelReader`: reads attribute and index data from
//! glTF/3D Tiles models, with deinterleaving, normalization, dequantization,
//! oct-decoding, and triangle strip/fan conversion.

use crate::{
    core::{
        attribute_compression::AttributeCompression, component_datatype::ComponentDatatype,
        index_datatype::IndexDatatype, Cartesian2, Cartesian3, Cartesian4, Matrix4, PrimitiveType, RuntimeError,
        TypedArray,
    },
    renderer::Gl,
    scene::{model_components as mc, AttributeType},
};

/// Reads raw and processed geometry data from model attributes and indices.
pub struct ModelReader;

impl ModelReader {
    /// Reads and returns a typed array for the given attribute.
    ///
    /// If the attribute is contained in an interleaved buffer, marked as
    /// normalized, quantized, or oct-encoded, then it will be deinterleaved,
    /// normalization will be applied, and it will be dequantized and oct-decoded
    /// as necessary.
    pub fn read_attribute_as_typed_array<G: Gl>(attribute: &mc::Attribute<G>) -> Result<TypedArray, RuntimeError> {
        let compact = Self::read_attribute_as_raw_compact_typed_array(attribute)?;

        let normalized = attribute.normalized;
        let quantization = attribute.quantization.as_ref();
        if quantization.is_none() && !normalized {
            return Ok(compact);
        }

        let element_type = attribute.attribute_type.ok_or_else(|| RuntimeError::new("Attribute type is required"))?;
        let element_count = attribute.count.ok_or_else(|| RuntimeError::new("Attribute count is required"))?;
        let _component_type = attribute.component_datatype.unwrap_or(ComponentDatatype::Float);

        let normalized_array = if normalized {
            let components_per_element = element_type.get_number_of_components();
            match compact {
                TypedArray::Int8(ref v) => {
                    let floats = AttributeCompression::dequantize(v, components_per_element, element_count)
                        .ok_or_else(|| RuntimeError::new("Failed to normalize Int8 attribute"))?;
                    TypedArray::Float32(floats)
                }
                TypedArray::Uint8(ref v) => {
                    let floats = AttributeCompression::dequantize(v, components_per_element, element_count)
                        .ok_or_else(|| RuntimeError::new("Failed to normalize Uint8 attribute"))?;
                    TypedArray::Float32(floats)
                }
                TypedArray::Int16(ref v) => {
                    let floats = AttributeCompression::dequantize(v, components_per_element, element_count)
                        .ok_or_else(|| RuntimeError::new("Failed to normalize Int16 attribute"))?;
                    TypedArray::Float32(floats)
                }
                TypedArray::Uint16(ref v) => {
                    let floats = AttributeCompression::dequantize(v, components_per_element, element_count)
                        .ok_or_else(|| RuntimeError::new("Failed to normalize Uint16 attribute"))?;
                    TypedArray::Float32(floats)
                }
                _ => compact,
            }
        } else {
            compact
        };

        if let Some(q) = quantization {
            Self::dequantize(&normalized_array, element_count, element_type, q)
        } else {
            Ok(normalized_array)
        }
    }

    /// Reads attribute data into a compact typed array, deinterleaving from buffer
    /// if necessary, but without applying normalization or dequantization.
    pub fn read_attribute_as_raw_compact_typed_array<G: Gl>(
        attribute: &mc::Attribute<G>,
    ) -> Result<TypedArray, RuntimeError> {
        let element_type = attribute.attribute_type.ok_or_else(|| RuntimeError::new("Attribute type is required"))?;
        let element_count = attribute.count.ok_or_else(|| RuntimeError::new("Attribute count is required"))?;
        let components_per_element = element_type.get_number_of_components();
        let total_components = element_count * components_per_element;

        let component_type = if let Some(q) = &attribute.quantization {
            q.component_datatype.ok_or_else(|| RuntimeError::new("quantization.componentDatatype is required"))?
        } else {
            attribute.component_datatype.unwrap_or(ComponentDatatype::Float)
        };

        let bytes_per_component = component_type.get_size_in_bytes();
        let default_byte_stride = components_per_element * bytes_per_component;
        let byte_stride = attribute.byte_stride.unwrap_or(default_byte_stride);
        let byte_offset = attribute.byte_offset.unwrap_or(0);

        if let Some(ref ta) = attribute.typed_array {
            if byte_stride == default_byte_stride {
                let start = byte_offset / bytes_per_component;
                let end = start + total_components;
                return Self::slice_typed_array(ta, start, end);
            }
            // Deinterleave from typed_array
            return Self::deinterleave_typed_array(
                ta,
                byte_offset,
                byte_stride,
                components_per_element,
                element_count,
                component_type,
            );
        }

        if let Some(ref buffer) = attribute.buffer {
            let total_bytes_needed = if element_count == 0 {
                0
            } else {
                byte_offset + (element_count - 1) * byte_stride + default_byte_stride
            };
            let bytes = buffer.get_buffer_data(byte_offset, total_bytes_needed.saturating_sub(byte_offset))?;

            if byte_stride == default_byte_stride {
                return Self::typed_array_from_bytes(&bytes, component_type, total_components);
            }

            // Interleaved read from bytes
            let element_stride = byte_stride;
            let mut result_values = Vec::with_capacity(total_components);

            for i in 0..element_count {
                let elem_offset = i * element_stride;
                for j in 0..components_per_element {
                    let comp_offset = elem_offset + j * bytes_per_component;
                    let val = read_f64_from_bytes(&bytes, comp_offset, component_type)?;
                    result_values.push(val);
                }
            }

            return Ok(TypedArray::from_values(component_type, &result_values));
        }

        if let Some(ref constant) = attribute.constant {
            let values = math_value_to_f64_vec(constant);
            let mut result = Vec::with_capacity(total_components);
            for _ in 0..element_count {
                result.extend_from_slice(&values);
            }
            return Ok(TypedArray::from_values(component_type, &result));
        }

        Err(RuntimeError::new("Attribute has neither typedArray, buffer, nor constant value"))
    }

    /// Dequantizes the data from `quantized_typed_array` according to `quantization`.
    pub fn dequantize(
        quantized_typed_array: &TypedArray,
        element_count: usize,
        element_type: AttributeType,
        quantization: &mc::Quantization,
    ) -> Result<TypedArray, RuntimeError> {
        if quantization.oct_encoded {
            let norm_range = quantization
                .normalization_range
                .as_ref()
                .map(|v| match v {
                    mc::MathValue::Number(n) => *n,
                    _ => 255.0,
                })
                .unwrap_or(255.0);

            let mut dequantized = Self::oct_decode(quantized_typed_array, element_count, norm_range)?;
            if quantization.oct_encoded_zxy {
                Self::convert_zxy_to_xyz_in_place(&mut dequantized, element_count);
            }
            return Ok(TypedArray::Float32(dequantized));
        }

        let step_size = quantization
            .quantized_volume_step_size
            .as_ref()
            .ok_or_else(|| RuntimeError::new("quantizedVolumeStepSize is required"))?;
        let offset = quantization
            .quantized_volume_offset
            .as_ref()
            .ok_or_else(|| RuntimeError::new("quantizedVolumeOffset is required"))?;

        let floats = quantized_typed_array.to_f64_vec();

        let result = match element_type {
            AttributeType::Scalar => {
                let s = math_value_to_f64_vec(step_size);
                let o = math_value_to_f64_vec(offset);
                Self::dequantize_1d(&floats, element_count, s[0], o[0])
            }
            AttributeType::Vec2 => {
                let s = math_value_to_c2(step_size)?;
                let o = math_value_to_c2(offset)?;
                Self::dequantize_2d(&floats, element_count, s, o)
            }
            AttributeType::Vec3 => {
                let s = math_value_to_c3(step_size)?;
                let o = math_value_to_c3(offset)?;
                Self::dequantize_3d(&floats, element_count, s, o)
            }
            AttributeType::Vec4 => {
                let s = math_value_to_c4(step_size)?;
                let o = math_value_to_c4(offset)?;
                Self::dequantize_4d(&floats, element_count, s, o)
            }
            _ => {
                return Err(RuntimeError::new(format!(
                    "Element type for dequantization must be SCALAR, VEC2, VEC3, or VEC4, but is {:?}",
                    element_type
                )));
            }
        };

        Ok(TypedArray::Float32(result))
    }

    /// Oct-decodes 2-component encoded normals into 3-component unit vectors.
    pub fn oct_decode(
        quantized_typed_array: &TypedArray,
        element_count: usize,
        normalization_range: f64,
    ) -> Result<Vec<f32>, RuntimeError> {
        let vals = quantized_typed_array.to_f64_vec();
        let mut result = Vec::with_capacity(element_count * 3);
        // Components per input element could be 2 or 3
        let stride = if vals.len() >= element_count * 3 { 3 } else { 2 };
        for i in 0..element_count {
            let x = vals.get(i * stride).copied().unwrap_or(0.0);
            let y = vals.get(i * stride + 1).copied().unwrap_or(0.0);
            let decoded = AttributeCompression::oct_decode_in_range(x, y, normalization_range);
            result.push(decoded.x as f32);
            result.push(decoded.y as f32);
            result.push(decoded.z as f32);
        }
        Ok(result)
    }

    /// Swizzles consecutive triplets from (z, x, y) to (x, y, z).
    pub fn convert_zxy_to_xyz(input: &[f32], element_count: usize) -> Vec<f32> {
        let mut output = vec![0.0f32; input.len()];
        let mut offset = 0;
        for _ in 0..element_count {
            if offset + 2 < input.len() {
                let z = input[offset];
                let x = input[offset + 1];
                let y = input[offset + 2];
                output[offset] = x;
                output[offset + 1] = y;
                output[offset + 2] = z;
            }
            offset += 3;
        }
        output
    }

    /// Swizzles in place from (z, x, y) to (x, y, z).
    pub fn convert_zxy_to_xyz_in_place(data: &mut [f32], element_count: usize) {
        let mut offset = 0;
        for _ in 0..element_count {
            if offset + 2 < data.len() {
                let z = data[offset];
                let x = data[offset + 1];
                let y = data[offset + 2];
                data[offset] = x;
                data[offset + 1] = y;
                data[offset + 2] = z;
            }
            offset += 3;
        }
    }

    /// Dequantizes 1D scalar components: `output[i] = input[i] * step_size + offset`.
    pub fn dequantize_1d(quantized: &[f64], element_count: usize, step_size: f64, offset: f64) -> Vec<f32> {
        let mut out = Vec::with_capacity(element_count);
        for &q in quantized.iter().take(element_count) {
            out.push((q * step_size + offset) as f32);
        }
        out
    }

    /// Dequantizes 2D vector components.
    pub fn dequantize_2d(
        quantized: &[f64],
        element_count: usize,
        step_size: Cartesian2,
        offset: Cartesian2,
    ) -> Vec<f32> {
        let mut out = Vec::with_capacity(element_count * 2);
        for i in 0..element_count {
            let x = quantized.get(i * 2).copied().unwrap_or(0.0) * step_size.x + offset.x;
            let y = quantized.get(i * 2 + 1).copied().unwrap_or(0.0) * step_size.y + offset.y;
            out.push(x as f32);
            out.push(y as f32);
        }
        out
    }

    /// Dequantizes 3D vector components.
    pub fn dequantize_3d(
        quantized: &[f64],
        element_count: usize,
        step_size: Cartesian3,
        offset: Cartesian3,
    ) -> Vec<f32> {
        let mut out = Vec::with_capacity(element_count * 3);
        for i in 0..element_count {
            let x = quantized.get(i * 3).copied().unwrap_or(0.0) * step_size.x + offset.x;
            let y = quantized.get(i * 3 + 1).copied().unwrap_or(0.0) * step_size.y + offset.y;
            let z = quantized.get(i * 3 + 2).copied().unwrap_or(0.0) * step_size.z + offset.z;
            out.push(x as f32);
            out.push(y as f32);
            out.push(z as f32);
        }
        out
    }

    /// Dequantizes 4D vector components.
    pub fn dequantize_4d(
        quantized: &[f64],
        element_count: usize,
        step_size: Cartesian4,
        offset: Cartesian4,
    ) -> Vec<f32> {
        let mut out = Vec::with_capacity(element_count * 4);
        for i in 0..element_count {
            let x = quantized.get(i * 4).copied().unwrap_or(0.0) * step_size.x + offset.x;
            let y = quantized.get(i * 4 + 1).copied().unwrap_or(0.0) * step_size.y + offset.y;
            let z = quantized.get(i * 4 + 2).copied().unwrap_or(0.0) * step_size.z + offset.z;
            let w = quantized.get(i * 4 + 3).copied().unwrap_or(0.0) * step_size.w + offset.w;
            out.push(x as f32);
            out.push(y as f32);
            out.push(z as f32);
            out.push(w as f32);
        }
        out
    }

    /// Transforms consecutive 3D points by a 4x4 transformation matrix.
    pub fn transform_3d(input: &[f32], matrix: &Matrix4) -> Vec<f32> {
        let element_count = input.len() / 3;
        let mut result = Vec::with_capacity(input.len());
        for i in 0..element_count {
            let p = Cartesian3::new(input[i * 3] as f64, input[i * 3 + 1] as f64, input[i * 3 + 2] as f64);
            let transformed = matrix.multiply_by_point(p);
            result.push(transformed.x as f32);
            result.push(transformed.y as f32);
            result.push(transformed.z as f32);
        }
        result
    }

    /// Reads index data from `primitive_indices` into a typed array.
    pub fn read_indices_as_typed_array<G: Gl>(primitive_indices: &mc::Indices<G>) -> Result<TypedArray, RuntimeError> {
        if let Some(ref indices) = primitive_indices.typed_array {
            return Ok(indices.clone());
        }
        let buffer = primitive_indices
            .buffer
            .as_ref()
            .ok_or_else(|| RuntimeError::new("primitiveIndices has neither typedArray nor buffer"))?;
        let count = primitive_indices.count.ok_or_else(|| RuntimeError::new("primitiveIndices.count is required"))?;
        let datatype = primitive_indices.index_datatype.unwrap_or(IndexDatatype::UnsignedShort);

        let comp_dt = match datatype {
            IndexDatatype::UnsignedByte => ComponentDatatype::UnsignedByte,
            IndexDatatype::UnsignedShort => ComponentDatatype::UnsignedShort,
            IndexDatatype::UnsignedInt => ComponentDatatype::UnsignedInt,
        };

        let bytes = buffer.get_buffer_data(0, count * datatype.get_size_in_bytes())?;
        Self::typed_array_from_bytes(&bytes, comp_dt, count)
    }

    /// Reads indices from primitive indices and converts `TriangleStrip` or
    /// `TriangleFan` into `Triangles` if needed.
    pub fn read_indices_as_triangle_indices_typed_array<G: Gl>(
        primitive_indices: &mc::Indices<G>,
        primitive_type: PrimitiveType,
    ) -> Result<TypedArray, RuntimeError> {
        let original_indices = Self::read_indices_as_typed_array(primitive_indices)?;
        match primitive_type {
            PrimitiveType::Triangles => Ok(original_indices),
            PrimitiveType::TriangleStrip => Self::convert_triangle_strip_to_triangle_indices(&original_indices),
            PrimitiveType::TriangleFan => Self::convert_triangle_fan_to_triangle_indices(&original_indices),
            _ => Err(RuntimeError::new(format!(
                "The primitiveType must be TRIANGLES, TRIANGLE_STRIP, or TRIANGLE_FAN, but is {:?}",
                primitive_type
            ))),
        }
    }

    /// Converts triangle strip indices to triangle indices.
    pub fn convert_triangle_strip_to_triangle_indices(indices: &TypedArray) -> Result<TypedArray, RuntimeError> {
        let len = indices.len();
        if len < 3 {
            return Ok(TypedArray::zeros(indices.component_datatype(), 0));
        }
        let triangle_count = len - 2;
        let mut triangle_indices = Vec::with_capacity(triangle_count * 3);
        let vals = indices.to_f64_vec();
        for i in 0..triangle_count {
            if i % 2 == 1 {
                triangle_indices.push(vals[i]);
                triangle_indices.push(vals[i + 2]);
                triangle_indices.push(vals[i + 1]);
            } else {
                triangle_indices.push(vals[i]);
                triangle_indices.push(vals[i + 1]);
                triangle_indices.push(vals[i + 2]);
            }
        }
        Ok(TypedArray::from_values(indices.component_datatype(), &triangle_indices))
    }

    /// Converts triangle fan indices to triangle indices.
    ///
    /// Upstream CesiumJS 23.0.0 uses consecutive indexing `[i+0, i+1, i+2]`.
    pub fn convert_triangle_fan_to_triangle_indices(indices: &TypedArray) -> Result<TypedArray, RuntimeError> {
        let len = indices.len();
        if len < 3 {
            return Ok(TypedArray::zeros(indices.component_datatype(), 0));
        }
        let triangle_count = len - 2;
        let mut triangle_indices = Vec::with_capacity(triangle_count * 3);
        let vals = indices.to_f64_vec();
        for i in 0..triangle_count {
            triangle_indices.push(vals[i]);
            triangle_indices.push(vals[i + 1]);
            triangle_indices.push(vals[i + 2]);
        }
        Ok(TypedArray::from_values(indices.component_datatype(), &triangle_indices))
    }

    /// Allocates an empty typed array for the given index data type.
    pub fn create_index_typed_array(index_datatype: IndexDatatype, size: usize) -> Result<TypedArray, RuntimeError> {
        match index_datatype {
            IndexDatatype::UnsignedByte => Ok(TypedArray::Uint8(vec![0; size])),
            IndexDatatype::UnsignedShort => Ok(TypedArray::Uint16(vec![0; size])),
            IndexDatatype::UnsignedInt => Ok(TypedArray::Uint32(vec![0; size])),
        }
    }

    fn slice_typed_array(ta: &TypedArray, start: usize, end: usize) -> Result<TypedArray, RuntimeError> {
        match ta {
            TypedArray::Int8(v) => Ok(TypedArray::Int8(v.get(start..end).unwrap_or(&[]).to_vec())),
            TypedArray::Uint8(v) => Ok(TypedArray::Uint8(v.get(start..end).unwrap_or(&[]).to_vec())),
            TypedArray::Int16(v) => Ok(TypedArray::Int16(v.get(start..end).unwrap_or(&[]).to_vec())),
            TypedArray::Uint16(v) => Ok(TypedArray::Uint16(v.get(start..end).unwrap_or(&[]).to_vec())),
            TypedArray::Int32(v) => Ok(TypedArray::Int32(v.get(start..end).unwrap_or(&[]).to_vec())),
            TypedArray::Uint32(v) => Ok(TypedArray::Uint32(v.get(start..end).unwrap_or(&[]).to_vec())),
            TypedArray::Float32(v) => Ok(TypedArray::Float32(v.get(start..end).unwrap_or(&[]).to_vec())),
            TypedArray::Float64(v) => Ok(TypedArray::Float64(v.get(start..end).unwrap_or(&[]).to_vec())),
        }
    }

    fn deinterleave_typed_array(
        ta: &TypedArray,
        byte_offset: usize,
        byte_stride: usize,
        components_per_element: usize,
        element_count: usize,
        component_type: ComponentDatatype,
    ) -> Result<TypedArray, RuntimeError> {
        let bytes = ta.to_le_bytes();
        let bytes_per_comp = component_type.get_size_in_bytes();
        let mut values = Vec::with_capacity(element_count * components_per_element);
        for i in 0..element_count {
            let elem_offset = byte_offset + i * byte_stride;
            for j in 0..components_per_element {
                let comp_offset = elem_offset + j * bytes_per_comp;
                values.push(read_f64_from_bytes(&bytes, comp_offset, component_type)?);
            }
        }
        Ok(TypedArray::from_values(component_type, &values))
    }

    fn typed_array_from_bytes(
        bytes: &[u8],
        component_type: ComponentDatatype,
        total_components: usize,
    ) -> Result<TypedArray, RuntimeError> {
        let bytes_per_comp = component_type.get_size_in_bytes();
        let mut values = Vec::with_capacity(total_components);
        for i in 0..total_components {
            let offset = i * bytes_per_comp;
            values.push(read_f64_from_bytes(bytes, offset, component_type)?);
        }
        Ok(TypedArray::from_values(component_type, &values))
    }
}

fn math_value_to_f64_vec(v: &mc::MathValue) -> Vec<f64> {
    match v {
        mc::MathValue::Number(x) => vec![*x],
        mc::MathValue::Cartesian2(c) => vec![c.x, c.y],
        mc::MathValue::Cartesian3(c) => vec![c.x, c.y, c.z],
        mc::MathValue::Cartesian4(c) => vec![c.x, c.y, c.z, c.w],
        mc::MathValue::Matrix2(m) => m.0.to_vec(),
        mc::MathValue::Matrix3(m) => m.0.to_vec(),
        mc::MathValue::Matrix4(m) => m.0.to_vec(),
        mc::MathValue::Quaternion(q) => vec![q.x, q.y, q.z, q.w],
    }
}

fn math_value_to_c2(v: &mc::MathValue) -> Result<Cartesian2, RuntimeError> {
    match v {
        mc::MathValue::Cartesian2(c) => Ok(*c),
        mc::MathValue::Number(n) => Ok(Cartesian2::new(*n, *n)),
        _ => Err(RuntimeError::new("Expected Cartesian2")),
    }
}

fn math_value_to_c3(v: &mc::MathValue) -> Result<Cartesian3, RuntimeError> {
    match v {
        mc::MathValue::Cartesian3(c) => Ok(*c),
        mc::MathValue::Number(n) => Ok(Cartesian3::new(*n, *n, *n)),
        _ => Err(RuntimeError::new("Expected Cartesian3")),
    }
}

fn math_value_to_c4(v: &mc::MathValue) -> Result<Cartesian4, RuntimeError> {
    match v {
        mc::MathValue::Cartesian4(c) => Ok(*c),
        mc::MathValue::Number(n) => Ok(Cartesian4::new(*n, *n, *n, *n)),
        _ => Err(RuntimeError::new("Expected Cartesian4")),
    }
}

fn read_f64_from_bytes(bytes: &[u8], offset: usize, dt: ComponentDatatype) -> Result<f64, RuntimeError> {
    let size = dt.get_size_in_bytes();
    let slice = bytes
        .get(offset..offset + size)
        .ok_or_else(|| RuntimeError::new("Byte offset out of bounds in buffer data"))?;
    match dt {
        ComponentDatatype::Byte => Ok(slice[0] as i8 as f64),
        ComponentDatatype::UnsignedByte => Ok(slice[0] as f64),
        ComponentDatatype::Short => {
            let arr = [slice[0], slice[1]];
            Ok(i16::from_le_bytes(arr) as f64)
        }
        ComponentDatatype::UnsignedShort => {
            let arr = [slice[0], slice[1]];
            Ok(u16::from_le_bytes(arr) as f64)
        }
        ComponentDatatype::Int => {
            let arr = [slice[0], slice[1], slice[2], slice[3]];
            Ok(i32::from_le_bytes(arr) as f64)
        }
        ComponentDatatype::UnsignedInt => {
            let arr = [slice[0], slice[1], slice[2], slice[3]];
            Ok(u32::from_le_bytes(arr) as f64)
        }
        ComponentDatatype::Float => {
            let arr = [slice[0], slice[1], slice[2], slice[3]];
            Ok(f32::from_le_bytes(arr) as f64)
        }
        ComponentDatatype::Double => {
            let arr = [slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7]];
            Ok(f64::from_le_bytes(arr))
        }
    }
}
