//! CesiumJS `LabelStyle`: how a label is filled and outlined.

numeric_enum!(LabelStyle: i32 {
    Fill = 0 => "FILL",
    Outline = 1 => "OUTLINE",
    FillAndOutline = 2 => "FILL_AND_OUTLINE",
});
