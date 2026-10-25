import ModelResources from '@cesium/engine/Source/Scene/Model/ModelRenderResources.js';
import NodeResources from '@cesium/engine/Source/Scene/Model/NodeRenderResources.js';
import PrimitiveResources from '@cesium/engine/Source/Scene/Model/PrimitiveRenderResources.js';
import Geometry from '@cesium/engine/Source/Scene/Model/GeometryPipelineStage.js';
import Dequantization from '@cesium/engine/Source/Scene/Model/DequantizationPipelineStage.js';
import Lighting from '@cesium/engine/Source/Scene/Model/LightingPipelineStage.js';
import VerticalExaggeration from '@cesium/engine/Source/Scene/Model/VerticalExaggerationPipelineStage.js';
import EdgeDetection from '@cesium/engine/Source/Scene/Model/EdgeDetectionPipelineStage.js';
import Atmosphere from '@cesium/engine/Source/Scene/Model/AtmospherePipelineStage.js';
import ModelSilhouette from '@cesium/engine/Source/Scene/Model/ModelSilhouettePipelineStage.js';
import Classification from '@cesium/engine/Source/Scene/Model/ClassificationPipelineStage.js';
import ModelType from '@cesium/engine/Source/Scene/Model/ModelType.js';
import LightingModel from '@cesium/engine/Source/Scene/Model/LightingModel.js';
import AttributeType from '@cesium/engine/Source/Scene/AttributeType.js';
import ComponentDatatype from '@cesium/engine/Source/Core/ComponentDatatype.js';
import PrimitiveType from '@cesium/engine/Source/Core/PrimitiveType.js';
import SceneMode from '@cesium/engine/Source/Scene/SceneMode.js';
import {BoundingSphere, Cartesian2, Cartesian3, Cartesian4, Color, Matrix2, Matrix3, Matrix4} from '@cesium/engine';
import {suite, enc} from './lib.mjs';

function uv(v) {
  if (v === undefined || v === null) return null;
  if (typeof v === 'boolean') return v ? 1 : 0;
  if (typeof v === 'number') return enc(v);
  if (typeof v === 'object') {
    if ('red' in v) return [v.red, v.green, v.blue, v.alpha].map(enc);
    if ('w' in v) return [v.x, v.y, v.z, v.w].map(enc);
    if ('z' in v) return [v.x, v.y, v.z].map(enc);
    if ('y' in v) return [v.x, v.y].map(enc);
    return enc(v);
  }
  return enc(v);
}

function mathValue(type, value) {
  if (value === undefined) return undefined;
  if (!Array.isArray(value)) return value;
  switch (type) {
    case 'SCALAR': return value[0];
    case 'VEC2': return new Cartesian2(...value);
    case 'VEC3': return new Cartesian3(...value);
    case 'VEC4': return new Cartesian4(...value);
    case 'MAT2': return Matrix2.fromColumnMajorArray(value);
    case 'MAT3': return Matrix3.fromColumnMajorArray(value);
    case 'MAT4': return Matrix4.fromColumnMajorArray(value);
    default: return value;
  }
}

function attribute(a) {
  const out = {
    semantic: a.semantic,
    name: a.name,
    setIndex: a.setIndex,
    type: a.type,
    componentDatatype: a.componentDatatype ?? ComponentDatatype.FLOAT,
    count: a.count ?? 4,
    normalized: a.normalized === true,
    byteOffset: a.byteOffset ?? 0,
    byteStride: a.byteStride ?? 12,
  };
  if (a.constant !== undefined) out.constant = mathValue(a.type, a.constant);
  if (a.min !== undefined) out.min = mathValue(a.type, a.min);
  if (a.max !== undefined) out.max = mathValue(a.type, a.max);
  if (a.buffer) out.buffer = {fake: true};
  if (a.quantization) {
    const q = a.quantization;
    out.quantization = {
      type: q.type,
      componentDatatype: q.componentDatatype ?? ComponentDatatype.FLOAT,
      octEncoded: q.octEncoded === true,
      octEncodedZXY: q.octEncodedZXY === true,
      normalizationRange: q.normalizationRange,
      quantizedVolumeOffset: mathValue(a.type, q.quantizedVolumeOffset),
      quantizedVolumeStepSize: mathValue(a.type, q.quantizedVolumeStepSize),
    };
  }
  return out;
}

function frameState(o = {}) {
  return {
    mode: o.mode ?? SceneMode.SCENE3D,
    scene3DOnly: o.scene3DOnly === true,
    context: {webgl2: o.webgl2 !== false},
    camera: {positionWC: new Cartesian3(...(o.cameraPosition ?? [1, 2, 3]))},
    fog: {density: o.fogDensity ?? 0},
    verticalExaggeration: o.verticalExaggeration ?? 1,
    verticalExaggerationRelativeHeight: o.verticalExaggerationRelativeHeight ?? 0,
  };
}

function resources(modelOverrides = {}, instanced = false) {
  const model = {
    type: ModelType.GLTF,
    _projectTo2D: false,
    classificationType: undefined,
    lightColor: undefined,
    boundingSphere: new BoundingSphere(new Cartesian3(1, 2, 3), 10),
    silhouetteColor: new Color(1, 0, 0, 1),
    silhouetteSize: 0,
    ...modelOverrides,
  };
  const modelResources = new ModelResources(model);
  const runtimeNode = {node: {instances: instanced ? {} : undefined}};
  const nodeResources = new NodeResources(modelResources, runtimeNode);
  nodeResources.instanceCount = instanced ? 4 : 0;
  return {model, modelResources, nodeResources};
}

function primitiveResources(nodeResources, primitive, opts = {}) {
  const runtimePrimitive = {primitive};
  const p = new PrimitiveResources(nodeResources, runtimePrimitive);
  if (opts.positionBuffer2D) p.runtimePrimitive.positionBuffer2D = {fake: true};
  if (opts.instanced) p.hasInstances = true;
  return p;
}

function shaderSnapshot(r) {
  const sb = r.shaderBuilder;
  const structLines = (ids) => ids.flatMap((id) => sb._structs[id].generateGlslLines());
  const functionLines = (ids) => ids.flatMap((id) => sb._functions[id].generateGlslLines());
  const position = sb._positionAttributeLine !== undefined ? [sb._positionAttributeLine] : [];
  const vertex = position
    .concat(
      sb._attributeLines,
      sb._vertexShaderParts.uniformLines,
      sb._vertexShaderParts.varyingLines,
      structLines(sb._vertexShaderParts.structIds),
      functionLines(sb._vertexShaderParts.functionIds),
      sb._vertexShaderParts.shaderLines,
    )
    .join('\n');
  const fragment = sb._fragmentShaderParts.uniformLines
    .concat(
      sb._fragmentShaderParts.varyingLines,
      structLines(sb._fragmentShaderParts.structIds),
      functionLines(sb._fragmentShaderParts.functionIds),
      sb._fragmentShaderParts.shaderLines,
    )
    .join('\n');
  return {
    vertex: {defines: sb._vertexShaderParts.defineLines, source: vertex},
    fragment: {defines: sb._fragmentShaderParts.defineLines, source: fragment},
    attributeLocations: {...sb._attributeLocations},
  };
}

function attributeSnapshot(a) {
  return {
    index: a.index,
    value: uv(a.value),
    buffer: a.vertexBuffer !== undefined,
    components: a.componentsPerAttribute,
    datatype: a.componentDatatype,
    offset: a.offsetInBytes ?? 0,
    stride: a.strideInBytes ?? 0,
    normalize: a.normalize === true,
  };
}

function snapshot(r, extra = {}) {
  return {
    ...shaderSnapshot(r),
    uniforms: Object.fromEntries(Object.entries(r.uniformMap).map(([k, f]) => [k, uv(f())])),
    attributes: r.attributes.map(attributeSnapshot),
    attributeIndex: r.attributeIndex,
    featureIdSetIndex: r.featureIdVertexAttributeSetIndex,
    hasSilhouette: r.hasSilhouette,
    ...extra,
  };
}

function geometryCase(c) {
  const {modelResources, nodeResources} = resources(
    {type: c.modelType ?? ModelType.GLTF, _projectTo2D: c.projectTo2D === true},
    c.instanced === true,
  );
  const primitive = {
    attributes: c.attrs.map(attribute),
    primitiveType: c.primitiveType === 'POINTS' ? PrimitiveType.POINTS : PrimitiveType.TRIANGLES,
    indices: c.indicesCount === undefined ? undefined : {count: c.indicesCount},
  };
  const p = primitiveResources(nodeResources, primitive, {
    positionBuffer2D: c.projectTo2D === true && c.instanced !== true,
    instanced: c.instanced === true,
  });
  Geometry.process(p, primitive, frameState(c));
  return snapshot(p);
}

function basePrimitive(attrs = []) {
  return {
    attributes: [
      {
        semantic: 'POSITION',
        type: 'VEC3',
        count: 4,
        min: new Cartesian3(-1, -1, -1),
        max: new Cartesian3(1, 1, 1),
        constant: new Cartesian3(1, 2, 3),
        byteOffset: 0,
        byteStride: 12,
        normalized: false,
      },
      ...attrs,
    ],
    primitiveType: PrimitiveType.TRIANGLES,
  };
}

function dequantizationCase(c) {
  const {nodeResources} = resources({classificationType: c.classification ? 1 : undefined});
  const primitive = basePrimitive(c.attrs.map(attribute));
  const p = primitiveResources(nodeResources, primitive);
  Dequantization.process(p, primitive, frameState());
  return snapshot(p);
}

function lightingCase(c) {
  const {nodeResources} = resources({lightColor: c.lightColor ? new Cartesian3(...c.lightColor) : undefined});
  const primitive = basePrimitive();
  const p = primitiveResources(nodeResources, primitive);
  p.lightingOptions.lightingModel = c.lightingModel === 'PBR' ? LightingModel.PBR : LightingModel.UNLIT;
  Lighting.process(p, primitive);
  return snapshot(p);
}

function verticalCase(c) {
  const {nodeResources} = resources();
  const primitive = basePrimitive();
  const p = primitiveResources(nodeResources, primitive);
  VerticalExaggeration.process(p, primitive, frameState(c));
  return snapshot(p);
}

function edgeCase() {
  const {nodeResources} = resources();
  const primitive = basePrimitive();
  const p = primitiveResources(nodeResources, primitive);
  EdgeDetection.process(p);
  return snapshot(p);
}

function atmosphereCase(c) {
  const {modelResources, model} = resources({
    boundingSphere: new BoundingSphere(new Cartesian3(...c.center), c.radius ?? 10),
  });
  Atmosphere.process(modelResources, model, frameState(c));
  return {
    ...shaderSnapshot(modelResources),
    uniforms: Object.fromEntries(
      Object.entries(modelResources.uniformMap).map(([k, f]) => [k, uv(f())]),
    ),
  };
}

function silhouetteCase(c) {
  const {modelResources, nodeResources, model} = resources({
    silhouetteColor: new Color(...c.color),
    silhouetteSize: c.size,
    _silhouetteId: c.id,
  });
  const primitive = basePrimitive();
  const p = primitiveResources(nodeResources, primitive);
  ModelSilhouette.process(p, model, frameState());
  // The uniform providers must read the latest model values.
  model.silhouetteColor = new Color(...c.afterColor);
  model.silhouetteSize = c.afterSize;
  return snapshot(p, {silhouetteId: model._silhouetteId});
}

function classificationCase(c) {
  const {nodeResources} = resources();
  const position = {
    semantic: 'POSITION',
    type: 'VEC3',
    componentDatatype: ComponentDatatype.FLOAT,
    count: c.count,
    min: new Cartesian3(-1, -1, -1),
    max: new Cartesian3(1, 1, 1),
    constant: new Cartesian3(1, 2, 3),
    byteOffset: 0,
    byteStride: 12,
    normalized: false,
  };
  const attrs = c.noPosition ? [] : [position];
  if (c.featureIds) {
    attrs.push({
      semantic: '_FEATURE_ID',
      setIndex: c.featureSetIndex ?? 0,
      type: 'SCALAR',
      componentDatatype: ComponentDatatype.UNSIGNED_SHORT,
      count: c.count,
      typedArray: new Uint16Array(c.featureIds),
      byteOffset: 0,
      byteStride: 2,
      normalized: false,
    });
  }
  const primitive = {
    attributes: attrs,
    primitiveType: PrimitiveType.TRIANGLES,
    indices:
      c.indices === undefined
        ? undefined
        : {count: c.indices.length, typedArray: new Uint16Array(c.indices)},
  };
  const p = primitiveResources(nodeResources, {
    attributes: [position],
    primitiveType: PrimitiveType.TRIANGLES,
    indices: primitive.indices,
  });
  const processPrimitive = c.noPosition
    ? {attributes: [], primitiveType: PrimitiveType.TRIANGLES}
    : primitive;
  Classification.process(p, processPrimitive, frameState());
  if (c.repeat) {
    // A second call must not recompute batches.
    const other = {
      attributes: [position],
      primitiveType: PrimitiveType.TRIANGLES,
      indices: {count: 2, typedArray: new Uint16Array([0, 1])},
    };
    Classification.process(p, other, frameState());
  }
  return {
    batchLengths: p.runtimePrimitive.batchLengths,
    batchOffsets: p.runtimePrimitive.batchOffsets,
    defines: p.shaderBuilder._vertexShaderParts.defineLines,
  };
}

export function sceneModelStages() {
  const s = suite('ModelStages');
  ModelSilhouette.silhouettesLength = 0;

  const position = {
    semantic: 'POSITION',
    type: 'VEC3',
    count: 4,
    constant: [1, 2, 3],
    min: [-1, -1, -1],
    max: [1, 1, 1],
    byteOffset: 0,
    byteStride: 12,
  };
  const normal = {
    semantic: 'NORMAL',
    type: 'VEC3',
    count: 4,
    constant: [0, 1, 0],
    byteOffset: 0,
    byteStride: 12,
  };
  const tangent = {
    semantic: 'TANGENT',
    type: 'VEC4',
    count: 4,
    constant: [1, 0, 0, 1],
    byteOffset: 0,
    byteStride: 16,
  };
  const texcoord = {
    semantic: 'TEXCOORD',
    setIndex: 0,
    type: 'VEC2',
    count: 4,
    constant: [0.25, 0.75],
    byteOffset: 0,
    byteStride: 8,
  };
  const color = {
    semantic: 'COLOR',
    setIndex: 0,
    type: 'VEC4',
    count: 4,
    constant: [0.1, 0.2, 0.3, 0.4],
    byteOffset: 0,
    byteStride: 16,
  };
  const featureId = {
    semantic: '_FEATURE_ID',
    setIndex: 0,
    type: 'SCALAR',
    count: 4,
    constant: [3],
    byteOffset: 0,
    byteStride: 4,
  };
  const custom = {
    name: '_CUSTOM',
    type: 'VEC3',
    count: 4,
    constant: [4, 5, 6],
    byteOffset: 0,
    byteStride: 12,
  };

  const geometryCases = [
    {attrs: [position]},
    {attrs: [position, normal, tangent, texcoord, color, featureId, custom]},
    {attrs: [position, texcoord], primitiveType: 'POINTS'},
    {attrs: [position, texcoord], projectTo2D: true, mode: SceneMode.SCENE2D},
    {attrs: [position, texcoord], projectTo2D: true, mode: SceneMode.SCENE2D, instanced: true},
    {attrs: [position], modelType: ModelType.TILE_PNTS},
    {
      attrs: [
        {
          semantic: 'POSITION',
          type: 'VEC3',
          count: 4,
          constant: [1, 2, 3],
          min: [-1, -1, -1],
          max: [1, 1, 1],
          quantization: {
            type: 'VEC3',
            componentDatatype: ComponentDatatype.BYTE,
            quantizedVolumeOffset: [-1, -1, -1],
            quantizedVolumeStepSize: [0.5, 0.5, 0.5],
          },
          byteOffset: 0,
          byteStride: 3,
        },
        {
          semantic: 'NORMAL',
          type: 'VEC3',
          count: 4,
          constant: [0, 1, 0],
          quantization: {
            type: 'VEC2',
            componentDatatype: ComponentDatatype.BYTE,
            octEncoded: true,
            normalizationRange: 1,
          },
          byteOffset: 0,
          byteStride: 2,
        },
        {
          semantic: 'TEXCOORD',
          setIndex: 0,
          type: 'VEC2',
          count: 4,
          constant: [0, 0],
          quantization: {
            type: 'VEC2',
            componentDatatype: ComponentDatatype.UNSIGNED_SHORT,
            quantizedVolumeOffset: [0, 0],
            quantizedVolumeStepSize: [1 / 65535, 1 / 65535],
          },
          byteOffset: 0,
          byteStride: 4,
        },
      ],
    },
    {
      attrs: [
        position,
        {
          name: '_MAT',
          type: 'MAT4',
          count: 4,
          buffer: true,
          byteOffset: 0,
          byteStride: 64,
        },
      ],
    },
  ];
  for (const c of geometryCases) s.add('geometry', [c], geometryCase);

  const dequantizationCases = [
    {
      attrs: [
        {
          semantic: 'POSITION',
          type: 'VEC3',
          count: 4,
          quantization: {
            type: 'VEC3',
            componentDatatype: ComponentDatatype.BYTE,
            quantizedVolumeOffset: [-1, -2, -3],
            quantizedVolumeStepSize: [0.5, 0.25, 0.125],
          },
          byteOffset: 0,
          byteStride: 3,
        },
        {
          semantic: 'TEXCOORD',
          setIndex: 0,
          type: 'VEC2',
          count: 4,
          quantization: {
            type: 'VEC2',
            componentDatatype: ComponentDatatype.UNSIGNED_SHORT,
            quantizedVolumeOffset: [0, 0],
            quantizedVolumeStepSize: [1 / 65535, 1 / 65535],
          },
          byteOffset: 0,
          byteStride: 4,
        },
        {
          semantic: 'COLOR',
          setIndex: 0,
          type: 'VEC3',
          count: 4,
          quantization: {
            type: 'VEC3',
            componentDatatype: ComponentDatatype.UNSIGNED_BYTE,
            quantizedVolumeOffset: [0, 0, 0],
            quantizedVolumeStepSize: [1 / 255, 1 / 255, 1 / 255],
          },
          byteOffset: 0,
          byteStride: 3,
        },
      ],
    },
    {
      attrs: [
        {
          semantic: 'NORMAL',
          type: 'VEC3',
          count: 4,
          quantization: {
            type: 'VEC2',
            componentDatatype: ComponentDatatype.BYTE,
            octEncoded: true,
            normalizationRange: 1,
          },
          byteOffset: 0,
          byteStride: 2,
        },
        {
          semantic: 'NORMAL',
          setIndex: 1,
          type: 'VEC3',
          count: 4,
          quantization: {
            type: 'VEC2',
            componentDatatype: ComponentDatatype.BYTE,
            octEncoded: true,
            octEncodedZXY: true,
            normalizationRange: 0.5,
          },
          byteOffset: 0,
          byteStride: 2,
        },
      ],
    },
    {
      classification: true,
      attrs: [
        {
          semantic: 'POSITION',
          type: 'VEC3',
          count: 4,
          quantization: {
            type: 'VEC3',
            componentDatatype: ComponentDatatype.BYTE,
            quantizedVolumeOffset: [0, 0, 0],
            quantizedVolumeStepSize: [1, 1, 1],
          },
          byteOffset: 0,
          byteStride: 3,
        },
        {
          semantic: 'COLOR',
          setIndex: 0,
          type: 'VEC3',
          count: 4,
          quantization: {
            type: 'VEC3',
            componentDatatype: ComponentDatatype.UNSIGNED_BYTE,
            quantizedVolumeOffset: [0, 0, 0],
            quantizedVolumeStepSize: [1, 1, 1],
          },
          byteOffset: 0,
          byteStride: 3,
        },
      ],
    },
  ];
  for (const c of dequantizationCases) s.add('dequantization', [c], dequantizationCase);

  for (const c of [
    {lightColor: [1, 2, 3], lightingModel: 'PBR'},
    {lightColor: [1, 2, 3], lightingModel: 'UNLIT'},
    {lightingModel: 'PBR'},
    {lightingModel: 'UNLIT'},
  ]) {
    s.add('lighting', [c], lightingCase);
  }

  for (const c of [
    {verticalExaggeration: 1, verticalExaggerationRelativeHeight: 0},
    {verticalExaggeration: 2.5, verticalExaggerationRelativeHeight: 100},
    {verticalExaggeration: 0, verticalExaggerationRelativeHeight: -5},
  ]) {
    s.add('vertical', [c], verticalCase);
  }

  s.add('edge', [{}], edgeCase);

  for (const c of [
    {center: [1, 2, 3], cameraPosition: [0, 0, 0], fogDensity: 0},
    {center: [0, 0, 0], cameraPosition: [0, 0, 0], fogDensity: 1e-2},
    {center: [0, 0, 0], cameraPosition: [0, 0, 0], fogDensity: 5e-3},
    {center: [1000, 0, 0], cameraPosition: [0, 0, 0], fogDensity: 1e-3},
  ]) {
    s.add('atmosphere', [c], atmosphereCase);
  }

  for (const c of [
    {color: [1, 0, 0, 1], size: 2, afterColor: [0, 1, 0, 1], afterSize: 3},
    {color: [0, 0, 1, 0.5], size: 0, afterColor: [1, 1, 1, 1], afterSize: 1, id: 7},
  ]) {
    s.add('silhouette', [c], silhouetteCase);
  }

  const classificationCases = [
    {count: 5},
    {count: 6, indices: [0, 1, 2, 3, 4, 5]},
    {count: 7, featureIds: [0, 0, 1, 1, 1, 2, 2]},
    {count: 6, indices: [0, 2, 4, 1, 3, 5], featureIds: [0, 1, 1, 2, 2, 2]},
    {count: 4, featureIds: [0, 0, 0, 0], featureSetIndex: 1},
    {count: 4, noPosition: true},
    {count: 5, featureIds: [0, 0, 1, 1, 1], repeat: true},
  ];
  for (const c of classificationCases) s.add('classification', [c], classificationCase);

  return s;
}
