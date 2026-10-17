//! CesiumJS `CornerType`: how corridor and polyline-volume corners are drawn.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CornerType {
    #[default]
    Rounded = 0,
    Mitered = 1,
    Beveled = 2,
}
