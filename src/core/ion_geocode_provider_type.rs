//! CesiumJS `IonGeocodeProviderType`: the geocoder Cesium ion uses.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IonGeocodeProviderType {
    Google,
    Bing,
    Default,
}

impl IonGeocodeProviderType {
    pub const ALL: [Self; 3] = [Self::Google, Self::Bing, Self::Default];

    /// Cesium's string value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "GOOGLE",
            Self::Bing => "BING",
            Self::Default => "DEFAULT",
        }
    }

    pub fn from_str_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == value)
    }
}
