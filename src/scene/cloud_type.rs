//! CesiumJS `CloudType`: the kinds of cloud a `CloudCollection` draws.

numeric_enum!(CloudType: i32 {
    Cumulus = 0 => "CUMULUS",
});

impl CloudType {
    /// Whether `value` is a cloud type.
    pub fn validate(value: f64) -> bool {
        value == Self::Cumulus.value() as f64
    }
}
