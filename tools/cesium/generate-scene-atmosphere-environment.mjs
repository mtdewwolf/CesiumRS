// Pinned CesiumJS atmosphere, sun, environment-map, panorama, provider,
// specular cube map and invert-classification CPU cases.
import * as C from '@cesium/engine';
import SkyAtmosphere from '@cesium/engine/Source/Scene/SkyAtmosphere.js';
import Sun from '@cesium/engine/Source/Scene/Sun.js';
import SunPostProcess from '@cesium/engine/Source/Scene/SunPostProcess.js';
import CubeMapPanorama from '@cesium/engine/Source/Scene/CubeMapPanorama.js';
import GoogleStreetViewCubeMapPanoramaProvider from '@cesium/engine/Source/Scene/GoogleStreetViewCubeMapPanoramaProvider.js';
import SpecularEnvironmentCubeMap from '@cesium/engine/Source/Scene/SpecularEnvironmentCubeMap.js';
import DynamicEnvironmentMapManager from '@cesium/engine/Source/Scene/DynamicEnvironmentMapManager.js';
import InvertClassification from '@cesium/engine/Source/Scene/InvertClassification.js';
import ClassificationPrimitive from '@cesium/engine/Source/Scene/ClassificationPrimitive.js';
import GroundPrimitive from '@cesium/engine/Source/Scene/GroundPrimitive.js';
import GroundPolylinePrimitive from '@cesium/engine/Source/Scene/GroundPolylinePrimitive.js';
import CesiumMath from '@cesium/engine/Source/Core/Math.js';
import Rectangle from '@cesium/engine/Source/Core/Rectangle.js';
import RectangleGeometry from '@cesium/engine/Source/Core/RectangleGeometry.js';
import BoundingSphere from '@cesium/engine/Source/Core/BoundingSphere.js';
import ApproximateTerrainHeights from '@cesium/engine/Source/Core/ApproximateTerrainHeights.js';
import ShadowVolumeAppearance from '@cesium/engine/Source/Scene/ShadowVolumeAppearance.js';
import RenderState from '@cesium/engine/Source/Renderer/RenderState.js';
import StencilConstants from '@cesium/engine/Source/Scene/StencilConstants.js';
import { readFileSync, writeFileSync } from 'node:fs';

for (const name of ['HTMLCanvasElement', 'HTMLImageElement', 'HTMLVideoElement', 'ImageBitmap', 'OffscreenCanvas']) {
  globalThis[name] ??= class {};
}

const num = (v) =>
  Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v;
const nums = (v) => Array.from(v, num);
const c3 = (v) => [num(v.x), num(v.y), num(v.z)];
const m3 = (m) => nums(m);
const m4 = (m) => nums(m);

// RenderState validates against ContextLimits statics.
C.ContextLimits._minimumAliasedLineWidth = 1;
C.ContextLimits._maximumAliasedLineWidth = 4096;
C.ContextLimits._maximumViewportWidth = 16384;
C.ContextLimits._maximumViewportHeight = 16384;
C.ContextLimits._maximumCubeMapSize = 16384;

function state(s) {
  return {
    cull: { enabled: s.cull.enabled, face: s.cull.face },
    depthTest: { enabled: s.depthTest.enabled, func: s.depthTest.func },
    depthMask: s.depthMask,
    colorMask: { red: s.colorMask.red, green: s.colorMask.green, blue: s.colorMask.blue, alpha: s.colorMask.alpha },
    polygonOffset: { enabled: s.polygonOffset.enabled, factor: s.polygonOffset.factor, units: s.polygonOffset.units },
    blending: {
      enabled: s.blending.enabled,
      rgb: s.blending.functionSourceRgb ?? 1,
      alpha: s.blending.functionSourceAlpha ?? 1,
      drgb: s.blending.functionDestinationRgb ?? 0,
      dalpha: s.blending.functionDestinationAlpha ?? 0,
      eq: s.blending.equationRgb ?? 32774,
      eqa: s.blending.equationAlpha ?? 32774,
    },
    stencilTest: {
      enabled: s.stencilTest.enabled,
      frontFunction: s.stencilTest.frontFunction,
      backFunction: s.stencilTest.backFunction,
      reference: s.stencilTest.reference,
      mask: s.stencilTest.mask,
      frontOperation: s.stencilTest.frontOperation,
      backOperation: s.stencilTest.backOperation,
    },
    stencilMask: s.stencilMask,
  };
}

// --- SkyAtmosphere ---------------------------------------------------------
const smallEllipsoid = new C.Ellipsoid(1000.5, 1200.5, 900.5);
const sky = new SkyAtmosphere(smallEllipsoid);
const skyBefore = {
  show: sky.show,
  perFragmentAtmosphere: sky.perFragmentAtmosphere,
  ellipsoidRadii: c3(sky.ellipsoid.radii),
  scaleMatrix: m4(sky._scaleMatrix),
  radiiAndColor: c3(sky._radiiAndDynamicAtmosphereColor),
  lightIntensity: sky.atmosphereLightIntensity,
  rayleigh: c3(sky.atmosphereRayleighCoefficient),
  mie: c3(sky.atmosphereMieCoefficient),
  rayleighScaleHeight: sky.atmosphereRayleighScaleHeight,
  mieScaleHeight: sky.atmosphereMieScaleHeight,
  mieAnisotropy: sky.atmosphereMieAnisotropy,
  hsb: [sky.hueShift, sky.saturationShift, sky.brightnessShift],
};
sky.setDynamicLighting(C.DynamicAtmosphereLightingType.SUNLIGHT);
sky.hueShift = 0.5;
sky.perFragmentAtmosphere = true;
const skyAfter = {
  radiiAndColor: c3(sky._radiiAndDynamicAtmosphereColor),
  hueShift: sky.hueShift,
  perFragmentAtmosphere: sky.perFragmentAtmosphere,
};
const skyDefault = new SkyAtmosphere();

// --- Sun -------------------------------------------------------------------
const sun = new Sun();
const sunBefore = { show: sun.show, glowFactor: sun.glowFactor, size: sun._size };
sun.glowFactor = -2;
const sunClamped = sun.glowFactor;
sun.glowFactor = 2;
const sunAfter = { glowFactor: sun.glowFactor, dirty: sun._glowFactorDirty };

// --- SunPostProcess --------------------------------------------------------
const sunPostProcess = new SunPostProcess();
const stages = [];
for (let i = 0; i < sunPostProcess.length; i++) {
  const stage = sunPostProcess.get(i);
  stages.push({
    textureScale: stage.textureScale,
    forcePowerOfTwo: stage.forcePowerOfTwo,
    sampleMode: stage.sampleMode,
    uniformNames: Object.keys(stage.uniforms ?? {}).sort(),
    fragmentShader: stage.fragmentShader?.source ?? stage.fragmentShader,
  });
}

// --- CubeMapPanorama -------------------------------------------------------
const transform = new C.Matrix3(0, 1, 0, -1, 0, 0, 0, 0, 1);
const panorama = new CubeMapPanorama({
  sources: {
    positiveX: 'px.png', negativeX: 'nx.png', positiveY: 'py.png',
    negativeY: 'ny.png', positiveZ: 'pz.png', negativeZ: 'nz.png',
  },
  transform,
  show: false,
  credit: 'Example',
});
const panoramaCase = {
  show: panorama.show,
  transform: m3(panorama.transform),
  credit: panorama.credit.html,
  returnCommand: panorama._returnCommand,
  hasVertexArray: panorama._command.vertexArray !== undefined,
};

// --- GoogleStreetViewCubeMapPanoramaProvider -------------------------------
const provider = new GoogleStreetViewCubeMapPanoramaProvider({
  key: 'KEY',
  url: 'https://example.com/streetview',
  tileSize: 512,
});
const metadataProvider = GoogleStreetViewCubeMapPanoramaProvider._parseMetadata({
  location: { lng: 12.5, lat: -3.25 },
});
const google = {
  baseUrl: provider._baseResource.url,
  metadataUrl: provider._metadataResource.url,
  tileSize: provider._tileSize,
  faceUrls: [
    provider._buildFaceUrl({ heading: 0, pitch: 0, tileSizeString: '512x512', panoId: 'PANO', signature: undefined }),
    provider._buildFaceUrl({ heading: -90, pitch: -90, tileSizeString: '512x512', panoId: 'PANO', signature: 'SIG' }),
  ],
  parsedCartographic: [metadataProvider.cartographic.longitude, metadataProvider.cartographic.latitude, metadataProvider.cartographic.height],
};
let missingKeyError = null;
try {
  await GoogleStreetViewCubeMapPanoramaProvider.fromUrl({});
} catch (error) {
  missingKeyError = error.message;
}

// --- SpecularEnvironmentCubeMap --------------------------------------------
const specular = new SpecularEnvironmentCubeMap('specular.ktx2');
const supportedContext = {
  colorBufferHalfFloat: true,
  halfFloatingPointTexture: true,
  floatingPointTexture: false,
  colorBufferFloat: false,
  supportsTextureLod: true,
};
const unsupportedContext = { ...supportedContext, supportsTextureLod: false };
const specularCase = {
  url: specular.url,
  ready: specular.ready,
  maximumMipmapLevel: specular.maximumMipmapLevel ?? null,
  supported: SpecularEnvironmentCubeMap.isSupported(supportedContext),
  unsupported: SpecularEnvironmentCubeMap.isSupported(unsupportedContext),
  hasTexture: specular.texture !== undefined,
};

// --- DynamicEnvironmentMapManager ------------------------------------------
const dynamic = new DynamicEnvironmentMapManager({
  mipmapLevels: 5,
  maximumSecondsDifference: 60,
  maximumPositionEpsilon: 12.5,
  atmosphereScatteringIntensity: 3,
  gamma: 1.2,
  brightness: 0.8,
  saturation: 1.1,
  groundColor: C.Color.fromCssColorString('#203b34'),
  groundAlbedo: 0.4,
});
const dynamicDefaults = new DynamicEnvironmentMapManager();
dynamic.position = new C.Cartesian3(1, 2, 3);
const movedEpsilon = () => {
  const before = C.Cartesian3.clone(dynamic._position);
  dynamic.position = new C.Cartesian3(1, 2, 12.5);
  return C.Cartesian3.equals(before, dynamic._position);
};
const ignoredSmallMove = movedEpsilon();
dynamic.position = new C.Cartesian3(10, 20, 30);
const acceptedMove = c3(dynamic._position);
const dynamicCase = {
  defaults: {
    enabled: dynamicDefaults.enabled,
    shouldUpdate: dynamicDefaults.shouldUpdate,
    maximumSecondsDifference: dynamicDefaults.maximumSecondsDifference,
    maximumPositionEpsilon: dynamicDefaults.maximumPositionEpsilon,
    atmosphereScatteringIntensity: dynamicDefaults.atmosphereScatteringIntensity,
    gamma: dynamicDefaults.gamma,
    brightness: dynamicDefaults.brightness,
    saturation: dynamicDefaults.saturation,
    groundColor: [dynamicDefaults.groundColor.red, dynamicDefaults.groundColor.green, dynamicDefaults.groundColor.blue, dynamicDefaults.groundColor.alpha],
    groundAlbedo: dynamicDefaults.groundAlbedo,
    mipmapLevels: dynamicDefaults._mipmapLevels,
    textureDimensions: [dynamicDefaults._textureDimensions.x, dynamicDefaults._textureDimensions.y],
    radianceCommands: dynamicDefaults._radianceMapComputeCommands.length,
    convolutionCommands: dynamicDefaults._convolutionComputeCommands.length,
    sh: dynamicDefaults._sphericalHarmonicCoefficients.map(c3),
    hasPosition: dynamicDefaults._position !== undefined,
    shouldReset: dynamicDefaults._shouldReset,
  },
  custom: {
    enabled: dynamic.enabled,
    maximumSecondsDifference: dynamic.maximumSecondsDifference,
    maximumPositionEpsilon: dynamic.maximumPositionEpsilon,
    atmosphereScatteringIntensity: dynamic.atmosphereScatteringIntensity,
    gamma: dynamic.gamma,
    brightness: dynamic.brightness,
    saturation: dynamic.saturation,
    groundColor: [dynamic.groundColor.red, dynamic.groundColor.green, dynamic.groundColor.blue, dynamic.groundColor.alpha],
    groundAlbedo: dynamic.groundAlbedo,
    mipmapLevels: dynamic._mipmapLevels,
    textureDimensions: [dynamic._textureDimensions.x, dynamic._textureDimensions.y],
  },
  ignoredSmallMove,
  acceptedMove,
  dynamicUpdateSupported: [
    DynamicEnvironmentMapManager.isDynamicUpdateSupported({ context: { halfFloatingPointTexture: true, colorBufferFloat: false } }),
    DynamicEnvironmentMapManager.isDynamicUpdateSupported({ context: { halfFloatingPointTexture: false, colorBufferFloat: true } }),
    DynamicEnvironmentMapManager.isDynamicUpdateSupported({ context: { halfFloatingPointTexture: false, colorBufferFloat: false } }),
  ],
  averageGroundColor: (() => {
    const c = DynamicEnvironmentMapManager.AVERAGE_EARTH_GROUND_COLOR;
    return [c.red, c.green, c.blue, c.alpha];
  })(),
  defaultSh: DynamicEnvironmentMapManager.DEFAULT_SPHERICAL_HARMONIC_COEFFICIENTS.map(c3),
};

// --- InvertClassification --------------------------------------------------
const invert = new InvertClassification();
const invertCase = {
  translucencySupported: [
    InvertClassification.isTranslucencySupported({ depthTexture: true, fragmentDepth: true }),
    InvertClassification.isTranslucencySupported({ depthTexture: true, fragmentDepth: false }),
    InvertClassification.isTranslucencySupported({ depthTexture: false, fragmentDepth: true }),
  ],
  hasUnclassifiedCommand: invert._unclassifiedCommand !== undefined,
  numSamples: invert._numSamples,
};

// --- Classification/ground render states -----------------------------------
const classificationStencil = (enableStencil, mask3DTiles) =>
  RenderState.fromCache({
    colorMask: { red: false, green: false, blue: false, alpha: false },
    stencilTest: {
      enabled: enableStencil,
      frontFunction: mask3DTiles ? C.StencilFunction.EQUAL : C.StencilFunction.ALWAYS,
      frontOperation: { fail: C.StencilOperation.KEEP, zFail: C.StencilOperation.DECREMENT_WRAP, zPass: C.StencilOperation.KEEP },
      backFunction: mask3DTiles ? C.StencilFunction.EQUAL : C.StencilFunction.ALWAYS,
      backOperation: { fail: C.StencilOperation.KEEP, zFail: C.StencilOperation.INCREMENT_WRAP, zPass: C.StencilOperation.KEEP },
      reference: StencilConstants.CESIUM_3D_TILE_MASK,
      mask: StencilConstants.CESIUM_3D_TILE_MASK,
    },
    stencilMask: StencilConstants.CLASSIFICATION_MASK,
    depthTest: { enabled: true, func: C.DepthFunction.LESS_OR_EQUAL },
    depthMask: false,
  });
const classificationColor = (enableStencil) =>
  RenderState.fromCache({
    stencilTest: {
      enabled: enableStencil,
      frontFunction: C.StencilFunction.NOT_EQUAL,
      frontOperation: { fail: C.StencilOperation.ZERO, zFail: C.StencilOperation.ZERO, zPass: C.StencilOperation.ZERO },
      backFunction: C.StencilFunction.NOT_EQUAL,
      backOperation: { fail: C.StencilOperation.ZERO, zFail: C.StencilOperation.ZERO, zPass: C.StencilOperation.ZERO },
      reference: 0,
      mask: StencilConstants.CLASSIFICATION_MASK,
    },
    stencilMask: StencilConstants.CLASSIFICATION_MASK,
    depthTest: { enabled: false },
    depthMask: false,
    blending: C.BlendingState.PRE_MULTIPLIED_ALPHA_BLEND,
  });
const classificationPick = RenderState.fromCache({
  stencilTest: {
    enabled: true,
    frontFunction: C.StencilFunction.NOT_EQUAL,
    frontOperation: { fail: C.StencilOperation.ZERO, zFail: C.StencilOperation.ZERO, zPass: C.StencilOperation.ZERO },
    backFunction: C.StencilFunction.NOT_EQUAL,
    backOperation: { fail: C.StencilOperation.ZERO, zFail: C.StencilOperation.ZERO, zPass: C.StencilOperation.ZERO },
    reference: 0,
    mask: StencilConstants.CLASSIFICATION_MASK,
  },
  stencilMask: StencilConstants.CLASSIFICATION_MASK,
  depthTest: { enabled: false },
  depthMask: false,
});
const groundPolyline = (mask3DTiles) =>
  RenderState.fromCache({
    cull: { enabled: true },
    blending: C.BlendingState.PRE_MULTIPLIED_ALPHA_BLEND,
    depthMask: false,
    stencilTest: {
      enabled: mask3DTiles,
      frontFunction: C.StencilFunction.EQUAL,
      frontOperation: { fail: C.StencilOperation.KEEP, zFail: C.StencilOperation.KEEP, zPass: C.StencilOperation.KEEP },
      backFunction: C.StencilFunction.EQUAL,
      backOperation: { fail: C.StencilOperation.KEEP, zFail: C.StencilOperation.KEEP, zPass: C.StencilOperation.KEEP },
      reference: StencilConstants.CESIUM_3D_TILE_MASK,
      mask: StencilConstants.CESIUM_3D_TILE_MASK,
    },
  });
const groundPolylineMorph = RenderState.fromCache({
  cull: { enabled: true, face: C.CullFace.FRONT },
  depthTest: { enabled: true },
  blending: C.BlendingState.PRE_MULTIPLIED_ALPHA_BLEND,
  depthMask: false,
});

// --- Ground terrain height / shadow volume --------------------------------
ApproximateTerrainHeights._terrainHeights = JSON.parse(readFileSync(new URL('./node_modules/@cesium/engine/Source/Assets/approximateTerrainHeights.json', import.meta.url)));
const rectangle = Rectangle.fromDegrees(-1.4, 0.5, -1.1, 0.7);
const projection = new C.GeographicProjection();
const ellipsoid = projection.ellipsoid;
const heights = ApproximateTerrainHeights.getMinimumMaximumHeights(rectangle, ellipsoid);
const spherical = ShadowVolumeAppearance.shouldUseSphericalCoordinates(rectangle);
const geometry = new RectangleGeometry({ rectangle, height: heights.minimumTerrainHeight, extrudedHeight: heights.maximumTerrainHeight });
const volumeGeometry = RectangleGeometry.createGeometry(
  RectangleGeometry.createShadowVolume(
    new RectangleGeometry({ rectangle, height: 0 }),
    () => heights.minimumTerrainHeight,
    (granularity, ellipsoid) => {
      const r = ellipsoid.maximumRadius;
      return heights.maximumTerrainHeight + r / Math.cos(granularity * 0.5) - r;
    },
  ),
);
const volumeSphere = volumeGeometry.boundingSphere;
const obb = C.OrientedBoundingBox.fromRectangle(rectangle, heights.minimumTerrainHeight, heights.maximumTerrainHeight, ellipsoid);
const obbSphere = BoundingSphere.fromOrientedBoundingBox(obb);
const sphere2d = BoundingSphere.fromRectangleWithHeights2D(rectangle, projection, heights.maximumTerrainHeight, heights.minimumTerrainHeight);
const swizzled = new C.Cartesian3(sphere2d.center.z, sphere2d.center.x, sphere2d.center.y);
const groundCase = {
  rectangle: [rectangle.west, rectangle.south, rectangle.east, rectangle.north],
  heights,
  spherical,
  volumeSphere: [volumeSphere.center.x, volumeSphere.center.y, volumeSphere.center.z, volumeSphere.radius],
  obbSphere: [obbSphere.center.x, obbSphere.center.y, obbSphere.center.z, obbSphere.radius],
  sphere2d: [swizzled.x, swizzled.y, swizzled.z, sphere2d.radius],
  rectangleGeometryVertices: RectangleGeometry.createGeometry(geometry).attributes.position.values.length / 3,
};

// --- Primitive support and constructor defaults ----------------------------
const classification = new ClassificationPrimitive({});
const ground = new GroundPrimitive({});
const groundPolylinePrimitive = new GroundPolylinePrimitive({});
const defaultsCase = {
  classification: {
    show: classification.show,
    classificationType: classification.classificationType,
    debugShowBoundingVolume: classification.debugShowBoundingVolume,
    debugShowShadowVolume: classification.debugShowShadowVolume,
    vertexCacheOptimize: classification.vertexCacheOptimize,
    interleave: classification.interleave,
    releaseGeometryInstances: classification.releaseGeometryInstances,
    allowPicking: classification.allowPicking,
    asynchronous: classification.asynchronous,
    compressVertices: classification.compressVertices,
    ready: classification.ready,
    isSupported: ClassificationPrimitive.isSupported({ context: { stencilBuffer: true } }),
    unsupported: ClassificationPrimitive.isSupported({ context: { stencilBuffer: false } }),
  },
  ground: {
    show: ground.show,
    classificationType: ground.classificationType,
    debugShowBoundingVolume: ground.debugShowBoundingVolume,
    debugShowShadowVolume: ground.debugShowShadowVolume,
    ready: ground.ready,
    supportsMaterials: GroundPrimitive.supportsMaterials({ frameState: { context: { depthTexture: true } } }),
    unsupportedMaterials: GroundPrimitive.supportsMaterials({ frameState: { context: { depthTexture: false } } }),
  },
  groundPolyline: {
    show: groundPolylinePrimitive.show,
    classificationType: groundPolylinePrimitive.classificationType,
    debugShowBoundingVolume: groundPolylinePrimitive.debugShowBoundingVolume,
    debugShowShadowVolume: groundPolylinePrimitive.debugShowShadowVolume,
    interleave: groundPolylinePrimitive.interleave,
    releaseGeometryInstances: groundPolylinePrimitive.releaseGeometryInstances,
    allowPicking: groundPolylinePrimitive.allowPicking,
    asynchronous: groundPolylinePrimitive.asynchronous,
    ready: groundPolylinePrimitive.ready,
    isSupported: GroundPolylinePrimitive.isSupported({ frameState: { context: { depthTexture: true } } }),
    unsupported: GroundPolylinePrimitive.isSupported({ frameState: { context: { depthTexture: false } } }),
  },
};

writeFileSync(new URL('../../tests/data/scene-atmosphere-environment.json', import.meta.url), JSON.stringify({
  skyAtmosphere: { before: skyBefore, after: skyAfter, defaultRadiiAndColor: c3(skyDefault._radiiAndDynamicAtmosphereColor), defaultScale: m4(skyDefault._scaleMatrix) },
  sun: { before: sunBefore, clamped: num(sunClamped), after: sunAfter },
  sunPostProcess: { length: sunPostProcess.length, stages },
  cubeMapPanorama: panoramaCase,
  google,
  specular: specularCase,
  dynamic: dynamicCase,
  invert: invertCase,
  classification: {
    stencilTerrain: state(classificationStencil(true, false)),
    stencilTiles: state(classificationStencil(true, true)),
    stencilDisabled: state(classificationStencil(false, false)),
    color: state(classificationColor(true)),
    colorDisabled: state(classificationColor(false)),
    pick: state(classificationPick),
  },
  groundPolyline: {
    terrain: state(groundPolyline(false)),
    tiles: state(groundPolyline(true)),
    morph: state(groundPolylineMorph),
  },
  ground: groundCase,
  defaults: defaultsCase,
}, null, 2));
console.log('wrote tests/data/scene-atmosphere-environment.json');
