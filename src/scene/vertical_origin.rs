//! CesiumJS `VerticalOrigin`: the vertical anchor of a billboard or label.

numeric_enum!(VerticalOrigin: i32 {
    Center = 0 => "CENTER",
    Bottom = 1 => "BOTTOM",
    Baseline = 2 => "BASELINE",
    Top = -1 => "TOP",
});
