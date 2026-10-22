//! Mouse, wheel, touch and pointer gesture state machine with explicit host events.
use super::{Cartesian2, KeyboardEventModifier as Modifier, RuntimeError, ScreenSpaceEventType as Kind};
use std::{collections::HashMap, rc::Rc};
#[derive(Clone, Debug, PartialEq)]
pub enum ScreenSpaceInputEvent {
    Position(Cartesian2),
    Movement {
        start_position: Cartesian2,
        end_position: Cartesian2,
    },
    Wheel(f64),
    TwoPositions {
        position1: Cartesian2,
        position2: Cartesian2,
    },
    PinchMovement {
        distance_start: Cartesian2,
        distance_end: Cartesian2,
        angle_height_start: Cartesian2,
        angle_height_end: Cartesian2,
    },
    End,
}
pub type ScreenSpaceInputAction = Rc<dyn Fn(&ScreenSpaceInputEvent)>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenInputKind {
    MouseDown,
    MouseUp,
    MouseMove,
    DoubleClick,
    Wheel,
    TouchStart,
    TouchMove,
    TouchEnd,
    PointerDown,
    PointerMove,
    PointerUp,
    Hold,
}
#[derive(Clone, Debug)]
pub struct ScreenInput {
    pub kind: ScreenInputKind,
    pub now: f64,
    pub client_position: Cartesian2,
    pub element_origin: Cartesian2,
    pub button: u8,
    pub modifier: Option<Modifier>,
    pub touches: Vec<(i64, Cartesian2)>,
    pub pointer_id: i64,
    pub pointer_touch: bool,
    pub wheel_delta: Option<f64>,
}
impl ScreenInput {
    pub fn new(kind: ScreenInputKind, now: f64) -> Self {
        Self {
            kind,
            now,
            client_position: Cartesian2::ZERO,
            element_origin: Cartesian2::ZERO,
            button: 0,
            modifier: None,
            touches: vec![],
            pointer_id: 0,
            pointer_touch: false,
            wheel_delta: None,
        }
    }
}
#[derive(Default)]
pub struct ScreenInputResult {
    pub prevent_default: bool,
    pub actions: Vec<(Kind, Option<Modifier>, ScreenSpaceInputEvent)>,
}
pub struct ScreenSpaceEventHandler {
    actions: HashMap<(Kind, Option<Modifier>), ScreenSpaceInputAction>,
    buttons: [bool; 3],
    pinching: bool,
    holding: bool,
    last_touch: f64,
    primary_start: Cartesian2,
    primary: Cartesian2,
    previous: Cartesian2,
    positions: Vec<(i64, Cartesian2)>,
    previous_positions: Vec<(i64, Cartesian2)>,
    hold: Option<(f64, Option<Modifier>)>,
    destroyed: bool,
    pub mouse_emulation_ignore_milliseconds: f64,
    pub touch_hold_delay_milliseconds: f64,
    pub click_pixel_tolerance: f64,
    pub hold_pixel_tolerance: f64,
}
impl Default for ScreenSpaceEventHandler {
    fn default() -> Self {
        Self {
            actions: Default::default(),
            buttons: [false; 3],
            pinching: false,
            holding: false,
            last_touch: -800.,
            primary_start: Cartesian2::ZERO,
            primary: Cartesian2::ZERO,
            previous: Cartesian2::ZERO,
            positions: vec![],
            previous_positions: vec![],
            hold: None,
            destroyed: false,
            mouse_emulation_ignore_milliseconds: 800.,
            touch_hold_delay_milliseconds: 1500.,
            click_pixel_tolerance: 5.,
            hold_pixel_tolerance: 25.,
        }
    }
}
impl ScreenSpaceEventHandler {
    pub fn set_input_action(
        &mut self,
        action: ScreenSpaceInputAction,
        kind: Kind,
        modifier: Option<Modifier>,
    ) -> Result<(), RuntimeError> {
        self.alive()?;
        self.actions.insert((kind, modifier), action);
        Ok(())
    }
    pub fn get_input_action(&self, kind: Kind, modifier: Option<Modifier>) -> Option<ScreenSpaceInputAction> {
        if self.destroyed {
            None
        } else {
            self.actions.get(&(kind, modifier)).cloned()
        }
    }
    pub fn remove_input_action(&mut self, kind: Kind, modifier: Option<Modifier>) -> Result<(), RuntimeError> {
        self.alive()?;
        self.actions.remove(&(kind, modifier));
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.actions.clear();
        self.hold = None;
    }
    pub fn hold_deadline(&self) -> Option<f64> {
        self.hold.map(|h| h.0)
    }
    fn alive(&self) -> Result<(), RuntimeError> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }
    pub fn modifier(shift: bool, ctrl: bool, alt: bool) -> Option<Modifier> {
        if shift {
            Some(Modifier::Shift)
        } else if ctrl {
            Some(Modifier::Ctrl)
        } else if alt {
            Some(Modifier::Alt)
        } else {
            None
        }
    }
    pub fn wheel_delta(delta_y: Option<f64>, delta_mode: u32, detail: Option<f64>, legacy: Option<f64>) -> Option<f64> {
        if let Some(d) = delta_y {
            Some(
                -d * match delta_mode {
                    0 => 1.,
                    1 => 40.,
                    _ => 120.,
                },
            )
        } else if let Some(d) = detail.filter(|d| *d > 0.) {
            Some(-d * 120.)
        } else {
            legacy
        }
    }
    fn emit(&self, r: &mut ScreenInputResult, k: Kind, m: Option<Modifier>, e: ScreenSpaceInputEvent) -> bool {
        if self.actions.contains_key(&(k, m)) {
            r.actions.push((k, m, e));
            true
        } else {
            false
        }
    }
    pub fn dispatch(&self, result: &ScreenInputResult) {
        for (k, m, e) in &result.actions {
            if let Some(action) = self.get_input_action(*k, *m) {
                action(e);
            }
        }
    }
    pub fn process(&mut self, event: &ScreenInput) -> Result<ScreenInputResult, RuntimeError> {
        self.alive()?;
        let mut result = ScreenInputResult::default();
        let m = event.modifier;
        let pos = event.client_position.subtract(event.element_origin);
        let mut kind = event.kind;
        let pointer =
            matches!(kind, ScreenInputKind::PointerDown | ScreenInputKind::PointerMove | ScreenInputKind::PointerUp);
        if pointer {
            kind = match kind {
                ScreenInputKind::PointerDown => {
                    if event.pointer_touch {
                        ScreenInputKind::TouchStart
                    } else {
                        ScreenInputKind::MouseDown
                    }
                }
                ScreenInputKind::PointerMove => {
                    if event.pointer_touch {
                        ScreenInputKind::TouchMove
                    } else {
                        ScreenInputKind::MouseMove
                    }
                }
                _ => {
                    if event.pointer_touch {
                        ScreenInputKind::TouchEnd
                    } else {
                        ScreenInputKind::MouseUp
                    }
                }
            };
        }
        if matches!(kind, ScreenInputKind::MouseDown | ScreenInputKind::MouseUp | ScreenInputKind::MouseMove)
            && event.now - self.last_touch <= self.mouse_emulation_ignore_milliseconds
        {
            return Ok(result);
        }
        match kind {
            ScreenInputKind::MouseDown => {
                if event.button < 3 {
                    self.buttons[event.button as usize] = true;
                    let k = [Kind::LeftDown, Kind::MiddleDown, Kind::RightDown][event.button as usize];
                    self.primary = pos;
                    self.primary_start = pos;
                    self.previous = pos;
                    result.prevent_default = self.emit(&mut result, k, m, ScreenSpaceInputEvent::Position(pos));
                }
            }
            ScreenInputKind::MouseUp => {
                if event.button < 3 {
                    for (i, up, click) in [
                        (0, Kind::LeftUp, Kind::LeftClick),
                        (1, Kind::MiddleUp, Kind::MiddleClick),
                        (2, Kind::RightUp, Kind::RightClick),
                    ] {
                        if self.buttons[i] {
                            if self.actions.contains_key(&(up, m)) || self.actions.contains_key(&(click, m)) {
                                self.primary = pos;
                                self.emit(&mut result, up, m, ScreenSpaceInputEvent::Position(pos));
                                if self.primary_start.distance(pos) < self.click_pixel_tolerance {
                                    self.emit(&mut result, click, m, ScreenSpaceInputEvent::Position(pos));
                                }
                            }
                            self.buttons[i] = false;
                        }
                    }
                }
            }
            ScreenInputKind::MouseMove => {
                self.primary = pos;
                self.emit(
                    &mut result,
                    Kind::MouseMove,
                    m,
                    ScreenSpaceInputEvent::Movement { start_position: self.previous, end_position: pos },
                );
                self.previous = pos;
                result.prevent_default = self.buttons.iter().any(|v| *v);
            }
            ScreenInputKind::DoubleClick => {
                if event.button == 0 {
                    self.emit(&mut result, Kind::LeftDoubleClick, m, ScreenSpaceInputEvent::Position(pos));
                }
            }
            ScreenInputKind::Wheel => {
                if let Some(delta) = event.wheel_delta {
                    result.prevent_default =
                        self.emit(&mut result, Kind::Wheel, m, ScreenSpaceInputEvent::Wheel(delta));
                }
            }
            ScreenInputKind::Hold => {
                if let Some((deadline, modifier)) = self.hold {
                    if event.now >= deadline {
                        self.hold = None;
                        self.holding = true;
                        if self
                            .previous_positions
                            .first()
                            .is_some_and(|(_, p)| self.primary_start.distance(*p) < self.hold_pixel_tolerance)
                        {
                            self.emit(
                                &mut result,
                                Kind::RightClick,
                                modifier,
                                ScreenSpaceInputEvent::Position(self.primary),
                            );
                        }
                    }
                }
            }
            ScreenInputKind::TouchStart | ScreenInputKind::TouchMove | ScreenInputKind::TouchEnd => {
                if !pointer {
                    self.last_touch = event.now;
                }
                let touches = if pointer {
                    vec![(event.pointer_id, pos)]
                } else {
                    event.touches.iter().map(|(id, p)| (*id, p.subtract(event.element_origin))).collect()
                };
                for &(id, p) in &touches {
                    match kind {
                        ScreenInputKind::TouchEnd => self.positions.retain(|(key, _)| *key != id),
                        ScreenInputKind::TouchMove => {
                            if let Some((_, v)) = self.positions.iter_mut().find(|(key, _)| *key == id) {
                                *v = p;
                            } else if pointer {
                                return Ok(result);
                            }
                        }
                        _ => set_position(&mut self.positions, id, p),
                    }
                }
                if kind == ScreenInputKind::TouchMove {
                    self.touch_move(&mut result, m);
                } else {
                    self.touch_events(&mut result, m, event.now);
                }
                for (id, _) in touches {
                    if kind == ScreenInputKind::TouchEnd {
                        self.previous_positions.retain(|(key, _)| *key != id);
                    } else if let Some((_, p)) = self.positions.iter().find(|(key, _)| *key == id) {
                        set_position(&mut self.previous_positions, id, *p);
                    }
                }
            }
            _ => {}
        }
        Ok(result)
    }
    fn touch_events(&mut self, r: &mut ScreenInputResult, m: Option<Modifier>, now: f64) {
        let count = self.positions.len();
        let pinching = self.pinching;
        if count != 1 && self.buttons[0] {
            self.buttons[0] = false;
            self.hold = None;
            self.emit(r, Kind::LeftUp, m, ScreenSpaceInputEvent::Position(self.primary));
            if count == 0
                && !self.holding
                && self
                    .previous_positions
                    .first()
                    .is_some_and(|(_, p)| self.primary_start.distance(*p) < self.click_pixel_tolerance)
            {
                self.emit(r, Kind::LeftClick, m, ScreenSpaceInputEvent::Position(self.primary));
            }
            self.holding = false;
        }
        if count == 0 && pinching {
            self.pinching = false;
            self.emit(r, Kind::PinchEnd, m, ScreenSpaceInputEvent::End);
        }
        if count == 1 && !pinching {
            let p = self.positions[0].1;
            self.primary = p;
            self.primary_start = p;
            self.previous = p;
            self.buttons[0] = true;
            self.emit(r, Kind::LeftDown, m, ScreenSpaceInputEvent::Position(p));
            self.hold = Some((now + self.touch_hold_delay_milliseconds, m));
            r.prevent_default = true;
        }
        if count == 2 && !pinching {
            self.pinching = true;
            r.prevent_default = self.emit(
                r,
                Kind::PinchStart,
                m,
                ScreenSpaceInputEvent::TwoPositions { position1: self.positions[0].1, position2: self.positions[1].1 },
            );
        }
    }
    fn touch_move(&mut self, r: &mut ScreenInputResult, m: Option<Modifier>) {
        if self.positions.len() == 1 && self.buttons[0] {
            let p = self.positions[0].1;
            self.primary = p;
            self.emit(
                r,
                Kind::MouseMove,
                m,
                ScreenSpaceInputEvent::Movement { start_position: self.previous, end_position: p },
            );
            self.previous = p;
            r.prevent_default = true;
        } else if self.positions.len() == 2 && self.pinching && self.previous_positions.len() >= 2 {
            let p = self.positions[0].1;
            let q = self.positions[1].1;
            let previous = self.previous_positions[0].1;
            let previous2 = self.previous_positions[1].1;
            let d = q.subtract(p);
            let old = previous2.subtract(previous);
            self.emit(
                r,
                Kind::PinchMove,
                m,
                ScreenSpaceInputEvent::PinchMovement {
                    distance_start: Cartesian2::new(0., old.magnitude() * 0.25),
                    distance_end: Cartesian2::new(0., d.magnitude() * 0.25),
                    angle_height_start: Cartesian2::new(old.y.atan2(old.x), (previous.y + previous2.y) * 0.125),
                    angle_height_end: Cartesian2::new(d.y.atan2(d.x), (p.y + q.y) * 0.125),
                },
            );
        }
    }
}
fn set_position(positions: &mut Vec<(i64, Cartesian2)>, id: i64, p: Cartesian2) {
    if let Some((_, v)) = positions.iter_mut().find(|(key, _)| *key == id) {
        *v = p;
    } else {
        positions.push((id, p));
    }
}
