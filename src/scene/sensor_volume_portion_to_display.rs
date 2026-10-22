//! CesiumJS `SensorVolumePortionToDisplay`: which part of a sensor volume to
//! draw relative to the ellipsoid horizon.

numeric_enum!(SensorVolumePortionToDisplay: i32 {
    Complete = 0x0000 => "COMPLETE",
    BelowEllipsoidHorizon = 0x0001 => "BELOW_ELLIPSOID_HORIZON",
    AboveEllipsoidHorizon = 0x0002 => "ABOVE_ELLIPSOID_HORIZON",
});

impl SensorVolumePortionToDisplay {
    /// Whether `value` is one of the three portions.
    pub fn validate(value: f64) -> bool {
        Self::ALL.iter().any(|v| v.value() as f64 == value)
    }

    /// Cesium's `toString`: the constant's name.
    pub fn to_str(self) -> &'static str {
        self.name()
    }
}
