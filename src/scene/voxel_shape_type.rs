//! Voxel shape identifiers and pinned default coordinate bounds. Constructor
//! dispatch for cylinder/ellipsoid is not yet implemented.
use crate::core::Cartesian3;
string_enum!(VoxelShapeType { Box = "BOX" => "BOX", Ellipsoid = "ELLIPSOID" => "ELLIPSOID", Cylinder = "CYLINDER" => "CYLINDER" });
impl VoxelShapeType {
    pub fn create_shape(self) -> Box<dyn crate::scene::voxel_shape::VoxelShape> {
        match self {
            Self::Box => Box::new(crate::scene::voxel_box_shape::VoxelBoxShape::default()),
            Self::Cylinder => Box::new(crate::scene::voxel_shape::VoxelCylinderShape::default()),
            Self::Ellipsoid => Box::new(crate::scene::voxel_shape::VoxelEllipsoidShape::default()),
        }
    }
    pub fn new_shape(self) -> Box<dyn crate::scene::voxel_shape::VoxelShape> { self.create_shape() }
    pub fn create(self) -> Box<dyn crate::scene::voxel_shape::VoxelShape> { self.create_shape() }
    pub fn min_bounds(self) -> Cartesian3 {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            Self::Box => Cartesian3::new(-1.0, -1.0, -1.0),
            Self::Ellipsoid => Cartesian3::new(-PI, -FRAC_PI_2, -6356752.314245179),
            Self::Cylinder => Cartesian3::new(0.0, -PI, -1.0),
        }
    }
    pub fn max_bounds(self) -> Cartesian3 {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            Self::Box => Cartesian3::new(1.0, 1.0, 1.0),
            Self::Ellipsoid => Cartesian3::new(PI, FRAC_PI_2, 63781370.0),
            Self::Cylinder => Cartesian3::new(1.0, PI, 1.0),
        }
    }
}
