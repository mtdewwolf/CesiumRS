//! CesiumJS `CameraEventType`: the mouse and touch inputs that drive the camera.

numeric_enum!(CameraEventType: i32 {
    LeftDrag = 0 => "LEFT_DRAG",
    RightDrag = 1 => "RIGHT_DRAG",
    MiddleDrag = 2 => "MIDDLE_DRAG",
    Wheel = 3 => "WHEEL",
    Pinch = 4 => "PINCH",
});
