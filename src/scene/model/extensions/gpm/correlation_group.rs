//! CesiumJS `CorrelationGroup` (`NGA_gpm_local`): an intra-tile correlation
//! group. `group_flags` is the JSON array as given, as upstream keeps it.

use super::Spdcf;
use crate::core::Cartesian3;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct CorrelationGroup {
    pub group_flags: Value,
    pub rotation_thetas: Cartesian3,
    pub params: Vec<Spdcf>,
}
