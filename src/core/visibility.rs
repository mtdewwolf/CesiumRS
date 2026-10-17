//! CesiumJS `Visibility`: how much of an occludee an occluder hides.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i8)]
pub enum Visibility {
    None = -1,
    Partial = 0,
    Full = 1,
}
