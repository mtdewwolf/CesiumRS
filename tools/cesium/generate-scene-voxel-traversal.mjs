// Pinned CesiumJS capture for VoxelTraversal and Megatexture. Textures are
// recorded at copyFrom instead of uploaded, so traversal state and the exact
// octree/megatexture texel bytes come from the real upstream logic.
import {writeFileSync} from 'node:fs';
import Cartesian2 from '@cesium/engine/Source/Core/Cartesian2.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Cartesian4 from '@cesium/engine/Source/Core/Cartesian4.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import CullingVolume from '@cesium/engine/Source/Core/CullingVolume.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import Texture from '@cesium/engine/Source/Renderer/Texture.js';
import Texture3D from '@cesium/engine/Source/Renderer/Texture3D.js';
import Megatexture from '@cesium/engine/Source/Scene/Megatexture.js';
import VoxelBoxShape from '@cesium/engine/Source/Scene/VoxelBoxShape.js';
import VoxelContent from '@cesium/engine/Source/Scene/VoxelContent.js';
import VoxelTraversal from '@cesium/engine/Source/Scene/VoxelTraversal.js';

ContextLimits._maximum3DTextureSize = 2048;
ContextLimits._maximumTextureSize = 4096;

const writes = [];
const noop = () => {};
const gl = new Proxy({}, {
  get(_target, key) {
    if (key === 'getParameter') return () => 2048;
    if (typeof key === 'string' && key.startsWith('create')) return () => ({});
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
  drawingBufferHeight: 1024,
};
const record = (texture) => (options) => {
  writes.push({
    texture,
    width: options.source?.width ?? options.width,
    height: options.source?.height ?? options.height,
    depth: options.source?.depth,
    x: options.xOffset ?? 0,
    y: options.yOffset ?? 0,
    z: options.zOffset ?? 0,
    bytes: Array.from(options.source.arrayBufferView),
  });
};
Texture.prototype.copyFrom = record('texture');
Texture3D.prototype.copyFrom = record('texture3d');
Texture.prototype.destroy = function () {};
Texture3D.prototype.destroy = function () {};

const dimensions = new Cartesian3(2, 2, 2);
const inputDimensions = new Cartesian3(4, 4, 4);
const shape = new VoxelBoxShape();
shape.update(Matrix4.IDENTITY, new Cartesian3(-1, -1, -1), new Cartesian3(1, 1, 1), new Cartesian3(-1, -1, -1), new Cartesian3(1, 1, 1));

const cullingVolume = new CullingVolume([
  new Cartesian4(1, 0, 0, 100),
  new Cartesian4(-1, 0, 0, 100),
  new Cartesian4(0, 1, 0, 100),
  new Cartesian4(0, -1, 0, 100),
  new Cartesian4(0, 0, 1, 100),
  new Cartesian4(0, 0, -1, 100),
]);
const events = {count: 0, raiseEvent() { this.count++; }};
const primitive = {
  _shape: shape,
  _levelBlendFactor: 0,
  _screenSpaceError: 4,
  _calculateStatistics: false,
  _availableLevels: 2,
  dimensions,
  inputDimensions,
  provider: {
    types: ['SCALAR'],
    componentTypes: ['UINT8'],
    maximumTileCount: 64,
    availableLevels: 2,
    keyframeCount: 1,
    requestData(options) {
      if (options.tileLevel >= 2) return undefined;
      return Promise.resolve(new VoxelContent({metadata: [new Uint8Array(64).fill(options.tileLevel + 1)]}));
    },
  },
  tileLoad: events,
  tileVisible: events,
  tileFailed: events,
  tileUnload: events,
  loadProgress: events,
  allTilesLoaded: events,
  initialTilesLoaded: events,
  statistics: {},
};

const traversal = new VoxelTraversal(primitive, context, 1, 8 * 1024 * 1024);
const construction = {
  textureMemoryByteLength: traversal.textureMemoryByteLength,
  internalNodeTilesPerRow: traversal.internalNodeTilesPerRow,
  internalNodeTexelSizeUv: [traversal.internalNodeTexelSizeUv.x, traversal.internalNodeTexelSizeUv.y],
  megatextures: traversal.megatextures.map((megatexture) => ({
    channelCount: megatexture.channelCount,
    maximumTileCount: megatexture.maximumTileCount,
    tileCounts: [megatexture.tileCounts.x, megatexture.tileCounts.y, megatexture.tileCounts.z],
    textureMemoryByteLength: megatexture.textureMemoryByteLength,
  })),
};

function frameState(frameNumber, cameraPosition) {
  return {
    context,
    camera: {positionWC: cameraPosition, frustum: {sseDenominator: 1.0}},
    pixelRatio: 1,
    frameNumber,
    cullingVolume,
    afterRender: [],
  };
}

function serializeNode(node) {
  return {
    level: node.level,
    coords: [node.x, node.y, node.z],
    screenSpaceError: node.screenSpaceError,
    visited: node.visitedFrameNumber,
    renderable: traversal.isRenderable(node),
    previous: node.renderableKeyframeNodePrevious
      ? {level: node.renderableKeyframeNodePrevious.spatialNode.level, keyframe: node.renderableKeyframeNodePrevious.keyframe, megatextureIndex: node.renderableKeyframeNodePrevious.megatextureIndex}
      : null,
    next: node.renderableKeyframeNodeNext
      ? {level: node.renderableKeyframeNodeNext.spatialNode.level, keyframe: node.renderableKeyframeNodeNext.keyframe, megatextureIndex: node.renderableKeyframeNodeNext.megatextureIndex}
      : null,
    lerp: node.renderableKeyframeNodeLerp,
    keyframes: node.keyframeNodes.map((keyframe) => ({
      keyframe: keyframe.keyframe,
      state: keyframe.state,
      priority: keyframe.priority,
      megatextureIndex: keyframe.megatextureIndex,
    })),
    children: node.children ? node.children.map(serializeNode) : null,
  };
}

const steps = [];
for (let step = 0; step < 3; step++) {
  writes.length = 0;
  traversal.update(frameState(step + 1, new Cartesian3(2.5, 2.5, 8)), 0, false, false);
  await new Promise((resolve) => setTimeout(resolve, 0));
  traversal.update(frameState(step + 2, new Cartesian3(2.5, 2.5, 8)), 0, false, false);
  steps.push({
    frameNumber: traversal._frameNumber,
    sampleCount: traversal._sampleCount,
    keyframeLocation: traversal._keyframeLocation,
    root: serializeNode(traversal.rootNode),
    highPriority: traversal._highPriorityKeyframeNodes.slice(0, traversal._highPriorityKeyframeNodeCount).map((node) => ({
      level: node.spatialNode.level,
      coords: [node.spatialNode.x, node.spatialNode.y, node.spatialNode.z],
      keyframe: node.keyframe,
      priority: node.priority,
      highPriorityFrameNumber: node.highPriorityFrameNumber,
      state: node.state,
      megatextureIndex: node.megatextureIndex,
    })),
    writes: writes.map((write) => ({
      texture: write.texture,
      width: write.width,
      height: write.height,
      depth: write.depth,
      x: write.x,
      y: write.y,
      z: write.z,
      bytes: write.bytes,
    })),
  });
}

// Megatexture allocation order and tile-offset writes.
const megatexture = new Megatexture(context, dimensions, 1, 'UINT8', 1024 * 1024, 4);
writes.length = 0;
const addOrder = [];
for (let i = 0; i < 4; i++) {
  addOrder.push(megatexture.add(new Uint8Array(8).fill(i + 1)));
}
const full = megatexture.isFull();
megatexture.remove(1);
const afterRemove = megatexture.add(new Uint8Array(8).fill(9));
const allocation = {
  addOrder,
  full,
  afterRemove,
  occupiedCount: megatexture.occupiedCount,
  isFull: megatexture.isFull(),
  writes: writes.map((write) => ({x: write.x, y: write.y, z: write.z, bytes: write.bytes})),
};

const dimensionCases = [];
for (const [dims, bytes, memory, tiles, max] of [
  [new Cartesian3(8, 8, 8), 1, 128 * 1024 * 1024, 100, 2048],
  [new Cartesian3(8, 8, 8), 1, 128, undefined, 2048],
  [new Cartesian3(1, 2, 4), 4, 1024, 10, 2048],
  [new Cartesian3(4, 2, 1), 8, 4096, undefined, 2048],
]) {
  try {
    const result = Megatexture.get3DTextureDimension(dims, bytes, memory, tiles);
    dimensionCases.push({dims: [dims.x, dims.y, dims.z], bytes, memory, tiles: tiles ?? null, result: [result.x, result.y, result.z]});
  } catch (error) {
    dimensionCases.push({dims: [dims.x, dims.y, dims.z], bytes, memory, tiles: tiles ?? null, throws: error.message});
  }
}

writeFileSync(new URL('../../tests/data/scene-voxel-traversal.json', import.meta.url), JSON.stringify({
  version: '23.0.0',
  dimensions: [2, 2, 2],
  inputDimensions: [4, 4, 4],
  construction,
  steps,
  allocation,
  dimensionCases,
}, null, 2) + '\n');
