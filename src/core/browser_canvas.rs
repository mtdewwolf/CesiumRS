//! Browser CanvasHost and DOM event registration for native Rust text, pins and gestures.
use super::browser_host::{BrowserImageHost, BrowserTransport};
use super::canvas::{CanvasCommand as Op, CanvasHost, CanvasTextMetrics};
use super::*;
use js_sys::{Array, Function, Reflect};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
#[wasm_bindgen(module = "/web/host/core.mjs")]
extern "C" {
    #[wasm_bindgen(catch,js_name=createCanvas)]
    fn create_canvas(width: u32, height: u32) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=canvasContext)]
    fn context(canvas: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=measureCanvasText)]
    fn measure(canvas: &JsValue, text: &str, font: &str) -> Result<Array, JsValue>;
    #[wasm_bindgen(catch,js_name=screenListeners)]
    fn listeners(element: &JsValue, callback: &Function) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name=removeScreenListeners)]
    fn remove_listeners(job: &JsValue);
}
fn err(e: JsValue) -> RuntimeError {
    RuntimeError::new(
        Reflect::get(&e, &"message".into())
            .ok()
            .and_then(|v| v.as_string())
            .or_else(|| e.as_string())
            .unwrap_or_else(|| "Canvas operation failed.".into()),
    )
}
fn js_err(e: RuntimeError) -> JsValue {
    e.to_string().into()
}
fn field(v: &JsValue, key: &str) -> JsValue {
    Reflect::get(v, &key.into()).unwrap_or(JsValue::UNDEFINED)
}
fn set(v: &JsValue, k: &str, x: JsValue) -> Result<(), RuntimeError> {
    Reflect::set(v, &k.into(), &x).map(|_| ()).map_err(err)
}
impl CanvasHost for BrowserImageHost {
    fn create_canvas(&self, w: u32, h: u32) -> Result<JsValue, RuntimeError> {
        create_canvas(w, h).map_err(err)
    }
    fn resize_canvas(&self, c: &JsValue, w: u32, h: u32) -> Result<(), RuntimeError> {
        set(c, "width", w.into())?;
        set(c, "height", h.into())
    }
    fn measure_text(&self, c: &JsValue, text: &str, font: &str) -> Result<CanvasTextMetrics, RuntimeError> {
        let a = measure(c, text, font).map_err(err)?;
        Ok(CanvasTextMetrics {
            width: a.get(0).as_f64().unwrap_or(0.),
            ascent: a.get(1).as_f64().unwrap_or(0.),
            descent: a.get(2).as_f64().unwrap_or(0.),
            left: a.get(3).as_f64().unwrap_or(0.),
        })
    }
    fn command(&self, c: &JsValue, op: Op<'_, JsValue>) -> Result<(), RuntimeError> {
        let ctx = context(c).map_err(err)?;
        let values = |v: &[f64]| v.iter().map(|&v| JsValue::from(v)).collect::<Array>();
        let (name, args) = match op {
            Op::Font(s) => return set(&ctx, "font", s.into()),
            Op::LineJoin(s) => return set(&ctx, "lineJoin", s.into()),
            Op::LineWidth(n) => return set(&ctx, "lineWidth", n.into()),
            Op::ImageSmoothing(b) => return set(&ctx, "imageSmoothingEnabled", b.into()),
            Op::FillStyle(s) => return set(&ctx, "fillStyle", s.into()),
            Op::StrokeStyle(s) => return set(&ctx, "strokeStyle", s.into()),
            Op::Composite(s) => return set(&ctx, "globalCompositeOperation", s.into()),
            Op::Save => ("save", Array::new()),
            Op::Restore => ("restore", Array::new()),
            Op::Scale(x, y) => ("scale", values(&[x, y])),
            Op::BeginPath => ("beginPath", Array::new()),
            Op::ClosePath => ("closePath", Array::new()),
            Op::MoveTo(x, y) => ("moveTo", values(&[x, y])),
            Op::LineTo(x, y) => ("lineTo", values(&[x, y])),
            Op::BezierCurveTo(v) => ("bezierCurveTo", values(&v)),
            Op::Fill => ("fill", Array::new()),
            Op::Stroke => ("stroke", Array::new()),
            Op::FillRect(x, y, w, h) => ("fillRect", values(&[x, y, w, h])),
            Op::FillText(s, x, y) | Op::StrokeText(s, x, y) => {
                let args = Array::new();
                args.push(&s.into());
                args.push(&x.into());
                args.push(&y.into());
                (if matches!(op, Op::FillText(..)) { "fillText" } else { "strokeText" }, args)
            }
            Op::DrawImage(i, x, y, w, h) => {
                let a = Array::new();
                a.push(i);
                for v in [x, y, w, h] {
                    a.push(&v.into());
                }
                ("drawImage", a)
            }
        };
        field(&ctx, name).dyn_into::<Function>().map_err(err)?.apply(&ctx, &args).map(|_| ()).map_err(err)
    }
}
fn color(v: &JsValue, default: Color) -> Color {
    if !Array::is_array(v) {
        return default;
    }
    let a = Array::from(v);
    if a.length() == 4 {
        Color::new(
            a.get(0).as_f64().unwrap_or(0.),
            a.get(1).as_f64().unwrap_or(0.),
            a.get(2).as_f64().unwrap_or(0.),
            a.get(3).as_f64().unwrap_or(1.),
        )
    } else {
        default
    }
}
#[wasm_bindgen(js_name=writeTextToCanvas)]
pub fn write_text(text: &str, o: JsValue) -> Result<JsValue, JsValue> {
    let number = |k, d| field(&o, k).as_f64().unwrap_or(d);
    let boolean = |k, d| field(&o, k).as_bool().unwrap_or(d);
    let options = WriteTextOptions {
        font: field(&o, "font").as_string().unwrap_or_else(|| "10px sans-serif".into()),
        stroke: boolean("stroke", false),
        fill: boolean("fill", true),
        stroke_width: number("strokeWidth", 1.),
        padding: number("padding", 0.),
        background_color: color(&field(&o, "backgroundColor"), Color::TRANSPARENT),
        stroke_color: color(&field(&o, "strokeColor"), Color::BLACK),
        fill_color: color(&field(&o, "fillColor"), Color::WHITE),
    };
    let Some(result) = write_text_to_canvas(&BrowserImageHost, text, &options).map_err(js_err)? else {
        return Ok(JsValue::UNDEFINED);
    };
    let d = result.dimensions;
    let dims = js_sys::Object::new();
    for (k, v) in
        [("width", d.width), ("height", d.height), ("ascent", d.ascent), ("descent", d.descent), ("minx", d.min_x)]
    {
        set(&dims, k, v.into()).map_err(js_err)?;
    }
    set(&result.canvas, "dimensions", dims.into()).map_err(js_err)?;
    Ok(result.canvas)
}
#[wasm_bindgen]
pub struct BrowserPinBuilder {
    builder: PinBuilder<BrowserImageHost>,
}
#[wasm_bindgen]
impl BrowserPinBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            builder: PinBuilder::new(
                Rc::new(BrowserImageHost),
                ResourceClient::new(Rc::new(BrowserTransport::default())),
            ),
        }
    }
    #[wasm_bindgen(js_name=fromColor)]
    pub fn from_color(&self, c: JsValue, size: f64) -> Result<JsValue, JsValue> {
        self.builder.from_color(color(&c, Color::WHITE), size).map_err(js_err)
    }
    #[wasm_bindgen(js_name=fromText)]
    pub fn from_text(&self, text: &str, c: JsValue, size: f64) -> Result<JsValue, JsValue> {
        self.builder.from_text(text, color(&c, Color::WHITE), size).map_err(js_err)
    }
    #[wasm_bindgen(js_name=fromUrl)]
    pub async fn from_url(&self, url: &str, c: JsValue, size: f64) -> Result<JsValue, JsValue> {
        let r = Resource::new(url).ok_or_else(|| JsValue::from_str("Invalid pin URL."))?;
        self.builder
            .from_url(r, color(&c, Color::WHITE), size)
            .ok_or_else(|| JsValue::from_str("Pin image request was throttled."))?
            .await
            .map_err(js_err)
    }
    #[wasm_bindgen(js_name=fromMakiIconId)]
    pub async fn from_maki(&self, id: &str, c: JsValue, size: f64, base_url: &str) -> Result<JsValue, JsValue> {
        let r = Resource::new(base_url).ok_or_else(|| JsValue::from_str("Invalid asset base URL."))?;
        self.builder
            .from_maki_icon_id(id, color(&c, Color::WHITE), size, &r)
            .ok_or_else(|| JsValue::from_str("Pin image request was throttled."))?
            .await
            .map_err(js_err)
    }
}
fn input(event: &JsValue) -> ScreenInput {
    let n = |k| field(event, k).as_f64().unwrap_or(0.);
    let kind = match field(event, "kind").as_string().unwrap_or_default().as_str() {
        "mousedown" => ScreenInputKind::MouseDown,
        "mouseup" => ScreenInputKind::MouseUp,
        "mousemove" => ScreenInputKind::MouseMove,
        "dblclick" => ScreenInputKind::DoubleClick,
        "wheel" => ScreenInputKind::Wheel,
        "touchstart" => ScreenInputKind::TouchStart,
        "touchend" => ScreenInputKind::TouchEnd,
        "touchmove" => ScreenInputKind::TouchMove,
        "pointerdown" => ScreenInputKind::PointerDown,
        "pointerup" => ScreenInputKind::PointerUp,
        "pointermove" => ScreenInputKind::PointerMove,
        _ => ScreenInputKind::Hold,
    };
    let mut e = ScreenInput::new(kind, n("now"));
    e.client_position = Cartesian2::new(n("x"), n("y"));
    e.element_origin = Cartesian2::new(n("left"), n("top"));
    e.button = n("button") as u8;
    e.modifier = ScreenSpaceEventHandler::modifier(
        field(event, "shift").is_truthy(),
        field(event, "ctrl").is_truthy(),
        field(event, "alt").is_truthy(),
    );
    e.pointer_id = n("pointerId") as i64;
    e.pointer_touch = field(event, "pointerTouch").is_truthy();
    e.wheel_delta = ScreenSpaceEventHandler::wheel_delta(
        field(event, "deltaY").as_f64(),
        n("deltaMode") as u32,
        field(event, "detail").as_f64(),
        field(event, "wheelDelta").as_f64(),
    );
    e.touches = Array::from(&field(event, "touches"))
        .iter()
        .map(|v| {
            let a = Array::from(&v);
            (
                a.get(0).as_f64().unwrap_or(0.) as i64,
                Cartesian2::new(a.get(1).as_f64().unwrap_or(0.), a.get(2).as_f64().unwrap_or(0.)),
            )
        })
        .collect();
    e
}
fn input_value(event: &ScreenSpaceInputEvent) -> JsValue {
    let p = |v: Cartesian2| serde_json::json!({"x":v.x,"y":v.y});
    let motion = |a: Cartesian2, b: Cartesian2| serde_json::json!({"startPosition":p(a),"endPosition":p(b)});
    let value = match event {
        ScreenSpaceInputEvent::Position(v) => serde_json::json!({"position":p(*v)}),
        ScreenSpaceInputEvent::Movement { start_position, end_position } => motion(*start_position, *end_position),
        ScreenSpaceInputEvent::Wheel(n) => return JsValue::from(*n),
        ScreenSpaceInputEvent::TwoPositions { position1, position2 } => {
            serde_json::json!({"position1":p(*position1),"position2":p(*position2)})
        }
        ScreenSpaceInputEvent::PinchMovement { distance_start, distance_end, angle_height_start, angle_height_end } => {
            serde_json::json!({"distance":motion(*distance_start,*distance_end),"angleAndHeight":motion(*angle_height_start,*angle_height_end)})
        }
        ScreenSpaceInputEvent::End => return JsValue::UNDEFINED,
    };
    js_sys::JSON::parse(&value.to_string()).unwrap()
}
#[wasm_bindgen]
pub struct BrowserScreenSpaceEventHandler {
    state: Rc<RefCell<ScreenSpaceEventHandler>>,
    functions: RefCell<HashMap<(u8, Option<u8>), Function>>,
    job: Option<JsValue>,
    _callback: Closure<dyn FnMut(JsValue) -> JsValue>,
}
#[wasm_bindgen]
impl BrowserScreenSpaceEventHandler {
    #[wasm_bindgen(constructor)]
    pub fn new(element: JsValue) -> Result<Self, JsValue> {
        let state = Rc::new(RefCell::new(ScreenSpaceEventHandler::default()));
        let weak = Rc::downgrade(&state);
        let callback = Closure::new(move |event: JsValue| -> JsValue {
            let Some(state) = weak.upgrade() else { return js_sys::Object::new().into() };
            let input = input(&event);
            let result = state.borrow_mut().process(&input);
            let Ok(result) = result else { return js_sys::Object::new().into() };
            for (k, m, e) in &result.actions {
                let action = state.borrow().get_input_action(*k, *m);
                if let Some(action) = action {
                    action(e);
                }
            }
            let out = js_sys::Object::new();
            let _ = set(&out, "preventDefault", result.prevent_default.into());
            if let Some(deadline) = state.borrow().hold_deadline() {
                let _ = set(&out, "holdDeadline", deadline.into());
            }
            out.into()
        });
        let job = listeners(&element, callback.as_ref().unchecked_ref())?;
        Ok(Self { state, functions: Default::default(), job: Some(job), _callback: callback })
    }
    #[wasm_bindgen(js_name=setInputAction)]
    pub fn set_action(&self, action: Function, kind: u8, modifier: Option<u8>) -> Result<(), JsValue> {
        let k = ScreenSpaceEventType::from_value(kind).ok_or_else(|| JsValue::from_str("Invalid input event type."))?;
        let m = modifier.and_then(KeyboardEventModifier::from_value);
        self.functions.borrow_mut().insert((kind, modifier), action.clone());
        self.state
            .borrow_mut()
            .set_input_action(
                Rc::new(move |e| {
                    let _ = action.call1(&JsValue::UNDEFINED, &input_value(e));
                }),
                k,
                m,
            )
            .map_err(js_err)
    }
    #[wasm_bindgen(js_name=getInputAction)]
    pub fn get_action(&self, kind: u8, modifier: Option<u8>) -> JsValue {
        self.functions.borrow().get(&(kind, modifier)).map(|f| f.clone().into()).unwrap_or(JsValue::UNDEFINED)
    }
    #[wasm_bindgen(js_name=removeInputAction)]
    pub fn remove_action(&self, kind: u8, modifier: Option<u8>) -> Result<(), JsValue> {
        let k = ScreenSpaceEventType::from_value(kind).ok_or_else(|| JsValue::from_str("Invalid input event type."))?;
        self.functions.borrow_mut().remove(&(kind, modifier));
        self.state
            .borrow_mut()
            .remove_input_action(k, modifier.and_then(KeyboardEventModifier::from_value))
            .map_err(js_err)
    }
    #[wasm_bindgen(js_name=isDestroyed)]
    pub fn destroyed(&self) -> bool {
        self.state.borrow().is_destroyed()
    }
    pub fn destroy(&mut self) {
        if let Some(job) = self.job.take() {
            remove_listeners(&job);
        }
        self.state.borrow_mut().destroy();
        self.functions.borrow_mut().clear();
    }
}
impl Drop for BrowserScreenSpaceEventHandler {
    fn drop(&mut self) {
        self.destroy();
    }
}
