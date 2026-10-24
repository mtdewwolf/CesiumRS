//! CesiumJS `GltfGpmLocal`: a glTF's `NGA_gpm_local` extension.
//!
//! Upstream checks, in debug builds only, that direct storage has anchor
//! points and a covariance and no indirect data, and the reverse for indirect
//! storage. Here the two constructors make those combinations the only ones
//! that can be built; the fields keep upstream's getters.

use super::{AnchorPointDirect, AnchorPointIndirect, CorrelationGroup, StorageType};
use crate::core::Matrix3;

#[derive(Clone, Debug, PartialEq)]
pub struct GltfGpmLocal {
    storage_type: StorageType,
    anchor_points_indirect: Option<Vec<AnchorPointIndirect>>,
    anchor_points_direct: Option<Vec<AnchorPointDirect>>,
    intra_tile_correlation_groups: Option<Vec<CorrelationGroup>>,
    covariance_direct: Option<Matrix3>,
}

impl GltfGpmLocal {
    /// Direct storage: anchor points and one covariance matrix.
    pub fn direct(anchor_points_direct: Vec<AnchorPointDirect>, covariance_direct: Matrix3) -> Self {
        Self {
            storage_type: StorageType::Direct,
            anchor_points_indirect: None,
            anchor_points_direct: Some(anchor_points_direct),
            intra_tile_correlation_groups: None,
            covariance_direct: Some(covariance_direct),
        }
    }

    /// Indirect storage: anchor points with covariances, and correlation groups.
    pub fn indirect(
        anchor_points_indirect: Vec<AnchorPointIndirect>,
        intra_tile_correlation_groups: Vec<CorrelationGroup>,
    ) -> Self {
        Self {
            storage_type: StorageType::Indirect,
            anchor_points_indirect: Some(anchor_points_indirect),
            anchor_points_direct: None,
            intra_tile_correlation_groups: Some(intra_tile_correlation_groups),
            covariance_direct: None,
        }
    }

    pub fn storage_type(&self) -> StorageType {
        self.storage_type
    }

    pub fn anchor_points_indirect(&self) -> Option<&[AnchorPointIndirect]> {
        self.anchor_points_indirect.as_deref()
    }

    pub fn anchor_points_direct(&self) -> Option<&[AnchorPointDirect]> {
        self.anchor_points_direct.as_deref()
    }

    pub fn intra_tile_correlation_groups(&self) -> Option<&[CorrelationGroup]> {
        self.intra_tile_correlation_groups.as_deref()
    }

    pub fn covariance_direct(&self) -> Option<Matrix3> {
        self.covariance_direct
    }
}
