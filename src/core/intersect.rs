//! CesiumJS `Intersect`: where a volume lies relative to a plane or frustum.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i8)]
pub enum Intersect {
    /// Entirely on the negative side or outside.
    Outside = -1,
    Intersecting = 0,
    /// Entirely on the positive side or inside.
    Inside = 1,
}
