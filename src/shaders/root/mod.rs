//! Root `Source/Shaders/*.js` modules.
//!
//! Each root shader is one `shaders.push` line in [`register`]. Its name is
//! the file stem: `PolylineCommon` for `PolylineCommon.js`. Upstream 23.0.0
//! gives these modules no `@alias`. The source sits next to this file as
//! `glsl/<stem>.glsl`.
//!
//! The text is the default export, not the upstream `.glsl` file. Cesium's
//! build does not escape backslashes, so in `PolylineShadowVolumeVS` and
//! `Vector3DTileClampedPolylinesVS` the `\ ` of a comment diagram becomes a
//! space. The export is what the engine compiles.

use super::ShaderModule;

/// `AdjustTranslucentFS`, the exact text of `AdjustTranslucentFS.js`.
pub static ADJUST_TRANSLUCENT_FS: ShaderModule = ShaderModule {
    id: "Shaders/AdjustTranslucentFS",
    name: "AdjustTranslucentFS",
    source: include_str!("glsl/AdjustTranslucentFS.glsl"),
};

/// `AtmosphereCommon`, the exact text of `AtmosphereCommon.js`.
pub static ATMOSPHERE_COMMON: ShaderModule = ShaderModule {
    id: "Shaders/AtmosphereCommon",
    name: "AtmosphereCommon",
    source: include_str!("glsl/AtmosphereCommon.glsl"),
};

/// `BillboardCollectionFS`, the exact text of `BillboardCollectionFS.js`.
pub static BILLBOARD_COLLECTION_FS: ShaderModule = ShaderModule {
    id: "Shaders/BillboardCollectionFS",
    name: "BillboardCollectionFS",
    source: include_str!("glsl/BillboardCollectionFS.glsl"),
};

/// `BillboardCollectionVS`, the exact text of `BillboardCollectionVS.js`.
pub static BILLBOARD_COLLECTION_VS: ShaderModule = ShaderModule {
    id: "Shaders/BillboardCollectionVS",
    name: "BillboardCollectionVS",
    source: include_str!("glsl/BillboardCollectionVS.glsl"),
};

/// `BrdfLutGeneratorFS`, the exact text of `BrdfLutGeneratorFS.js`.
pub static BRDF_LUT_GENERATOR_FS: ShaderModule = ShaderModule {
    id: "Shaders/BrdfLutGeneratorFS",
    name: "BrdfLutGeneratorFS",
    source: include_str!("glsl/BrdfLutGeneratorFS.glsl"),
};

/// `CloudCollectionFS`, the exact text of `CloudCollectionFS.js`.
pub static CLOUD_COLLECTION_FS: ShaderModule = ShaderModule {
    id: "Shaders/CloudCollectionFS",
    name: "CloudCollectionFS",
    source: include_str!("glsl/CloudCollectionFS.glsl"),
};

/// `CloudCollectionVS`, the exact text of `CloudCollectionVS.js`.
pub static CLOUD_COLLECTION_VS: ShaderModule = ShaderModule {
    id: "Shaders/CloudCollectionVS",
    name: "CloudCollectionVS",
    source: include_str!("glsl/CloudCollectionVS.glsl"),
};

/// `CloudNoiseFS`, the exact text of `CloudNoiseFS.js`.
pub static CLOUD_NOISE_FS: ShaderModule =
    ShaderModule { id: "Shaders/CloudNoiseFS", name: "CloudNoiseFS", source: include_str!("glsl/CloudNoiseFS.glsl") };

/// `CloudNoiseVS`, the exact text of `CloudNoiseVS.js`.
pub static CLOUD_NOISE_VS: ShaderModule =
    ShaderModule { id: "Shaders/CloudNoiseVS", name: "CloudNoiseVS", source: include_str!("glsl/CloudNoiseVS.glsl") };

/// `CompareAndPackTranslucentDepth`, the exact text of `CompareAndPackTranslucentDepth.js`.
pub static COMPARE_AND_PACK_TRANSLUCENT_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/CompareAndPackTranslucentDepth",
    name: "CompareAndPackTranslucentDepth",
    source: include_str!("glsl/CompareAndPackTranslucentDepth.glsl"),
};

/// `CompositeOITFS`, the exact text of `CompositeOITFS.js`.
pub static COMPOSITE_OITFS: ShaderModule = ShaderModule {
    id: "Shaders/CompositeOITFS",
    name: "CompositeOITFS",
    source: include_str!("glsl/CompositeOITFS.glsl"),
};

/// `ComputeIrradianceFS`, the exact text of `ComputeIrradianceFS.js`.
pub static COMPUTE_IRRADIANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/ComputeIrradianceFS",
    name: "ComputeIrradianceFS",
    source: include_str!("glsl/ComputeIrradianceFS.glsl"),
};

/// `ComputeRadianceMapFS`, the exact text of `ComputeRadianceMapFS.js`.
pub static COMPUTE_RADIANCE_MAP_FS: ShaderModule = ShaderModule {
    id: "Shaders/ComputeRadianceMapFS",
    name: "ComputeRadianceMapFS",
    source: include_str!("glsl/ComputeRadianceMapFS.glsl"),
};

/// `ConvolveSpecularMapFS`, the exact text of `ConvolveSpecularMapFS.js`.
pub static CONVOLVE_SPECULAR_MAP_FS: ShaderModule = ShaderModule {
    id: "Shaders/ConvolveSpecularMapFS",
    name: "ConvolveSpecularMapFS",
    source: include_str!("glsl/ConvolveSpecularMapFS.glsl"),
};

/// `ConvolveSpecularMapVS`, the exact text of `ConvolveSpecularMapVS.js`.
pub static CONVOLVE_SPECULAR_MAP_VS: ShaderModule = ShaderModule {
    id: "Shaders/ConvolveSpecularMapVS",
    name: "ConvolveSpecularMapVS",
    source: include_str!("glsl/ConvolveSpecularMapVS.glsl"),
};

/// `CubeMapPanoramaVS`, the exact text of `CubeMapPanoramaVS.js`.
pub static CUBE_MAP_PANORAMA_VS: ShaderModule = ShaderModule {
    id: "Shaders/CubeMapPanoramaVS",
    name: "CubeMapPanoramaVS",
    source: include_str!("glsl/CubeMapPanoramaVS.glsl"),
};

/// `DepthPlaneFS`, the exact text of `DepthPlaneFS.js`.
pub static DEPTH_PLANE_FS: ShaderModule =
    ShaderModule { id: "Shaders/DepthPlaneFS", name: "DepthPlaneFS", source: include_str!("glsl/DepthPlaneFS.glsl") };

/// `DepthPlaneVS`, the exact text of `DepthPlaneVS.js`.
pub static DEPTH_PLANE_VS: ShaderModule =
    ShaderModule { id: "Shaders/DepthPlaneVS", name: "DepthPlaneVS", source: include_str!("glsl/DepthPlaneVS.glsl") };

/// `EllipsoidFS`, the exact text of `EllipsoidFS.js`.
pub static ELLIPSOID_FS: ShaderModule =
    ShaderModule { id: "Shaders/EllipsoidFS", name: "EllipsoidFS", source: include_str!("glsl/EllipsoidFS.glsl") };

/// `EllipsoidVS`, the exact text of `EllipsoidVS.js`.
pub static ELLIPSOID_VS: ShaderModule =
    ShaderModule { id: "Shaders/EllipsoidVS", name: "EllipsoidVS", source: include_str!("glsl/EllipsoidVS.glsl") };

/// `FXAA3_11`, the exact text of `FXAA3_11.js`.
pub static FXAA3_11: ShaderModule =
    ShaderModule { id: "Shaders/FXAA3_11", name: "FXAA3_11", source: include_str!("glsl/FXAA3_11.glsl") };

/// `GlobeFS`, the exact text of `GlobeFS.js`.
pub static GLOBE_FS: ShaderModule =
    ShaderModule { id: "Shaders/GlobeFS", name: "GlobeFS", source: include_str!("glsl/GlobeFS.glsl") };

/// `GlobeVS`, the exact text of `GlobeVS.js`.
pub static GLOBE_VS: ShaderModule =
    ShaderModule { id: "Shaders/GlobeVS", name: "GlobeVS", source: include_str!("glsl/GlobeVS.glsl") };

/// `GroundAtmosphere`, the exact text of `GroundAtmosphere.js`.
pub static GROUND_ATMOSPHERE: ShaderModule = ShaderModule {
    id: "Shaders/GroundAtmosphere",
    name: "GroundAtmosphere",
    source: include_str!("glsl/GroundAtmosphere.glsl"),
};

/// `PointPrimitiveCollectionFS`, the exact text of `PointPrimitiveCollectionFS.js`.
pub static POINT_PRIMITIVE_COLLECTION_FS: ShaderModule = ShaderModule {
    id: "Shaders/PointPrimitiveCollectionFS",
    name: "PointPrimitiveCollectionFS",
    source: include_str!("glsl/PointPrimitiveCollectionFS.glsl"),
};

/// `PointPrimitiveCollectionVS`, the exact text of `PointPrimitiveCollectionVS.js`.
pub static POINT_PRIMITIVE_COLLECTION_VS: ShaderModule = ShaderModule {
    id: "Shaders/PointPrimitiveCollectionVS",
    name: "PointPrimitiveCollectionVS",
    source: include_str!("glsl/PointPrimitiveCollectionVS.glsl"),
};

/// `PolygonSignedDistanceFS`, the exact text of `PolygonSignedDistanceFS.js`.
pub static POLYGON_SIGNED_DISTANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/PolygonSignedDistanceFS",
    name: "PolygonSignedDistanceFS",
    source: include_str!("glsl/PolygonSignedDistanceFS.glsl"),
};

/// `PolylineCommon`, the exact text of `PolylineCommon.js`.
pub static POLYLINE_COMMON: ShaderModule = ShaderModule {
    id: "Shaders/PolylineCommon",
    name: "PolylineCommon",
    source: include_str!("glsl/PolylineCommon.glsl"),
};

/// `PolylineFS`, the exact text of `PolylineFS.js`.
pub static POLYLINE_FS: ShaderModule =
    ShaderModule { id: "Shaders/PolylineFS", name: "PolylineFS", source: include_str!("glsl/PolylineFS.glsl") };

/// `PolylineShadowVolumeFS`, the exact text of `PolylineShadowVolumeFS.js`.
pub static POLYLINE_SHADOW_VOLUME_FS: ShaderModule = ShaderModule {
    id: "Shaders/PolylineShadowVolumeFS",
    name: "PolylineShadowVolumeFS",
    source: include_str!("glsl/PolylineShadowVolumeFS.glsl"),
};

/// `PolylineShadowVolumeMorphFS`, the exact text of `PolylineShadowVolumeMorphFS.js`.
pub static POLYLINE_SHADOW_VOLUME_MORPH_FS: ShaderModule = ShaderModule {
    id: "Shaders/PolylineShadowVolumeMorphFS",
    name: "PolylineShadowVolumeMorphFS",
    source: include_str!("glsl/PolylineShadowVolumeMorphFS.glsl"),
};

/// `PolylineShadowVolumeMorphVS`, the exact text of `PolylineShadowVolumeMorphVS.js`.
pub static POLYLINE_SHADOW_VOLUME_MORPH_VS: ShaderModule = ShaderModule {
    id: "Shaders/PolylineShadowVolumeMorphVS",
    name: "PolylineShadowVolumeMorphVS",
    source: include_str!("glsl/PolylineShadowVolumeMorphVS.glsl"),
};

/// `PolylineShadowVolumeVS`, the exact text of `PolylineShadowVolumeVS.js`.
pub static POLYLINE_SHADOW_VOLUME_VS: ShaderModule = ShaderModule {
    id: "Shaders/PolylineShadowVolumeVS",
    name: "PolylineShadowVolumeVS",
    source: include_str!("glsl/PolylineShadowVolumeVS.glsl"),
};

/// `PolylineVS`, the exact text of `PolylineVS.js`.
pub static POLYLINE_VS: ShaderModule =
    ShaderModule { id: "Shaders/PolylineVS", name: "PolylineVS", source: include_str!("glsl/PolylineVS.glsl") };

/// `PrimitiveGaussianSplatFS`, the exact text of `PrimitiveGaussianSplatFS.js`.
pub static PRIMITIVE_GAUSSIAN_SPLAT_FS: ShaderModule = ShaderModule {
    id: "Shaders/PrimitiveGaussianSplatFS",
    name: "PrimitiveGaussianSplatFS",
    source: include_str!("glsl/PrimitiveGaussianSplatFS.glsl"),
};

/// `PrimitiveGaussianSplatVS`, the exact text of `PrimitiveGaussianSplatVS.js`.
pub static PRIMITIVE_GAUSSIAN_SPLAT_VS: ShaderModule = ShaderModule {
    id: "Shaders/PrimitiveGaussianSplatVS",
    name: "PrimitiveGaussianSplatVS",
    source: include_str!("glsl/PrimitiveGaussianSplatVS.glsl"),
};

/// `ReprojectWebMercatorFS`, the exact text of `ReprojectWebMercatorFS.js`.
pub static REPROJECT_WEB_MERCATOR_FS: ShaderModule = ShaderModule {
    id: "Shaders/ReprojectWebMercatorFS",
    name: "ReprojectWebMercatorFS",
    source: include_str!("glsl/ReprojectWebMercatorFS.glsl"),
};

/// `ReprojectWebMercatorVS`, the exact text of `ReprojectWebMercatorVS.js`.
pub static REPROJECT_WEB_MERCATOR_VS: ShaderModule = ShaderModule {
    id: "Shaders/ReprojectWebMercatorVS",
    name: "ReprojectWebMercatorVS",
    source: include_str!("glsl/ReprojectWebMercatorVS.glsl"),
};

/// `ShadowVolumeAppearanceFS`, the exact text of `ShadowVolumeAppearanceFS.js`.
pub static SHADOW_VOLUME_APPEARANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/ShadowVolumeAppearanceFS",
    name: "ShadowVolumeAppearanceFS",
    source: include_str!("glsl/ShadowVolumeAppearanceFS.glsl"),
};

/// `ShadowVolumeAppearanceVS`, the exact text of `ShadowVolumeAppearanceVS.js`.
pub static SHADOW_VOLUME_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/ShadowVolumeAppearanceVS",
    name: "ShadowVolumeAppearanceVS",
    source: include_str!("glsl/ShadowVolumeAppearanceVS.glsl"),
};

/// `ShadowVolumeFS`, the exact text of `ShadowVolumeFS.js`.
pub static SHADOW_VOLUME_FS: ShaderModule = ShaderModule {
    id: "Shaders/ShadowVolumeFS",
    name: "ShadowVolumeFS",
    source: include_str!("glsl/ShadowVolumeFS.glsl"),
};

/// `SkyAtmosphereCommon`, the exact text of `SkyAtmosphereCommon.js`.
pub static SKY_ATMOSPHERE_COMMON: ShaderModule = ShaderModule {
    id: "Shaders/SkyAtmosphereCommon",
    name: "SkyAtmosphereCommon",
    source: include_str!("glsl/SkyAtmosphereCommon.glsl"),
};

/// `SkyAtmosphereFS`, the exact text of `SkyAtmosphereFS.js`.
pub static SKY_ATMOSPHERE_FS: ShaderModule = ShaderModule {
    id: "Shaders/SkyAtmosphereFS",
    name: "SkyAtmosphereFS",
    source: include_str!("glsl/SkyAtmosphereFS.glsl"),
};

/// `SkyAtmosphereVS`, the exact text of `SkyAtmosphereVS.js`.
pub static SKY_ATMOSPHERE_VS: ShaderModule = ShaderModule {
    id: "Shaders/SkyAtmosphereVS",
    name: "SkyAtmosphereVS",
    source: include_str!("glsl/SkyAtmosphereVS.glsl"),
};

/// `SkyBoxFS`, the exact text of `SkyBoxFS.js`.
pub static SKY_BOX_FS: ShaderModule =
    ShaderModule { id: "Shaders/SkyBoxFS", name: "SkyBoxFS", source: include_str!("glsl/SkyBoxFS.glsl") };

/// `SkyBoxVS`, the exact text of `SkyBoxVS.js`.
pub static SKY_BOX_VS: ShaderModule =
    ShaderModule { id: "Shaders/SkyBoxVS", name: "SkyBoxVS", source: include_str!("glsl/SkyBoxVS.glsl") };

/// `SunFS`, the exact text of `SunFS.js`.
pub static SUN_FS: ShaderModule =
    ShaderModule { id: "Shaders/SunFS", name: "SunFS", source: include_str!("glsl/SunFS.glsl") };

/// `SunTextureFS`, the exact text of `SunTextureFS.js`.
pub static SUN_TEXTURE_FS: ShaderModule =
    ShaderModule { id: "Shaders/SunTextureFS", name: "SunTextureFS", source: include_str!("glsl/SunTextureFS.glsl") };

/// `SunVS`, the exact text of `SunVS.js`.
pub static SUN_VS: ShaderModule =
    ShaderModule { id: "Shaders/SunVS", name: "SunVS", source: include_str!("glsl/SunVS.glsl") };

/// `Vector3DTileClampedPolylinesFS`, the exact text of `Vector3DTileClampedPolylinesFS.js`.
pub static VECTOR3D_TILE_CLAMPED_POLYLINES_FS: ShaderModule = ShaderModule {
    id: "Shaders/Vector3DTileClampedPolylinesFS",
    name: "Vector3DTileClampedPolylinesFS",
    source: include_str!("glsl/Vector3DTileClampedPolylinesFS.glsl"),
};

/// `Vector3DTileClampedPolylinesVS`, the exact text of `Vector3DTileClampedPolylinesVS.js`.
pub static VECTOR3D_TILE_CLAMPED_POLYLINES_VS: ShaderModule = ShaderModule {
    id: "Shaders/Vector3DTileClampedPolylinesVS",
    name: "Vector3DTileClampedPolylinesVS",
    source: include_str!("glsl/Vector3DTileClampedPolylinesVS.glsl"),
};

/// `Vector3DTilePolylinesVS`, the exact text of `Vector3DTilePolylinesVS.js`.
pub static VECTOR3D_TILE_POLYLINES_VS: ShaderModule = ShaderModule {
    id: "Shaders/Vector3DTilePolylinesVS",
    name: "Vector3DTilePolylinesVS",
    source: include_str!("glsl/Vector3DTilePolylinesVS.glsl"),
};

/// `VectorTileVS`, the exact text of `VectorTileVS.js`.
pub static VECTOR_TILE_VS: ShaderModule =
    ShaderModule { id: "Shaders/VectorTileVS", name: "VectorTileVS", source: include_str!("glsl/VectorTileVS.glsl") };

/// `ViewportQuadFS`, the exact text of `ViewportQuadFS.js`.
pub static VIEWPORT_QUAD_FS: ShaderModule = ShaderModule {
    id: "Shaders/ViewportQuadFS",
    name: "ViewportQuadFS",
    source: include_str!("glsl/ViewportQuadFS.glsl"),
};

/// `ViewportQuadVS`, the exact text of `ViewportQuadVS.js`.
pub static VIEWPORT_QUAD_VS: ShaderModule = ShaderModule {
    id: "Shaders/ViewportQuadVS",
    name: "ViewportQuadVS",
    source: include_str!("glsl/ViewportQuadVS.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&ADJUST_TRANSLUCENT_FS);
    shaders.push(&ATMOSPHERE_COMMON);
    shaders.push(&BILLBOARD_COLLECTION_FS);
    shaders.push(&BILLBOARD_COLLECTION_VS);
    shaders.push(&BRDF_LUT_GENERATOR_FS);
    shaders.push(&CLOUD_COLLECTION_FS);
    shaders.push(&CLOUD_COLLECTION_VS);
    shaders.push(&CLOUD_NOISE_FS);
    shaders.push(&CLOUD_NOISE_VS);
    shaders.push(&COMPARE_AND_PACK_TRANSLUCENT_DEPTH);
    shaders.push(&COMPOSITE_OITFS);
    shaders.push(&COMPUTE_IRRADIANCE_FS);
    shaders.push(&COMPUTE_RADIANCE_MAP_FS);
    shaders.push(&CONVOLVE_SPECULAR_MAP_FS);
    shaders.push(&CONVOLVE_SPECULAR_MAP_VS);
    shaders.push(&CUBE_MAP_PANORAMA_VS);
    shaders.push(&DEPTH_PLANE_FS);
    shaders.push(&DEPTH_PLANE_VS);
    shaders.push(&ELLIPSOID_FS);
    shaders.push(&ELLIPSOID_VS);
    shaders.push(&FXAA3_11);
    shaders.push(&GLOBE_FS);
    shaders.push(&GLOBE_VS);
    shaders.push(&GROUND_ATMOSPHERE);
    shaders.push(&POINT_PRIMITIVE_COLLECTION_FS);
    shaders.push(&POINT_PRIMITIVE_COLLECTION_VS);
    shaders.push(&POLYGON_SIGNED_DISTANCE_FS);
    shaders.push(&POLYLINE_COMMON);
    shaders.push(&POLYLINE_FS);
    shaders.push(&POLYLINE_SHADOW_VOLUME_FS);
    shaders.push(&POLYLINE_SHADOW_VOLUME_MORPH_FS);
    shaders.push(&POLYLINE_SHADOW_VOLUME_MORPH_VS);
    shaders.push(&POLYLINE_SHADOW_VOLUME_VS);
    shaders.push(&POLYLINE_VS);
    shaders.push(&PRIMITIVE_GAUSSIAN_SPLAT_FS);
    shaders.push(&PRIMITIVE_GAUSSIAN_SPLAT_VS);
    shaders.push(&REPROJECT_WEB_MERCATOR_FS);
    shaders.push(&REPROJECT_WEB_MERCATOR_VS);
    shaders.push(&SHADOW_VOLUME_APPEARANCE_FS);
    shaders.push(&SHADOW_VOLUME_APPEARANCE_VS);
    shaders.push(&SHADOW_VOLUME_FS);
    shaders.push(&SKY_ATMOSPHERE_COMMON);
    shaders.push(&SKY_ATMOSPHERE_FS);
    shaders.push(&SKY_ATMOSPHERE_VS);
    shaders.push(&SKY_BOX_FS);
    shaders.push(&SKY_BOX_VS);
    shaders.push(&SUN_FS);
    shaders.push(&SUN_TEXTURE_FS);
    shaders.push(&SUN_VS);
    shaders.push(&VECTOR3D_TILE_CLAMPED_POLYLINES_FS);
    shaders.push(&VECTOR3D_TILE_CLAMPED_POLYLINES_VS);
    shaders.push(&VECTOR3D_TILE_POLYLINES_VS);
    shaders.push(&VECTOR_TILE_VS);
    shaders.push(&VIEWPORT_QUAD_FS);
    shaders.push(&VIEWPORT_QUAD_VS);
}
