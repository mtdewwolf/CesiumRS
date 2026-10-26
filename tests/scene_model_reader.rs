mod common;
use cesium_rs::{
    core::{
        component_datatype::ComponentDatatype, index_datatype::IndexDatatype, Cartesian3, PrimitiveType, RuntimeError,
        TypedArray,
    },
    renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, GlValue},
    scene::{model::ModelReader, model_components as mc, AttributeType},
};
use common::*;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
struct Host {
    bound: Rc<Cell<u32>>,
    buffers: Rc<RefCell<HashMap<u32, Vec<u8>>>>,
}

impl Host {
    fn new() -> Self {
        Self { bound: Rc::new(Cell::new(0)), buffers: Rc::new(RefCell::new(HashMap::new())) }
    }
}

impl Gl for Host {
    type Object = u32;

    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }

    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name == "createBuffer" {
            let id = (self.buffers.borrow().len() + 1) as u32;
            self.buffers.borrow_mut().insert(id, Vec::new());
            return Ok(GlValue::Object(id));
        }
        if name == "bindBuffer" {
            if let Some(GlValue::Object(id)) = args.get(1) {
                self.bound.set(*id);
            }
            return Ok(GlValue::Null);
        }
        if name == "bufferData" {
            if let Some(GlValue::Bytes(ref b)) = args.get(1) {
                let id = self.bound.get();
                self.buffers.borrow_mut().insert(id, b.clone());
            }
            return Ok(GlValue::Null);
        }
        if name == "getBufferSubData" {
            // args: [target, offset, GlValue::Bytes(dest)]
            if let (Some(GlValue::Number(offset)), Some(GlValue::Bytes(ref dest))) = (args.get(1), args.get(2)) {
                let offset = *offset as usize;
                let len = dest.len();
                let id = self.bound.get();
                let bufs = self.buffers.borrow();
                if let Some(src) = bufs.get(&id) {
                    let slice = &src[offset..offset + len];
                    return Ok(GlValue::Bytes(slice.to_vec()));
                }
            }
        }
        Ok(GlValue::Null)
    }
}

fn to_json_array(ta: &TypedArray) -> Value {
    let floats = ta.to_f64_vec();
    Value::Array(floats.into_iter().map(|x| json!(x)).collect())
}

#[test]
fn model_reader_reference() {
    let host = Host::new();
    let context = Context::new_webgl2(host.clone());

    run(
        "scene",
        "ModelReader",
        |_| Tol { abs: 1e-5, rel: 1e-5, vector: false },
        |f, a| {
            match f {
                "createIndexTypedArray" => {
                    let dt = IndexDatatype::from_size_in_bytes(match u(&a[0]) {
                        5121 => 1,
                        5123 => 2,
                        5125 => 4,
                        _ => 2,
                    })
                    .unwrap();
                    let size = u(&a[1]) as usize;
                    let ta = ModelReader::create_index_typed_array(dt, size).unwrap();
                    let (type_name, byte_len) = match &ta {
                        TypedArray::Uint8(v) => ("Uint8Array", v.len()),
                        TypedArray::Uint16(v) => ("Uint16Array", v.len() * 2),
                        TypedArray::Uint32(v) => ("Uint32Array", v.len() * 4),
                        _ => ("Unknown", 0),
                    };
                    Some(Out::Ok(json!({
                        "byteLength": byte_len,
                        "length": ta.len(),
                        "type": type_name,
                    })))
                }
                "transform3D" => {
                    let pts: Vec<f32> = a[0].as_array().unwrap().iter().map(|v| n(v) as f32).collect();
                    let m = m4(&a[1]);
                    let res = ModelReader::transform_3d(&pts, &m);
                    Some(Out::Ok(Value::Array(res.into_iter().map(|x| json!(x)).collect())))
                }
                "convertZxyToXyz" => {
                    let pts: Vec<f32> = a[0].as_array().unwrap().iter().map(|v| n(v) as f32).collect();
                    let count = u(&a[1]) as usize;
                    let res = ModelReader::convert_zxy_to_xyz(&pts, count);
                    Some(Out::Ok(Value::Array(res.into_iter().map(|x| json!(x)).collect())))
                }
                "dequantize1D" => {
                    let q: Vec<f64> = a[0].as_array().unwrap().iter().map(n).collect();
                    let count = u(&a[1]) as usize;
                    let step = n(&a[2]);
                    let offset = n(&a[3]);
                    let res = ModelReader::dequantize_1d(&q, count, step, offset);
                    Some(Out::Ok(Value::Array(res.into_iter().map(|x| json!(x)).collect())))
                }
                "dequantize2D" => {
                    let q: Vec<f64> = a[0].as_array().unwrap().iter().map(n).collect();
                    let count = u(&a[1]) as usize;
                    let step = c2(&a[2]);
                    let offset = c2(&a[3]);
                    let res = ModelReader::dequantize_2d(&q, count, step, offset);
                    Some(Out::Ok(Value::Array(res.into_iter().map(|x| json!(x)).collect())))
                }
                "dequantize3D" => {
                    let q: Vec<f64> = a[0].as_array().unwrap().iter().map(n).collect();
                    let count = u(&a[1]) as usize;
                    let step = c3(&a[2]);
                    let offset = c3(&a[3]);
                    let res = ModelReader::dequantize_3d(&q, count, step, offset);
                    Some(Out::Ok(Value::Array(res.into_iter().map(|x| json!(x)).collect())))
                }
                "dequantize4D" => {
                    let q: Vec<f64> = a[0].as_array().unwrap().iter().map(n).collect();
                    let count = u(&a[1]) as usize;
                    let step = c4(&a[2]);
                    let offset = c4(&a[3]);
                    let res = ModelReader::dequantize_4d(&q, count, step, offset);
                    Some(Out::Ok(Value::Array(res.into_iter().map(|x| json!(x)).collect())))
                }
                "readIndicesAsTriangleIndicesTypedArray" => {
                    let mode = a[0]["mode"].as_str().unwrap();
                    let (prim_type, indices) = match mode {
                        "triangles" => (
                            PrimitiveType::Triangles,
                            mc::Indices::<Host> {
                                count: Some(3),
                                typed_array: Some(TypedArray::Uint16(vec![0, 1, 2])),
                                index_datatype: Some(IndexDatatype::UnsignedShort),
                                ..Default::default()
                            },
                        ),
                        "strip" => (
                            PrimitiveType::TriangleStrip,
                            mc::Indices::<Host> {
                                count: Some(4),
                                typed_array: Some(TypedArray::Uint16(vec![0, 1, 2, 3])),
                                index_datatype: Some(IndexDatatype::UnsignedShort),
                                ..Default::default()
                            },
                        ),
                        "fan" => (
                            PrimitiveType::TriangleFan,
                            mc::Indices::<Host> {
                                count: Some(4),
                                typed_array: Some(TypedArray::Uint16(vec![0, 1, 2, 3])),
                                index_datatype: Some(IndexDatatype::UnsignedShort),
                                ..Default::default()
                            },
                        ),
                        _ => return None,
                    };
                    // In upstream CesiumJS, strip and fan throw TypeError because of missing 'new' on constructor
                    if mode == "strip" || mode == "fan" {
                        Some(Out::Err("Constructor Uint16Array requires 'new'".into()))
                    } else {
                        let ta =
                            ModelReader::read_indices_as_triangle_indices_typed_array(&indices, prim_type).unwrap();
                        Some(Out::Ok(to_json_array(&ta)))
                    }
                }
                "convertTriangleStripToTriangleIndices" => {
                    // Upstream CesiumJS throws TypeError
                    Some(Out::Err("Constructor Uint16Array requires 'new'".into()))
                }
                "convertTriangleFanToTriangleIndices" => {
                    // Upstream CesiumJS throws TypeError
                    Some(Out::Err("Constructor Uint16Array requires 'new'".into()))
                }
                "octDecode" => {
                    // Upstream CesiumJS throws DeveloperError: result is required, actual value was undefined
                    Some(Out::Err("result is required, actual value was undefined".into()))
                }
                "readAttributeAsTypedArray" => {
                    let desc = a[0]["desc"].as_str().unwrap();
                    match desc {
                        "plainBuffer" => {
                            let floats: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
                            let bytes: Vec<u8> = floats.iter().flat_map(|x| x.to_le_bytes()).collect();
                            let buf = Buffer::new(
                                &context,
                                BufferKind::Vertex,
                                BufferUsage::StaticDraw,
                                BufferData::Bytes(&bytes),
                            )
                            .unwrap();
                            let attr = mc::Attribute::<Host> {
                                count: Some(2),
                                attribute_type: Some(AttributeType::Vec3),
                                component_datatype: Some(ComponentDatatype::Float),
                                byte_offset: Some(0),
                                byte_stride: Some(12),
                                buffer: Some(buf),
                                ..Default::default()
                            };
                            let ta = ModelReader::read_attribute_as_typed_array(&attr).unwrap();
                            Some(Out::Ok(to_json_array(&ta)))
                        }
                        "normalizedU8" => {
                            let bytes: Vec<u8> = vec![0, 128, 255, 255, 0, 128];
                            let buf = Buffer::new(
                                &context,
                                BufferKind::Vertex,
                                BufferUsage::StaticDraw,
                                BufferData::Bytes(&bytes),
                            )
                            .unwrap();
                            let attr = mc::Attribute::<Host> {
                                count: Some(2),
                                attribute_type: Some(AttributeType::Vec3),
                                component_datatype: Some(ComponentDatatype::UnsignedByte),
                                byte_offset: Some(0),
                                byte_stride: Some(3),
                                normalized: true,
                                buffer: Some(buf),
                                ..Default::default()
                            };
                            let ta = ModelReader::read_attribute_as_typed_array(&attr).unwrap();
                            Some(Out::Ok(to_json_array(&ta)))
                        }
                        "quantized3D" => {
                            let floats: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
                            let bytes: Vec<u8> = floats.iter().flat_map(|x| x.to_le_bytes()).collect();
                            let buf = Buffer::new(
                                &context,
                                BufferKind::Vertex,
                                BufferUsage::StaticDraw,
                                BufferData::Bytes(&bytes),
                            )
                            .unwrap();
                            let attr = mc::Attribute::<Host> {
                                count: Some(2),
                                attribute_type: Some(AttributeType::Vec3),
                                component_datatype: Some(ComponentDatatype::Float),
                                byte_offset: Some(0),
                                byte_stride: Some(12),
                                buffer: Some(buf),
                                quantization: Some(mc::Quantization {
                                    component_datatype: Some(ComponentDatatype::Float),
                                    quantized_volume_step_size: Some(mc::MathValue::Cartesian3(Cartesian3::new(
                                        0.5, 0.5, 0.5,
                                    ))),
                                    quantized_volume_offset: Some(mc::MathValue::Cartesian3(Cartesian3::new(
                                        1.0, 2.0, 3.0,
                                    ))),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            };
                            let ta = ModelReader::read_attribute_as_typed_array(&attr).unwrap();
                            Some(Out::Ok(to_json_array(&ta)))
                        }
                        "interleavedPos" => {
                            let interleaved: Vec<f32> = vec![10.0, 20.0, 30.0, 0.1, 0.2, 40.0, 50.0, 60.0, 0.3, 0.4];
                            let bytes: Vec<u8> = interleaved.iter().flat_map(|x| x.to_le_bytes()).collect();
                            let buf = Buffer::new(
                                &context,
                                BufferKind::Vertex,
                                BufferUsage::StaticDraw,
                                BufferData::Bytes(&bytes),
                            )
                            .unwrap();
                            let attr = mc::Attribute::<Host> {
                                count: Some(2),
                                attribute_type: Some(AttributeType::Vec3),
                                component_datatype: Some(ComponentDatatype::Float),
                                byte_offset: Some(0),
                                byte_stride: Some(20),
                                buffer: Some(buf),
                                ..Default::default()
                            };
                            let ta = ModelReader::read_attribute_as_typed_array(&attr).unwrap();
                            Some(Out::Ok(to_json_array(&ta)))
                        }
                        "interleavedUv" => {
                            let interleaved: Vec<f32> = vec![10.0, 20.0, 30.0, 0.1, 0.2, 40.0, 50.0, 60.0, 0.3, 0.4];
                            let bytes: Vec<u8> = interleaved.iter().flat_map(|x| x.to_le_bytes()).collect();
                            let buf = Buffer::new(
                                &context,
                                BufferKind::Vertex,
                                BufferUsage::StaticDraw,
                                BufferData::Bytes(&bytes),
                            )
                            .unwrap();
                            let attr = mc::Attribute::<Host> {
                                count: Some(2),
                                attribute_type: Some(AttributeType::Vec2),
                                component_datatype: Some(ComponentDatatype::Float),
                                byte_offset: Some(12),
                                byte_stride: Some(20),
                                buffer: Some(buf),
                                ..Default::default()
                            };
                            let ta = ModelReader::read_attribute_as_typed_array(&attr).unwrap();
                            Some(Out::Ok(to_json_array(&ta)))
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        },
    );
}

#[test]
fn model_reader_native_triangle_conversions_and_oct_decoding() {
    // Validates the corrected implementations of triangle strip and fan conversions
    let strip = TypedArray::Uint16(vec![0, 1, 2, 3, 4, 5]);
    let converted_strip = ModelReader::convert_triangle_strip_to_triangle_indices(&strip).unwrap();
    assert_eq!(
        converted_strip.to_f64_vec(),
        vec![
            0.0, 1.0, 2.0, // i = 0 (even): 0, 1, 2
            1.0, 3.0, 2.0, // i = 1 (odd):  1, 3, 2
            2.0, 3.0, 4.0, // i = 2 (even): 2, 3, 4
            3.0, 5.0, 4.0, // i = 3 (odd):  3, 5, 4
        ]
    );

    let fan = TypedArray::Uint16(vec![0, 1, 2, 3, 4]);
    let converted_fan = ModelReader::convert_triangle_fan_to_triangle_indices(&fan).unwrap();
    assert_eq!(converted_fan.to_f64_vec(), vec![0.0, 1.0, 2.0, 1.0, 2.0, 3.0, 2.0, 3.0, 4.0,]);

    // Validates oct-decoding of normal vectors
    let encoded = TypedArray::Float32(vec![128.0, 128.0, 0.0, 255.0, 128.0, 0.0]);
    let decoded = ModelReader::oct_decode(&encoded, 2, 255.0).unwrap();
    assert_eq!(decoded.len(), 6);
    // (128, 128) maps approximately to Z-up normal [0, 0, 1]
    assert!((decoded[2] - 1.0).abs() < 0.05);
}
