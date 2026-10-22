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

function present(result) {
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

  return [decode];
}
