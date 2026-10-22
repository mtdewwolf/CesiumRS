//! CesiumJS `SplitDirection`: which side of the splitter a primitive is drawn on.

numeric_enum!(SplitDirection: i32 {
    Left = -1 => "LEFT",
    None = 0 => "NONE",
    Right = 1 => "RIGHT",
});
