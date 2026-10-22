//! `Source/Shaders/Builtin/Constants`.
//!
//! Each constant is one `shaders.push` line in [`register`]. Its name is the
//! `czm_` key `CzmBuiltins.js` gives it, which is also the GLSL identifier.
//! The source sits next to this file in `constants/<stem>.glsl`.

use super::super::ShaderModule;

/// `czm_degreesPerRadian`, the exact text of `Constants/degreesPerRadian.js`.
pub static DEGREES_PER_RADIAN: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/degreesPerRadian",
    name: "czm_degreesPerRadian",
    source: include_str!("constants/degreesPerRadian.glsl"),
};

/// `czm_depthRange`, the exact text of `Constants/depthRange.js`.
pub static DEPTH_RANGE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/depthRange",
    name: "czm_depthRange",
    source: include_str!("constants/depthRange.glsl"),
};

/// `czm_epsilon1`, the exact text of `Constants/epsilon1.js`.
pub static EPSILON1: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/epsilon1",
    name: "czm_epsilon1",
    source: include_str!("constants/epsilon1.glsl"),
};

/// `czm_epsilon2`, the exact text of `Constants/epsilon2.js`.
pub static EPSILON2: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/epsilon2",
    name: "czm_epsilon2",
    source: include_str!("constants/epsilon2.glsl"),
};

/// `czm_epsilon3`, the exact text of `Constants/epsilon3.js`.
pub static EPSILON3: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/epsilon3",
    name: "czm_epsilon3",
    source: include_str!("constants/epsilon3.glsl"),
};

/// `czm_epsilon4`, the exact text of `Constants/epsilon4.js`.
pub static EPSILON4: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/epsilon4",
    name: "czm_epsilon4",
    source: include_str!("constants/epsilon4.glsl"),
};

/// `czm_epsilon5`, the exact text of `Constants/epsilon5.js`.
pub static EPSILON5: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/epsilon5",
    name: "czm_epsilon5",
    source: include_str!("constants/epsilon5.glsl"),
};

/// `czm_epsilon6`, the exact text of `Constants/epsilon6.js`.
pub static EPSILON6: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/epsilon6",
    name: "czm_epsilon6",
    source: include_str!("constants/epsilon6.glsl"),
};

/// `czm_epsilon7`, the exact text of `Constants/epsilon7.js`.
pub static EPSILON7: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/epsilon7",
    name: "czm_epsilon7",
    source: include_str!("constants/epsilon7.glsl"),
};

/// `czm_infinity`, the exact text of `Constants/infinity.js`.
pub static INFINITY: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/infinity",
    name: "czm_infinity",
    source: include_str!("constants/infinity.glsl"),
};

/// `czm_oneOverPi`, the exact text of `Constants/oneOverPi.js`.
pub static ONE_OVER_PI: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/oneOverPi",
    name: "czm_oneOverPi",
    source: include_str!("constants/oneOverPi.glsl"),
};

/// `czm_oneOverTwoPi`, the exact text of `Constants/oneOverTwoPi.js`.
pub static ONE_OVER_TWO_PI: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/oneOverTwoPi",
    name: "czm_oneOverTwoPi",
    source: include_str!("constants/oneOverTwoPi.glsl"),
};

/// `czm_passCesium3DTile`, the exact text of `Constants/passCesium3DTile.js`.
pub static PASS_CESIUM3D_TILE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passCesium3DTile",
    name: "czm_passCesium3DTile",
    source: include_str!("constants/passCesium3DTile.glsl"),
};

/// `czm_passCesium3DTileClassification`, the exact text of `Constants/passCesium3DTileClassification.js`.
pub static PASS_CESIUM3D_TILE_CLASSIFICATION: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passCesium3DTileClassification",
    name: "czm_passCesium3DTileClassification",
    source: include_str!("constants/passCesium3DTileClassification.glsl"),
};

/// `czm_passCesium3DTileClassificationIgnoreShow`, the exact text of `Constants/passCesium3DTileClassificationIgnoreShow.js`.
pub static PASS_CESIUM3D_TILE_CLASSIFICATION_IGNORE_SHOW: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passCesium3DTileClassificationIgnoreShow",
    name: "czm_passCesium3DTileClassificationIgnoreShow",
    source: include_str!("constants/passCesium3DTileClassificationIgnoreShow.glsl"),
};

/// `czm_passCesium3DTileEdges`, the exact text of `Constants/passCesium3DTileEdges.js`.
pub static PASS_CESIUM3D_TILE_EDGES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passCesium3DTileEdges",
    name: "czm_passCesium3DTileEdges",
    source: include_str!("constants/passCesium3DTileEdges.glsl"),
};

/// `czm_passClassification`, the exact text of `Constants/passClassification.js`.
pub static PASS_CLASSIFICATION: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passClassification",
    name: "czm_passClassification",
    source: include_str!("constants/passClassification.glsl"),
};

/// `czm_passCompute`, the exact text of `Constants/passCompute.js`.
pub static PASS_COMPUTE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passCompute",
    name: "czm_passCompute",
    source: include_str!("constants/passCompute.glsl"),
};

/// `czm_passEnvironment`, the exact text of `Constants/passEnvironment.js`.
pub static PASS_ENVIRONMENT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passEnvironment",
    name: "czm_passEnvironment",
    source: include_str!("constants/passEnvironment.glsl"),
};

/// `czm_passGaussianSplats`, the exact text of `Constants/passGaussianSplats.js`.
pub static PASS_GAUSSIAN_SPLATS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passGaussianSplats",
    name: "czm_passGaussianSplats",
    source: include_str!("constants/passGaussianSplats.glsl"),
};

/// `czm_passGlobe`, the exact text of `Constants/passGlobe.js`.
pub static PASS_GLOBE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passGlobe",
    name: "czm_passGlobe",
    source: include_str!("constants/passGlobe.glsl"),
};

/// `czm_passOpaque`, the exact text of `Constants/passOpaque.js`.
pub static PASS_OPAQUE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passOpaque",
    name: "czm_passOpaque",
    source: include_str!("constants/passOpaque.glsl"),
};

/// `czm_passOverlay`, the exact text of `Constants/passOverlay.js`.
pub static PASS_OVERLAY: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passOverlay",
    name: "czm_passOverlay",
    source: include_str!("constants/passOverlay.glsl"),
};

/// `czm_passTerrainClassification`, the exact text of `Constants/passTerrainClassification.js`.
pub static PASS_TERRAIN_CLASSIFICATION: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passTerrainClassification",
    name: "czm_passTerrainClassification",
    source: include_str!("constants/passTerrainClassification.glsl"),
};

/// `czm_passTranslucent`, the exact text of `Constants/passTranslucent.js`.
pub static PASS_TRANSLUCENT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passTranslucent",
    name: "czm_passTranslucent",
    source: include_str!("constants/passTranslucent.glsl"),
};

/// `czm_passVoxels`, the exact text of `Constants/passVoxels.js`.
pub static PASS_VOXELS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/passVoxels",
    name: "czm_passVoxels",
    source: include_str!("constants/passVoxels.glsl"),
};

/// `czm_pi`, the exact text of `Constants/pi.js`.
pub static PI: ShaderModule =
    ShaderModule { id: "Shaders/Builtin/Constants/pi", name: "czm_pi", source: include_str!("constants/pi.glsl") };

/// `czm_piOverFour`, the exact text of `Constants/piOverFour.js`.
pub static PI_OVER_FOUR: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/piOverFour",
    name: "czm_piOverFour",
    source: include_str!("constants/piOverFour.glsl"),
};

/// `czm_piOverSix`, the exact text of `Constants/piOverSix.js`.
pub static PI_OVER_SIX: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/piOverSix",
    name: "czm_piOverSix",
    source: include_str!("constants/piOverSix.glsl"),
};

/// `czm_piOverThree`, the exact text of `Constants/piOverThree.js`.
pub static PI_OVER_THREE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/piOverThree",
    name: "czm_piOverThree",
    source: include_str!("constants/piOverThree.glsl"),
};

/// `czm_piOverTwo`, the exact text of `Constants/piOverTwo.js`.
pub static PI_OVER_TWO: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/piOverTwo",
    name: "czm_piOverTwo",
    source: include_str!("constants/piOverTwo.glsl"),
};

/// `czm_radiansPerDegree`, the exact text of `Constants/radiansPerDegree.js`.
pub static RADIANS_PER_DEGREE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/radiansPerDegree",
    name: "czm_radiansPerDegree",
    source: include_str!("constants/radiansPerDegree.glsl"),
};

/// `czm_sceneMode2D`, the exact text of `Constants/sceneMode2D.js`.
pub static SCENE_MODE2D: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/sceneMode2D",
    name: "czm_sceneMode2D",
    source: include_str!("constants/sceneMode2D.glsl"),
};

/// `czm_sceneMode3D`, the exact text of `Constants/sceneMode3D.js`.
pub static SCENE_MODE3D: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/sceneMode3D",
    name: "czm_sceneMode3D",
    source: include_str!("constants/sceneMode3D.glsl"),
};

/// `czm_sceneModeColumbusView`, the exact text of `Constants/sceneModeColumbusView.js`.
pub static SCENE_MODE_COLUMBUS_VIEW: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/sceneModeColumbusView",
    name: "czm_sceneModeColumbusView",
    source: include_str!("constants/sceneModeColumbusView.glsl"),
};

/// `czm_sceneModeMorphing`, the exact text of `Constants/sceneModeMorphing.js`.
pub static SCENE_MODE_MORPHING: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/sceneModeMorphing",
    name: "czm_sceneModeMorphing",
    source: include_str!("constants/sceneModeMorphing.glsl"),
};

/// `czm_solarRadius`, the exact text of `Constants/solarRadius.js`.
pub static SOLAR_RADIUS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/solarRadius",
    name: "czm_solarRadius",
    source: include_str!("constants/solarRadius.glsl"),
};

/// `czm_threePiOver2`, the exact text of `Constants/threePiOver2.js`.
pub static THREE_PI_OVER2: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/threePiOver2",
    name: "czm_threePiOver2",
    source: include_str!("constants/threePiOver2.glsl"),
};

/// `czm_twoPi`, the exact text of `Constants/twoPi.js`.
pub static TWO_PI: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/twoPi",
    name: "czm_twoPi",
    source: include_str!("constants/twoPi.glsl"),
};

/// `czm_webMercatorMaxLatitude`, the exact text of `Constants/webMercatorMaxLatitude.js`.
pub static WEB_MERCATOR_MAX_LATITUDE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Constants/webMercatorMaxLatitude",
    name: "czm_webMercatorMaxLatitude",
    source: include_str!("constants/webMercatorMaxLatitude.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&DEGREES_PER_RADIAN);
    shaders.push(&DEPTH_RANGE);
    shaders.push(&EPSILON1);
    shaders.push(&EPSILON2);
    shaders.push(&EPSILON3);
    shaders.push(&EPSILON4);
    shaders.push(&EPSILON5);
    shaders.push(&EPSILON6);
    shaders.push(&EPSILON7);
    shaders.push(&INFINITY);
    shaders.push(&ONE_OVER_PI);
    shaders.push(&ONE_OVER_TWO_PI);
    shaders.push(&PASS_CESIUM3D_TILE);
    shaders.push(&PASS_CESIUM3D_TILE_CLASSIFICATION);
    shaders.push(&PASS_CESIUM3D_TILE_CLASSIFICATION_IGNORE_SHOW);
    shaders.push(&PASS_CESIUM3D_TILE_EDGES);
    shaders.push(&PASS_CLASSIFICATION);
    shaders.push(&PASS_COMPUTE);
    shaders.push(&PASS_ENVIRONMENT);
    shaders.push(&PASS_GAUSSIAN_SPLATS);
    shaders.push(&PASS_GLOBE);
    shaders.push(&PASS_OPAQUE);
    shaders.push(&PASS_OVERLAY);
    shaders.push(&PASS_TERRAIN_CLASSIFICATION);
    shaders.push(&PASS_TRANSLUCENT);
    shaders.push(&PASS_VOXELS);
    shaders.push(&PI);
    shaders.push(&PI_OVER_FOUR);
    shaders.push(&PI_OVER_SIX);
    shaders.push(&PI_OVER_THREE);
    shaders.push(&PI_OVER_TWO);
    shaders.push(&RADIANS_PER_DEGREE);
    shaders.push(&SCENE_MODE2D);
    shaders.push(&SCENE_MODE3D);
    shaders.push(&SCENE_MODE_COLUMBUS_VIEW);
    shaders.push(&SCENE_MODE_MORPHING);
    shaders.push(&SOLAR_RADIUS);
    shaders.push(&THREE_PI_OVER2);
    shaders.push(&TWO_PI);
    shaders.push(&WEB_MERCATOR_MAX_LATITUDE);
}
