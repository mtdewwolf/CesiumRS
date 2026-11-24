// Direct reference captures from pinned @cesium/engine 23.0.0 for the vector
// 3D Tiles and Gaussian splat Scene modules:
// - createBillboardPointCallback (canvas drawing command trace)
// - Vector3DTilePrimitive (constructor metrics and updateCommands splits)
// - Vector3DTileContent (vctr parsing, batch-id defaults, metrics, errors)
// - GaussianSplat3DTileContent (half floats, SH degree detection and packing)
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const C = await import('@cesium/engine');
const { default: TaskProcessor } = await import('@cesium/engine/Source/Core/TaskProcessor.js');
const { default: createBillboardPointCallback } = await import('@cesium/engine/Source/Scene/createBillboardPointCallback.js');
const { default: Vector3DTilePrimitive } = await import('@cesium/engine/Source/Scene/Vector3DTilePrimitive.js');
const { default: Vector3DTileContent } = await import('@cesium/engine/Source/Scene/Vector3DTileContent.js');
const { invoke, loadWorker } = await import('./fixtures/workers-terrain.mjs');

const num = (v) => (Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v);
const arr = (v) => Array.from(v, num);
const attempt = (fn) => {
  try {
    return { value: fn() };
  } catch (e) {
    return { error: String(e.message ?? e) };
  }
};

// --- createBillboardPointCallback -------------------------------------------
function canvasTrace(centerAlpha, cssColor, cssOutlineColor, cssOutlineWidth, pixelSize) {
  const ops = [];
  const context = {
    clearRect: (...args) => ops.push({ op: 'clearRect', args }),
    beginPath: () => ops.push({ op: 'beginPath' }),
    closePath: () => ops.push({ op: 'closePath' }),
    arc: (...args) => ops.push({ op: 'arc', args }),
    fill: () => ops.push({ op: 'fill' }),
    save: () => ops.push({ op: 'save' }),
    restore: () => ops.push({ op: 'restore' }),
  };
  for (const key of ['fillStyle', 'globalCompositeOperation']) {
    Object.defineProperty(context, key, {
      set: (value) => ops.push({ op: 'set', key, value }),
      get: () => undefined,
    });
  }
  // HTMLCanvasElement width/height are WebIDL `unsigned long`: truncate then
  // take the result modulo 2^32.
  const canvas = {
    _width: 0,
    _height: 0,
    get width() {
      return this._width;
    },
    set width(value) {
      this._width = Number(value) >>> 0;
    },
    get height() {
      return this._height;
    },
    set height(value) {
      this._height = Number(value) >>> 0;
    },
    getContext: () => context,
  };
  const previous = globalThis.document;
  globalThis.document = { createElement: () => canvas };
  try {
    const callback = createBillboardPointCallback(centerAlpha, cssColor, cssOutlineColor, cssOutlineWidth, pixelSize);
    callback();
  } finally {
    if (previous === undefined) delete globalThis.document;
    else globalThis.document = previous;
  }
  return { args: [centerAlpha, cssColor, cssOutlineColor, cssOutlineWidth, pixelSize], width: canvas.width, height: canvas.height, ops };
}

const pointCallback = [
  canvasTrace(1.0, '#ff0000', '#0000ff', 2, 8),
  canvasTrace(0.5, 'rgba(255, 0, 0, 0.5)', '#0000ff', 2, 8),
  canvasTrace(1, 'red', 'black', 0, 5),
  canvasTrace(0.75, '#123456', '#abcdef', 1.5, 6.9),
].map((trace) => ({
  ...trace,
  ops: trace.ops.map((entry) => ({
    ...entry,
    args: entry.args ? entry.args.map(num) : undefined,
  })),
}));

// --- Vector3DTilePrimitive ---------------------------------------------------
function bounds(batch) {
  return batch.map((b) => ({ offset: b.offset, count: b.count, rgba: num(b.color.toRgba()), batchIds: arr(b.batchIds) }));
}

function primitiveCase(name, { indices, indexCounts, indexOffsets, batchedIndices, batchIds, operations }) {
  const positions = new Float32Array((batchIds.length || 1) * 3);
  const vertexBatchIds = new Uint16Array(batchIds.length || 1);
  const primitive = new Vector3DTilePrimitive({
    positions,
    vertexBatchIds,
    indices: new Uint16Array(indices),
    indexCounts: new Uint32Array(indexCounts),
    indexOffsets: new Uint32Array(indexOffsets),
    batchedIndices: batchedIndices.map((b) => new C.Vector3DTileBatch({ offset: b.offset, count: b.count, color: new C.Color(...b.color), batchIds: b.batchIds })),
    batchIds: new Uint16Array(batchIds),
    boundingVolume: new C.BoundingSphere(C.Cartesian3.ZERO, 1),
    boundingVolumes: undefined,
    center: C.Cartesian3.ZERO,
    batchTable: {},
  });
  const initial = bounds(primitive._batchedIndices);
  const steps = [];
  for (const [batchId, color] of operations) {
    primitive.updateCommands(batchId, new C.Color(...color));
    steps.push({ batchId, color, batches: bounds(primitive._batchedIndices) });
  }
  return {
    name,
    trianglesLength: num(primitive.trianglesLength),
    geometryByteLength: primitive.geometryByteLength,
    indexCounts,
    indexOffsets,
    batchIds,
    initial,
    steps,
  };
}

const vector3DTilePrimitive = [
  primitiveCase('split-middle', {
    indices: Array.from({ length: 12 }, (_, i) => i % 3),
    indexCounts: [3, 9],
    indexOffsets: [0, 3],
    batchedIndices: [{ offset: 0, count: 12, color: [1, 1, 1, 1], batchIds: [0, 1] }],
    batchIds: [0, 1],
    operations: [[1, [1, 0, 0, 1]]],
  }),
  primitiveCase('split-start', {
    indices: Array.from({ length: 12 }, (_, i) => i % 3),
    indexCounts: [4, 4, 4],
    indexOffsets: [0, 4, 8],
    batchedIndices: [{ offset: 0, count: 12, color: [1, 1, 1, 1], batchIds: [0, 1, 2] }],
    batchIds: [0, 1, 2],
    operations: [[0, [0, 1, 0, 1]]],
  }),
  primitiveCase('split-end', {
    indices: Array.from({ length: 12 }, (_, i) => i % 3),
    indexCounts: [4, 4, 4],
    indexOffsets: [0, 4, 8],
    batchedIndices: [{ offset: 0, count: 12, color: [1, 1, 1, 1], batchIds: [0, 1, 2] }],
    batchIds: [0, 1, 2],
    operations: [[2, [0, 0, 1, 1]]],
  }),
  primitiveCase('unknown-batch-id', {
    indices: [0, 1, 2],
    indexCounts: [3],
    indexOffsets: [0],
    batchedIndices: [{ offset: 0, count: 3, color: [1, 1, 1, 1], batchIds: [0] }],
    batchIds: [0],
    operations: [[7, [1, 0, 0, 1]]],
  }),
  primitiveCase('fractional-metrics', {
    indices: [0, 1, 2, 0],
    indexCounts: [4],
    indexOffsets: [0],
    batchedIndices: [{ offset: 0, count: 4, color: [1, 1, 1, 1], batchIds: [0] }],
    batchIds: [0],
    operations: [],
  }),
];

// --- Vector3DTileContent -----------------------------------------------------
const workers = {};
for (const name of [
  'createVectorTilePoints',
  'createVectorTilePolylines',
  'createVectorTileClampedPolylines',
  'createVectorTilePolygons',
  'createVectorTileGeometries',
]) {
  workers[name] = await loadWorker(name);
}
const workerCalls = [];
const originalSchedule = TaskProcessor.prototype.scheduleTask;
TaskProcessor.prototype.scheduleTask = function (parameters, transferableObjects) {
  const name = String(this._workerPath).split('/').pop().replace('.js', '');
  const worker = workers[name];
  if (!worker) throw new Error(`unexpected worker ${this._workerPath}`);
  // Capture the parameters as the module passed them (typed arrays become arrays).
  const capture = {};
  for (const [key, value] of Object.entries(parameters)) {
    if (ArrayBuffer.isView(value)) capture[key] = { typed: value.constructor.name, values: arr(value) };
    else if (value instanceof ArrayBuffer) {
      const decode = (Ctor) => {
        try {
          return arr(new Ctor(value));
        } catch {
          return null;
        }
      };
      const values = key === 'packedBuffer'
        ? decode(Float64Array)
        : ['positions', 'widths'].includes(key)
          ? decode(Uint16Array)
          : key === 'batchIds'
            ? decode(name === 'createVectorTilePolygons' ? Uint32Array : Uint16Array)
            : ['counts', 'indexCounts', 'batchTableColors'].includes(key)
              ? decode(Uint32Array)
              : null;
      capture[key] = { arrayBufferByteLength: value.byteLength, values };
    } else capture[key] = value;
  }
  workerCalls.push({ worker: name, parameters: capture, transferCount: transferableObjects?.length ?? 0 });
  const flushed = {};
  for (const [key, value] of Object.entries(parameters)) {
    if (ArrayBuffer.isView(value)) flushed[key] = value.buffer;
    else if (value && typeof value === 'object' && value.byteLength !== undefined) flushed[key] = value;
    else flushed[key] = value;
  }
  return invoke(worker, flushed).then(({ result }) => result);
};

function zigZag(values) {
  const out = new Uint16Array(values.length);
  let previous = 0;
  for (let i = 0; i < values.length; i++) {
    const d = values[i] - previous;
    previous = values[i];
    out[i] = ((d << 1) ^ (d >> 31)) & 0xffff;
  }
  return out;
}
function encode(us, vs, hs) {
  const planes = [zigZag(us), zigZag(vs)];
  if (hs) planes.push(zigZag(hs));
  const out = new Uint16Array(planes.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of planes) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

function buildVectorTile({ version = 1, byteLengthOverride, emptyFeatureJson = false, featureTable, featureBinary = new Uint8Array(0), batchTable, batchTableBinary = new Uint8Array(0), polygonIndices = [], polygonPositions = [], polylinePositions = [], pointsPositions = [] }) {
  const encoder = new TextEncoder();
  let featureJson = emptyFeatureJson ? new Uint8Array(0) : encoder.encode(JSON.stringify(featureTable));
  while (featureJson.length % 8 !== 0) featureJson = new Uint8Array([...featureJson, 0x20]);
  const batchJson = batchTable === undefined ? new Uint8Array(0) : encoder.encode(JSON.stringify(batchTable));
  const headerLength = 4 + 4 + 4 + 8 * 4;
  let offset = headerLength + featureJson.length + featureBinary.length + batchJson.length + batchTableBinary.length;
  offset += (4 - (offset % 4)) % 4;
  const total = offset + polygonIndices.length * 4 + polygonPositions.length * 2 + polylinePositions.length * 2 + pointsPositions.length * 2;
  const bytes = new Uint8Array(total);
  const view = new DataView(bytes.buffer);
  bytes.set(encoder.encode('vctr'), 0);
  view.setUint32(4, version, true);
  view.setUint32(8, byteLengthOverride ?? total, true);
  view.setUint32(12, featureJson.length, true);
  view.setUint32(16, featureBinary.length, true);
  view.setUint32(20, batchJson.length, true);
  view.setUint32(24, batchTableBinary.length, true);
  view.setUint32(28, polygonIndices.length * 4, true);
  view.setUint32(32, polygonPositions.length * 2, true);
  view.setUint32(36, polylinePositions.length * 2, true);
  view.setUint32(40, pointsPositions.length * 2, true);
  let cursor = headerLength;
  bytes.set(featureJson, cursor);
  cursor += featureJson.length;
  bytes.set(featureBinary, cursor);
  cursor += featureBinary.length;
  bytes.set(batchJson, cursor);
  cursor += batchJson.length;
  bytes.set(batchTableBinary, cursor);
  cursor += batchTableBinary.length;
  cursor += (4 - (cursor % 4)) % 4;
  new Uint32Array(bytes.buffer, cursor, polygonIndices.length).set(polygonIndices);
  cursor += polygonIndices.length * 4;
  new Uint16Array(bytes.buffer, cursor, polygonPositions.length).set(polygonPositions);
  cursor += polygonPositions.length * 2;
  new Uint16Array(bytes.buffer, cursor, polylinePositions.length).set(polylinePositions);
  cursor += polylinePositions.length * 2;
  new Uint16Array(bytes.buffer, cursor, pointsPositions.length).set(pointsPositions);
  return bytes;
}

function contentCase(name, mutate, tileOptions = {}) {
  workerCalls.length = 0;
  const region = [-1.0, -1.0, 1.0, 1.0, 10, 100];
  const featureTable = {
    POLYGONS_LENGTH: 2,
    POLYGON_COUNTS: [4, 3],
    POLYGON_INDEX_COUNTS: [6, 3],
    POLYLINES_LENGTH: 2,
    POLYLINE_COUNTS: [3, 2],
    POINTS_LENGTH: 2,
    REGION: region,
  };
  const polygonIndices = [0, 1, 2, 0, 2, 1, 0, 1, 2];
  const polygonPositions = [100, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1100, 1200, 1300, 1400];
  const polylinePositions = encode(
    [1000, 1200, 1400, 1600, 1800],
    [2000, 2200, 2400, 2600, 2800],
    [3000, 3200, 3400, 3600, 3800],
  );
  const pointsPositions = encode([500, 700], [900, 1100], [1300, 1500]);
  const source = buildVectorTile({
    featureTable,
    batchTable: { name: ['a', 'b', 'c', 'd', 'e', 'f'] },
    polygonIndices,
    polygonPositions,
    polylinePositions,
    pointsPositions,
  });
  const tile = {
    computedTransform: C.Matrix4.IDENTITY,
    boundingVolume: { boundingVolume: new C.BoundingSphere(C.Cartesian3.ZERO, 1000) },
    content: {},
    tileset: undefined,
  };
  const tileset = {
    classificationType: tileOptions.classificationType,
    heightReference: C.HeightReference.NONE,
    vectorKeepDecodedPositions: false,
    examineVectorLinesFunction: undefined,
    scene: undefined,
  };
  tile.tileset = tileset;
  const resource = { getUrlComponent: () => 'https://example.test/tile.vctr' };
  let content;
  const outcome = attempt(() => {
    content = new Vector3DTileContent(tileset, tile, resource, source.buffer, 0);
    return true;
  });
  if (outcome.error) return { name, error: outcome.error, workerCalls: workerCalls.slice() };
  const fakeFrame = { passes: { render: false, pick: false } };
  content._polygons?.update(fakeFrame);
  content._polylines?.update(fakeFrame);
  const polygons = content._polygons;
  const polylines = content._polylines;
  const points = content._points;
  const shape = (collection) =>
    collection && {
      batchIds: arr(collection._batchIds),
      counts: collection._counts ? arr(collection._counts) : undefined,
      positionsLength: collection._positions ? collection._positions.length : undefined,
      widths: collection._widths ? arr(collection._widths) : undefined,
      minHeight: num(collection._minimumHeight),
      maxHeight: num(collection._maximumHeight),
      center: collection._center ? arr([collection._center.x, collection._center.y, collection._center.z]) : undefined,
      classificationType: collection._classificationType ?? collection.classificationType,
    };
  return {
    name,
    featuresLength: content.featuresLength,
    pointsLength: content.pointsLength,
    trianglesLength: num(content.trianglesLength),
    geometryByteLength: content.geometryByteLength,
    texturesByteLength: content.texturesByteLength,
    batchTableByteLength: content.batchTableByteLength,
    ready: content.ready,
    url: content.url,
    featurePropertiesDirty: content.featurePropertiesDirty,
    innerContents: content.innerContents ?? null,
    metadata: content.metadata ?? null,
    group: content.group ?? null,
    hasProperty: content.hasProperty(0, 'name'),
    getFeatureInvalid: attempt(() => content.getFeature(-1)).error ?? null,
    polygons: shape(polygons),
    polylines: shape(polylines),
    points: shape(points),
    workerCalls: workerCalls.map((call) => ({
      worker: call.worker,
      transferCount: call.transferCount,
      packedBuffer: call.parameters.packedBuffer?.values ?? null,
      positions: call.parameters.positions?.values ?? null,
      counts: call.parameters.counts?.values ?? null,
      widths: call.parameters.widths?.values ?? null,
      batchIds: call.parameters.batchIds?.values ?? null,
      indexCounts: call.parameters.indexCounts?.values ?? null,
      batchTableColors: call.parameters.batchTableColors?.values ?? null,
    })),
  };
}

const vector3DTileContent = [
  contentCase('floating', {}),
  contentCase('clamped', {}, { classificationType: C.ClassificationType.TERRAIN }),
];

// Error cases with minimal payloads.
function contentError(name, options) {
  workerCalls.length = 0;
  const source = buildVectorTile(options);
  const tile = { computedTransform: C.Matrix4.IDENTITY, boundingVolume: { boundingVolume: new C.BoundingSphere(C.Cartesian3.ZERO, 1) } };
  const tileset = {};
  tile.tileset = tileset;
  const resource = { getUrlComponent: () => 'https://example.test/tile.vctr' };
  const outcome = attempt(() => new Vector3DTileContent(tileset, tile, resource, source.buffer, 0));
  return { name, error: outcome.error ?? null, ready: outcome.value?.ready ?? null, featuresLength: outcome.value?.featuresLength ?? null };
}

const vector3DTileContentErrors = [
  contentError('bad-version', { version: 2, featureTable: { POLYGONS_LENGTH: 0 } }),
  contentError('zero-byte-length', { byteLengthOverride: 0, featureTable: { POLYGONS_LENGTH: 0 } }),
  contentError('empty-feature-json', { emptyFeatureJson: true, featureTable: {} }),
  contentError('missing-region', { featureTable: { POLYGONS_LENGTH: 1, POLYGON_COUNTS: [3], POLYGON_INDEX_COUNTS: [3] }, polygonIndices: [0, 1, 2], polygonPositions: [0, 1, 2, 3, 4, 5] }),
  contentError('missing-polygon-counts', { featureTable: { POLYGONS_LENGTH: 1, POLYGON_INDEX_COUNTS: [3], REGION: [-1, -1, 1, 1, 0, 1] }, polygonIndices: [0, 1, 2], polygonPositions: [] }),
  contentError('missing-index-counts', { featureTable: { POLYGONS_LENGTH: 1, POLYGON_COUNTS: [3], REGION: [-1, -1, 1, 1, 0, 1] }, polygonIndices: [0, 1, 2], polygonPositions: [0, 1, 2, 3, 4, 5] }),
  contentError('missing-polyline-counts', { featureTable: { POLYLINES_LENGTH: 1, REGION: [-1, -1, 1, 1, 0, 1] }, polylinePositions: encode([1, 2], [3, 4], [5, 6]) }),
];

TaskProcessor.prototype.scheduleTask = originalSchedule;

// --- GaussianSplat3DTileContent internals ------------------------------------
const sourcePath = new URL('./node_modules/@cesium/engine/Source/Scene/GaussianSplat3DTileContent.js', import.meta.url);
const { readFile, writeFile, unlink } = await import('node:fs/promises');
const modifiedPath = new URL('./node_modules/@cesium/engine/Source/Scene/__GaussianSplat3DTileContent.exposed.mjs', import.meta.url);
const modified = `${await readFile(sourcePath, 'utf8')}\nexport { float32ToFloat16, degreeAndCoefFromAttributes, extractSHDegreeAndCoef, packSphericalHarmonicsData, getShAttributePrefix };\n`;
await writeFile(modifiedPath, modified);
let gaussian;
try {
  gaussian = await import(modifiedPath.href);
} finally {
  await unlink(modifiedPath);
}

const floatValues = [
  0, -0, 1, -1, 0.5, -0.5, 2, 3.140625, 65504, 65520, 65536, 1e-7, 6e-8, 5.96e-8, 1e-8, 0.333333, 1.5e-5,
  Infinity, -Infinity, NaN,
];
const halfFloats = floatValues.map((value) => ({ value: num(value), bits: gaussian.float32ToFloat16(value) }));

const shName = (prefix, l, n) => `${prefix}SH_DEGREE_${l}_COEF_${n}`;
const shAttributes = (count, prefix = '_') => Array.from({ length: count }, (_, i) => ({ name: shName(prefix, 1, i), typedArray: new Float32Array([i + 0.25, i + 0.5, i + 0.75, 0, 0, 0]) }));
const degreeCases = [0, 1, 2, 3, 8, 15, 16].map((count) => {
  const { l, n } = gaussian.degreeAndCoefFromAttributes(shAttributes(count));
  return { count, l, n };
});

function packingCase(degree, points, seed) {
  const coefs = { 1: 9, 2: 24, 3: 45 }[degree];
  const attributes = [];
  for (let n = 0; n < coefs; n++) {
    const values = new Float32Array(points * 3);
    for (let i = 0; i < values.length; i++) values[i] = ((seed + i * 13 + n * 7) % 211) / 17 - 6;
    attributes.push({ name: shName('_', degree, n), typedArray: values });
  }
  const content = {
    sphericalHarmonicsDegree: degree,
    sphericalHarmonicsCoefficientCount: coefs,
    pointsLength: points,
    gltfPrimitive: { attributes },
  };
  return { degree, points, packed: arr(gaussian.packSphericalHarmonicsData(content)) };
}

const gaussianSplat3DTileContent = {
  halfFloats,
  degreeCases,
  degreeNames: [['_SH_DEGREE_1_COEF_0', 0, 0], ['KHR_gaussian_splatting:SH_DEGREE_2_COEF_3', 0, 0], ['_SH_DEGREE_3', 0, 0]].map(([name]) => {
    const { l, n } = gaussian.extractSHDegreeAndCoef(name);
    return { name, prefix: gaussian.getShAttributePrefix(name), l, n };
  }),
  packing: [packingCase(1, 2, 3), packingCase(2, 2, 5), packingCase(3, 1, 11)],
};

const out = {
  cesium: '23.0.0',
  createBillboardPointCallback: pointCallback,
  vector3DTilePrimitive,
  vector3DTileContent,
  vector3DTileContentErrors,
  gaussianSplat3DTileContent,
};
writeFileSync(new URL('../../tests/data/scene-vector-gaussian.json', import.meta.url), JSON.stringify(out, null, 2) + '\n');
console.log('Wrote tests/data/scene-vector-gaussian.json');
