// decodeI3S through its real createTaskProcessorWorker handler.
// Blob URLs are replaced with the bytes they pointed at. Schema order is passed
// as arrays; a featureData override is applied by the worker and recorded as
// that same resolved order.
import * as C from '@cesium/engine';
import {invoke, loadWorker} from './workers-terrain.mjs';

const suite = module => ({
  module,
  cases: [],
  async add(f, args, fn) {
    let r;
    try { r = enc(await fn(...args)); }
    catch (e) { r = {throws: String(e.message ?? e), kind: e.name ?? 'Error'}; }
    this.cases.push({f, a: args.map(enc), r});
  },
});

function enc(v) {
  if (v === undefined || v === null) return null;
  if (typeof v === 'number') {
    if (Number.isNaN(v)) return 'NaN';
    if (v === Infinity) return 'Infinity';
    if (v === -Infinity) return '-Infinity';
    if (Object.is(v, -0)) return '-0';
    return v;
  }
  if (typeof v === 'boolean' || typeof v === 'string') return v;
  if (ArrayBuffer.isView(v)) return Array.from(v).map(enc);
  if (Array.isArray(v)) return v.map(enc);
  if (typeof v === 'object') {
    const out = {};
    for (const [k, val] of Object.entries(v)) out[k] = enc(val);
    return out;
  }
  throw new Error(`cannot encode ${typeof v}`);
}

const blobs = new Map();
let blobId = 0;
globalThis.URL.createObjectURL = blob => {
  const id = `blob:i3s/${blobId++}`;
  blobs.set(id, blob);
  return id;
};

function concat(chunks) {
  const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.length;
  }
  return out;
}

function bytesOf(view) {
  return new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
}

function header(vertices, features) {
  const view = new DataView(new ArrayBuffer(8));
  view.setUint32(0, vertices, true);
  view.setUint32(4, features, true);
  return new Uint8Array(view.buffer);
}

/** @param {{name: string, data: ArrayBufferView}[]} attributes */
function binary(vertices, features, attributes) {
  return concat([header(vertices, features), ...attributes.map(attribute => bytesOf(attribute.data))]);
}

const wgs84 = C.Ellipsoid.WGS84;
const center = C.Cartographic.fromDegrees(-90, 40, 15);
const cartesian = wgs84.cartographicToCartesian(center);
const frame = {
  cartographicCenter: {longitude: center.longitude, latitude: center.latitude, height: center.height},
  cartesianCenter: {x: cartesian.x, y: cartesian.y, z: cartesian.z},
  parentRotation: [1, 0, 0, 0, 1, 0, 0, 0, 1],
  ellipsoidRadiiSquare: {x: wgs84.radiiSquared.x, y: wgs84.radiiSquared.y, z: wgs84.radiiSquared.z},
  url: 'https://i3s.test/node/0',
};

function copyBuffer(bytes) {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
}

async function owned(meshData) {
  const buffers = [];
  for (const buffer of meshData.buffers) {
    const blob = blobs.get(buffer.uri);
    const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
    buffers.push({byteLength: buffer.byteLength, bytes});
  }
  return {...meshData, buffers};
}

function trianglePositions() {
  return new Float32Array([0, 0, 0, 1, 0, 2, 0, 1, 4]);
}

async function dracoBytes() {
  const draco = await (await import('draco3d/draco_encoder_nodejs.js')).default();
  const builder = new draco.MeshBuilder();
  const mesh = new draco.Mesh();
  const meta = new draco.MetadataBuilder();
  const positions = new Float32Array([0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0]);
  const normals = new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0]);
  const uvs = new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]);
  const colors = new Uint8Array([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 200, 10, 20, 30, 255]);
  const features = new Int16Array([4, 4, 1, 9]);
  const faces = new Int32Array([0, 1, 2, 0, 2, 3]);
  const positionId = builder.AddFloatAttributeToMesh(mesh, draco.POSITION, 4, 3, positions);
  builder.AddFloatAttributeToMesh(mesh, draco.NORMAL, 4, 3, normals);
  builder.AddFloatAttributeToMesh(mesh, draco.TEX_COORD, 4, 2, uvs);
  builder.AddUInt8Attribute(mesh, draco.COLOR, 4, 4, colors);
  const featureId = builder.AddInt16Attribute(mesh, draco.GENERIC, 4, 1, features);
  if (builder.AddFacesToMesh(mesh, 2, faces) < 0) throw new Error('draco faces');
  const scales = new draco.Metadata();
  meta.AddDoubleEntry(scales, 'i3s-scale_x', 2);
  meta.AddDoubleEntry(scales, 'i3s-scale_y', 3);
  builder.SetMetadataForAttribute(mesh, positionId, scales);
  const renamed = new draco.Metadata();
  meta.AddStringEntry(renamed, 'i3s-attribute-type', 'feature-index');
  builder.SetMetadataForAttribute(mesh, featureId, renamed);
  const encoder = new draco.Encoder();
  encoder.SetEncodingMethod(draco.MESH_EDGEBREAKER_ENCODING);
  encoder.SetAttributeQuantization(draco.POSITION, 14);
  encoder.SetAttributeQuantization(draco.NORMAL, 10);
  encoder.SetAttributeQuantization(draco.TEX_COORD, 12);
  encoder.SetSpeedOptions(5, 5);
  const encoded = new draco.DracoInt8Array();
  const length = encoder.EncodeMeshToDracoBuffer(mesh, encoded);
  if (length <= 0) throw new Error(`draco encode ${length}`);
  const bytes = new Uint8Array(length);
  for (let i = 0; i < length; i++) bytes[i] = encoded.GetValue(i);
  return bytes;
}

export default async function workersI3s() {
  const worker = await loadWorker('decodeI3S');
  const s = suite('decodeI3S');
  await invoke(worker, {webAssemblyConfig: {}});

  const decode = async (bytes, options) => {
    const parameters = {
      binaryData: copyBuffer(bytes),
      schema: options.ordering
        ? {ordering: options.schemaOrdering ?? options.ordering, featureAttributeOrder: options.schemaFeatureOrder ?? options.featureAttributeOrder ?? []}
        : undefined,
      bufferInfo: options.bufferAttributes ? {attributes: options.bufferAttributes} : undefined,
      featureData: options.featureData,
      geoidDataList: options.geoidDataList,
      cartographicCenter: options.cartographicCenter,
      cartesianCenter: options.cartesianCenter,
      parentRotation: options.parentRotation,
      ellipsoidRadiiSquare: options.ellipsoidRadiiSquare,
      url: options.url,
      enableFeatures: options.enableFeatures ?? false,
      splitGeometryByColorTransparency: options.splitGeometryByColorTransparency ?? false,
      calculateNormals: options.calculateNormals ?? false,
      symbologyData: options.symbologyData,
    };
    const {result} = await invoke(worker, parameters);
    return owned(result.meshData);
  };

  await s.add('init', [{}], async () => (await invoke(worker, {webAssemblyConfig: {}})).result);

  const positions = trianglePositions();
  const normals = new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]);
  const uvs = new Float32Array([0, 0, 1, 0, 0.25, 0.5]);
  const colors = new Uint8Array([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]);
  const regions = new Uint16Array([0, 0, 65535, 65535, 0, 0, 32768, 65535, 1000, 2000, 4000, 8000]);
  const faceRange = new Uint32Array([0, 0]);
  const full = binary(3, 1, [
    {name: 'position', data: positions},
    {name: 'normal', data: normals},
    {name: 'uv0', data: uvs},
    {name: 'color', data: colors},
    {name: 'uvRegion', data: regions},
    {name: 'faceRange', data: faceRange},
  ]);
  await s.add('decode', [full, {
    ...frame,
    ordering: ['position', 'normal', 'uv0', 'color', 'uvRegion', 'faceRange'],
    featureAttributeOrder: [],
    enableFeatures: true,
    symbologyData: {default: {edges: {color: 1}}, '0': {edges: null}},
  }], decode);

  // bufferInfo order, unknown name skipped, and a non-identity parent rotation.
  const rotated = [0, 0, 1, 0, 1, 0, -1, 0, 0];
  const buffered = binary(3, 1, [
    {name: 'position', data: positions},
    {name: 'color', data: colors},
  ]);
  await s.add('decode', [buffered, {
    ...frame,
    parentRotation: rotated,
    bufferAttributes: ['position', 'not-an-attribute', 'color'],
    enableFeatures: false,
    splitGeometryByColorTransparency: false,
  }], decode);

  // featureData key order overrides the schema. The recorded ordering is the resolved one.
  const overridden = binary(3, 0, [
    {name: 'uv0', data: uvs},
    {name: 'position', data: positions},
  ]);
  await s.add('decode', [overridden, {
    ...frame,
    ordering: ['uv0', 'position'],
    featureAttributeOrder: [],
    schemaOrdering: ['position', 'uv0'],
    featureData: {geometryData: [{params: {vertexAttributes: {uv0: 1, position: 1}, featureAttributes: {}}}]},
  }], decode);

  const shifted = new Float32Array([10, -4, 1, 0, 0, 0, -6, 3, 2]);
  const geoidBinary = binary(3, 0, [{name: 'position', data: shifted}]);
  const geoid = {
    projectionType: 'Geographic',
    nativeExtent: {west: -1, south: -1, east: 1, north: 1},
    width: 2,
    height: 2,
    buffer: [1, 5, 9, 13],
    scale: 2,
    offset: -3,
  };
  await s.add('decode', [geoidBinary, {...frame, ordering: ['position'], featureAttributeOrder: [], geoidDataList: [geoid]}], decode);

  const mercator = {
    projectionType: 'WebMercator',
    projection: {_ellipsoid: {_radii: {x: wgs84.radii.x, y: wgs84.radii.y, z: wgs84.radii.z}}},
    nativeExtent: {west: -2e6, south: -2e6, east: 2e6, north: 2e6},
    width: 2,
    height: 2,
    buffer: [4, 8, 12, 16],
    scale: 0.5,
    offset: 1,
  };
  await s.add('decode', [geoidBinary, {...frame, ordering: ['position'], featureAttributeOrder: [], geoidDataList: [mercator, geoid]}], decode);

  // Outside every geoid tile: the sample is 0 and the vertex z is unchanged by the geoid.
  const outside = {...geoid, nativeExtent: {west: 2, south: 2, east: 3, north: 3}};
  await s.add('decode', [geoidBinary, {...frame, ordering: ['position'], featureAttributeOrder: [], geoidDataList: [outside]}], decode);

  const opaque = new Uint8Array([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]);
  const transparent = new Uint8Array([255, 255, 255, 10, 0, 0, 0, 255, 0, 0, 0, 255]);
  const mixedColors = concat([opaque, transparent]);
  const mixedPositions = concat([bytesOf(positions), bytesOf(new Float32Array([2, 0, 0, 3, 0, 1, 2, 2, 0]))]);
  const mixed = binary(6, 2, [
    {name: 'position', data: mixedPositions},
    {name: 'color', data: mixedColors},
    {name: 'faceRange', data: new Uint32Array([0, 0, 1, 1])},
  ]);
  await s.add('decode', [mixed, {
    ...frame,
    ordering: ['position', 'color', 'faceRange'],
    featureAttributeOrder: [],
    enableFeatures: true,
    splitGeometryByColorTransparency: true,
  }], decode);

  const allTransparent = binary(3, 0, [
    {name: 'position', data: positions},
    {name: 'color', data: new Uint8Array([1, 2, 3, 0, 4, 5, 6, 20, 7, 8, 9, 254])},
  ]);
  await s.add('decode', [allTransparent, {
    ...frame,
    ordering: ['position', 'color'],
    featureAttributeOrder: [],
    splitGeometryByColorTransparency: true,
  }], decode);

  await s.add('decode', [binary(3, 0, [{name: 'position', data: positions}]), {
    ...frame,
    ordering: ['position'],
    featureAttributeOrder: [],
    calculateNormals: true,
  }], decode);

  // Two coplanar-ish triangles that weld a shared edge, plus a sharp pair.
  const welded = new Float32Array([
    0, 0, 0, 1, 0, 0, 0, 1, 0,
    1, 0, 0, 0, 0, 0, 0, 0, 1,
  ]);
  await s.add('decode', [binary(6, 0, [{name: 'position', data: welded}]), {
    ...frame,
    ordering: ['position'],
    featureAttributeOrder: [],
    symbologyData: {default: {edges: {width: 1}}},
  }], decode);

  const smooth = new Float32Array([
    0, 0, 0, 4, 0, 0, 0, 4, 0,
    0, 0, 0, 0, 4, 0, 4, 0, 0.05,
  ]);
  await s.add('decode', [binary(6, 0, [{name: 'position', data: smooth}]), {
    ...frame,
    ordering: ['position'],
    featureAttributeOrder: [],
    symbologyData: {'1': {edges: {}}, default: {edges: {width: 1}}},
    enableFeatures: false,
  }], decode);

  // Per-feature outlines. Feature 0 has no edges of its own and does not fall through.
  const ranged = binary(6, 2, [
    {name: 'position', data: welded},
    {name: 'faceRange', data: new Uint32Array([0, 0, 1, 1])},
  ]);
  await s.add('decode', [ranged, {
    ...frame,
    ordering: ['position', 'faceRange'],
    featureAttributeOrder: [],
    enableFeatures: true,
    symbologyData: {'0': {}, default: {edges: {width: 2}}, '1': {edges: {width: 1}}},
  }], decode);

  await s.add('decode', [header(0, 0), {...frame, ordering: ['position'], featureAttributeOrder: []}], decode);
  await s.add('decode', [header(4, 1).slice(0, 6), {ordering: ['position'], featureAttributeOrder: []}], decode);
  await s.add('decode', [concat([header(3, 0), bytesOf(positions)]).slice(0, 12), {
    ...frame,
    ordering: ['position', 'normal'],
    featureAttributeOrder: [],
  }], decode);
  await s.add('decode', [new Uint8Array([1, 2, 3]), {}], decode);
  await s.add('decode', [binary(3, 0, [{name: 'position', data: positions}]), {
    ordering: ['position'],
    featureAttributeOrder: [],
    url: 'https://i3s.test/missing-frame',
  }], decode);

  const draco = await dracoBytes();
  await s.add('decode', [draco, {
    ...frame,
    enableFeatures: true,
    splitGeometryByColorTransparency: true,
    symbologyData: {default: {edges: {width: 1}}},
  }], decode);
  await s.add('decode', [draco, {...frame, enableFeatures: false, calculateNormals: true}], decode);
  await s.add('decode', [new Uint8Array([68, 82, 65, 67, 79, 1, 2, 3, 4]), {...frame}], decode);

  return [s];
}
