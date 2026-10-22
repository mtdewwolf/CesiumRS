//! `Source/Shaders/Builtin/Functions`.
//!
//! Each function is one `shaders.push` line in [`register`]. Its name is the
//! `czm_` key `CzmBuiltins.js` gives it, which is also the GLSL identifier.
//! The source sits next to this file in `functions/<stem>.glsl`.

use super::super::ShaderModule;

/// `czm_HSBToRGB`, the exact text of `Functions/HSBToRGB.js`.
pub static HSB_TO_RGB: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/HSBToRGB",
    name: "czm_HSBToRGB",
    source: include_str!("functions/HSBToRGB.glsl"),
};

/// `czm_HSLToRGB`, the exact text of `Functions/HSLToRGB.js`.
pub static HSL_TO_RGB: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/HSLToRGB",
    name: "czm_HSLToRGB",
    source: include_str!("functions/HSLToRGB.glsl"),
};

/// `czm_RGBToHSB`, the exact text of `Functions/RGBToHSB.js`.
pub static RGB_TO_HSB: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/RGBToHSB",
    name: "czm_RGBToHSB",
    source: include_str!("functions/RGBToHSB.glsl"),
};

/// `czm_RGBToHSL`, the exact text of `Functions/RGBToHSL.js`.
pub static RGB_TO_HSL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/RGBToHSL",
    name: "czm_RGBToHSL",
    source: include_str!("functions/RGBToHSL.glsl"),
};

/// `czm_RGBToXYZ`, the exact text of `Functions/RGBToXYZ.js`.
pub static RGB_TO_XYZ: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/RGBToXYZ",
    name: "czm_RGBToXYZ",
    source: include_str!("functions/RGBToXYZ.glsl"),
};

/// `czm_XYZToRGB`, the exact text of `Functions/XYZToRGB.js`.
pub static XYZ_TO_RGB: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/XYZToRGB",
    name: "czm_XYZToRGB",
    source: include_str!("functions/XYZToRGB.glsl"),
};

/// `czm_acesTonemapping`, the exact text of `Functions/acesTonemapping.js`.
pub static ACES_TONEMAPPING: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/acesTonemapping",
    name: "czm_acesTonemapping",
    source: include_str!("functions/acesTonemapping.glsl"),
};

/// `czm_alphaWeight`, the exact text of `Functions/alphaWeight.js`.
pub static ALPHA_WEIGHT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/alphaWeight",
    name: "czm_alphaWeight",
    source: include_str!("functions/alphaWeight.glsl"),
};

/// `czm_antialias`, the exact text of `Functions/antialias.js`.
pub static ANTIALIAS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/antialias",
    name: "czm_antialias",
    source: include_str!("functions/antialias.glsl"),
};

/// `czm_applyHSBShift`, the exact text of `Functions/applyHSBShift.js`.
pub static APPLY_HSB_SHIFT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/applyHSBShift",
    name: "czm_applyHSBShift",
    source: include_str!("functions/applyHSBShift.glsl"),
};

/// `czm_approximateSphericalCoordinates`, the exact text of `Functions/approximateSphericalCoordinates.js`.
pub static APPROXIMATE_SPHERICAL_COORDINATES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/approximateSphericalCoordinates",
    name: "czm_approximateSphericalCoordinates",
    source: include_str!("functions/approximateSphericalCoordinates.glsl"),
};

/// `czm_approximateTanh`, the exact text of `Functions/approximateTanh.js`.
pub static APPROXIMATE_TANH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/approximateTanh",
    name: "czm_approximateTanh",
    source: include_str!("functions/approximateTanh.glsl"),
};

/// `czm_backFacing`, the exact text of `Functions/backFacing.js`.
pub static BACK_FACING: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/backFacing",
    name: "czm_backFacing",
    source: include_str!("functions/backFacing.glsl"),
};

/// `czm_branchFreeTernary`, the exact text of `Functions/branchFreeTernary.js`.
pub static BRANCH_FREE_TERNARY: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/branchFreeTernary",
    name: "czm_branchFreeTernary",
    source: include_str!("functions/branchFreeTernary.glsl"),
};

/// `czm_cascadeColor`, the exact text of `Functions/cascadeColor.js`.
pub static CASCADE_COLOR: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/cascadeColor",
    name: "czm_cascadeColor",
    source: include_str!("functions/cascadeColor.glsl"),
};

/// `czm_cascadeDistance`, the exact text of `Functions/cascadeDistance.js`.
pub static CASCADE_DISTANCE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/cascadeDistance",
    name: "czm_cascadeDistance",
    source: include_str!("functions/cascadeDistance.glsl"),
};

/// `czm_cascadeMatrix`, the exact text of `Functions/cascadeMatrix.js`.
pub static CASCADE_MATRIX: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/cascadeMatrix",
    name: "czm_cascadeMatrix",
    source: include_str!("functions/cascadeMatrix.glsl"),
};

/// `czm_cascadeWeights`, the exact text of `Functions/cascadeWeights.js`.
pub static CASCADE_WEIGHTS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/cascadeWeights",
    name: "czm_cascadeWeights",
    source: include_str!("functions/cascadeWeights.glsl"),
};

/// `czm_clipPolygons`, the exact text of `Functions/clipPolygons.js`.
pub static CLIP_POLYGONS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/clipPolygons",
    name: "czm_clipPolygons",
    source: include_str!("functions/clipPolygons.glsl"),
};

/// `czm_columbusViewMorph`, the exact text of `Functions/columbusViewMorph.js`.
pub static COLUMBUS_VIEW_MORPH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/columbusViewMorph",
    name: "czm_columbusViewMorph",
    source: include_str!("functions/columbusViewMorph.glsl"),
};

/// `czm_computeAtmosphereColor`, the exact text of `Functions/computeAtmosphereColor.js`.
pub static COMPUTE_ATMOSPHERE_COLOR: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/computeAtmosphereColor",
    name: "czm_computeAtmosphereColor",
    source: include_str!("functions/computeAtmosphereColor.glsl"),
};

/// `czm_computeGroundAtmosphereScattering`, the exact text of `Functions/computeGroundAtmosphereScattering.js`.
pub static COMPUTE_GROUND_ATMOSPHERE_SCATTERING: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/computeGroundAtmosphereScattering",
    name: "czm_computeGroundAtmosphereScattering",
    source: include_str!("functions/computeGroundAtmosphereScattering.glsl"),
};

/// `czm_computePosition`, the exact text of `Functions/computePosition.js`.
pub static COMPUTE_POSITION: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/computePosition",
    name: "czm_computePosition",
    source: include_str!("functions/computePosition.glsl"),
};

/// `czm_computeScattering`, the exact text of `Functions/computeScattering.js`.
pub static COMPUTE_SCATTERING: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/computeScattering",
    name: "czm_computeScattering",
    source: include_str!("functions/computeScattering.glsl"),
};

/// `czm_cosineAndSine`, the exact text of `Functions/cosineAndSine.js`.
pub static COSINE_AND_SINE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/cosineAndSine",
    name: "czm_cosineAndSine",
    source: include_str!("functions/cosineAndSine.glsl"),
};

/// `czm_decodeRGB8`, the exact text of `Functions/decodeRGB8.js`.
pub static DECODE_RGB8: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/decodeRGB8",
    name: "czm_decodeRGB8",
    source: include_str!("functions/decodeRGB8.glsl"),
};

/// `czm_decompressTextureCoordinates`, the exact text of `Functions/decompressTextureCoordinates.js`.
pub static DECOMPRESS_TEXTURE_COORDINATES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/decompressTextureCoordinates",
    name: "czm_decompressTextureCoordinates",
    source: include_str!("functions/decompressTextureCoordinates.glsl"),
};

/// `czm_depthClamp`, the exact text of `Functions/depthClamp.js`.
pub static DEPTH_CLAMP: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/depthClamp",
    name: "czm_depthClamp",
    source: include_str!("functions/depthClamp.glsl"),
};

/// `czm_eastNorthUpToEyeCoordinates`, the exact text of `Functions/eastNorthUpToEyeCoordinates.js`.
pub static EAST_NORTH_UP_TO_EYE_COORDINATES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/eastNorthUpToEyeCoordinates",
    name: "czm_eastNorthUpToEyeCoordinates",
    source: include_str!("functions/eastNorthUpToEyeCoordinates.glsl"),
};

/// `czm_ellipsoidContainsPoint`, the exact text of `Functions/ellipsoidContainsPoint.js`.
pub static ELLIPSOID_CONTAINS_POINT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/ellipsoidContainsPoint",
    name: "czm_ellipsoidContainsPoint",
    source: include_str!("functions/ellipsoidContainsPoint.glsl"),
};

/// `czm_ellipsoidTextureCoordinates`, the exact text of `Functions/ellipsoidTextureCoordinates.js`.
pub static ELLIPSOID_TEXTURE_COORDINATES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/ellipsoidTextureCoordinates",
    name: "czm_ellipsoidTextureCoordinates",
    source: include_str!("functions/ellipsoidTextureCoordinates.glsl"),
};

/// `czm_equalsEpsilon`, the exact text of `Functions/equalsEpsilon.js`.
pub static EQUALS_EPSILON: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/equalsEpsilon",
    name: "czm_equalsEpsilon",
    source: include_str!("functions/equalsEpsilon.glsl"),
};

/// `czm_eyeOffset`, the exact text of `Functions/eyeOffset.js`.
pub static EYE_OFFSET: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/eyeOffset",
    name: "czm_eyeOffset",
    source: include_str!("functions/eyeOffset.glsl"),
};

/// `czm_eyeToWindowCoordinates`, the exact text of `Functions/eyeToWindowCoordinates.js`.
pub static EYE_TO_WINDOW_COORDINATES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/eyeToWindowCoordinates",
    name: "czm_eyeToWindowCoordinates",
    source: include_str!("functions/eyeToWindowCoordinates.glsl"),
};

/// `czm_fastApproximateAtan`, the exact text of `Functions/fastApproximateAtan.js`.
pub static FAST_APPROXIMATE_ATAN: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/fastApproximateAtan",
    name: "czm_fastApproximateAtan",
    source: include_str!("functions/fastApproximateAtan.glsl"),
};

/// `czm_fog`, the exact text of `Functions/fog.js`.
pub static FOG: ShaderModule =
    ShaderModule { id: "Shaders/Builtin/Functions/fog", name: "czm_fog", source: include_str!("functions/fog.glsl") };

/// `czm_gammaCorrect`, the exact text of `Functions/gammaCorrect.js`.
pub static GAMMA_CORRECT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/gammaCorrect",
    name: "czm_gammaCorrect",
    source: include_str!("functions/gammaCorrect.glsl"),
};

/// `czm_geodeticSurfaceNormal`, the exact text of `Functions/geodeticSurfaceNormal.js`.
pub static GEODETIC_SURFACE_NORMAL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/geodeticSurfaceNormal",
    name: "czm_geodeticSurfaceNormal",
    source: include_str!("functions/geodeticSurfaceNormal.glsl"),
};

/// `czm_getDefaultMaterial`, the exact text of `Functions/getDefaultMaterial.js`.
pub static GET_DEFAULT_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/getDefaultMaterial",
    name: "czm_getDefaultMaterial",
    source: include_str!("functions/getDefaultMaterial.glsl"),
};

/// `czm_getDynamicAtmosphereLightDirection`, the exact text of `Functions/getDynamicAtmosphereLightDirection.js`.
pub static GET_DYNAMIC_ATMOSPHERE_LIGHT_DIRECTION: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/getDynamicAtmosphereLightDirection",
    name: "czm_getDynamicAtmosphereLightDirection",
    source: include_str!("functions/getDynamicAtmosphereLightDirection.glsl"),
};

/// `czm_getLambertDiffuse`, the exact text of `Functions/getLambertDiffuse.js`.
pub static GET_LAMBERT_DIFFUSE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/getLambertDiffuse",
    name: "czm_getLambertDiffuse",
    source: include_str!("functions/getLambertDiffuse.glsl"),
};

/// `czm_getSpecular`, the exact text of `Functions/getSpecular.js`.
pub static GET_SPECULAR: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/getSpecular",
    name: "czm_getSpecular",
    source: include_str!("functions/getSpecular.glsl"),
};

/// `czm_getWaterNoise`, the exact text of `Functions/getWaterNoise.js`.
pub static GET_WATER_NOISE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/getWaterNoise",
    name: "czm_getWaterNoise",
    source: include_str!("functions/getWaterNoise.glsl"),
};

/// `czm_hue`, the exact text of `Functions/hue.js`.
pub static HUE: ShaderModule =
    ShaderModule { id: "Shaders/Builtin/Functions/hue", name: "czm_hue", source: include_str!("functions/hue.glsl") };

/// `czm_inverseGamma`, the exact text of `Functions/inverseGamma.js`.
pub static INVERSE_GAMMA: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/inverseGamma",
    name: "czm_inverseGamma",
    source: include_str!("functions/inverseGamma.glsl"),
};

/// `czm_isEmpty`, the exact text of `Functions/isEmpty.js`.
pub static IS_EMPTY: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/isEmpty",
    name: "czm_isEmpty",
    source: include_str!("functions/isEmpty.glsl"),
};

/// `czm_isFull`, the exact text of `Functions/isFull.js`.
pub static IS_FULL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/isFull",
    name: "czm_isFull",
    source: include_str!("functions/isFull.glsl"),
};

/// `czm_latitudeToWebMercatorFraction`, the exact text of `Functions/latitudeToWebMercatorFraction.js`.
pub static LATITUDE_TO_WEB_MERCATOR_FRACTION: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/latitudeToWebMercatorFraction",
    name: "czm_latitudeToWebMercatorFraction",
    source: include_str!("functions/latitudeToWebMercatorFraction.glsl"),
};

/// `czm_lineDistance`, the exact text of `Functions/lineDistance.js`.
pub static LINE_DISTANCE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/lineDistance",
    name: "czm_lineDistance",
    source: include_str!("functions/lineDistance.glsl"),
};

/// `czm_linearToSrgb`, the exact text of `Functions/linearToSrgb.js`.
pub static LINEAR_TO_SRGB: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/linearToSrgb",
    name: "czm_linearToSrgb",
    source: include_str!("functions/linearToSrgb.glsl"),
};

/// `czm_luminance`, the exact text of `Functions/luminance.js`.
pub static LUMINANCE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/luminance",
    name: "czm_luminance",
    source: include_str!("functions/luminance.glsl"),
};

/// `czm_maximumComponent`, the exact text of `Functions/maximumComponent.js`.
pub static MAXIMUM_COMPONENT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/maximumComponent",
    name: "czm_maximumComponent",
    source: include_str!("functions/maximumComponent.glsl"),
};

/// `czm_metersPerPixel`, the exact text of `Functions/metersPerPixel.js`.
pub static METERS_PER_PIXEL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/metersPerPixel",
    name: "czm_metersPerPixel",
    source: include_str!("functions/metersPerPixel.glsl"),
};

/// `czm_modelToWindowCoordinates`, the exact text of `Functions/modelToWindowCoordinates.js`.
pub static MODEL_TO_WINDOW_COORDINATES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/modelToWindowCoordinates",
    name: "czm_modelToWindowCoordinates",
    source: include_str!("functions/modelToWindowCoordinates.glsl"),
};

/// `czm_multiplyWithColorBalance`, the exact text of `Functions/multiplyWithColorBalance.js`.
pub static MULTIPLY_WITH_COLOR_BALANCE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/multiplyWithColorBalance",
    name: "czm_multiplyWithColorBalance",
    source: include_str!("functions/multiplyWithColorBalance.glsl"),
};

/// `czm_nearFarScalar`, the exact text of `Functions/nearFarScalar.js`.
pub static NEAR_FAR_SCALAR: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/nearFarScalar",
    name: "czm_nearFarScalar",
    source: include_str!("functions/nearFarScalar.glsl"),
};

/// `czm_octDecode`, the exact text of `Functions/octDecode.js`.
pub static OCT_DECODE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/octDecode",
    name: "czm_octDecode",
    source: include_str!("functions/octDecode.glsl"),
};

/// `czm_packDepth`, the exact text of `Functions/packDepth.js`.
pub static PACK_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/packDepth",
    name: "czm_packDepth",
    source: include_str!("functions/packDepth.glsl"),
};

/// `czm_pbrLighting`, the exact text of `Functions/pbrLighting.js`.
pub static PBR_LIGHTING: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/pbrLighting",
    name: "czm_pbrLighting",
    source: include_str!("functions/pbrLighting.glsl"),
};

/// `czm_pbrNeutralTonemapping`, the exact text of `Functions/pbrNeutralTonemapping.js`.
pub static PBR_NEUTRAL_TONEMAPPING: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/pbrNeutralTonemapping",
    name: "czm_pbrNeutralTonemapping",
    source: include_str!("functions/pbrNeutralTonemapping.glsl"),
};

/// `czm_phong`, the exact text of `Functions/phong.js`.
pub static PHONG: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/phong",
    name: "czm_phong",
    source: include_str!("functions/phong.glsl"),
};

/// `czm_planeDistance`, the exact text of `Functions/planeDistance.js`.
pub static PLANE_DISTANCE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/planeDistance",
    name: "czm_planeDistance",
    source: include_str!("functions/planeDistance.glsl"),
};

/// `czm_pointAlongRay`, the exact text of `Functions/pointAlongRay.js`.
pub static POINT_ALONG_RAY: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/pointAlongRay",
    name: "czm_pointAlongRay",
    source: include_str!("functions/pointAlongRay.glsl"),
};

/// `czm_rayEllipsoidIntersectionInterval`, the exact text of `Functions/rayEllipsoidIntersectionInterval.js`.
pub static RAY_ELLIPSOID_INTERSECTION_INTERVAL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/rayEllipsoidIntersectionInterval",
    name: "czm_rayEllipsoidIntersectionInterval",
    source: include_str!("functions/rayEllipsoidIntersectionInterval.glsl"),
};

/// `czm_raySphereIntersectionInterval`, the exact text of `Functions/raySphereIntersectionInterval.js`.
pub static RAY_SPHERE_INTERSECTION_INTERVAL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/raySphereIntersectionInterval",
    name: "czm_raySphereIntersectionInterval",
    source: include_str!("functions/raySphereIntersectionInterval.glsl"),
};

/// `czm_readDepth`, the exact text of `Functions/readDepth.js`.
pub static READ_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/readDepth",
    name: "czm_readDepth",
    source: include_str!("functions/readDepth.glsl"),
};

/// `czm_readNonPerspective`, the exact text of `Functions/readNonPerspective.js`.
pub static READ_NON_PERSPECTIVE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/readNonPerspective",
    name: "czm_readNonPerspective",
    source: include_str!("functions/readNonPerspective.glsl"),
};

/// `czm_reverseLogDepth`, the exact text of `Functions/reverseLogDepth.js`.
pub static REVERSE_LOG_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/reverseLogDepth",
    name: "czm_reverseLogDepth",
    source: include_str!("functions/reverseLogDepth.glsl"),
};

/// `czm_round`, the exact text of `Functions/round.js`.
pub static ROUND: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/round",
    name: "czm_round",
    source: include_str!("functions/round.glsl"),
};

/// `czm_saturation`, the exact text of `Functions/saturation.js`.
pub static SATURATION: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/saturation",
    name: "czm_saturation",
    source: include_str!("functions/saturation.glsl"),
};

/// `czm_shadowDepthCompare`, the exact text of `Functions/shadowDepthCompare.js`.
pub static SHADOW_DEPTH_COMPARE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/shadowDepthCompare",
    name: "czm_shadowDepthCompare",
    source: include_str!("functions/shadowDepthCompare.glsl"),
};

/// `czm_shadowVisibility`, the exact text of `Functions/shadowVisibility.js`.
pub static SHADOW_VISIBILITY: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/shadowVisibility",
    name: "czm_shadowVisibility",
    source: include_str!("functions/shadowVisibility.glsl"),
};

/// `czm_signNotZero`, the exact text of `Functions/signNotZero.js`.
pub static SIGN_NOT_ZERO: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/signNotZero",
    name: "czm_signNotZero",
    source: include_str!("functions/signNotZero.glsl"),
};

/// `czm_sphericalHarmonics`, the exact text of `Functions/sphericalHarmonics.js`.
pub static SPHERICAL_HARMONICS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/sphericalHarmonics",
    name: "czm_sphericalHarmonics",
    source: include_str!("functions/sphericalHarmonics.glsl"),
};

/// `czm_srgbToLinear`, the exact text of `Functions/srgbToLinear.js`.
pub static SRGB_TO_LINEAR: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/srgbToLinear",
    name: "czm_srgbToLinear",
    source: include_str!("functions/srgbToLinear.glsl"),
};

/// `czm_tangentToEyeSpaceMatrix`, the exact text of `Functions/tangentToEyeSpaceMatrix.js`.
pub static TANGENT_TO_EYE_SPACE_MATRIX: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/tangentToEyeSpaceMatrix",
    name: "czm_tangentToEyeSpaceMatrix",
    source: include_str!("functions/tangentToEyeSpaceMatrix.glsl"),
};

/// `czm_textureCube`, the exact text of `Functions/textureCube.js`.
pub static TEXTURE_CUBE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/textureCube",
    name: "czm_textureCube",
    source: include_str!("functions/textureCube.glsl"),
};

/// `czm_transformPlane`, the exact text of `Functions/transformPlane.js`.
pub static TRANSFORM_PLANE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/transformPlane",
    name: "czm_transformPlane",
    source: include_str!("functions/transformPlane.glsl"),
};

/// `czm_translateRelativeToEye`, the exact text of `Functions/translateRelativeToEye.js`.
pub static TRANSLATE_RELATIVE_TO_EYE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/translateRelativeToEye",
    name: "czm_translateRelativeToEye",
    source: include_str!("functions/translateRelativeToEye.glsl"),
};

/// `czm_translucentPhong`, the exact text of `Functions/translucentPhong.js`.
pub static TRANSLUCENT_PHONG: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/translucentPhong",
    name: "czm_translucentPhong",
    source: include_str!("functions/translucentPhong.glsl"),
};

/// `czm_transpose`, the exact text of `Functions/transpose.js`.
pub static TRANSPOSE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/transpose",
    name: "czm_transpose",
    source: include_str!("functions/transpose.glsl"),
};

/// `czm_unpackClippingExtents`, the exact text of `Functions/unpackClippingExtents.js`.
pub static UNPACK_CLIPPING_EXTENTS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/unpackClippingExtents",
    name: "czm_unpackClippingExtents",
    source: include_str!("functions/unpackClippingExtents.glsl"),
};

/// `czm_unpackDepth`, the exact text of `Functions/unpackDepth.js`.
pub static UNPACK_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/unpackDepth",
    name: "czm_unpackDepth",
    source: include_str!("functions/unpackDepth.glsl"),
};

/// `czm_unpackFloat`, the exact text of `Functions/unpackFloat.js`.
pub static UNPACK_FLOAT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/unpackFloat",
    name: "czm_unpackFloat",
    source: include_str!("functions/unpackFloat.glsl"),
};

/// `czm_unpackTexture`, the exact text of `Functions/unpackTexture.js`.
pub static UNPACK_TEXTURE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/unpackTexture",
    name: "czm_unpackTexture",
    source: include_str!("functions/unpackTexture.glsl"),
};

/// `czm_unpackUint`, the exact text of `Functions/unpackUint.js`.
pub static UNPACK_UINT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/unpackUint",
    name: "czm_unpackUint",
    source: include_str!("functions/unpackUint.glsl"),
};

/// `czm_valueTransform`, the exact text of `Functions/valueTransform.js`.
pub static VALUE_TRANSFORM: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/valueTransform",
    name: "czm_valueTransform",
    source: include_str!("functions/valueTransform.glsl"),
};

/// `czm_vertexLogDepth`, the exact text of `Functions/vertexLogDepth.js`.
pub static VERTEX_LOG_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/vertexLogDepth",
    name: "czm_vertexLogDepth",
    source: include_str!("functions/vertexLogDepth.glsl"),
};

/// `czm_windowToEyeCoordinates`, the exact text of `Functions/windowToEyeCoordinates.js`.
pub static WINDOW_TO_EYE_COORDINATES: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/windowToEyeCoordinates",
    name: "czm_windowToEyeCoordinates",
    source: include_str!("functions/windowToEyeCoordinates.glsl"),
};

/// `czm_writeDepthClamp`, the exact text of `Functions/writeDepthClamp.js`.
pub static WRITE_DEPTH_CLAMP: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/writeDepthClamp",
    name: "czm_writeDepthClamp",
    source: include_str!("functions/writeDepthClamp.glsl"),
};

/// `czm_writeLogDepth`, the exact text of `Functions/writeLogDepth.js`.
pub static WRITE_LOG_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/writeLogDepth",
    name: "czm_writeLogDepth",
    source: include_str!("functions/writeLogDepth.glsl"),
};

/// `czm_writeNonPerspective`, the exact text of `Functions/writeNonPerspective.js`.
pub static WRITE_NON_PERSPECTIVE: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Functions/writeNonPerspective",
    name: "czm_writeNonPerspective",
    source: include_str!("functions/writeNonPerspective.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&HSB_TO_RGB);
    shaders.push(&HSL_TO_RGB);
    shaders.push(&RGB_TO_HSB);
    shaders.push(&RGB_TO_HSL);
    shaders.push(&RGB_TO_XYZ);
    shaders.push(&XYZ_TO_RGB);
    shaders.push(&ACES_TONEMAPPING);
    shaders.push(&ALPHA_WEIGHT);
    shaders.push(&ANTIALIAS);
    shaders.push(&APPLY_HSB_SHIFT);
    shaders.push(&APPROXIMATE_SPHERICAL_COORDINATES);
    shaders.push(&APPROXIMATE_TANH);
    shaders.push(&BACK_FACING);
    shaders.push(&BRANCH_FREE_TERNARY);
    shaders.push(&CASCADE_COLOR);
    shaders.push(&CASCADE_DISTANCE);
    shaders.push(&CASCADE_MATRIX);
    shaders.push(&CASCADE_WEIGHTS);
    shaders.push(&CLIP_POLYGONS);
    shaders.push(&COLUMBUS_VIEW_MORPH);
    shaders.push(&COMPUTE_ATMOSPHERE_COLOR);
    shaders.push(&COMPUTE_GROUND_ATMOSPHERE_SCATTERING);
    shaders.push(&COMPUTE_POSITION);
    shaders.push(&COMPUTE_SCATTERING);
    shaders.push(&COSINE_AND_SINE);
    shaders.push(&DECODE_RGB8);
    shaders.push(&DECOMPRESS_TEXTURE_COORDINATES);
    shaders.push(&DEPTH_CLAMP);
    shaders.push(&EAST_NORTH_UP_TO_EYE_COORDINATES);
    shaders.push(&ELLIPSOID_CONTAINS_POINT);
    shaders.push(&ELLIPSOID_TEXTURE_COORDINATES);
    shaders.push(&EQUALS_EPSILON);
    shaders.push(&EYE_OFFSET);
    shaders.push(&EYE_TO_WINDOW_COORDINATES);
    shaders.push(&FAST_APPROXIMATE_ATAN);
    shaders.push(&FOG);
    shaders.push(&GAMMA_CORRECT);
    shaders.push(&GEODETIC_SURFACE_NORMAL);
    shaders.push(&GET_DEFAULT_MATERIAL);
    shaders.push(&GET_DYNAMIC_ATMOSPHERE_LIGHT_DIRECTION);
    shaders.push(&GET_LAMBERT_DIFFUSE);
    shaders.push(&GET_SPECULAR);
    shaders.push(&GET_WATER_NOISE);
    shaders.push(&HUE);
    shaders.push(&INVERSE_GAMMA);
    shaders.push(&IS_EMPTY);
    shaders.push(&IS_FULL);
    shaders.push(&LATITUDE_TO_WEB_MERCATOR_FRACTION);
    shaders.push(&LINE_DISTANCE);
    shaders.push(&LINEAR_TO_SRGB);
    shaders.push(&LUMINANCE);
    shaders.push(&MAXIMUM_COMPONENT);
    shaders.push(&METERS_PER_PIXEL);
    shaders.push(&MODEL_TO_WINDOW_COORDINATES);
    shaders.push(&MULTIPLY_WITH_COLOR_BALANCE);
    shaders.push(&NEAR_FAR_SCALAR);
    shaders.push(&OCT_DECODE);
    shaders.push(&PACK_DEPTH);
    shaders.push(&PBR_LIGHTING);
    shaders.push(&PBR_NEUTRAL_TONEMAPPING);
    shaders.push(&PHONG);
    shaders.push(&PLANE_DISTANCE);
    shaders.push(&POINT_ALONG_RAY);
    shaders.push(&RAY_ELLIPSOID_INTERSECTION_INTERVAL);
    shaders.push(&RAY_SPHERE_INTERSECTION_INTERVAL);
    shaders.push(&READ_DEPTH);
    shaders.push(&READ_NON_PERSPECTIVE);
    shaders.push(&REVERSE_LOG_DEPTH);
    shaders.push(&ROUND);
    shaders.push(&SATURATION);
    shaders.push(&SHADOW_DEPTH_COMPARE);
    shaders.push(&SHADOW_VISIBILITY);
    shaders.push(&SIGN_NOT_ZERO);
    shaders.push(&SPHERICAL_HARMONICS);
    shaders.push(&SRGB_TO_LINEAR);
    shaders.push(&TANGENT_TO_EYE_SPACE_MATRIX);
    shaders.push(&TEXTURE_CUBE);
    shaders.push(&TRANSFORM_PLANE);
    shaders.push(&TRANSLATE_RELATIVE_TO_EYE);
    shaders.push(&TRANSLUCENT_PHONG);
    shaders.push(&TRANSPOSE);
    shaders.push(&UNPACK_CLIPPING_EXTENTS);
    shaders.push(&UNPACK_DEPTH);
    shaders.push(&UNPACK_FLOAT);
    shaders.push(&UNPACK_TEXTURE);
    shaders.push(&UNPACK_UINT);
    shaders.push(&VALUE_TRANSFORM);
    shaders.push(&VERTEX_LOG_DEPTH);
    shaders.push(&WINDOW_TO_EYE_COORDINATES);
    shaders.push(&WRITE_DEPTH_CLAMP);
    shaders.push(&WRITE_LOG_DEPTH);
    shaders.push(&WRITE_NON_PERSPECTIVE);
}
