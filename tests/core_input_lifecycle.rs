use cesium_rs::core::*;
use std::{cell::RefCell, rc::Rc};
#[test]
fn touch_emulation_hold_and_destroy() {
    let mut handler = ScreenSpaceEventHandler::default();
    let events = Rc::new(RefCell::new(vec![]));
    for k in ScreenSpaceEventType::ALL {
        let events = events.clone();
        handler.set_input_action(Rc::new(move |e| events.borrow_mut().push((k, e.clone()))), k, None).unwrap();
    }
    let mut touch = ScreenInput::new(ScreenInputKind::TouchStart, 100.);
    touch.touches = vec![(4, Cartesian2::new(10., 20.))];
    let r = handler.process(&touch).unwrap();
    assert!(r.prevent_default);
    handler.dispatch(&r);
    let mouse = ScreenInput::new(ScreenInputKind::MouseDown, 101.);
    assert!(handler.process(&mouse).unwrap().actions.is_empty());
    let hold = ScreenInput::new(ScreenInputKind::Hold, 1600.);
    let r = handler.process(&hold).unwrap();
    handler.dispatch(&r);
    assert_eq!(events.borrow().last().unwrap().0, ScreenSpaceEventType::RightClick);
    touch.kind = ScreenInputKind::TouchEnd;
    touch.now = 1601.;
    let r = handler.process(&touch).unwrap();
    assert!(!r.actions.iter().any(|(k, _, _)| *k == ScreenSpaceEventType::LeftClick));
    handler.dispatch(&r);
    let mouse = ScreenInput::new(ScreenInputKind::MouseDown, 2402.);
    assert_eq!(handler.process(&mouse).unwrap().actions[0].0, ScreenSpaceEventType::LeftDown);
    handler.destroy();
    assert!(handler.is_destroyed());
    assert!(handler.process(&mouse).is_err());
    assert!(handler.get_input_action(ScreenSpaceEventType::LeftDown, None).is_none());
}
#[test]
fn wheel_modifiers_and_click_tolerance() {
    assert_eq!(ScreenSpaceEventHandler::modifier(true, true, true), Some(KeyboardEventModifier::Shift));
    assert_eq!(ScreenSpaceEventHandler::wheel_delta(Some(2.), 0, None, None), Some(-2.));
    assert_eq!(ScreenSpaceEventHandler::wheel_delta(Some(2.), 1, None, None), Some(-80.));
    assert_eq!(ScreenSpaceEventHandler::wheel_delta(Some(2.), 2, None, None), Some(-240.));
    let mut h = ScreenSpaceEventHandler::default();
    h.set_input_action(Rc::new(|_| {}), ScreenSpaceEventType::LeftClick, None).unwrap();
    let mut e = ScreenInput::new(ScreenInputKind::MouseDown, 10.);
    h.process(&e).unwrap();
    e.kind = ScreenInputKind::MouseUp;
    e.client_position = Cartesian2::new(3., 4.);
    assert!(h.process(&e).unwrap().actions.is_empty());
}
