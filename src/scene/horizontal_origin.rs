//! CesiumJS `HorizontalOrigin`: the horizontal anchor of a billboard or label.

numeric_enum!(HorizontalOrigin: i32 {
    Center = 0 => "CENTER",
    Left = 1 => "LEFT",
    Right = -1 => "RIGHT",
});
