//! CesiumJS `AnchorPointDirect` (`NGA_gpm_local`): an anchor point with its
//! adjustment parameters, for direct storage.

use crate::core::Cartesian3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorPointDirect {
    pub position: Cartesian3,
    pub adjustment_params: Cartesian3,
}
