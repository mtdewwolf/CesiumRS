//! Concrete WebGL host, including WebGL2 typed `web-sys` fast paths, cached
//! dynamic dispatch, and WebGL1 extension/VAO fallback.
use super::context::*;
use crate::core::RuntimeError;
use js_sys::{Array, Float32Array, Function, Int32Array, Reflect, Uint8Array};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    WebGl2RenderingContext as Gl2, WebGlBuffer, WebGlFramebuffer, WebGlProgram, WebGlRenderbuffer, WebGlTexture,
    WebGlUniformLocation, WebGlVertexArrayObject,
};

pub struct BrowserGl {
    gl: JsValue,
    gl2: Option<Gl2>,
    webgl2: bool,
    vaos: RefCell<Vec<(JsValue, VirtualVao)>>,
    current_vao: Cell<usize>,
    methods: RefCell<HashMap<String, (JsValue, Function)>>,
    extensions: RefCell<HashMap<String, JsValue>>,
    params: RefCell<HashMap<u32, GlValue<JsValue>>>,
    precisions: RefCell<HashMap<(u32, u32), GlValue<JsValue>>>,
}

fn is_immutable_gl_param(pname: u32) -> bool {
    matches!(
        pname,
        0x0D33 // MAX_TEXTURE_SIZE
        | 0x0D3A // MAX_VIEWPORT_DIMS
        | 0x0D56 // DEPTH_BITS
        | 0x0D57 // STENCIL_BITS
        | 0x0D50 // SUBPIXEL_BITS
        | 0x8073 // MAX_3D_TEXTURE_SIZE
        | 0x846D // ALIASED_POINT_SIZE_RANGE
        | 0x846E // ALIASED_LINE_WIDTH_RANGE
        | 0x84FF // MAX_TEXTURE_MAX_ANISOTROPY_EXT
        | 0x851C // MAX_CUBE_MAP_TEXTURE_SIZE
        | 0x8824 // MAX_DRAW_BUFFERS
        | 0x8869 // MAX_VERTEX_ATTRIBS
        | 0x8872 // MAX_TEXTURE_IMAGE_UNITS
        | 0x8B4C // MAX_VERTEX_TEXTURE_IMAGE_UNITS
        | 0x8B4D // MAX_COMBINED_TEXTURE_IMAGE_UNITS
        | 0x8CDF // MAX_COLOR_ATTACHMENTS
        | 0x8D57 // MAX_SAMPLES
        | 0x84E8 // MAX_RENDERBUFFER_SIZE
        | 0x8DFB // MAX_VERTEX_UNIFORM_VECTORS
        | 0x8DFC // MAX_VARYING_VECTORS
        | 0x8DFD // MAX_FRAGMENT_UNIFORM_VECTORS
    )
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
        let gl2 = gl.clone().dyn_into::<Gl2>().ok();
        Ok(Self {
            gl,
            gl2,
            webgl2: true,
            vaos: RefCell::new(vec![]),
            current_vao: Cell::new(0),
            methods: RefCell::new(HashMap::new()),
            extensions: RefCell::new(HashMap::new()),
            params: RefCell::new(HashMap::new()),
            precisions: RefCell::new(HashMap::new()),
        })
    }

    pub fn new_webgl1(gl: JsValue) -> Result<Self> {
        ensure(Reflect::get(&gl, &"getParameter".into()).map_err(error)?.is_function(), "A WebGL context is required")?;
        let host = Self {
            gl,
            gl2: None,
            webgl2: false,
            vaos: RefCell::new(vec![]),
            current_vao: Cell::new(0),
            methods: RefCell::new(HashMap::new()),
            extensions: RefCell::new(HashMap::new()),
            params: RefCell::new(HashMap::new()),
            precisions: RefCell::new(HashMap::new()),
        };
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

    pub fn webgl2_context(&self) -> Option<&Gl2> {
        self.gl2.as_ref()
    }
}

fn error(e: JsValue) -> RuntimeError {
    RuntimeError::new(format!("WebGL host exception: {e:?}"))
}

#[inline]
fn arg_u32(args: &[GlValue<JsValue>], i: usize) -> Option<u32> {
    match args.get(i) {
        Some(GlValue::Number(n)) => Some(*n as u32),
        _ => None,
    }
}

#[inline]
fn arg_i32(args: &[GlValue<JsValue>], i: usize) -> Option<i32> {
    match args.get(i) {
        Some(GlValue::Number(n)) => Some(*n as i32),
        _ => None,
    }
}

#[inline]
fn arg_f32(args: &[GlValue<JsValue>], i: usize) -> Option<f32> {
    match args.get(i) {
        Some(GlValue::Number(n)) => Some(*n as f32),
        _ => None,
    }
}

#[inline]
fn arg_bool(args: &[GlValue<JsValue>], i: usize) -> Option<bool> {
    match args.get(i) {
        Some(GlValue::Bool(b)) => Some(*b),
        Some(GlValue::Number(n)) => Some(*n != 0.0),
        _ => None,
    }
}

#[inline]
fn arg_opt_ref<T: JsCast>(args: &[GlValue<JsValue>], i: usize) -> Option<Option<&T>> {
    match args.get(i) {
        Some(GlValue::Null) | None => Some(None),
        Some(GlValue::Object(o)) => o.dyn_ref::<T>().map(Some),
        _ => None,
    }
}

impl Gl for BrowserGl {
    type Object = JsValue;

    fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
        if let Some(gl2) = &self.gl2 {
            return Ok([gl2.drawing_buffer_width().max(0) as u32, gl2.drawing_buffer_height().max(0) as u32]);
        }
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
        if let Some(gl2) = &self.gl2 {
            if let Some(result) = self.invoke_webgl2_fast(gl2, method, args) {
                return result;
            }
        }
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
    /// Typed `web-sys` fast paths for hot WebGL2 render-loop calls: zero
    /// `js_sys::Array` allocation, zero `Reflect::get`, zero `Function::apply`.
    fn invoke_webgl2_fast(
        &self,
        gl: &Gl2,
        method: &str,
        args: &[GlValue<JsValue>],
    ) -> Option<Result<GlValue<JsValue>>> {
        match method {
            "getParameter" => {
                let pname = arg_u32(args, 0)?;
                if is_immutable_gl_param(pname) {
                    if let Some(cached) = self.params.borrow().get(&pname) {
                        return Some(Ok(cached.clone()));
                    }
                    let res = self.invoke_raw(method, args);
                    if let Ok(ref val) = res {
                        self.params.borrow_mut().insert(pname, val.clone());
                    }
                    return Some(res);
                }
                None
            }
            "getShaderPrecisionFormat" => {
                let st = arg_u32(args, 0)?;
                let pt = arg_u32(args, 1)?;
                if let Some(cached) = self.precisions.borrow().get(&(st, pt)) {
                    return Some(Ok(cached.clone()));
                }
                let res = self.invoke_raw(method, args);
                if let Ok(ref val) = res {
                    self.precisions.borrow_mut().insert((st, pt), val.clone());
                }
                Some(res)
            }
            "getExtension" => {
                if let Some(GlValue::String(name)) = args.first() {
                    if let Some(cached) = self.extensions.borrow().get(name) {
                        return Some(Ok(if cached.is_null() || cached.is_undefined() {
                            GlValue::Null
                        } else {
                            GlValue::Object(cached.clone())
                        }));
                    }
                    match gl.get_extension(name) {
                        Ok(opt) => {
                            let val = opt.map(JsValue::from).unwrap_or(JsValue::NULL);
                            self.extensions.borrow_mut().insert(name.clone(), val.clone());
                            return Some(Ok(if val.is_null() { GlValue::Null } else { GlValue::Object(val) }));
                        }
                        Err(e) => return Some(Err(error(e))),
                    }
                }
                None
            }
            "getError" => Some(Ok(GlValue::Number(gl.get_error() as f64))),
            "flush" => {
                gl.flush();
                Some(Ok(GlValue::Null))
            }
            "finish" => {
                gl.finish();
                Some(Ok(GlValue::Null))
            }
            "enable" => {
                gl.enable(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "disable" => {
                gl.disable(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "useProgram" => {
                gl.use_program(arg_opt_ref::<WebGlProgram>(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "bindVertexArray" => {
                gl.bind_vertex_array(arg_opt_ref::<WebGlVertexArrayObject>(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "bindBuffer" => {
                gl.bind_buffer(arg_u32(args, 0)?, arg_opt_ref::<WebGlBuffer>(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "bindTexture" => {
                gl.bind_texture(arg_u32(args, 0)?, arg_opt_ref::<WebGlTexture>(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "activeTexture" => {
                gl.active_texture(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "bindFramebuffer" => {
                gl.bind_framebuffer(arg_u32(args, 0)?, arg_opt_ref::<WebGlFramebuffer>(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "bindRenderbuffer" => {
                gl.bind_renderbuffer(arg_u32(args, 0)?, arg_opt_ref::<WebGlRenderbuffer>(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "viewport" => {
                gl.viewport(arg_i32(args, 0)?, arg_i32(args, 1)?, arg_i32(args, 2)?, arg_i32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "scissor" => {
                gl.scissor(arg_i32(args, 0)?, arg_i32(args, 1)?, arg_i32(args, 2)?, arg_i32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "clear" => {
                gl.clear(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "clearColor" => {
                gl.clear_color(arg_f32(args, 0)?, arg_f32(args, 1)?, arg_f32(args, 2)?, arg_f32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "clearDepth" => {
                gl.clear_depth(arg_f32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "clearStencil" => {
                gl.clear_stencil(arg_i32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "colorMask" => {
                gl.color_mask(arg_bool(args, 0)?, arg_bool(args, 1)?, arg_bool(args, 2)?, arg_bool(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "depthMask" => {
                gl.depth_mask(arg_bool(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "depthFunc" => {
                gl.depth_func(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "depthRange" => {
                gl.depth_range(arg_f32(args, 0)?, arg_f32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "cullFace" => {
                gl.cull_face(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "frontFace" => {
                gl.front_face(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "lineWidth" => {
                gl.line_width(arg_f32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "polygonOffset" => {
                gl.polygon_offset(arg_f32(args, 0)?, arg_f32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "blendColor" => {
                gl.blend_color(arg_f32(args, 0)?, arg_f32(args, 1)?, arg_f32(args, 2)?, arg_f32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "blendEquation" => {
                gl.blend_equation(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "blendEquationSeparate" => {
                gl.blend_equation_separate(arg_u32(args, 0)?, arg_u32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "blendFunc" => {
                gl.blend_func(arg_u32(args, 0)?, arg_u32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "blendFuncSeparate" => {
                gl.blend_func_separate(arg_u32(args, 0)?, arg_u32(args, 1)?, arg_u32(args, 2)?, arg_u32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "stencilFunc" => {
                gl.stencil_func(arg_u32(args, 0)?, arg_i32(args, 1)?, arg_u32(args, 2)?);
                Some(Ok(GlValue::Null))
            }
            "stencilFuncSeparate" => {
                gl.stencil_func_separate(arg_u32(args, 0)?, arg_u32(args, 1)?, arg_i32(args, 2)?, arg_u32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "stencilOp" => {
                gl.stencil_op(arg_u32(args, 0)?, arg_u32(args, 1)?, arg_u32(args, 2)?);
                Some(Ok(GlValue::Null))
            }
            "stencilOpSeparate" => {
                gl.stencil_op_separate(arg_u32(args, 0)?, arg_u32(args, 1)?, arg_u32(args, 2)?, arg_u32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "stencilMask" => {
                gl.stencil_mask(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "stencilMaskSeparate" => {
                gl.stencil_mask_separate(arg_u32(args, 0)?, arg_u32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "pixelStorei" => {
                gl.pixel_storei(arg_u32(args, 0)?, arg_i32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "texParameteri" => {
                gl.tex_parameteri(arg_u32(args, 0)?, arg_u32(args, 1)?, arg_i32(args, 2)?);
                Some(Ok(GlValue::Null))
            }
            "texParameterf" => {
                gl.tex_parameterf(arg_u32(args, 0)?, arg_u32(args, 1)?, arg_f32(args, 2)?);
                Some(Ok(GlValue::Null))
            }
            "drawArrays" => {
                gl.draw_arrays(arg_u32(args, 0)?, arg_i32(args, 1)?, arg_i32(args, 2)?);
                Some(Ok(GlValue::Null))
            }
            "drawElements" => {
                gl.draw_elements_with_i32(arg_u32(args, 0)?, arg_i32(args, 1)?, arg_u32(args, 2)?, arg_i32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "drawArraysInstanced" => {
                gl.draw_arrays_instanced(arg_u32(args, 0)?, arg_i32(args, 1)?, arg_i32(args, 2)?, arg_i32(args, 3)?);
                Some(Ok(GlValue::Null))
            }
            "drawElementsInstanced" => {
                gl.draw_elements_instanced_with_i32(
                    arg_u32(args, 0)?,
                    arg_i32(args, 1)?,
                    arg_u32(args, 2)?,
                    arg_i32(args, 3)?,
                    arg_i32(args, 4)?,
                );
                Some(Ok(GlValue::Null))
            }
            "enableVertexAttribArray" => {
                gl.enable_vertex_attrib_array(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "disableVertexAttribArray" => {
                gl.disable_vertex_attrib_array(arg_u32(args, 0)?);
                Some(Ok(GlValue::Null))
            }
            "vertexAttribDivisor" => {
                gl.vertex_attrib_divisor(arg_u32(args, 0)?, arg_u32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "vertexAttribPointer" => {
                gl.vertex_attrib_pointer_with_i32(
                    arg_u32(args, 0)?,
                    arg_i32(args, 1)?,
                    arg_u32(args, 2)?,
                    arg_bool(args, 3)?,
                    arg_i32(args, 4)?,
                    arg_i32(args, 5)?,
                );
                Some(Ok(GlValue::Null))
            }
            "vertexAttribIPointer" => {
                gl.vertex_attrib_i_pointer_with_i32(
                    arg_u32(args, 0)?,
                    arg_i32(args, 1)?,
                    arg_u32(args, 2)?,
                    arg_i32(args, 3)?,
                    arg_i32(args, 4)?,
                );
                Some(Ok(GlValue::Null))
            }
            "uniform1i" => {
                gl.uniform1i(arg_opt_ref::<WebGlUniformLocation>(args, 0)?, arg_i32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "uniform1f" => {
                gl.uniform1f(arg_opt_ref::<WebGlUniformLocation>(args, 0)?, arg_f32(args, 1)?);
                Some(Ok(GlValue::Null))
            }
            "uniform2f" => {
                gl.uniform2f(arg_opt_ref::<WebGlUniformLocation>(args, 0)?, arg_f32(args, 1)?, arg_f32(args, 2)?);
                Some(Ok(GlValue::Null))
            }
            "uniform3f" => {
                gl.uniform3f(
                    arg_opt_ref::<WebGlUniformLocation>(args, 0)?,
                    arg_f32(args, 1)?,
                    arg_f32(args, 2)?,
                    arg_f32(args, 3)?,
                );
                Some(Ok(GlValue::Null))
            }
            "uniform4f" => {
                gl.uniform4f(
                    arg_opt_ref::<WebGlUniformLocation>(args, 0)?,
                    arg_f32(args, 1)?,
                    arg_f32(args, 2)?,
                    arg_f32(args, 3)?,
                    arg_f32(args, 4)?,
                );
                Some(Ok(GlValue::Null))
            }
            "uniform1iv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                if let Some(GlValue::Ints(v)) = args.get(1) {
                    gl.uniform1iv_with_i32_array(loc, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "uniform1fv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                if let Some(GlValue::Floats(v)) = args.get(1) {
                    gl.uniform1fv_with_f32_array(loc, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "uniform2fv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                if let Some(GlValue::Floats(v)) = args.get(1) {
                    gl.uniform2fv_with_f32_array(loc, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "uniform3fv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                if let Some(GlValue::Floats(v)) = args.get(1) {
                    gl.uniform3fv_with_f32_array(loc, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "uniform4fv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                if let Some(GlValue::Floats(v)) = args.get(1) {
                    gl.uniform4fv_with_f32_array(loc, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "uniformMatrix2fv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                let transpose = arg_bool(args, 1)?;
                if let Some(GlValue::Floats(v)) = args.get(2) {
                    gl.uniform_matrix2fv_with_f32_array(loc, transpose, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "uniformMatrix3fv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                let transpose = arg_bool(args, 1)?;
                if let Some(GlValue::Floats(v)) = args.get(2) {
                    gl.uniform_matrix3fv_with_f32_array(loc, transpose, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "uniformMatrix4fv" => {
                let loc = arg_opt_ref::<WebGlUniformLocation>(args, 0)?;
                let transpose = arg_bool(args, 1)?;
                if let Some(GlValue::Floats(v)) = args.get(2) {
                    gl.uniform_matrix4fv_with_f32_array(loc, transpose, v);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            "bufferData" => {
                let target = arg_u32(args, 0)?;
                let usage = arg_u32(args, 2)?;
                match args.get(1) {
                    Some(GlValue::Number(size)) => {
                        gl.buffer_data_with_i32(target, *size as i32, usage);
                        Some(Ok(GlValue::Null))
                    }
                    Some(GlValue::Bytes(bytes)) => {
                        gl.buffer_data_with_u8_array(target, bytes, usage);
                        Some(Ok(GlValue::Null))
                    }
                    _ => None,
                }
            }
            "bufferSubData" => {
                let target = arg_u32(args, 0)?;
                let offset = arg_i32(args, 1)?;
                if let Some(GlValue::Bytes(bytes)) = args.get(2) {
                    gl.buffer_sub_data_with_i32_and_u8_array(target, offset, bytes);
                    return Some(Ok(GlValue::Null));
                }
                None
            }
            _ => None,
        }
    }

    fn resolve_method(&self, method: &str, args: &[GlValue<JsValue>]) -> Result<Option<(JsValue, Function)>> {
        if let Some(cached) = self.methods.borrow().get(method) {
            return Ok(Some(cached.clone()));
        }
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
                    && matches!(args.get(1), Some(GlValue::Number(n)) if *n == 0.)
                {
                    return Ok(None);
                }
                ensure(!receiver.is_null(), "WebGL extension entrypoint unavailable")?;
                entry.push_str(suffix);
            }
        }
        let f = Reflect::get(&receiver, &entry.into()).map_err(error)?.dyn_into::<Function>().map_err(error)?;
        let pair = (receiver, f);
        self.methods.borrow_mut().insert(method.to_string(), pair.clone());
        Ok(Some(pair))
    }

    fn invoke_raw(&self, method: &str, args: &[GlValue<JsValue>]) -> Result<GlValue<JsValue>> {
        let Some((receiver, f)) = self.resolve_method(method, args)? else {
            return Ok(GlValue::Null);
        };
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
