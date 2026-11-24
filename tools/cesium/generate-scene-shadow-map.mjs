// Pinned CesiumJS ShadowMap reference cases: constructor defaults, render
// states, texture layouts and the CPU camera/cascade fitting driven through
// ShadowMap.update with framebuffer resources stubbed out (the pinned module
// performs no GL calls in update when a framebuffer already matches the target
// size).
import * as C from '@cesium/engine';
import ShadowMap from '@cesium/engine/Source/Scene/ShadowMap.js';
import { writeFileSync } from 'node:fs';

// Node has no Context, so seed the static limits RenderState validates.
C.ContextLimits._minimumAliasedLineWidth = 1;
C.ContextLimits._maximumAliasedLineWidth = 4096;
C.ContextLimits._maximumViewportWidth = 16384;
C.ContextLimits._maximumViewportHeight = 16384;
C.ContextLimits._maximumTextureSize = 16384;
C.ContextLimits._maximumCubeMapSize = 16384;

const num = (v) =>
  Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v;
const nums = (v) => Array.from(v, num);
const m4 = (m) => nums(m);
const c2 = (v) => [num(v.x), num(v.y)];
const c3 = (v) => [num(v.x), num(v.y), num(v.z)];
const c4 = (v) => [num(v.x), num(v.y), num(v.z), num(v.w)];

function matrix4(values) {
  return new C.Matrix4(...values);
}

function state(s) {
  return {
    cull: { enabled: s.cull.enabled, face: s.cull.face, frontFace: s.cull.frontFace },
    depthTest: { enabled: s.depthTest.enabled, func: s.depthTest.func },
    depthMask: s.depthMask,
    colorMask: s.colorMask,
    polygonOffset: { enabled: s.polygonOffset.enabled, factor: s.polygonOffset.factor, units: s.polygonOffset.units },
    blending: s.blending.enabled,
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

function bias(b) {
  return {
    polygonOffset: b.polygonOffset,
    polygonOffsetFactor: b.polygonOffsetFactor,
    polygonOffsetUnits: b.polygonOffsetUnits,
    normalOffset: b.normalOffset,
    normalOffsetScale: b.normalOffsetScale,
    normalShading: b.normalShading,
    normalShadingSmooth: b.normalShadingSmooth,
    depthBias: b.depthBias,
  };
}

function makeCameraSpec(view, inverseView, position, direction, up, frustum) {
  return {
    viewMatrix: matrix4(view),
    inverseViewMatrix: matrix4(inverseView),
    positionWC: new C.Cartesian3(...position),
    directionWC: new C.Cartesian3(...direction),
    upWC: new C.Cartesian3(...up),
    rightWC: new C.Cartesian3(...up),
    frustum,
  };
}

function perspective(near, far, fov = Math.PI / 3, aspect = 1.5) {
  const f = new C.PerspectiveFrustum();
  f.fov = fov;
  f.aspectRatio = aspect;
  f.near = near;
  f.far = far;
  return f;
}

function realCamera(position, direction, up, frustum) {
  const scene = {
    drawingBufferWidth: 1024,
    drawingBufferHeight: 768,
    mapProjection: new C.GeographicProjection(),
    mode: C.SceneMode.SCENE3D,
    ellipsoid: C.Ellipsoid.WGS84,
    screenSpaceCameraController: {},
  };
  const camera = new C.Camera(scene);
  camera.position = new C.Cartesian3(...position);
  camera.direction = new C.Cartesian3(...direction);
  camera.up = new C.Cartesian3(...up);
  camera.right = C.Cartesian3.cross(camera.direction, camera.up, new C.Cartesian3());
  camera.frustum = frustum;
  return camera;
}

function shadowCameraSpec(view, inverseView, position, direction, up, frustum) {
  return realCamera(position, direction, up, frustum);
}

const fakeContext = {
  depthTexture: true,
  stencilBuffer: true,
  drawingBufferWidth: 1024,
  drawingBufferHeight: 768,
  halfFloatingPointTexture: true,
  colorBufferFloat: true,
};

function stubShadowMap(shadowMap) {
  shadowMap._passes.forEach((pass) => {
    pass.framebuffer = { status: C.WebGLConstants.FRAMEBUFFER_COMPLETE, isDestroyed: () => false };
  });
  shadowMap._shadowMapTexture = { width: shadowMap._textureSize.x, height: shadowMap._textureSize.y };
}

function makeCameraOptions(name) {
  const view = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const frustum = perspective(1, 5000);
  return {
    name,
    camera: shadowCameraSpec(view, view, [7000000, 0, 0], [-1, 0, 0], [0, 1, 0], frustum),
  };
}

// --- constructor cases ------------------------------------------------------
const constructorCases = [];
const constructorSpecs = [
  { name: 'default', options: {} },
  { name: 'oneCascade', options: { numberOfCascades: 1 } },
  { name: 'size1024', options: { size: 1024 } },
  { name: 'pointLight', options: { isPointLight: true, pointLightRadius: 250 } },
  { name: 'spotLight', options: { cascadesEnabled: false } },
];
for (const spec of constructorSpecs) {
  const lightCamera = makeCameraOptions(spec.name).camera;
  const shadowMap = new ShadowMap({
    context: fakeContext,
    lightCamera,
    enabled: false,
    softShadows: true,
    darkness: 0.4,
    maximumDistance: 4000,
    normalOffset: false,
    fadingEnabled: false,
    ...spec.options,
  });
  constructorCases.push({
    name: spec.name,
    size: shadowMap.size,
    enabled: shadowMap.enabled,
    softShadows: shadowMap.softShadows,
    normalOffset: shadowMap.normalOffset,
    darkness: shadowMap.darkness,
    maximumDistance: shadowMap.maximumDistance,
    fadingEnabled: shadowMap.fadingEnabled,
    isPointLight: shadowMap.isPointLight,
    textureSize: c2(shadowMap._textureSize),
    textureOffsets: shadowMap._passes.map((p) => (p.textureOffsets ? m4(p.textureOffsets) : null)),
    viewports: shadowMap._passes.map((p) => [p.passState.viewport.x, p.passState.viewport.y, p.passState.viewport.width, p.passState.viewport.height]),
    passCount: shadowMap._passes.length,
    primitiveBias: bias(shadowMap._primitiveBias),
    terrainBias: bias(shadowMap._terrainBias),
    pointBias: bias(shadowMap._pointBias),
    primitiveRenderState: state(shadowMap._primitiveRenderState),
    terrainRenderState: state(shadowMap._terrainRenderState),
    pointRenderState: state(shadowMap._pointRenderState),
  });
}

// --- update cases -----------------------------------------------------------
const cameraSpec = makeCameraOptions('update').camera;
const light = shadowCameraSpec(
  [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
  [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
  [7000000, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  perspective(1, 10000, Math.PI / 4, 1),
);

const updateCases = [];
for (const [name, near, far, closest, maximumDistance] of [
  ['cascades', 1, 5000, 1000, 5000],
  ['cascadesClose', 10, 10000, 100, 5000],
  ['single', 1, 5000, 1000, 5000],
]) {
  const options = { maximumDistance };
  if (name === 'single') options.numberOfCascades = 1;
  const shadowMap = new ShadowMap({
    context: fakeContext,
    lightCamera: light,
    ...options,
  });
  stubShadowMap(shadowMap);
  const frameState = {
    mode: C.SceneMode.SCENE3D,
    passes: { render: true, pick: false },
    context: fakeContext,
    camera: cameraSpec,
    mapProjection: new C.GeographicProjection(),
    shadowState: { nearPlane: near, farPlane: far, closestObjectSize: closest },
    cullingVolume: C.CullingVolume.fromBoundingSphere(new C.BoundingSphere(new C.Cartesian3(7000000, 0, 0), 100)),
  };
  shadowMap.update(frameState);
  updateCases.push({
    name,
    lightCamera: {
      view: m4(light.viewMatrix),
      inverseView: m4(light.inverseViewMatrix),
      positionWC: c3(light.positionWC),
      directionWC: c3(light.directionWC),
      upWC: c3(light.upWC),
      rightWC: c3(light.rightWC),
      frustum: {
        near: light.frustum.near,
        far: light.frustum.far,
        fov: light.frustum.fov,
        aspectRatio: light.frustum.aspectRatio,
        xOffset: light.frustum.xOffset ?? 0,
        yOffset: light.frustum.yOffset ?? 0,
      },
    },
    sceneCamera: {
      view: m4(cameraSpec.viewMatrix),
      inverseView: m4(cameraSpec.inverseViewMatrix),
      positionWC: c3(cameraSpec.positionWC),
      directionWC: c3(cameraSpec.directionWC),
      upWC: c3(cameraSpec.upWC),
      rightWC: c3(cameraSpec.rightWC),
      frustum: {
        near: cameraSpec.frustum.near,
        far: cameraSpec.frustum.far,
        fov: cameraSpec.frustum.fov,
        aspectRatio: cameraSpec.frustum.aspectRatio,
      },
    },
    shadowMapCamera: {
      view: m4(shadowMap._shadowMapCamera.viewMatrix),
      inverseView: m4(shadowMap._shadowMapCamera.inverseViewMatrix),
      positionWC: c3(shadowMap._shadowMapCamera.positionWC),
      directionWC: c3(shadowMap._shadowMapCamera.directionWC),
      upWC: c3(shadowMap._shadowMapCamera.upWC),
      rightWC: c3(shadowMap._shadowMapCamera.rightWC),
      frustum: {
        left: shadowMap._shadowMapCamera.frustum.left,
        right: shadowMap._shadowMapCamera.frustum.right,
        bottom: shadowMap._shadowMapCamera.frustum.bottom,
        top: shadowMap._shadowMapCamera.frustum.top,
        near: shadowMap._shadowMapCamera.frustum.near,
        far: shadowMap._shadowMapCamera.frustum.far,
      },
    },
    sceneCameraNearFar: [shadowMap._sceneCamera.frustum.near, shadowMap._sceneCamera.frustum.far],
    distance: shadowMap._distance,
    outOfView: shadowMap._outOfView,
    darkness: shadowMap._darkness,
    lightDirectionEC: c3(shadowMap._lightDirectionEC),
    lightPositionEC: c4(shadowMap._lightPositionEC),
    shadowMapMatrix: m4(shadowMap._shadowMapMatrix),
    cascadeSplits: shadowMap._cascadeSplits.map(c4),
    cascadeDistances: c4(shadowMap._cascadeDistances),
    cascadeMatrices: shadowMap._cascadeMatrices.map(m4),
    passes: shadowMap._passes.map((p) => ({
      cameraView: m4(p.camera.viewMatrix),
      frustum: {
        left: p.camera.frustum.left,
        right: p.camera.frustum.right,
        bottom: p.camera.frustum.bottom,
        top: p.camera.frustum.top,
        near: p.camera.frustum.near,
        far: p.camera.frustum.far,
      },
      cullingPlanes: p.cullingVolume ? p.cullingVolume.planes.map(c4) : null,
    })),
    cullingPlanes: shadowMap._shadowMapCullingVolume ? shadowMap._shadowMapCullingVolume.planes.map(c4) : null,
  });
}

writeFileSync(new URL('../../tests/data/scene-shadow-map.json', import.meta.url), JSON.stringify({
  maximumDistance: ShadowMap.MAXIMUM_DISTANCE,
  constructor: constructorCases,
  update: updateCases,
}, null, 2));
console.log('wrote tests/data/scene-shadow-map.json');
