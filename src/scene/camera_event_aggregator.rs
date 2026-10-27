//! Frame aggregation of host input, matching Cesium's modifier handoff,
//! previous-frame movement, wheel scaling and continuous pinch angles.
use crate::core::{
    Cartesian2, KeyboardEventModifier as Modifier, RuntimeError, ScreenInput, ScreenSpaceEventHandler,
    ScreenSpaceEventType as Kind, ScreenSpaceInputEvent as Input,
};
use crate::scene::CameraEventType as Type;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraMovement {
    pub start_position: Cartesian2,
    pub end_position: Cartesian2,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraEventMovement {
    pub movement: CameraMovement,
    pub distance: Option<CameraMovement>,
    pub angle_and_height: Option<CameraMovement>,
}
#[derive(Clone, Copy, Debug, Default)]
struct State {
    moving: bool,
    down: bool,
    start: Cartesian2,
    press: Option<f64>,
    release: Option<f64>,
    movement: CameraEventMovement,
    last: Option<CameraMovement>,
    previous_angle: f64,
}
/// Time arguments and button times are seconds. `ScreenInput::now` is converted
/// from the core handler's milliseconds. `process_event` accepts normalized
/// events directly for hosts with their own event handler.
pub struct CameraEventAggregator {
    handler: ScreenSpaceEventHandler,
    states: [[State; 4]; 5],
    width: f64,
    buttons_down: usize,
    current_mouse_position: Cartesian2,
    destroyed: bool,
}
fn mi(m: Option<Modifier>) -> usize {
    m.map_or(0, |m| m as usize + 1)
}
const MODIFIERS: [Option<Modifier>; 4] = [None, Some(Modifier::Shift), Some(Modifier::Ctrl), Some(Modifier::Alt)];
impl CameraEventAggregator {
    pub fn new(client_width: f64) -> Self {
        let mut handler = ScreenSpaceEventHandler::default();
        for modifier in MODIFIERS {
            for kind in [
                Kind::LeftDown,
                Kind::LeftUp,
                Kind::RightDown,
                Kind::RightUp,
                Kind::MiddleDown,
                Kind::MiddleUp,
                Kind::MouseMove,
                Kind::Wheel,
                Kind::PinchStart,
                Kind::PinchEnd,
                Kind::PinchMove,
            ] {
                handler.set_input_action(Rc::new(|_| {}), kind, modifier).unwrap();
            }
        }
        let mut states = [[State::default(); 4]; 5];
        for s in &mut states[Type::Pinch as usize] {
            s.movement.distance = Some(CameraMovement::default());
            s.movement.angle_and_height = Some(CameraMovement::default());
        }
        Self {
            handler,
            states,
            width: client_width,
            buttons_down: 0,
            current_mouse_position: Cartesian2::ZERO,
            destroyed: false,
        }
    }
    pub fn set_client_width(&mut self, width: f64) {
        self.width = width;
    }
    pub fn current_mouse_position(&self) -> Cartesian2 {
        self.current_mouse_position
    }
    pub fn any_button_down(&self) -> bool {
        self.buttons_down > 0 || self.states[Type::Wheel as usize].iter().any(|s| s.moving)
    }
    fn state(&self, t: Type, m: Option<Modifier>) -> &State {
        &self.states[t as usize][mi(m)]
    }
    pub fn is_moving(&self, t: Type, m: Option<Modifier>) -> bool {
        self.state(t, m).moving
    }
    pub fn get_movement(&self, t: Type, m: Option<Modifier>) -> CameraEventMovement {
        self.state(t, m).movement
    }
    pub fn get_last_movement(&self, t: Type, m: Option<Modifier>) -> Option<CameraMovement> {
        self.state(t, m).last
    }
    pub fn is_button_down(&self, t: Type, m: Option<Modifier>) -> bool {
        self.state(t, m).down
    }
    pub fn get_start_mouse_position(&self, t: Type, m: Option<Modifier>) -> Cartesian2 {
        if t == Type::Wheel {
            self.current_mouse_position
        } else {
            self.state(t, m).start
        }
    }
    pub fn get_button_press_time(&self, t: Type, m: Option<Modifier>) -> Option<f64> {
        self.state(t, m).press
    }
    pub fn get_button_release_time(&self, t: Type, m: Option<Modifier>) -> Option<f64> {
        self.state(t, m).release
    }
    pub fn reset(&mut self) {
        for row in &mut self.states {
            for s in row {
                s.moving = false;
            }
        }
    }
    pub fn process(&mut self, input: &ScreenInput) -> Result<bool, RuntimeError> {
        self.alive()?;
        let result = self.handler.process(input)?;
        for (kind, modifier, event) in result.actions {
            self.process_event(kind, modifier, &event, input.now / 1000.)?;
        }
        Ok(result.prevent_default)
    }
    pub fn process_event(
        &mut self,
        kind: Kind,
        modifier: Option<Modifier>,
        event: &Input,
        time: f64,
    ) -> Result<(), RuntimeError> {
        self.alive()?;
        let time = (time * 1000.).trunc() / 1000.;
        let index = mi(modifier);
        let down = match kind {
            Kind::LeftDown => Some(Type::LeftDrag),
            Kind::RightDown => Some(Type::RightDrag),
            Kind::MiddleDown => Some(Type::MiddleDrag),
            _ => None,
        };
        let up = match kind {
            Kind::LeftUp => Some(Type::LeftDrag),
            Kind::RightUp => Some(Type::RightDrag),
            Kind::MiddleUp => Some(Type::MiddleDrag),
            _ => None,
        };
        if let (Some(t), Input::Position(p)) = (down, event) {
            self.buttons_down += 1;
            let s = &mut self.states[t as usize][index];
            s.down = true;
            s.last = None;
            s.press = Some(time);
            s.start = *p;
        } else if let Some(t) = up {
            for s in &mut self.states[t as usize] {
                if s.down {
                    self.buttons_down = self.buttons_down.saturating_sub(1);
                }
                s.down = false;
                s.release = Some(time);
            }
        } else {
            match (kind, event) {
                (Kind::Wheel, Input::Wheel(delta)) => {
                    let s = &mut self.states[Type::Wheel as usize][index];
                    s.press = Some(time);
                    s.release = Some(time);
                    s.moving = true;
                    s.movement.movement.end_position = Cartesian2::new(0., 7.5 * delta.to_radians());
                    s.last = Some(s.movement.movement);
                }
                (Kind::PinchStart, Input::TwoPositions { position1, position2 }) => {
                    self.buttons_down += 1;
                    let s = &mut self.states[Type::Pinch as usize][index];
                    s.down = true;
                    s.press = Some(time);
                    s.start = position1.add(*position2).multiply_by_scalar(0.5);
                }
                (Kind::PinchEnd, _) => {
                    self.buttons_down = self.buttons_down.saturating_sub(1);
                    let s = &mut self.states[Type::Pinch as usize][index];
                    s.down = false;
                    s.release = Some(time);
                }
                (
                    Kind::PinchMove,
                    Input::PinchMovement { distance_start, distance_end, angle_height_start, angle_height_end },
                ) => {
                    let s = &mut self.states[Type::Pinch as usize][index];
                    if s.down {
                        if !s.moving {
                            s.movement.distance =
                                Some(CameraMovement { start_position: *distance_start, end_position: *distance_end });
                            s.movement.angle_and_height = Some(CameraMovement {
                                start_position: *angle_height_start,
                                end_position: *angle_height_end,
                            });
                            s.previous_angle = angle_height_start.x;
                            s.moving = true;
                        } else {
                            s.movement.distance.as_mut().unwrap().end_position = *distance_end;
                            s.movement.angle_and_height.as_mut().unwrap().end_position = *angle_height_end;
                        }
                        let a = s.movement.angle_and_height.as_mut().unwrap();
                        let mut angle = a.end_position.x;
                        while angle >= s.previous_angle + std::f64::consts::PI {
                            angle -= std::f64::consts::TAU;
                        }
                        while angle < s.previous_angle - std::f64::consts::PI {
                            angle += std::f64::consts::TAU;
                        }
                        a.end_position.x = -angle * self.width / 12.;
                        a.start_position.x = -s.previous_angle * self.width / 12.;
                    }
                }
                (Kind::MouseMove, Input::Movement { start_position, end_position }) => {
                    for row in &mut self.states {
                        let mut transfer = false;
                        for (j, s) in row.iter_mut().enumerate() {
                            if j != index && s.down {
                                s.down = false;
                                s.release = Some(time);
                                transfer = true;
                                self.buttons_down = self.buttons_down.saturating_sub(1);
                            }
                        }
                        let s = &mut row[index];
                        if transfer {
                            s.down = true;
                            s.press = Some(time);
                            s.last = None;
                            self.buttons_down += 1;
                        }
                        if s.down {
                            if s.moving {
                                s.movement.movement.end_position = *end_position;
                            } else {
                                s.last = Some(s.movement.movement);
                                s.movement.movement =
                                    CameraMovement { start_position: *start_position, end_position: *end_position };
                                s.moving = true;
                            }
                        }
                    }
                    self.current_mouse_position = *end_position;
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.handler.destroy();
        self.buttons_down = 0;
    }
    fn alive(&self) -> Result<(), RuntimeError> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }
}
