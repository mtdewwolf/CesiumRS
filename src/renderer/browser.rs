//! Concrete WebGL host, including WebGL1 extension dispatch and VAO fallback.
use super::context::*;
use crate::core::RuntimeError;
use js_sys::{Array, Float32Array, Function, Int32Array, Reflect, Uint8Array};
use std::cell::{Cell, RefCell};
use wasm_bindgen::{JsCast, JsValue};
pub struct BrowserGl {
    gl: JsValue,
    webgl2: bool,
    vaos: RefCell<Vec<(JsValue, VirtualVao)>>,
    current_vao: Cell<usize>,
}
#[derive(Default)]
struct VirtualVao {
    calls: Vec<(String, Vec<GlValue<JsValue>>)>,
    element: Option<JsValue>,
}
impl BrowserGl {
    pub fn new(gl: JsValue) -> Result<Self> {
        ensure(
            Reflect::get(&gl, &"createVertexArray".into()).map_err(error)?.is_function(),
            "A WebGL2 context is required",
        )?;
        Ok(Self { gl, webgl2: true, vaos: RefCell::new(vec![]), current_vao: Cell::new(0) })
    }
    pub fn new_webgl1(gl: JsValue) -> Result<Self> {
        ensure(Reflect::get(&gl, &"getParameter".into()).map_err(error)?.is_function(), "A WebGL context is required")?;
        let host = Self { gl, webgl2: false, vaos: RefCell::new(vec![]), current_vao: Cell::new(0) };
        if host.extension("OES_vertex_array_object")?.is_null() {
            host.vaos.borrow_mut().push((JsValue::NULL, VirtualVao::default()));
        }
        Ok(host)
    }
    fn extension(&self, name: &str) -> Result<JsValue> {
        let function =
            Reflect::get(&self.gl, &"getExtension".into()).map_err(error)?.dyn_into::<Function>().map_err(error)?;
        function.call1(&self.gl, &name.into()).map_err(error)
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
        // Without OES_vertex_array_object, retain each layout and replay it on
        // bind. Buffer contents remain in GL; only attribute state is virtual.
        if !self.vaos.borrow().is_empty() {
            match method {
                "createVertexArray" => {
                    let id: JsValue = js_sys::Object::new().into();
                    let mut vaos = self.vaos.borrow_mut();
                    if let Some(slot) = vaos.iter_mut().skip(1).find(|(key, _)| key.is_undefined()) {
                        *slot = (id.clone(), VirtualVao::default());
                    } else {
                        vaos.push((id.clone(), VirtualVao::default()));
                    }
                    return Ok(GlValue::Object(id));
                }
                "deleteVertexArray" => {
                    if let Some(GlValue::Object(id)) = args.first() {
                        let index = self.vaos.borrow().iter().position(|(key, _)| js_sys::Object::is(key, id));
                        if let Some(index) = index.filter(|i| *i > 0) {
                            if self.current_vao.get() == index {
                                self.invoke("bindVertexArray", &[GlValue::Null])?;
                            }
                            self.vaos.borrow_mut()[index] = (JsValue::UNDEFINED, VirtualVao::default());
                        }
                    }
                    return Ok(GlValue::Null);
                }
                "bindVertexArray" => {
                    let id = match args.first() {
                        Some(GlValue::Object(id)) => id.clone(),
                        _ => JsValue::NULL,
                    };
                    let index = self
                        .vaos
                        .borrow()
                        .iter()
                        .position(|(key, _)| js_sys::Object::is(key, &id))
                        .ok_or_else(|| RuntimeError::new("Unknown vertex array"))?;
                    self.current_vao.set(index);
                    let vaos = self.vaos.borrow();
                    let (_, vao) = &vaos[index];
                    let get = Reflect::get(&self.gl, &"getParameter".into())
                        .map_err(error)?
                        .dyn_into::<Function>()
                        .map_err(error)?;
                    let max = get.call1(&self.gl, &34921u32.into()).map_err(error)?.as_f64().unwrap_or(8.) as u32;
                    let disable = Reflect::get(&self.gl, &"disableVertexAttribArray".into())
                        .map_err(error)?
                        .dyn_into::<Function>()
                        .map_err(error)?;
                    for i in 0..max {
                        disable.call1(&self.gl, &i.into()).map_err(error)?;
                    }
                    for (name, values) in &vao.calls {
                        self.invoke_raw(name, values)?;
                    }
                    self.invoke_raw(
                        "bindBuffer",
                        &[34963u32.into(), vao.element.clone().map(GlValue::Object).unwrap_or(GlValue::Null)],
                    )?;
                    return Ok(GlValue::Null);
                }
                "bindBuffer" if matches!(args.first(),Some(GlValue::Number(n)) if *n==34963.) => {
                    self.vaos.borrow_mut()[self.current_vao.get()].1.element = match args.get(1) {
                        Some(GlValue::Object(o)) => Some(o.clone()),
                        _ => None,
                    };
                }
                "vertexAttribPointer"
                | "enableVertexAttribArray"
                | "disableVertexAttribArray"
                | "vertexAttribDivisor" => {
                    let mut vaos = self.vaos.borrow_mut();
                    let vao = &mut vaos[self.current_vao.get()].1;
                    if method == "vertexAttribPointer" {
                        // Attribute pointers capture the ARRAY_BUFFER binding.
                        let get = Reflect::get(&self.gl, &"getParameter".into())
                            .map_err(error)?
                            .dyn_into::<Function>()
                            .map_err(error)?;
                        let buffer = get.call1(&self.gl, &34964u32.into()).map_err(error)?;
                        vao.calls.push(("bindBuffer".into(), vec![34962u32.into(), GlValue::Object(buffer)]));
                    }
                    vao.calls.push((method.into(), args.to_vec()));
                }
                _ => {}
            }
        }
        self.invoke_raw(method, args)
    }
}
impl BrowserGl {
    fn invoke_raw(&self, method: &str, args: &[GlValue<JsValue>]) -> Result<GlValue<JsValue>> {
        let mut receiver = self.gl.clone();
        let mut entry = method.to_string();
        if !self.webgl2 {
            let extension = match method {
                "createVertexArray" | "bindVertexArray" | "deleteVertexArray" => {
                    Some(("OES_vertex_array_object", "OES"))
                }
                "vertexAttribDivisor" | "drawArraysInstanced" | "drawElementsInstanced" => {
                    Some(("ANGLE_instanced_arrays", "ANGLE"))
                }
                "drawBuffers" => Some(("WEBGL_draw_buffers", "WEBGL")),
                _ => None,
            };
            if let Some((name, suffix)) = extension {
                receiver = self.extension(name)?;
                if receiver.is_null()
                    && method == "vertexAttribDivisor"
                    && matches!(args.get(1),Some(GlValue::Number(n)) if *n==0.)
                {
                    return Ok(GlValue::Null);
                }
                ensure(!receiver.is_null(), "WebGL extension entrypoint unavailable")?;
                entry.push_str(suffix);
            }
        }
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
        let f = Reflect::get(&receiver, &entry.into()).map_err(error)?.dyn_into::<Function>().map_err(error)?;
        let result = f.apply(&receiver, &array).map_err(error)?;
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
