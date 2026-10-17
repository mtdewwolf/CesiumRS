//! CesiumJS `ArcType`: the path drawn between two points on the ellipsoid.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ArcType {
    /// Straight line that does not follow the ellipsoid surface.
    None = 0,
    /// Shortest path on the ellipsoid.
    #[default]
    Geodesic = 1,
    /// Path of constant heading.
    Rhumb = 2,
}
