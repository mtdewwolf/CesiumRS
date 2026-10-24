//! CesiumJS `AnchorPointIndirect` (`NGA_gpm_local`): an anchor point with its
//! adjustment parameters and their covariance, for indirect storage.

use crate::core::{Cartesian3, Matrix3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorPointIndirect {
    pub position: Cartesian3,
    pub adjustment_params: Cartesian3,
    pub covariance_matrix: Matrix3,
}
