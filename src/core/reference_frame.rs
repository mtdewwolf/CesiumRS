//! CesiumJS `ReferenceFrame`: the frame a position is expressed in.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReferenceFrame {
    /// Fixed to the Earth.
    #[default]
    Fixed = 0,
    /// The International Celestial Reference Frame.
    Inertial = 1,
}
