//! Concrete WebGL2 host; no renderer algorithms are delegated to JavaScript.
use super::context::*;
use crate::core::RuntimeError;
use js_sys::{Array, Float32Array, Function, Int32Array, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
pub struct BrowserGl {
    gl: JsValue,
}
impl BrowserGl {
    pub fn new(gl: JsValue) -> Result<Self> {
        ensure(
            Reflect::get(&gl, &"createVertexArray".into()).map_err(error)?.is_function(),
            "A WebGL2 context is required",
        )?;
        Ok(Self { gl })
    }
    pub fn raw_context(&self) -> &JsValue {
        &self.gl
    }
}
fn error(e: JsValue) -> RuntimeError {
    RuntimeError::new(format!("WebGL host exception: {e:?}"))
}
impl Gl for BrowserGl {
    type Object = JsValue;
    fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
        let get = |name: &str| -> Result<u32> {
            Reflect::get(&self.gl, &name.into())
                .map_err(error)?
                .as_f64()
                .map(|n| n as u32)
                .ok_or_else(|| RuntimeError::new("Missing drawing buffer dimensions"))
        };
        Ok([get("drawingBufferWidth")?, get("drawingBufferHeight")?])
    }
    fn invoke(&self, method: &str, args: &[GlValue<JsValue>]) -> Result<GlValue<JsValue>> {
        let array = Array::new();
        for a in args {
            array.push(&match a {
                GlValue::Null => JsValue::NULL,
                GlValue::Number(n) => (*n).into(),
                GlValue::Bool(b) => (*b).into(),
                GlValue::Bools(b) => b.iter().map(|v| JsValue::from_bool(*v)).collect::<Array>().into(),
                GlValue::String(s) => s.into(),
                GlValue::Bytes(b) => Uint8Array::from(b.as_slice()).into(),
                GlValue::Floats(b) => Float32Array::from(b.as_slice()).into(),
                GlValue::Ints(b) => Int32Array::from(b.as_slice()).into(),
                GlValue::UInts(b) => js_sys::Uint32Array::from(b.as_slice()).into(),
                GlValue::Numbers(b) => js_sys::Float64Array::from(b.as_slice()).into(),
                GlValue::Record(v) => js_sys::JSON::parse(&v.to_string()).map_err(error)?,
                GlValue::Object(o) => o.clone(),
                GlValue::Pixels { bytes, datatype } => {
                    let array = Uint8Array::from(bytes.as_slice());
                    match datatype.get_typed_array_type() {
                        crate::core::pixel_datatype::PixelArrayType::Uint8 => array.into(),
                        crate::core::pixel_datatype::PixelArrayType::Uint16 => {
                            js_sys::Uint16Array::new(&array.buffer()).into()
                        }
                        crate::core::pixel_datatype::PixelArrayType::Uint32 => {
                            js_sys::Uint32Array::new(&array.buffer()).into()
                        }
                        crate::core::pixel_datatype::PixelArrayType::Float32 => {
                            Float32Array::new(&array.buffer()).into()
                        }
                    }
                }
            });
        }
        let f = Reflect::get(&self.gl, &method.into()).map_err(error)?.dyn_into::<Function>().map_err(error)?;
        let result = f.apply(&self.gl, &array).map_err(error)?;
        if method == "getBufferSubData" || method == "readPixels" {
            let index = if method == "getBufferSubData" { 2 } else { 6 };
            if matches!(args.get(index), Some(GlValue::Bytes(_) | GlValue::Pixels { .. })) {
                let target = array.get(index as u32);
                let buffer = Reflect::get(&target, &"buffer".into()).map_err(error)?;
                return Ok(GlValue::Bytes(Uint8Array::new(&buffer).to_vec()));
            }
        }
        if !result.is_null()
            && !result.is_undefined()
            && matches!(method, "getActiveAttrib" | "getActiveUniform" | "getShaderPrecisionFormat")
        {
            let keys = if method == "getShaderPrecisionFormat" {
                vec!["rangeMin", "rangeMax", "precision"]
            } else {
                vec!["name", "type", "size"]
            };
            let mut values = serde_json::Map::new();
            for key in keys {
                let value = Reflect::get(&result, &key.into()).map_err(error)?;
                values.insert(
                    key.into(),
                    if let Some(s) = value.as_string() {
                        serde_json::json!(s)
                    } else {
                        serde_json::json!(value.as_f64())
                    },
                );
            }
            return Ok(GlValue::Record(values.into()));
        }
        if Array::is_array(&result) && Array::from(&result).iter().all(|v| v.as_bool().is_some()) {
            return Ok(GlValue::Bools(Array::from(&result).iter().map(|v| v.as_bool().unwrap()).collect()));
        }
        if Array::is_array(&result) || js_sys::ArrayBuffer::is_view(&result) {
            return Ok(GlValue::Numbers(Array::from(&result).iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect()));
        }
        Ok(if result.is_null() || result.is_undefined() {
            GlValue::Null
        } else if let Some(n) = result.as_f64() {
            GlValue::Number(n)
        } else if let Some(b) = result.as_bool() {
            GlValue::Bool(b)
        } else if let Some(s) = result.as_string() {
            GlValue::String(s)
        } else {
            GlValue::Object(result)
        })
    }
}
