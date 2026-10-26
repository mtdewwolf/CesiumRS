import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import Buffer from '@cesium/engine/Source/Renderer/Buffer.js';
import ShaderDestination from '@cesium/engine/Source/Renderer/ShaderDestination.js';
import ModelResources from '@cesium/engine/Source/Scene/Model/ModelRenderResources.js';
import NodeResources from '@cesium/engine/Source/Scene/Model/NodeRenderResources.js';
import PrimitiveResources from '@cesium/engine/Source/Scene/Model/PrimitiveRenderResources.js';
import InstancingPipelineStage from '@cesium/engine/Source/Scene/Model/InstancingPipelineStage.js';
import SceneMode2DPipelineStage from '@cesium/engine/Source/Scene/Model/SceneMode2DPipelineStage.js';
import ImageBasedLightingPipelineStage from '@cesium/engine/Source/Scene/Model/ImageBasedLightingPipelineStage.js';
import PointCloudStylingPipelineStage from '@cesium/engine/Source/Scene/Model/PointCloudStylingPipelineStage.js';
import EdgeVisibilityPipelineStage from '@cesium/engine/Source/Scene/Model/EdgeVisibilityPipelineStage.js';
import ModelType from '@cesium/engine/Source/Scene/Model/ModelType.js';
import InstanceAttributeSemantic from '@cesium/engine/Source/Scene/InstanceAttributeSemantic.js';
import SceneMode from '@cesium/engine/Source/Scene/SceneMode.js';
import {
  BoundingSphere,
  Cartesian2,
  Cartesian3,
  Cartesian4,
  Matrix3,
  Matrix4,
  PerspectiveFrustum,
  OrthographicFrustum,
  GeographicProjection,
  Pass,
} from '@cesium/engine';
import {suite, enc} from './lib.mjs';

function uv(v) {
  if (v === undefined || v === null) return null;
  if (typeof v === 'boolean') return v ? 1 : 0;
  if (typeof v === 'number') return enc(v);
  if (Array.isArray(v)) return v.map(uv);
  if (typeof v === 'object') {
    if ('red' in v) return [v.red, v.green, v.blue, v.alpha].map(enc);
    if ('w' in v) return [v.x, v.y, v.z, v.w].map(enc);
    if ('z' in v) return [v.x, v.y, v.z].map(enc);
    if ('y' in v) return [v.x, v.y].map(enc);
    if ('values' in v) return Array.from(v.values).map(enc);
    return enc(v);
  }
  return enc(v);
}

function shaderSnapshot(r) {
  const sb = r.shaderBuilder;
  return {
    vertex: {
      defines: [...sb._vertexShaderParts.defineLines],
    },
    fragment: {
      defines: [...sb._fragmentShaderParts.defineLines],
    },
    attributeLocations: {...sb._attributeLocations},
  };
}

function uniformSnapshot(r) {
  const out = {};
  for (const [k, fn] of Object.entries(r.uniformMap)) {
    try {
      const val = fn();
      out[k] = uv(val);
    } catch {
      out[k] = null;
    }
  }
  return out;
}

export function sceneModelStagesBatch2() {
  const s = suite('ModelStagesBatch2');

  const oldLimits = [
    ContextLimits._minimumAliasedLineWidth,
    ContextLimits._maximumAliasedLineWidth,
    ContextLimits._maximumTextureSize,
  ];
  ContextLimits._maximumTextureSize = 4096;
  ContextLimits._minimumAliasedLineWidth = 1;
  ContextLimits._maximumAliasedLineWidth = 1;

  const oldV = Buffer.createVertexBuffer;
  const oldI = Buffer.createIndexBuffer;
  Buffer.createVertexBuffer = (o) => ({
    _gpu: true,
    sizeInBytes: o.typedArray ? o.typedArray.byteLength : o.sizeInBytes,
  });
  Buffer.createIndexBuffer = (o) => ({
    _gpu: true,
    sizeInBytes: o.typedArray ? o.typedArray.byteLength : o.sizeInBytes,
    indexDatatype: o.indexDatatype,
  });

  try {
    // 1. InstancingPipelineStage cases
    const runInstancing = (c) => {
      const model = {
        type: ModelType.GLTF,
        _modelResources: [],
        sceneGraph: {
          computedModelMatrix: Matrix4.fromTranslation(new Cartesian3(c.tx ?? 0, c.ty ?? 0, c.tz ?? 0)),
        },
      };
      const m = new ModelResources(model);
      const attrs = [];
      if (c.hasTranslation) {
        attrs.push({
          semantic: 'TRANSLATION',
          type: 'VEC3',
          count: 2,
          typedArray: new Float32Array([1, 2, 3, 4, 5, 6]),
        });
      }
      if (c.hasRotation) {
        attrs.push({
          semantic: 'ROTATION',
          type: 'VEC4',
          count: 2,
          typedArray: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1]),
        });
      }
      if (c.hasScale) {
        attrs.push({
          semantic: 'SCALE',
          type: 'VEC3',
          count: 2,
          typedArray: new Float32Array([1, 1, 1, 2, 2, 2]),
        });
      }
      if (c.hasFeatureId) {
        attrs.push({
          semantic: InstanceAttributeSemantic.FEATURE_ID,
          setIndex: 0,
          type: 'SCALAR',
          count: 2,
          typedArray: new Float32Array([0, 1]),
        });
      }

      const node = {
        instances: {
          attributes: attrs,
          transformInWorldSpace: c.transformInWorldSpace ?? false,
        },
      };
      const n = new NodeResources(m, {node, computedTransform: Matrix4.IDENTITY});
      const frame = {
        mode: c.mode2D ? SceneMode.SCENE2D : SceneMode.SCENE3D,
        scene3DOnly: !c.mode2D,
        morphTime: 1.0,
        mapProjection: new GeographicProjection(),
        context: {uniformState: {view: Matrix4.IDENTITY}},
      };

      InstancingPipelineStage.process(n, node, frame);

      return {
        ...shaderSnapshot(n),
        attributeCount: n.attributes.length,
        hasInstances: n.hasInstances,
        instanceCount: n.instanceCount,
      };
    };

    s.add('instancing', [{hasTranslation: true}], runInstancing);
    s.add('instancing', [{hasTranslation: true, hasRotation: true, hasScale: true}], runInstancing);
    s.add('instancing', [{hasTranslation: true, hasFeatureId: true, transformInWorldSpace: true}], runInstancing);
    s.add('instancing', [{hasTranslation: true, mode2D: true, tx: 6378137}], runInstancing);

    // 2. SceneMode2DPipelineStage cases
    const runSceneMode2D = (c) => {
      const model = {
        type: ModelType.GLTF,
        _modelResources: [],
        sceneGraph: {
          computedModelMatrix: Matrix4.fromTranslation(new Cartesian3(c.tx ?? 6378137, 0, 0)),
        },
      };
      const m = new ModelResources(model);
      const node = {instances: c.instanced ? {} : undefined};
      const n = new NodeResources(m, {node, computedTransform: Matrix4.IDENTITY});
      const prim = {
        attributes: [
          {
            semantic: 'POSITION',
            type: 'VEC3',
            count: 2,
            min: new Cartesian3(-1, -1, -1),
            max: new Cartesian3(1, 1, 1),
            typedArray: new Float32Array([-1, -1, -1, 1, 1, 1]),
          },
        ],
        primitiveType: 4,
      };
      const p = new PrimitiveResources(n, {primitive: prim});
      if (c.instanced) p.hasInstances = true;

      const frame = {
        mode: SceneMode.SCENE2D,
        scene3DOnly: false,
        morphTime: 1.0,
        mapProjection: new GeographicProjection(),
        context: {uniformState: {view: Matrix4.IDENTITY}},
      };

      SceneMode2DPipelineStage.process(p, prim, frame);

      return {
        ...shaderSnapshot(p),
        uniforms: uniformSnapshot(p),
        hasPositionBuffer2D: p.runtimePrimitive.positionBuffer2D !== undefined,
        boundingSphere2D: p.runtimePrimitive.boundingSphere2D
          ? {
              center: [
                p.runtimePrimitive.boundingSphere2D.center.x,
                p.runtimePrimitive.boundingSphere2D.center.y,
                p.runtimePrimitive.boundingSphere2D.center.z,
              ].map(enc),
              radius: enc(p.runtimePrimitive.boundingSphere2D.radius),
            }
          : null,
      };
    };

    s.add('sceneMode2D', [{instanced: false, tx: 6378137}], runSceneMode2D);
    s.add('sceneMode2D', [{instanced: true, tx: 6378137}], runSceneMode2D);
    s.add('sceneMode2D', [{instanced: false, tx: 0}], runSceneMode2D);

    // 3. ImageBasedLightingPipelineStage cases
    const runIBL = (c) => {
      const sh = c.customSH
        ? [
            new Cartesian3(1, 2, 3),
            new Cartesian3(4, 5, 6),
            new Cartesian3(7, 8, 9),
            new Cartesian3(1, 0, 0),
            new Cartesian3(0, 1, 0),
            new Cartesian3(0, 0, 1),
            new Cartesian3(1, 1, 0),
            new Cartesian3(0, 1, 1),
            new Cartesian3(1, 0, 1),
          ]
        : undefined;

      const model = {
        type: ModelType.GLTF,
        _modelResources: [],
        imageBasedLighting: {
          enabled: c.enabled ?? true,
          imageBasedLightingFactor: new Cartesian2(c.factorX ?? 1, c.factorY ?? 1),
          sphericalHarmonicCoefficients: sh,
          useSphericalHarmonics: c.useSphericalHarmonics ?? false,
          useSpecularEnvironmentMaps: false,
          useDefaultSphericalHarmonics: c.useDefaultSH ?? false,
          useDefaultSpecularMaps: c.useDefaultSpecular ?? false,
          specularEnvironmentCubeMap: c.cubeMapReady
            ? {
                ready: true,
                maximumMipmapLevel: 5.0,
                texture: {fake: true},
              }
            : undefined,
        },
        environmentMapManager: {
          intensity: c.intensity ?? 1.0,
          sphericalHarmonicCoefficients: undefined,
          radianceCubeMap: undefined,
        },
        _iblReferenceFrameMatrix: Matrix3.IDENTITY,
        useDefaultSpecularMaps: c.useDefaultSpecular ?? false,
      };

      const m = new ModelResources(model);
      const frame = {
        context: {
          webgl2: true,
          floatingPointTexture: true,
          colorBufferFloat: true,
          supportsTextureLod: true,
          textureFloatLinear: true,
        },
      };

      ImageBasedLightingPipelineStage.process(m, model, frame);

      return {
        ...shaderSnapshot(m),
        uniforms: uniformSnapshot(m),
      };
    };

    s.add('ibl', [{customSH: false, useDefaultSH: true, useDefaultSpecular: true}], runIBL);
    s.add('ibl', [{customSH: true, useSphericalHarmonics: true, cubeMapReady: true, intensity: 2.0}], runIBL);
    s.add('ibl', [{enabled: false, factorX: 0.5, factorY: 0.5}], runIBL);

    // 4. PointCloudStylingPipelineStage cases
    const runPointCloud = (c) => {
      const model = {
        type: ModelType.GLTF,
        _modelResources: [],
        pointCloudShading: {
          attenuation: c.attenuation ?? false,
          backFaceCulling: c.backFaceCulling ?? false,
          maximumAttenuation: c.maxAttenuation ?? 4,
          geometricErrorScale: c.geomScale ?? 1.0,
          baseResolution: c.baseRes ?? 0.05,
        },
      };

      const m = new ModelResources(model);
      const n = new NodeResources(m, {node: {}});
      const prim = {
        attributes: [
          {
            semantic: 'POSITION',
            type: 'VEC3',
            count: 10,
            min: new Cartesian3(-1, -1, -1),
            max: new Cartesian3(1, 1, 1),
          },
        ],
      };
      const p = new PrimitiveResources(n, {primitive: prim});

      const frustum = new PerspectiveFrustum();
      frustum.aspectRatio = 1.0;
      frustum.fov = Math.PI / 3;

      const frame = {
        pixelRatio: c.pixelRatio ?? 1.0,
        mode: c.mode2D ? SceneMode.SCENE2D : SceneMode.SCENE3D,
        camera: {frustum: c.ortho ? new OrthographicFrustum() : frustum},
        drawingBufferHeight: 1080,
      };

      PointCloudStylingPipelineStage.process(p, prim, frame);

      return {
        ...shaderSnapshot(p),
        uniforms: uniformSnapshot(p),
      };
    };

    s.add('pointCloud', [{attenuation: true, backFaceCulling: false, pixelRatio: 1.0}], runPointCloud);
    s.add('pointCloud', [{attenuation: true, backFaceCulling: true, pixelRatio: 2.0, maxAttenuation: 8}], runPointCloud);
    s.add('pointCloud', [{attenuation: true, mode2D: true}], runPointCloud);

    // 5. EdgeVisibilityPipelineStage cases
    const runEdgeVisibility = (c) => {
      const model = {
        type: ModelType.GLTF,
        _modelResources: [],
      };
      const m = new ModelResources(model);
      const n = new NodeResources(m, {node: {}});
      const prim = {
        attributes: [
          {
            semantic: 'POSITION',
            type: 'VEC3',
            count: 3,
            min: new Cartesian3(0, 0, 0),
            max: new Cartesian3(1, 1, 0),
            typedArray: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
          },
        ],
        indices: {
          count: 3,
          typedArray: new Uint16Array([0, 1, 2]),
        },
        edgeVisibility: c.hasEdges ? {visibility: new Uint8Array([7])} : undefined,
      };
      const p = new PrimitiveResources(n, {primitive: prim});
      p.shaderBuilder.addFunction('setDynamicVaryingsVS', 'void setDynamicVaryingsVS()', ShaderDestination.VERTEX);
      const frame = {
        edgeVisibilityRequested: false,
        context: {
          createVertexArray: () => ({}),
          bindVertexArray: () => {},
        },
      };

      EdgeVisibilityPipelineStage.process(p, prim, frame);

      return {
        ...shaderSnapshot(p),
        hasEdgeGeometry: p.edgeGeometry !== undefined,
        edgePass: p.edgeGeometry ? p.edgeGeometry.pass : null,
        edgeIndexCount: p.edgeGeometry ? p.edgeGeometry.indexCount : null,
        edgeVisibilityRequested: frame.edgeVisibilityRequested,
      };
    };

    s.add('edgeVisibility', [{hasEdges: true}], runEdgeVisibility);
    s.add('edgeVisibility', [{hasEdges: false}], runEdgeVisibility);

  } finally {
    Buffer.createVertexBuffer = oldV;
    Buffer.createIndexBuffer = oldI;
    [
      ContextLimits._minimumAliasedLineWidth,
      ContextLimits._maximumAliasedLineWidth,
      ContextLimits._maximumTextureSize,
    ] = oldLimits;
  }

  return s;
}
