// Pinned CesiumJS shader and draw-command capture for VoxelRenderResources,
// processVoxelProperties and buildVoxelDrawCommands. ShaderProgram.fromCache is
// replaced so no WebGL context is needed; the generated source is the real
// ShaderBuilder output.
import {writeFileSync} from 'node:fs';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import ClippingPlane from '@cesium/engine/Source/Scene/ClippingPlane.js';
import ClippingPlaneCollection from '@cesium/engine/Source/Scene/ClippingPlaneCollection.js';
import CustomShader from '@cesium/engine/Source/Scene/Model/CustomShader.js';
import MetadataType from '@cesium/engine/Source/Scene/MetadataType.js';
import VoxelBoxShape from '@cesium/engine/Source/Scene/VoxelBoxShape.js';
import VoxelCylinderShape from '@cesium/engine/Source/Scene/VoxelCylinderShape.js';
import VoxelEllipsoidShape from '@cesium/engine/Source/Scene/VoxelEllipsoidShape.js';
import VoxelMetadataOrder from '@cesium/engine/Source/Scene/VoxelMetadataOrder.js';
import VoxelShapeType from '@cesium/engine/Source/Scene/VoxelShapeType.js';
import buildVoxelDrawCommands from '@cesium/engine/Source/Scene/buildVoxelDrawCommands.js';
import ShaderProgram from '@cesium/engine/Source/Renderer/ShaderProgram.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';

ContextLimits._minimumAliasedLineWidth = 1;
ContextLimits._maximumAliasedLineWidth = 1;
ContextLimits._maximumTextureSize = 4096;

const captures = [];
const originalFromCache = ShaderProgram.fromCache;
ShaderProgram.fromCache = function (options) {
  captures.push({
    defines: options.fragmentShaderSource.defines,
    vertexDefines: options.vertexShaderSource.defines,
    vertex: options.vertexShaderSource.sources.join('\n'),
    fragment: options.fragmentShaderSource.sources.join('\n'),
    attributes: options.attributeLocations,
  });
  return {_bind() {}, destroy() {}};
};

const context = {
  floatingPointTexture: true,
  drawBuffers: true,
  fragmentDepth: true,
  getViewportQuadVertexArray() { return {}; },
  createPickId() { return {color: new Color(0, 0, 0, 1)}; },
};

const defaultShader = new CustomShader({
  fragmentShaderText: `void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material)
{
    material.diffuse = vec3(1.0);
    material.alpha = 1.0;
}`,
});

function makeShape(type) {
  const shape = type === 'BOX' ? new VoxelBoxShape() : type === 'CYLINDER' ? new VoxelCylinderShape() : new VoxelEllipsoidShape();
  const min = type === 'ELLIPSOID' ? Cartesian3.fromElements(-Math.PI, -Math.PI / 2, 0) : type === 'CYLINDER' ? Cartesian3.fromElements(0, -Math.PI, -1) : Cartesian3.fromElements(-1, -1, -1);
  const max = type === 'ELLIPSOID' ? Cartesian3.fromElements(Math.PI, Math.PI / 2, 1000) : type === 'CYLINDER' ? Cartesian3.fromElements(1, Math.PI, 1) : Cartesian3.fromElements(1, 1, 1);
  const model = type === 'ELLIPSOID' ? Matrix4.fromScale(new Cartesian3(6378137, 6378137, 6356752.314245179)) : Matrix4.IDENTITY;
  shape.update(model, min, max, min, max);
  return shape;
}

function makePrimitive({type, depthTest, clipping, union, padding, yUp, logDepth, sampleCount, statistics}) {
  const shape = makeShape(type);
  const clippingPlanes = clipping === 0 ? undefined : new ClippingPlaneCollection({
    planes: Array.from({length: clipping}, (_, i) => new ClippingPlane(new Cartesian3(0, 0, 1), i * 0.25)),
    unionClippingRegions: union,
  });
  const names = statistics ? ['density', 'velocity', 'flag'] : [];
  const provider = {
    metadataOrder: yUp ? VoxelMetadataOrder.Y_UP : VoxelMetadataOrder.Z_UP,
    shape: type,
    names,
    types: statistics ? [MetadataType.SCALAR, MetadataType.VEC3, MetadataType.VEC4] : [],
    componentTypes: ['FLOAT32', 'UINT8', 'FLOAT32'],
    minimumValues: statistics ? [[0], [0, 0, 0], [0, 0, 0, 0]] : undefined,
    maximumValues: statistics ? [[1], [2, 3, 4], [1, 1, 1, 1]] : undefined,
  };
  const primitive = {
    _provider: provider,
    _customShader: defaultShader,
    _uniformMap: {u_stepSize() { return 1; }},
    _clippingPlanes: clippingPlanes,
    _shape: shape,
    _depthTest: depthTest,
    paddingBefore: padding ? new Cartesian3(1, 1, 1) : Cartesian3.ZERO,
    paddingAfter: padding ? new Cartesian3(2, 2, 2) : Cartesian3.ZERO,
    _useLogDepth: logDepth,
    _traversal: {_sampleCount: sampleCount},
    _compoundModelMatrix: Matrix4.IDENTITY,
    provider,
  };
  return primitive;
}

const cases = [];
for (const type of [VoxelShapeType.BOX, VoxelShapeType.CYLINDER, VoxelShapeType.ELLIPSOID]) {
  for (const depthTest of [true, false]) {
    for (const clipping of [0, 1, 2]) {
      for (const union of clipping === 2 ? [false, true] : [false]) {
        const settings = {
          type,
          depthTest,
          clipping,
          union,
          padding: clipping === 0,
          yUp: type !== VoxelShapeType.ELLIPSOID,
          logDepth: depthTest,
          sampleCount: depthTest ? 2 : 1,
          statistics: clipping === 0,
        };
        const primitive = makePrimitive(settings);
        captures.length = 0;
        buildVoxelDrawCommands(primitive, context);
        const command = primitive._drawCommand;
        const pick = primitive._drawCommandPick;
        const pickVoxel = primitive._drawCommandPickVoxel;
        cases.push({
          settings,
          shader: {
            defines: captures[0].defines,
            vertexDefines: captures[0].vertexDefines,
            vertex: captures[0].vertex,
            fragment: captures[0].fragment,
            attributes: captures[0].attributes,
            pickDefines: captures[1].defines,
            pickVoxelDefines: captures[2].defines,
            pickSharesSource:
              captures[1].fragment === captures[0].fragment &&
              captures[1].vertex === captures[0].vertex,
            pickVoxelSharesSource:
              captures[2].fragment === captures[0].fragment &&
              captures[2].vertex === captures[0].vertex,
          },
          renderState: {
            cull: command.renderState.cull,
            depthTest: command.renderState.depthTest,
            depthMask: command.renderState.depthMask,
            blending: command.renderState.blending,
          },
          command: {
            primitiveType: command.primitiveType,
            pass: command.pass,
            executeInClosestFrustum: command.executeInClosestFrustum,
            cull: command.cull,
            occlude: command.occlude,
            pickOnly: pick.pickOnly,
            pickVoxelOnly: pickVoxel.pickOnly,
            uniformNames: Object.keys(command.uniformMap).sort(),
            modelMatrix: Array.from(command.modelMatrix),
          },
        });
      }
    }
  }
}

ShaderProgram.fromCache = originalFromCache;
writeFileSync(new URL('../../tests/data/scene-voxel-draw-commands.json', import.meta.url), JSON.stringify({version: '23.0.0', cases}, null, 2) + '\n');
