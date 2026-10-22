//! `Source/Shaders/Voxels`.
//!
//! Each shader is one `shaders.push` line in [`register`]. Its name is the
//! file stem, which is how Scene imports it
//! (`import VoxelFS from "../Shaders/Voxels/VoxelFS.js"`).
//! Upstream gives these modules no `@alias`. The source sits next to this
//! file as `glsl/<stem>.glsl`.

use super::ShaderModule;

/// `IntersectBox`, the exact text of `Voxels/IntersectBox.js`.
pub static INTERSECT_BOX: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/IntersectBox",
    name: "IntersectBox",
    source: include_str!("glsl/IntersectBox.glsl"),
};

/// `IntersectCylinder`, the exact text of `Voxels/IntersectCylinder.js`.
pub static INTERSECT_CYLINDER: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/IntersectCylinder",
    name: "IntersectCylinder",
    source: include_str!("glsl/IntersectCylinder.glsl"),
};

/// `IntersectDepth`, the exact text of `Voxels/IntersectDepth.js`.
pub static INTERSECT_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/IntersectDepth",
    name: "IntersectDepth",
    source: include_str!("glsl/IntersectDepth.glsl"),
};

/// `IntersectEllipsoid`, the exact text of `Voxels/IntersectEllipsoid.js`.
pub static INTERSECT_ELLIPSOID: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/IntersectEllipsoid",
    name: "IntersectEllipsoid",
    source: include_str!("glsl/IntersectEllipsoid.glsl"),
};

/// `IntersectLongitude`, the exact text of `Voxels/IntersectLongitude.js`.
pub static INTERSECT_LONGITUDE: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/IntersectLongitude",
    name: "IntersectLongitude",
    source: include_str!("glsl/IntersectLongitude.glsl"),
};

/// `IntersectPlane`, the exact text of `Voxels/IntersectPlane.js`.
pub static INTERSECT_PLANE: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/IntersectPlane",
    name: "IntersectPlane",
    source: include_str!("glsl/IntersectPlane.glsl"),
};

/// `Intersection`, the exact text of `Voxels/Intersection.js`.
pub static INTERSECTION: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/Intersection",
    name: "Intersection",
    source: include_str!("glsl/Intersection.glsl"),
};

/// `IntersectionUtils`, the exact text of `Voxels/IntersectionUtils.js`.
pub static INTERSECTION_UTILS: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/IntersectionUtils",
    name: "IntersectionUtils",
    source: include_str!("glsl/IntersectionUtils.glsl"),
};

/// `Megatexture`, the exact text of `Voxels/Megatexture.js`.
pub static MEGATEXTURE: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/Megatexture",
    name: "Megatexture",
    source: include_str!("glsl/Megatexture.glsl"),
};

/// `Octree`, the exact text of `Voxels/Octree.js`.
pub static OCTREE: ShaderModule =
    ShaderModule { id: "Shaders/Voxels/Octree", name: "Octree", source: include_str!("glsl/Octree.glsl") };

/// `VoxelFS`, the exact text of `Voxels/VoxelFS.js`.
pub static VOXEL_FS: ShaderModule =
    ShaderModule { id: "Shaders/Voxels/VoxelFS", name: "VoxelFS", source: include_str!("glsl/VoxelFS.glsl") };

/// `VoxelUtils`, the exact text of `Voxels/VoxelUtils.js`.
pub static VOXEL_UTILS: ShaderModule =
    ShaderModule { id: "Shaders/Voxels/VoxelUtils", name: "VoxelUtils", source: include_str!("glsl/VoxelUtils.glsl") };

/// `VoxelVS`, the exact text of `Voxels/VoxelVS.js`.
pub static VOXEL_VS: ShaderModule =
    ShaderModule { id: "Shaders/Voxels/VoxelVS", name: "VoxelVS", source: include_str!("glsl/VoxelVS.glsl") };

/// `convertLocalToBoxUv`, the exact text of `Voxels/convertLocalToBoxUv.js`.
pub static CONVERT_LOCAL_TO_BOX_UV: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/convertLocalToBoxUv",
    name: "convertLocalToBoxUv",
    source: include_str!("glsl/convertLocalToBoxUv.glsl"),
};

/// `convertLocalToCylinderUv`, the exact text of `Voxels/convertLocalToCylinderUv.js`.
pub static CONVERT_LOCAL_TO_CYLINDER_UV: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/convertLocalToCylinderUv",
    name: "convertLocalToCylinderUv",
    source: include_str!("glsl/convertLocalToCylinderUv.glsl"),
};

/// `convertLocalToEllipsoidUv`, the exact text of `Voxels/convertLocalToEllipsoidUv.js`.
pub static CONVERT_LOCAL_TO_ELLIPSOID_UV: ShaderModule = ShaderModule {
    id: "Shaders/Voxels/convertLocalToEllipsoidUv",
    name: "convertLocalToEllipsoidUv",
    source: include_str!("glsl/convertLocalToEllipsoidUv.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&INTERSECT_BOX);
    shaders.push(&INTERSECT_CYLINDER);
    shaders.push(&INTERSECT_DEPTH);
    shaders.push(&INTERSECT_ELLIPSOID);
    shaders.push(&INTERSECT_LONGITUDE);
    shaders.push(&INTERSECT_PLANE);
    shaders.push(&INTERSECTION);
    shaders.push(&INTERSECTION_UTILS);
    shaders.push(&MEGATEXTURE);
    shaders.push(&OCTREE);
    shaders.push(&VOXEL_FS);
    shaders.push(&VOXEL_UTILS);
    shaders.push(&VOXEL_VS);
    shaders.push(&CONVERT_LOCAL_TO_BOX_UV);
    shaders.push(&CONVERT_LOCAL_TO_CYLINDER_UV);
    shaders.push(&CONVERT_LOCAL_TO_ELLIPSOID_UV);
}
