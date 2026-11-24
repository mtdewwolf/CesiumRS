// Pinned CesiumJS PointCloud shader/state capture. A synthetic pnts buffer is
// parsed by the real PntsParser; Buffer.createVertexBuffer, VertexArray and
// ShaderProgram.fromCache are stubbed so the generated shader and attribute
// layout come from upstream without a WebGL context.
import {writeFileSync} from 'node:fs';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import ComponentDatatype from '@cesium/engine/Source/Core/ComponentDatatype.js';
import ClippingPlane from '@cesium/engine/Source/Scene/ClippingPlane.js';
import ClippingPlaneCollection from '@cesium/engine/Source/Scene/ClippingPlaneCollection.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import RendererBuffer from '@cesium/engine/Source/Renderer/Buffer.js';
import ShaderProgram from '@cesium/engine/Source/Renderer/ShaderProgram.js';
import Pass from '@cesium/engine/Source/Renderer/Pass.js';
import PointCloud from '@cesium/engine/Source/Scene/PointCloud.js';
import SceneMode from '@cesium/engine/Source/Scene/SceneMode.js';

ContextLimits._maximumTextureSize = 4096;
ContextLimits._minimumAliasedLineWidth = 1;
ContextLimits._maximumAliasedLineWidth = 1;

function pnts({points, colors = false, normals = false, batchIds = false, translucent = false}) {
  const position = new Float32Array(points * 3);
  for (let i = 0; i < position.length; i++) position[i] = i * 0.5 - 1;
  const featureParts = [new Uint8Array(position.buffer)];
  const featureTable = {POINTS_LENGTH: points, POSITION: {byteOffset: 0}};
  let offset = position.byteLength;
  if (colors) {
    const color = new Uint8Array(points * (translucent ? 4 : 3));
    for (let i = 0; i < color.length; i++) color[i] = (i * 7) & 0xff;
    featureTable.RGB = {byteOffset: offset};
    if (translucent) {
      delete featureTable.RGB;
      featureTable.RGBA = {byteOffset: offset};
    }
    featureParts.push(color);
    offset += color.byteLength;
  }
  if (normals) {
    const normal = new Float32Array(points * 3);
    for (let i = 0; i < normal.length; i++) normal[i] = 1;
    featureTable.NORMAL = {byteOffset: offset};
    featureParts.push(new Uint8Array(normal.buffer));
    offset += normal.byteLength;
  }
  if (batchIds) {
    const ids = new Uint8Array(points);
    for (let i = 0; i < points; i++) ids[i] = i % 2;
    featureTable.BATCH_ID = {byteOffset: offset};
    featureTable.BATCH_LENGTH = 2;
    featureParts.push(ids);
    offset += ids.byteLength;
  }
  const featureJson = Buffer.from(JSON.stringify(featureTable));
  const featureJsonLength = Math.ceil(featureJson.length / 8) * 8;
  const featureJsonPadded = Buffer.alloc(featureJsonLength, 0x20);
  featureJson.copy(featureJsonPadded);
  const featureBinary = Buffer.concat(featureParts);
  const batchJson = batchIds ? {name: ['zero', 'one']} : null;
  const batchJsonBuffer = batchJson ? Buffer.from(JSON.stringify(batchJson)) : Buffer.alloc(0);
  const batchJsonLength = Math.ceil(batchJsonBuffer.length / 8) * 8;
  const batchJsonPadded = Buffer.alloc(batchJsonLength, 0x20);
  batchJsonBuffer.copy(batchJsonPadded);
  const byteLength = 28 + featureJsonLength + featureBinary.length + batchJsonLength;
  const header = Buffer.alloc(28);
  header.write('pnts', 0, 'ascii');
  header.writeUInt32LE(1, 4);
  header.writeUInt32LE(byteLength, 8);
  header.writeUInt32LE(featureJsonLength, 12);
  header.writeUInt32LE(featureBinary.length, 16);
  header.writeUInt32LE(batchJsonLength, 20);
  header.writeUInt32LE(0, 24);
  return Buffer.concat([header, featureJsonPadded, featureBinary, batchJsonPadded]);
}

const captures = [];
ShaderProgram.fromCache = function (options) {
  const source = (value) => (typeof value === 'string' ? value : value.sources.join('\n'));
  captures.push({
    vertex: source(options.vertexShaderSource),
    fragment: source(options.fragmentShaderSource),
    defines: options.fragmentShaderSource.defines ?? [],
    attributes: options.attributeLocations,
  });
  return {_bind() {}, destroy() {}};
};
const vertexBuffers = [];
RendererBuffer.createVertexBuffer = function (options) {
  vertexBuffers.push({bytes: options.typedArray.byteLength});
  return {sizeInBytes: options.typedArray.byteLength, destroy() {}};
};
const noop = () => {};
const gl = new Proxy({}, {
  get(_t, key) {
    if (typeof key === 'string' && key.startsWith('create')) return () => ({});
    if (key === 'getParameter') return () => 4096;
    return noop;
  },
});
const context = {
  webgl2: true,
  _gl: gl,
  options: {getWebGLStub: () => true},
  floatingPointTexture: true,
  halfFloatingPointTexture: true,
  _textureFilterAnisotropic: true,
  drawingBufferWidth: 64,
  drawingBufferHeight: 64,
  defaultTexture: {},
  floatingPointTexture: true,
  capabilities() { return {floatingPointTexture: true}; },
  limits() { return {maximumTextureSize: 4096}; },
  uniformState: {view3D: Matrix4.IDENTITY, view: Matrix4.IDENTITY, projection: Matrix4.IDENTITY},
  createPickId() { return {color: new Color(0, 0, 0, 1), destroy() {}}; },
  createVertexArray() { return {}; },
  createBuffer() { return {}; },
  isDestroyed() { return false; },
};

function frameState(mode = SceneMode.SCENE3D, clippingPlanes) {
  return {
    context,
    mode,
    pixelRatio: 2,
    drawingBufferHeight: 64,
    commandList: [],
    passes: {render: true, pick: false},
    camera: {frustum: {sseDenominator: 1.0}},
    mapProjection: {},
    afterRender: [],
  };
}

const cases = [];
const settingsList = [
  {points: 4, colors: false, normals: false, batchIds: false, translucent: false, attenuation: false, backFaceCulling: false, normalShading: true, style: null, clipping: 0, split: false},
  {points: 4, colors: true, normals: true, batchIds: false, translucent: false, attenuation: false, backFaceCulling: false, normalShading: true, style: null, clipping: 0, split: false},
  {points: 4, colors: true, normals: true, batchIds: false, translucent: true, attenuation: false, backFaceCulling: false, normalShading: true, style: null, clipping: 0, split: false},
  {points: 4, colors: true, normals: true, batchIds: true, translucent: false, attenuation: true, backFaceCulling: true, normalShading: true, style: null, clipping: 0, split: false},
  {points: 4, colors: false, normals: false, batchIds: false, translucent: false, attenuation: false, backFaceCulling: false, normalShading: true, style: {color: 'color("red")'}, clipping: 0, split: false},
  {points: 4, colors: false, normals: false, batchIds: false, translucent: false, attenuation: false, backFaceCulling: false, normalShading: true, style: null, clipping: 1, split: false},
];

for (const settings of settingsList) {
  const arrayBuffer = pnts(settings);
  const cloud = new PointCloud({
    arrayBuffer: arrayBuffer.buffer.slice(arrayBuffer.byteOffset, arrayBuffer.byteOffset + arrayBuffer.byteLength),
    cull: true,
    opaquePass: settings.batchIds ? Pass.CESIUM_3D_TILE : Pass.OPAQUE,
    pickIdLoaded: () => 'czm_pickColor',
    vertexShaderLoaded: (shader) => shader,
    fragmentShaderLoaded: (shader) => shader,
    uniformMapLoaded: (map) => map,
    batchTableLoaded: () => {},
  });
  cloud.attenuation = settings.attenuation;
  cloud.backFaceCulling = settings.backFaceCulling;
  cloud.normalShading = settings.normalShading;
  cloud.geometricError = 2.5;
  cloud.geometricErrorScale = 3;
  cloud.maximumAttenuation = 10;
  if (settings.style) {
    const {default: Cesium3DTileStyle} = await import('@cesium/engine/Source/Scene/Cesium3DTileStyle.js');
    cloud.style = new Cesium3DTileStyle(settings.style);
  }
  if (settings.clipping) {
    cloud.clippingPlanes = new ClippingPlaneCollection({planes: [new ClippingPlane(new Cartesian3(0, 0, 1), 0)]});
    cloud.isClipped = true;
  }
  captures.length = 0;
  vertexBuffers.length = 0;
  const frame = frameState();
  cloud.update(frame);
  const command = cloud._drawCommand;
  const attributeStates = [];
  for (let i = 0; i < command.vertexArray.numberOfAttributes; i++) {
    const attribute = command.vertexArray.getAttribute(i);
    attributeStates.push({
      index: attribute.index,
      enabled: attribute.enabled,
      componentsPerAttribute: attribute.componentsPerAttribute,
      componentDatatype: attribute.componentDatatype,
      normalize: attribute.normalize,
    });
  }
  cases.push({
    settings,
    arrayBufferBase64: arrayBuffer.toString('base64'),
    pointsLength: cloud.pointsLength,
    geometryByteLength: cloud.geometryByteLength,
    ready: cloud.ready,
    isTranslucent: cloud._isTranslucent,
    styleTranslucent: cloud._styleTranslucent,
    renderState: {
      pass: command.pass,
      cull: command.cull,
      primitiveType: command.primitiveType,
      count: command.count,
      depthTest: command.renderState.depthTest,
      depthMask: command.renderState.depthMask,
      blending: command.renderState.blending,
    },
    attributes: attributeStates,
    buffers: vertexBuffers.map((buffer) => buffer.bytes),
    shader: captures[0] ?? null,
    uniformNames: Object.keys(command.uniformMap).sort(),
  });
}

writeFileSync(new URL('../../tests/data/scene-point-cloud.json', import.meta.url), JSON.stringify({version: '23.0.0', cases}, null, 2) + '\n');
