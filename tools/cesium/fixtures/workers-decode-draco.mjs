// decodeDraco through its real worker, with buffers from the draco3d encoder
// that worker loads. The Rust port decodes the same bytes with draco-core.
import { createRequire } from "node:module";
import { enc } from "./lib.mjs";
import { invoke, loadWorker } from "./workers-terrain.mjs";

const require = createRequire(new URL("../package.json", import.meta.url));
const draco3d = require("draco3d");

const suite = (module) => ({
  module,
  cases: [],
  async add(f, args, fn) {
    let r;
    try {
      r = enc(await fn());
    } catch (e) {
      r = { throws: String(e.message ?? e), kind: e.name ?? "Error" };
    }
    this.cases.push({ f, a: args.map(enc), r });
  },
});

function bytesOf(out, len) {
  const raw = new Uint8Array(len);
  for (let i = 0; i < len; i++) raw[i] = out.GetValue(i);
  return raw;
}

function encodeMesh(encoderModule) {
  const builder = new encoderModule.MeshBuilder();
  const mesh = new encoderModule.Mesh();
  const positions = new Float32Array([0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
  const normals = new Float32Array([0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0]);
  const uvs = new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]);
  const colors = new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0]);
  const generic = new Float32Array([0.25, 0.5, 0.75, 1]);
  const faces = new Uint32Array([0, 1, 2, 1, 3, 2]);
  const ids = {
    POSITION: builder.AddFloatAttributeToMesh(mesh, encoderModule.POSITION, 4, 3, positions),
    NORMAL: builder.AddFloatAttributeToMesh(mesh, encoderModule.NORMAL, 4, 3, normals),
    TEX_COORD: builder.AddFloatAttributeToMesh(mesh, encoderModule.TEX_COORD, 4, 2, uvs),
    COLOR: builder.AddFloatAttributeToMesh(mesh, encoderModule.COLOR, 4, 3, colors),
    GENERIC: builder.AddFloatAttributeToMesh(mesh, encoderModule.GENERIC, 4, 1, generic),
  };
  builder.AddFacesToMesh(mesh, 2, faces);
  const encoder = new encoderModule.Encoder();
  encoder.SetAttributeQuantization(encoderModule.POSITION, 14);
  encoder.SetAttributeQuantization(encoderModule.NORMAL, 10);
  encoder.SetAttributeQuantization(encoderModule.TEX_COORD, 12);
  encoder.SetAttributeQuantization(encoderModule.COLOR, 8);
  encoder.SetAttributeQuantization(encoderModule.GENERIC, 8);
  encoder.SetSpeedOptions(5, 5);
  const out = new encoderModule.DracoInt8Array();
  const len = encoder.EncodeMeshToDracoBuffer(mesh, out);
  if (!len) throw new Error("mesh encode failed");
  const raw = bytesOf(out, len);
  encoderModule.destroy(out);
  encoderModule.destroy(encoder);
  encoderModule.destroy(mesh);
  encoderModule.destroy(builder);
  return { raw, ids };
}

function encodeCloud(encoderModule) {
  const builder = new encoderModule.PointCloudBuilder();
  const cloud = new encoderModule.PointCloud();
  const positions = new Float32Array([1, 2, 3, 7, 8, 9, 4, 5, 6]);
  const normals = new Float32Array([0, 0, 1, 0, 1, 0, 1, 0, 0]);
  const generic = new Float32Array([9, 8, 7]);
  const ids = {
    POSITION: builder.AddFloatAttribute(cloud, encoderModule.POSITION, 3, 3, positions),
    NORMAL: builder.AddFloatAttribute(cloud, encoderModule.NORMAL, 3, 3, normals),
    GENERIC: builder.AddFloatAttribute(cloud, encoderModule.GENERIC, 3, 1, generic),
  };
  const encoder = new encoderModule.Encoder();
  encoder.SetAttributeQuantization(encoderModule.POSITION, 11);
  encoder.SetAttributeQuantization(encoderModule.NORMAL, 10);
  encoder.SetAttributeQuantization(encoderModule.GENERIC, 8);
  encoder.SetSpeedOptions(5, 5);
  const out = new encoderModule.DracoInt8Array();
  const len = encoder.EncodePointCloudToDracoBuffer(cloud, false, out);
  if (!len) throw new Error("point cloud encode failed");
  const raw = bytesOf(out, len);
  encoderModule.destroy(out);
  encoderModule.destroy(encoder);
  encoderModule.destroy(cloud);
  encoderModule.destroy(builder);
  return { raw, ids };
}

// A quantized POSITION is what lets draco3d encode a point cloud that also
// carries an integer attribute. The integer values are added with the typed
// Add*Attribute calls. i32::MIN and i32::MAX make that encoder's own decoder
// fail, so those bounds are a separate sequential buffer below.
function encodeTyped(encoderModule, add) {
  const count = 4;
  const builder = new encoderModule.PointCloudBuilder();
  const cloud = new encoderModule.PointCloud();
  builder.AddFloatAttribute(
    cloud,
    encoderModule.POSITION,
    count,
    3,
    new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1]),
  );
  const id = add(builder, cloud, encoderModule, count);
  const encoder = new encoderModule.Encoder();
  encoder.SetSpeedOptions(5, 5);
  encoder.SetAttributeQuantization(encoderModule.POSITION, 14);
  const out = new encoderModule.DracoInt8Array();
  const len = encoder.EncodePointCloudToDracoBuffer(cloud, false, out);
  if (!len) throw new Error("typed attribute encode failed");
  const raw = bytesOf(out, len);
  encoderModule.destroy(out);
  encoderModule.destroy(encoder);
  encoderModule.destroy(cloud);
  encoderModule.destroy(builder);
  return { raw, id };
}

// draco3d's encoder has no AddBoolAttribute, AddInt64Attribute, or
// AddUInt64Attribute. These sequential point clouds come from draco-core
// 2.3.2 (encoding method 0) and are checked against the same worker.
const boolCloud = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 4, 0, 0, 0, 1, 1, 4, 11, 1, 0, 0, 0, 0, 1, 2, 255,
]);
const int64Cloud = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 8, 0, 0, 0, 1, 1, 4, 7, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
  0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 0, 255, 255, 255, 127,
  255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 127, 0, 0, 0, 0, 0, 0, 0, 128,
]);
const uint64Cloud = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 6, 0, 0, 0, 1, 1, 4, 8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
  0, 0, 42, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 255, 255, 255, 255, 255, 255,
  255, 255,
]);
// In-range values after the first out-of-range value. 2^31 then 5, and 5 then
// 2^31 then 7. The uint64 pair is 2^32 then 5, and 5 then 2^32 then 7.
const int64After = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 2, 0, 0, 0, 1, 1, 4, 7, 1, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0,
  0, 0,
]);
const int64Mid = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 3, 0, 0, 0, 1, 1, 4, 7, 1, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 0,
  0, 0, 7, 0, 0, 0, 0, 0, 0, 0,
]);
const uint64After = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 2, 0, 0, 0, 1, 1, 4, 8, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0, 0, 0,
  0, 0,
]);
const uint64Mid = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 3, 0, 0, 0, 1, 1, 4, 8, 1, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0,
  0, 0, 7, 0, 0, 0, 0, 0, 0, 0,
]);
// i32::MIN and i32::MAX. draco3d's AddInt32Attribute cannot produce a buffer
// its decoder accepts for these bounds. These sequential clouds are draco-core
// 2.3.2, encoding method 0.
const int32Bounds = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 2, 0, 0, 0, 1, 1, 4, 5, 1, 0, 0, 1, 254, 1, 0, 33, 127, 1, 64, 1, 0, 255, 255,
  255, 255, 254, 255, 255, 255,
]);
const int32Span = Uint8Array.from([
  68, 82, 65, 67, 79, 2, 3, 0, 0, 0, 0, 5, 0, 0, 0, 1, 1, 4, 5, 1, 0, 0, 1, 254, 1, 0, 33, 3, 153, 25, 205, 12, 115,
  157, 25, 3, 168, 251, 176, 255, 255, 255, 255, 233, 255, 255, 255, 15,
]);


// A cube with four vertices per face. Shared corner positions with different
// normals, UVs and colors give the edgebreaker encoder attribute seams, so the
// decoded attributes map points through explicit indices.
function encodeCube(encoderModule, { speed = 5, positionBits = 14, colorBits = 8 } = {}) {
  const builder = new encoderModule.MeshBuilder();
  const mesh = new encoderModule.Mesh();
  const faces = [
    [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, -1]],
    [[0, 0, 1], [0, 1, 1], [1, 1, 1], [1, 0, 1], [0, 0, 1]],
    [[0, 0, 0], [0, 1, 0], [0, 1, 1], [0, 0, 1], [-1, 0, 0]],
    [[1, 0, 0], [1, 0, 1], [1, 1, 1], [1, 1, 0], [1, 0, 0]],
    [[0, 0, 0], [0, 0, 1], [1, 0, 1], [1, 0, 0], [0, -1, 0]],
    [[0, 1, 0], [1, 1, 0], [1, 1, 1], [0, 1, 1], [0, 1, 0]],
  ];
  const positions = [], normals = [], uvs = [], colors = [], intColors = [], indices = [];
  faces.forEach(([a, b, c, d, n], f) => {
    const base = f * 4;
    for (const [k, p] of [a, b, c, d].entries()) {
      positions.push(p[0] * 3 - 1, p[1] * 2 + 0.5, p[2] * 5);
      normals.push(...n);
      uvs.push((k & 1) * 0.5 + f * 0.1, (k >> 1) * 0.5);
      colors.push(f / 6, k / 4, 1 - f / 6);
      intColors.push(f * 40, k * 60, 255 - f * 40, 255);
    }
    indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
  });
  const count = positions.length / 3;
  const ids = {
    POSITION: builder.AddFloatAttributeToMesh(mesh, encoderModule.POSITION, count, 3, new Float32Array(positions)),
    NORMAL: builder.AddFloatAttributeToMesh(mesh, encoderModule.NORMAL, count, 3, new Float32Array(normals)),
    TEX_COORD: builder.AddFloatAttributeToMesh(mesh, encoderModule.TEX_COORD, count, 2, new Float32Array(uvs)),
    COLOR: builder.AddFloatAttributeToMesh(mesh, encoderModule.COLOR, count, 3, new Float32Array(colors)),
    COLOR_INT: builder.AddUInt8Attribute(mesh, encoderModule.COLOR, count, 4, new Uint8Array(intColors)),
  };
  builder.AddFacesToMesh(mesh, indices.length / 3, new Uint32Array(indices));
  const encoder = new encoderModule.Encoder();
  encoder.SetAttributeQuantization(encoderModule.POSITION, positionBits);
  encoder.SetAttributeQuantization(encoderModule.NORMAL, 8);
  encoder.SetAttributeQuantization(encoderModule.TEX_COORD, 10);
  encoder.SetAttributeQuantization(encoderModule.COLOR, colorBits);
  encoder.SetSpeedOptions(speed, speed);
  const out = new encoderModule.DracoInt8Array();
  const len = encoder.EncodeMeshToDracoBuffer(mesh, out);
  if (!len) throw new Error("cube encode failed");
  const raw = bytesOf(out, len);
  encoderModule.destroy(out);
  encoderModule.destroy(encoder);
  encoderModule.destroy(mesh);
  encoderModule.destroy(builder);
  return { raw, ids };
}

function encodeCloudAt(encoderModule, speed) {
  const builder = new encoderModule.PointCloudBuilder();
  const cloud = new encoderModule.PointCloud();
  const count = 40;
  const positions = new Float32Array(count * 3).map((_, i) => Math.sin(i * 1.7) * 50 + (i % 3) * 7);
  const normals = new Float32Array(count * 3);
  for (let i = 0; i < count; i++) {
    const v = [Math.cos(i), Math.sin(i * 0.7), 0.4 + (i % 5) * 0.1];
    const l = Math.hypot(...v);
    normals.set(v.map((x) => x / l), i * 3);
  }
  const colors = new Uint8Array(count * 3).map((_, i) => (i * 37) & 255);
  const ids = {
    POSITION: builder.AddFloatAttribute(cloud, encoderModule.POSITION, count, 3, positions),
    NORMAL: builder.AddFloatAttribute(cloud, encoderModule.NORMAL, count, 3, normals),
    RGB: builder.AddUInt8Attribute(cloud, encoderModule.COLOR, count, 3, colors),
  };
  const encoder = new encoderModule.Encoder();
  encoder.SetAttributeQuantization(encoderModule.POSITION, 12);
  encoder.SetAttributeQuantization(encoderModule.NORMAL, 9);
  encoder.SetSpeedOptions(speed, speed);
  const out = new encoderModule.DracoInt8Array();
  const len = encoder.EncodePointCloudToDracoBuffer(cloud, false, out);
  if (!len) throw new Error("point cloud encode failed");
  const raw = bytesOf(out, len);
  encoderModule.destroy(out);
  encoderModule.destroy(encoder);
  encoderModule.destroy(cloud);
  encoderModule.destroy(builder);
  return { raw, ids };
}

function presentAttributes(attributes) {
  const out = {};
  for (const [name, value] of Object.entries(attributes)) {
    out[name] = {
      array: Array.from(value.array),
      data: {
        componentsPerAttribute: value.data.componentsPerAttribute,
        componentDatatype: value.data.componentDatatype,
        byteOffset: value.data.byteOffset,
        byteStride: value.data.byteStride,
        normalized: value.data.normalized,
        quantization: value.data.quantization ?? null,
      },
    };
  }
  return out;
}

/** Plain JSON for a worker result; typed array types become GL datatypes. */
export function present(result) {
  if (result === true || result == null) return result ?? null;
  if (result.indexArray) {
    const typed = result.indexArray.typedArray;
    return {
      indexArray: {
        typedArray: Array.from(typed),
        numberOfIndices: result.indexArray.numberOfIndices,
        indexDatatype: typed.BYTES_PER_ELEMENT === 2 ? 5123 : 5125,
      },
      attributeData: presentAttributes(result.attributeData),
    };
  }
  return presentAttributes(result);
}

export default async function workersDecodeDraco() {
  const encoderModule = await draco3d.createEncoderModule();
  const mesh = encodeMesh(encoderModule);
  const cloud = encodeCloud(encoderModule);
  const worker = await loadWorker("decodeDraco");
  const decode = suite("decodeDraco");
  const run = (parameters) => invoke(worker, parameters).then(({ result }) => present(result));

  await decode.add("decodeDraco", [{ webAssemblyConfig: {} }], () => run({ webAssemblyConfig: {} }));
  await decode.add(
    "decodeDraco",
    [{ webAssemblyConfig: { wasmBinaryFile: "draco_decoder.wasm" }, array: Array.from(mesh.raw) }],
    () => run({ webAssemblyConfig: { wasmBinaryFile: "draco_decoder.wasm" }, array: mesh.raw }),
  );

  const meshParameters = {
    array: mesh.raw,
    bufferView: { byteLength: mesh.raw.length },
    compressedAttributes: mesh.ids,
  };
  await decode.add("decodeDraco", [meshParameters], () => run(meshParameters));
  await decode.add(
    "decodeDraco",
    [{ ...meshParameters, dequantizeInShader: false, attributesToSkipTransform: [] }],
    () => run({ ...meshParameters, dequantizeInShader: false, attributesToSkipTransform: [] }),
  );
  const positionOnly = {
    array: mesh.raw,
    bufferView: { byteLength: mesh.raw.length },
    compressedAttributes: { POSITION: mesh.ids.POSITION },
  };
  await decode.add("decodeDraco", [positionOnly], () => run(positionOnly));
  const padded = new Uint8Array(mesh.raw.length + 4);
  padded.set(mesh.raw);
  await decode.add(
    "decodeDraco",
    [{ array: padded, bufferView: { byteLength: mesh.raw.length }, compressedAttributes: { GENERIC: mesh.ids.GENERIC } }],
    () => run({ array: padded, bufferView: { byteLength: mesh.raw.length }, compressedAttributes: { GENERIC: mesh.ids.GENERIC } }),
  );
  await decode.add(
    "decodeDraco",
    [{ array: mesh.raw, bufferView: { byteLength: mesh.raw.length }, compressedAttributes: {} }],
    () => run({ array: mesh.raw, bufferView: { byteLength: mesh.raw.length }, compressedAttributes: {} }),
  );

  const cloudParameters = {
    buffer: cloud.raw,
    properties: { POSITION: 0, NORMAL: 0, extra: cloud.ids.GENERIC },
  };
  await decode.add("decodeDraco", [cloudParameters], () => run(cloudParameters));
  // POSITION is found by semantic. The numeric id is ignored, including a stale one.
  await decode.add(
    "decodeDraco",
    [{ buffer: cloud.raw, properties: { POSITION: 99 } }],
    () => run({ buffer: cloud.raw, properties: { POSITION: 99 } }),
  );
  await decode.add(
    "decodeDraco",
    [{ buffer: cloud.raw, properties: {} }],
    () => run({ buffer: cloud.raw, properties: {} }),
  );
  await decode.add(
    "decodeDraco",
    [{ webAssemblyConfig: null, buffer: cloud.raw, properties: { NORMAL: cloud.ids.NORMAL } }],
    () => run({ webAssemblyConfig: null, buffer: cloud.raw, properties: { NORMAL: cloud.ids.NORMAL } }),
  );

  const typed = {
    int8: encodeTyped(encoderModule, (builder, cloud, module, count) =>
      builder.AddInt8Attribute(cloud, module.GENERIC, count, 1, new Int8Array([-128, -1, 0, 127])),
    ),
    uint8: encodeTyped(encoderModule, (builder, cloud, module, count) =>
      builder.AddUInt8Attribute(cloud, module.GENERIC, count, 1, new Uint8Array([0, 1, 128, 255])),
    ),
    int16: encodeTyped(encoderModule, (builder, cloud, module, count) =>
      builder.AddInt16Attribute(cloud, module.GENERIC, count, 1, new Int16Array([-32768, -2, 3, 32767])),
    ),
    uint16: encodeTyped(encoderModule, (builder, cloud, module, count) =>
      builder.AddUInt16Attribute(cloud, module.GENERIC, count, 1, new Uint16Array([0, 2, 40000, 65535])),
    ),
    int32: encodeTyped(encoderModule, (builder, cloud, module, count) =>
      builder.AddInt32Attribute(cloud, module.GENERIC, count, 1, new Int32Array([-1073741823, -5, 9, 1073741823])),
    ),
  };
  for (const encoded of Object.values(typed)) {
    const parameters = { buffer: encoded.raw, properties: { value: encoded.id } };
    await decode.add("decodeDraco", [parameters], () => run(parameters));
  }
  for (const buffer of [boolCloud, int64Cloud, uint64Cloud, int64After, int64Mid, uint64After, uint64Mid, int32Bounds, int32Span]) {
    const parameters = { buffer, properties: { value: 0 } };
    await decode.add("decodeDraco", [parameters], () => run(parameters));
  }

  await decode.add(
    "decodeDraco",
    [{ buffer: mesh.raw, properties: { POSITION: 0 } }],
    () => run({ buffer: mesh.raw, properties: { POSITION: 0 } }),
  );
  await decode.add(
    "decodeDraco",
    [{ array: cloud.raw, bufferView: { byteLength: cloud.raw.length }, compressedAttributes: { POSITION: 0 } }],
    () => run({ array: cloud.raw, bufferView: { byteLength: cloud.raw.length }, compressedAttributes: { POSITION: 0 } }),
  );
  await decode.add(
    "decodeDraco",
    [{ array: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], bufferView: { byteLength: 12 }, compressedAttributes: { POSITION: 0 } }],
    () => run({ array: new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]), bufferView: { byteLength: 12 }, compressedAttributes: { POSITION: 0 } }),
  );
  await decode.add(
    "decodeDraco",
    [{ array: mesh.raw, bufferView: { byteLength: 8 }, compressedAttributes: mesh.ids }],
    () => run({ array: mesh.raw, bufferView: { byteLength: 8 }, compressedAttributes: mesh.ids }),
  );
  await decode.add(
    "decodeDraco",
    [{ array: mesh.raw, bufferView: { byteLength: 20 }, compressedAttributes: { POSITION: mesh.ids.POSITION } }],
    () => run({ array: mesh.raw, bufferView: { byteLength: 20 }, compressedAttributes: { POSITION: mesh.ids.POSITION } }),
  );
  await decode.add(
    "decodeDraco",
    [{ array: mesh.raw, bufferView: { byteLength: mesh.raw.length }, compressedAttributes: { POSITION: 99 } }],
    () => run({ array: mesh.raw, bufferView: { byteLength: mesh.raw.length }, compressedAttributes: { POSITION: 99 } }),
  );
  await decode.add(
    "decodeDraco",
    [{ buffer: cloud.raw, properties: { extra: 99 } }],
    () => run({ buffer: cloud.raw, properties: { extra: 99 } }),
  );

  // dequantizeInShader: skipped transforms keep the portable integers.
  const all = ["POSITION", "NORMAL", "TEX_COORD", "COLOR", "GENERIC"];
  const skipCase = async (encoded, attributesToSkipTransform, extra = {}) => {
    const parameters = {
      array: encoded.raw,
      bufferView: { byteLength: encoded.raw.length },
      compressedAttributes: encoded.ids,
      dequantizeInShader: true,
      attributesToSkipTransform,
      ...extra,
    };
    await decode.add("decodeDraco", [parameters], () => run(parameters));
  };
  await skipCase(mesh, all);
  await skipCase(mesh, ["POSITION", "NORMAL"]);
  await skipCase(mesh, ["TEX_COORD"]);
  await skipCase(mesh, ["INVALID"]);
  await skipCase(mesh, ["NOT_A_TYPE"]);
  await skipCase(mesh, all, { dequantizeInShader: 1 });
  await skipCase(mesh, undefined);
  for (const options of [{}, { speed: 10 }, { speed: 0 }, { positionBits: 20, colorBits: 16 }, { positionBits: 24, speed: 10 }]) {
    const cube = encodeCube(encoderModule, options);
    await skipCase(cube, [...all]);
    await skipCase(cube, ["COLOR"]);
    await skipCase(cube, []);
  }
  for (const speed of [5, 10, 0]) {
    const encoded = encodeCloudAt(encoderModule, speed);
    for (const dequantizeInShader of [true, false]) {
      const parameters = { buffer: encoded.raw, properties: { POSITION: 0, NORMAL: 0, rgb: encoded.ids.RGB }, dequantizeInShader };
      await decode.add("decodeDraco", [parameters], () => run(parameters));
    }
  }
  const skippedCloud = { buffer: cloud.raw, properties: { POSITION: 0, NORMAL: 0, extra: cloud.ids.GENERIC }, dequantizeInShader: true };
  await decode.add("decodeDraco", [skippedCloud], () => run(skippedCloud));

  return [decode];
}
