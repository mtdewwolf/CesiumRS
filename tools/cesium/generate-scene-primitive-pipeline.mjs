// Direct reference cases for the PrimitivePipeline worker wire format from the
// pinned CesiumJS engine: packCombineGeometryParameters,
// unpackCombineGeometryParameters, packCombineGeometryResults and
// unpackCombineGeometryResults.
import * as C from '@cesium/engine';
import PrimitivePipeline from '@cesium/engine/Source/Scene/PrimitivePipeline.js';
import { writeFileSync } from 'node:fs';

const num = (v) => (Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v);
const nums = (v) => Array.from(v, num);
const sphere = (s) => (s === undefined ? null : [s.center.x, s.center.y, s.center.z, s.radius].map(num));
const box = (d) => C.BoxGeometry.createGeometry(C.BoxGeometry.fromDimensions({ vertexFormat: C.VertexFormat.POSITION_ONLY, dimensions: new C.Cartesian3(...d) }));
const enu = (lon, lat, h = 0) => C.Transforms.eastNorthUpToFixedFrame(C.Cartesian3.fromDegrees(lon, lat, h));

// --- Parameters -----------------------------------------------------------
function parametersCase(name, options) {
  const ellipsoid = options.ellipsoid ?? C.Ellipsoid.WGS84;
  const projection = options.webMercator ? new C.WebMercatorProjection(ellipsoid) : new C.GeographicProjection(ellipsoid);
  const instances = options.instances.map((spec) => {
    const instance = new C.GeometryInstance({ geometry: box([1, 1, 1]), modelMatrix: spec.matrix ?? C.Matrix4.IDENTITY });
    if (spec.offset) instance.attributes = { offset: new C.OffsetGeometryInstanceAttribute(...spec.offset) };
    return instance;
  });
  const groups = options.groups.map((count) => Array.from({ length: count }, (_, i) => (options.missing?.includes(i) ? undefined : box([i + 1, 2, 3]))));
  const createGeometryResults = groups.map((group) => PrimitivePipeline.packCreateGeometryResults(group, []));
  const parameters = {
    createGeometryResults,
    instances,
    ellipsoid,
    projection,
    elementIndexUintSupported: options.elementIndexUintSupported ?? true,
    scene3DOnly: options.scene3DOnly ?? false,
    vertexCacheOptimize: options.vertexCacheOptimize ?? false,
    compressVertices: options.compressVertices ?? true,
    modelMatrix: options.modelMatrix ?? C.Matrix4.IDENTITY,
    createPickOffsets: options.createPickOffsets ?? false,
  };
  const packed = PrimitivePipeline.packCombineGeometryParameters(parameters, []);
  let unpacked;
  try {
    const u = PrimitivePipeline.unpackCombineGeometryParameters(packed);
    unpacked = {
      instances: u.instances.map((i) => ({
        modelMatrix: nums(C.Matrix4.pack(i.modelMatrix, [])),
        offset: i.attributes?.offset ? nums(i.attributes.offset.value) : null,
        geometry: i.geometry === undefined ? null : sphere(i.geometry.boundingSphere),
      })),
      ellipsoid: [u.ellipsoid.radii.x, u.ellipsoid.radii.y, u.ellipsoid.radii.z].map(num),
      isGeographic: u.projection instanceof C.GeographicProjection,
      elementIndexUintSupported: u.elementIndexUintSupported,
      scene3DOnly: u.scene3DOnly,
      vertexCacheOptimize: u.vertexCacheOptimize,
      compressVertices: u.compressVertices,
      modelMatrix: nums(C.Matrix4.pack(u.modelMatrix, [])),
      createPickOffsets: u.createPickOffsets,
    };
  } catch (error) {
    unpacked = { throws: error.name ?? 'Error' };
  }
  return {
    name,
    input: {
      instances: options.instances.map((spec) => ({ modelMatrix: nums(C.Matrix4.pack(spec.matrix ?? C.Matrix4.IDENTITY, [])), offset: spec.offset ?? null })),
      createGeometryResults: createGeometryResults.map((r) => ({ stringTable: r.stringTable, packedData: nums(r.packedData) })),
      ellipsoid: [ellipsoid.radii.x, ellipsoid.radii.y, ellipsoid.radii.z].map(num),
      isGeographic: !options.webMercator,
      elementIndexUintSupported: parameters.elementIndexUintSupported,
      scene3DOnly: parameters.scene3DOnly,
      vertexCacheOptimize: parameters.vertexCacheOptimize,
      compressVertices: parameters.compressVertices,
      modelMatrix: nums(C.Matrix4.pack(parameters.modelMatrix, [])),
      createPickOffsets: parameters.createPickOffsets,
    },
    packed: {
      packedInstances: nums(packed.packedInstances),
      isGeographic: packed.isGeographic,
    },
    unpacked,
  };
}

const parameters = [
  parametersCase('empty', { instances: [], groups: [] }),
  parametersCase('identity', { instances: [{}], groups: [1] }),
  parametersCase('matrices-and-offsets', {
    instances: [{ matrix: enu(10, 20, 100), offset: [1, 2, 3] }, { matrix: enu(-30, 5, 50) }, { matrix: C.Matrix4.fromScale(new C.Cartesian3(2, 3, 4)), offset: [0.1, -0.2, 1e-8] }],
    groups: [2, 1],
    webMercator: true,
    scene3DOnly: true,
    vertexCacheOptimize: true,
    compressVertices: false,
    elementIndexUintSupported: false,
    createPickOffsets: true,
    modelMatrix: C.Matrix4.fromTranslation(new C.Cartesian3(5, -6, 7)),
    ellipsoid: new C.Ellipsoid(1000, 1000, 500),
  }),
  parametersCase('missing-geometry', { instances: [{}, {}, {}], groups: [3], missing: [1] }),
  parametersCase('fewer-geometries', { instances: [{}, {}, {}], groups: [1, 1] }),
  parametersCase('more-geometries', { instances: [{}], groups: [2] }),
];

// --- Results --------------------------------------------------------------
function resultsCase(name, spheres, spheresCV) {
  const s = (v) => (v === null ? undefined : new C.BoundingSphere(new C.Cartesian3(v[0], v[1], v[2]), v[3]));
  const results = {
    geometries: [box([1, 1, 1])],
    attributeLocations: { position: 0 },
    modelMatrix: C.Matrix4.IDENTITY,
    pickOffsets: undefined,
    offsetInstanceExtend: undefined,
    boundingSpheres: spheres.map(s),
    boundingSpheresCV: spheresCV.map(s),
  };
  const packed = PrimitivePipeline.packCombineGeometryResults(results, []);
  const unpacked = PrimitivePipeline.unpackCombineGeometryResults(packed);
  const slots = (array) => Array.from({ length: array.length }, (_, i) => sphere(array[i]));
  return {
    name,
    input: { boundingSpheres: spheres, boundingSpheresCV: spheresCV },
    packed: { boundingSpheres: nums(packed.boundingSpheres), boundingSpheresCV: nums(packed.boundingSpheresCV) },
    unpacked: { boundingSpheres: slots(unpacked.boundingSpheres), boundingSpheresCV: slots(unpacked.boundingSpheresCV) },
  };
}

const results = [
  resultsCase('empty', [], []),
  resultsCase('single', [[1, 2, 3, 4]], [[0, 0, 0, 1]]),
  resultsCase('f32-rounding', [[6378137.123456, -0.1, 1e-40, 0.3], null, [16777217, 3.4e38, -2.5, 1e-7]], [null, [1 / 3, 2 / 3, 1, 0], null]),
];

writeFileSync('../../tests/data/scene-primitive-pipeline.json', JSON.stringify({ parameters, results }, null, 2) + '\n');
