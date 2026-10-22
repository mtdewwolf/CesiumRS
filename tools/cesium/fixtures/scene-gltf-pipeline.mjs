// Scene/GltfPipeline: the glTF JSON utilities, evaluated by the pinned CesiumJS
// engine on glTF 0.8, 1.0 and 2.0 documents and binary containers.
//
// Buffer sources travel as `{ "$source": [bytes], "pad": n, "tail": m }`: a
// Uint8Array view that starts `pad` bytes into its ArrayBuffer and has `tail`
// more bytes after it (pad and tail bytes are 0xEE).
// Results are the JSON after `JSON.stringify`, with sources written back as
// `{ "$source": [view bytes] }`.
import { suite } from "./lib.mjs";

const pipeline = async (name) => (await import(`@cesium/engine/Source/Scene/GltfPipeline/${name}.js`)).default;

function decode(value) {
  if (Array.isArray(value)) return value.map(decode);
  if (value && typeof value === "object") {
    if ("$source" in value) {
      const pad = value.pad ?? 0;
      const buffer = new Uint8Array(pad + value.$source.length + (value.tail ?? 0)).fill(0xee);
      buffer.set(value.$source, pad);
      return buffer.subarray(pad, pad + value.$source.length);
    }
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, decode(v)]));
  }
  return value;
}
function encode(value) {
  return JSON.parse(JSON.stringify(value, (_, v) => (v instanceof Uint8Array ? { $source: Array.from(v) } : v)));
}
const clone = (value) => JSON.parse(JSON.stringify(value));

// ---- sample data ------------------------------------------------------------

const bytesOf = (Type, values) => Array.from(new Uint8Array(new Type(values).buffer));
const source = (bytes, pad = 0) => ({ $source: bytes, pad });

// Positions (3 VEC3 floats), normals, texcoords, indices, joints (BYTE, SHORT,
// FLOAT), weights (BYTE, SHORT), interleaved data and an animation.
const positions = bytesOf(Float32Array, [0, 0, 0, 1, -2, 0.5, -1.5, 3, 2.25]);
const normals = bytesOf(Float32Array, [0, 0, 1, 0, 1, 0, 1, 0, 0]);
const texcoords = bytesOf(Float32Array, [0, 0, 1, 0, 0.5, 1]);
const indices = bytesOf(Uint16Array, [0, 1, 2, 2, 1, 0]);
const jointsByte = bytesOf(Int8Array, [0, 1, 2, -1, 3, 4, 5, 6, 7, 8, 9, 10]);
const jointsShort = bytesOf(Int16Array, [0, 1, 2, 300, 3, 4, 5, -6, 7, 8, 9, 10]);
const jointsFloat = bytesOf(Float32Array, [0, 1.5, 2, 70000, 3, 4, 5, 6, -1, 8, 9, 10]);
const weightsByte = bytesOf(Int8Array, [127, 0, 0, 0, 64, 63, 0, 0, -1, 2, 3, 4]);
const weightsShort = bytesOf(Int16Array, [32767, 0, 0, 0, 1000, 31767, 0, 0, -2, 5, 6, 7]);
const times = bytesOf(Float32Array, [0, 0.5, 1]);
const rotations = bytesOf(Float32Array, [0, 0, 1, 0, 0, 1, 0, 1.5707963, 1, 0, 0, 3.14159265]);

function concat(...parts) {
  const out = [];
  for (const p of parts) {
    while (out.length % 4) out.push(0);
    out.push(...p);
  }
  return out;
}

const body = concat(positions, normals, texcoords, indices, jointsByte, jointsShort, jointsFloat, weightsByte, weightsShort, times, rotations);
const offsets = (() => {
  const result = [];
  let at = 0;
  for (const p of [positions, normals, texcoords, indices, jointsByte, jointsShort, jointsFloat, weightsByte, weightsShort, times, rotations]) {
    while (at % 4) at++;
    result.push(at);
    at += p.length;
  }
  return result;
})();
const [oPos, oNor, oTex, oIdx, oJb, oJs, oJf, oWb, oWs, oTime, oRot] = offsets;

// A glTF 2.0 document with unused elements and extension references.
function gltf20() {
  return {
    asset: { version: "2.0" },
    extensionsUsed: ["KHR_draco_mesh_compression", "EXT_texture_webp", "KHR_texture_basisu", "EXT_meshopt_compression", "EXT_mesh_gpu_instancing", "CESIUM_primitive_outline", "EXT_structural_metadata", "EXT_mesh_features", "KHR_materials_specular"],
    extensionsRequired: ["KHR_draco_mesh_compression"],
    scene: 0,
    scenes: [{ nodes: [0, 3, 6] }, { nodes: [5] }],
    nodes: [
      { mesh: 0, children: [1, 2], translation: [1, 2, 3] },
      { matrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1] },
      { mesh: 2 },
      { skin: 0, mesh: 1, extensions: { EXT_mesh_gpu_instancing: { attributes: { TRANSLATION: 9 } } } },
      { children: [] },
      { camera: 0, rotation: [0, 0, 0, 1] },
      { children: [4], extras: { keep: true } },
      { mesh: 3 },
    ],
    meshes: [
      { primitives: [{ attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2 }, indices: 3, material: 1, targets: [{ POSITION: 0 }] }] },
      { primitives: [{ attributes: { POSITION: 0, JOINTS_0: 4, WEIGHTS_0: 7 }, mode: 1, extensions: { CESIUM_primitive_outline: { indices: 3 }, KHR_draco_mesh_compression: { bufferView: 8, attributes: { POSITION: 0 } }, EXT_mesh_features: { featureIds: [{ texture: { index: 3 } }, { attribute: 0 }] } } }] },
      { primitives: [] },
      { primitives: [{ attributes: { POSITION: 0, JOINTS_0: 6, WEIGHTS_0: 8 } }] },
    ],
    materials: [
      { name: "unused" },
      {
        pbrMetallicRoughness: { baseColorTexture: { index: 1 }, metallicRoughnessTexture: { index: 2, texCoord: 1 } },
        normalTexture: { index: 2 },
        emissiveTexture: { index: 4 },
        alphaMode: "MASK",
        extensions: { KHR_materials_specular: { specularTexture: { index: 4 }, specularColorTexture: { index: 1 } } },
      },
    ],
    textures: [{ source: 0 }, { source: 1, sampler: 1 }, { source: 2, extensions: { EXT_texture_webp: { source: 3 } } }, { extensions: { KHR_texture_basisu: { source: 4 } } }, { source: 1, sampler: 0 }],
    images: [{ uri: "unused.png" }, { uri: "a.png" }, { uri: "b.png" }, { bufferView: 9, mimeType: "image/webp" }, { uri: "c.ktx2" }],
    samplers: [{ magFilter: 9729 }, { wrapS: 33071 }, { minFilter: 9987 }],
    cameras: [{ type: "perspective", perspective: { yfov: 0.8, znear: 0.1 } }],
    skins: [{ joints: [1, 2], skeleton: 0, inverseBindMatrices: 10 }],
    animations: [{ channels: [{ sampler: 0, target: { node: 5, path: "rotation" } }, { sampler: 1, target: { node: 2, path: "weights" } }], samplers: [{ input: 11, output: 12 }, { input: 11, output: 12, interpolation: "STEP" }] }],
    accessors: [
      { bufferView: 0, componentType: 5126, count: 3, type: "VEC3", min: [-9, -9, -9], max: [9, 9, 9] },
      { bufferView: 1, byteOffset: 0, componentType: 5126, count: 3, type: "VEC3" },
      { bufferView: 2, componentType: 5126, count: 3, type: "VEC2" },
      { bufferView: 3, componentType: 5123, count: 6, type: "SCALAR" },
      { bufferView: 4, componentType: 5120, count: 3, type: "VEC4" },
      { componentType: 5126, count: 2, type: "VEC3" },
      { bufferView: 6, componentType: 5126, count: 3, type: "VEC4" },
      { bufferView: 7, componentType: 5120, count: 3, type: "VEC4", normalized: true },
      { bufferView: 10, componentType: 5122, count: 3, type: "VEC4", normalized: true },
      { bufferView: 11, componentType: 5126, count: 1, type: "VEC3" },
      { bufferView: 12, componentType: 5126, count: 1, type: "MAT4" },
      { bufferView: 13, componentType: 5126, count: 3, type: "SCALAR" },
      { bufferView: 14, componentType: 5126, count: 3, type: "VEC4" },
      { bufferView: 0, componentType: 5126, count: 1, type: "SCALAR" },
    ],
    bufferViews: [
      { buffer: 0, byteOffset: oPos, byteLength: positions.length },
      { buffer: 0, byteOffset: oNor, byteLength: normals.length, byteStride: 12 },
      { buffer: 0, byteOffset: oTex, byteLength: texcoords.length },
      { buffer: 0, byteOffset: oIdx, byteLength: indices.length },
      { buffer: 0, byteOffset: oJb, byteLength: jointsByte.length },
      { buffer: 1, byteLength: 4, extensions: { EXT_meshopt_compression: { buffer: 2 } } },
      { buffer: 0, byteOffset: oJf, byteLength: jointsFloat.length },
      { buffer: 0, byteOffset: oWb, byteLength: weightsByte.length },
      { buffer: 2, byteLength: 4 },
      { buffer: 0, byteOffset: 0, byteLength: 8 },
      { buffer: 0, byteOffset: oWs, byteLength: weightsShort.length },
      { buffer: 0, byteOffset: 0, byteLength: 12 },
      { buffer: 0, byteOffset: 0, byteLength: 64 },
      { buffer: 0, byteOffset: oTime, byteLength: times.length },
      { buffer: 0, byteOffset: oRot, byteLength: rotations.length },
      { buffer: 3, byteLength: 4 },
    ],
    buffers: [
      { byteLength: body.length, extras: { _pipeline: { source: source(body, 3) } } },
      { byteLength: 4, uri: "unused.bin" },
      { byteLength: 4, uri: "meshopt.bin" },
      { byteLength: 4, uri: "orphan.bin" },
    ],
    extensions: {
      EXT_structural_metadata: { propertyTables: [{ class: "c", count: 1, properties: { a: { values: 15, arrayOffsets: 9, stringOffsets: 11 } } }], propertyTextures: [{ class: "c", properties: { p: { index: 3, texCoord: 0 } } }] },
    },
  };
}

// A glTF 1.0 document with techniques, KHR_materials_common and binary glTF bits.
function gltf10() {
  return {
    asset: { version: "1.0", profile: { api: "WebGL", version: "1.0.3" }, premultipliedAlpha: true },
    extensionsUsed: ["KHR_materials_common", "KHR_binary_glTF", "CESIUM_RTC", "OTHER_ext"],
    glExtensionsUsed: ["OES_element_index_uint"],
    scene: "defaultScene",
    scenes: { defaultScene: { nodes: ["root", "empty", "camNode"] } },
    nodes: {
      root: { children: ["child", "emptyChild"], meshes: ["meshA", "meshB", "meshC"], translation: [0, 0, 0], rotation: [0, 0, 0, 1] },
      child: { jointName: "joint1", skin: "skin1", skeletons: ["root"], meshes: ["meshA"], matrix: [2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1] },
      emptyChild: { translation: [0, 0, 0], scale: [1, 1, 1] },
      empty: { children: ["emptyChild2"] },
      emptyChild2: {},
      camNode: { camera: "cam", jointName: "joint2" },
    },
    cameras: { cam: { type: "perspective", perspective: { aspectRatio: 0, yfov: 0, zfar: 100, znear: 0.1 } } },
    skins: { skin1: { jointNames: ["joint1", "joint2", "missing"], inverseBindMatrices: "accIbm", bindShapeMatrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1] } },
    meshes: {
      meshA: { primitives: [{ attributes: { POSITION: "accPos", NORMAL: "accNor", TEXCOORD: "accTex", COLOR: "accTex", JOINT: "accJoint", WEIGHT: "accWeight", BATCHID: "accTex" }, indices: "accIdx", material: "matTechnique", mode: 4 }] },
      meshB: { primitives: [{ attributes: { POSITION: "accPos", TEXCOORD_1: "accTex", _CUSTOM: "accTex", APP_SPECIFIC_2: "accTex" }, material: "matCommon" }] },
      meshC: { primitives: [{ attributes: { POSITION: "accPos" }, material: "matConstant" }] },
    },
    accessors: {
      accPos: { bufferView: "bvVertex", byteOffset: 0, byteStride: 12, componentType: 5126, count: 3, type: "VEC3" },
      accNor: { bufferView: "bvVertex", byteOffset: 36, byteStride: 0, componentType: 5126, count: 3, type: "VEC3" },
      accTex: { bufferView: "bvTex", byteOffset: 0, byteStride: 8, componentType: 5126, count: 3, type: "VEC2" },
      accIdx: { bufferView: "bvIndex", byteOffset: 0, componentType: 5123, count: 6, type: "SCALAR" },
      accJoint: { bufferView: "bvSkin", byteOffset: 0, componentType: 5122, count: 3, type: "VEC4" },
      accWeight: { bufferView: "bvSkin", byteOffset: 24, componentType: 5126, count: 3, type: "VEC4", min: [5, 5, 5, 5] },
      accIbm: { bufferView: "bvIbm", byteOffset: 0, componentType: 5126, count: 1, type: "MAT4" },
      accTime: { bufferView: "bvAnim", byteOffset: 0, componentType: 5126, count: 3, type: "SCALAR" },
      accRot: { bufferView: "bvAnim", byteOffset: 12, componentType: 5126, count: 3, type: "VEC4", max: [1, 1, 1, 1] },
    },
    bufferViews: {
      bvVertex: { buffer: "main", byteOffset: oPos, target: 34962 },
      bvTex: { buffer: "main", byteOffset: oTex, byteLength: 4 },
      bvIndex: { buffer: "main", byteOffset: oIdx, byteLength: 12, target: 34963 },
      bvSkin: { buffer: "main", byteOffset: oJs, byteLength: 72 },
      bvIbm: { buffer: "main", byteOffset: 0, byteLength: 64 },
      bvAnim: { buffer: "main", byteOffset: oTime, byteLength: 60 },
      bvShader: { buffer: "binary_glTF", byteOffset: 0, byteLength: 8 },
    },
    buffers: {
      main: { type: "arraybuffer", extras: { _pipeline: { source: source(body) } } },
      binary_glTF: { byteLength: 8, type: "arraybuffer", extras: { _pipeline: { source: source([1, 2, 3, 4, 5, 6, 7, 8]) } } },
    },
    animations: {
      anim: {
        channels: [{ sampler: "s0", target: { id: "root", path: "rotation" } }, { sampler: "s1", target: { id: "child", path: "translation" } }],
        parameters: { TIME: "accTime", rotation: "accRot" },
        samplers: { s0: { input: "TIME", interpolation: "LINEAR", output: "rotation" }, s1: { input: "TIME", output: "rotation" } },
      },
    },
    materials: {
      matTechnique: { technique: "tech", values: { u_diffuse: "texA", u_shininess: [10], u_emission: [0.1, 0.2, 0.3, 1], u_unmapped: 1 } },
      matCommon: { extensions: { KHR_materials_common: { technique: "PHONG", doubleSided: true, transparent: true, values: { diffuse: "texA", ambient: [0.5, 0.25, 0.125, 1], emission: [0.1, 0.1, 0.1, 1], transparency: 0.5 } } } },
      matConstant: { name: "Named", extensions: { KHR_materials_common: { technique: "CONSTANT", values: { emission: [0.9, 0.04, 0.5, 1], ambient: "texB" } } } },
      matLambert: { extensions: { KHR_materials_common: { technique: "LAMBERT", values: { diffuse: [0.5, 0.5, 0.5, 1] }, transparent: false } } },
    },
    techniques: {
      tech: {
        parameters: {
          diffuse: { type: 35678 },
          shininess: { type: 5126, value: [8] },
          modelView: { type: 35676, semantic: "MODELVIEW" },
          rtc: { type: 35676, semantic: "CESIUM_RTC_MODELVIEW" },
          position: { type: 35665, semantic: "POSITION" },
          texcoord: { type: 35664, semantic: "TEXCOORD" },
          color: { type: 35666, semantic: "COLOR" },
          jointNode: { type: 35676, node: "child" },
          tex2: { type: 35678, value: "texB" },
          custom: { type: 5126, semantic: "BATCHID" },
        },
        attributes: { a_position: "position", a_texcoord: "texcoord", a_color: "color", a_batch: "custom" },
        uniforms: { u_diffuse: "diffuse", u_shininess: "shininess", u_modelView: "modelView", u_rtc: "rtc", u_joint: "jointNode", u_tex2: "tex2" },
        program: "prog",
        states: { enable: [3042, 2929], functions: { blendFuncSeparate: [770, 771, 1, 771], blendEquationSeparate: [32774, 32774] } },
      },
      tech2: { parameters: {}, attributes: {}, uniforms: {}, program: "prog", states: { enable: [2884], functions: { blendFuncSeparate: [770, 771, 1, 32769] } } },
    },
    programs: { prog: { attributes: ["a_position"], fragmentShader: "fs", vertexShader: "vs" } },
    shaders: { fs: { type: 35632, uri: "fs.glsl" }, vs: { type: 35633, extensions: { KHR_binary_glTF: { bufferView: "bvShader" } } } },
    textures: { texA: { sampler: "samp", source: "img", format: 6408, internalFormat: 6408, target: 3553, type: 5121 }, texB: { source: "img2" } },
    images: { img: { uri: "a.png" }, img2: { extensions: { KHR_binary_glTF: { bufferView: "bvShader", mimeType: "image/png", height: 1, width: 1 } } } },
    samplers: { samp: { magFilter: 9729 } },
  };
}

// A glTF 0.8 document.
function gltf08() {
  return {
    version: "0.8",
    asset: { profile: "WebGL 1.0.2", generator: "collada2gltf" },
    allExtensions: ["KHR_materials_common"],
    lights: { light0: { type: "directional", directional: { color: [1, 1, 1] } } },
    scene: "s",
    scenes: { s: { nodes: ["n0"] } },
    nodes: {
      n0: { rotation: [0, 0, 1, 1.5707963267948966], meshes: ["m"], children: ["n1"] },
      n1: { instanceSkin: { skeletons: ["n0"], skin: "skin", meshes: ["m"] }, rotation: [1, 0, 0, 0] },
    },
    skins: { skin: { jointNames: [], inverseBindMatrices: "accIbm" } },
    meshes: { m: { primitives: [{ attributes: { POSITION: "accPos" }, primitive: 1, material: "mat" }, { attributes: { POSITION: "accPos" }, material: "mat" }] } },
    materials: { mat: { instanceTechnique: { technique: "tech", values: { color: [1, 0, 0, 1] } } }, mat2: { instanceTechnique: { values: {} } } },
    techniques: {
      tech: { pass: "pass0", passes: { pass0: { instanceProgram: { attributes: { a_position: "position" }, program: "prog", uniforms: { u_color: "color" } }, states: { enable: [2929] } } }, parameters: { position: { type: 35665, semantic: "POSITION" }, color: { type: 35666 } } },
      tech2: { passes: { defaultPass: { instanceProgram: { attributes: {}, program: "prog" } } }, parameters: { x: { type: 5126, value: 2 } }, uniforms: { kept: "x" } },
    },
    programs: { prog: { fragmentShader: "fs", vertexShader: "vs" } },
    shaders: { fs: { uri: "fs.glsl" }, vs: { uri: "vs.glsl" } },
    animations: { a: { channels: [{ sampler: "s", target: { id: "n0", path: "rotation" } }], parameters: { TIME: "accTime", rotation: "accRot" }, samplers: { s: { input: "TIME", output: "rotation" } } } },
    accessors: {
      accPos: { bufferView: "bv", byteOffset: oPos, componentType: 5126, count: 3, type: "VEC3" },
      accTime: { bufferView: "bv", byteOffset: oTime, componentType: 5126, count: 3, type: "SCALAR" },
      accRot: { bufferView: "bv", byteOffset: oRot, componentType: 5126, count: 3, type: "VEC4" },
      accIbm: { bufferView: "bv", byteOffset: 0, componentType: 5126, count: 1, type: "MAT4" },
    },
    bufferViews: { bv: { buffer: "b", byteOffset: 0 } },
    buffers: { b: { byteLength: body.length, extras: { _pipeline: { source: source(body, 4) } } } },
  };
}

// ---- GLB containers -----------------------------------------------------------

const utf8 = (s) => Array.from(new TextEncoder().encode(s));
const u32 = (v) => bytesOf(Uint32Array, [v]);
function pad4(bytes, fill) {
  const out = [...bytes];
  while (out.length % 4) out.push(fill);
  return out;
}
function glb2(json, bin, options = {}) {
  const jsonChunk = pad4(utf8(JSON.stringify(json)), 0x20);
  const chunks = [...u32(jsonChunk.length), ...u32(0x4e4f534a), ...jsonChunk];
  if (bin) {
    const binChunk = pad4(bin, 0);
    chunks.push(...u32(binChunk.length), ...u32(0x004e4942), ...binChunk);
  }
  if (options.extraChunk) chunks.push(...u32(4), ...u32(0x12345678), 9, 9, 9, 9);
  const length = options.length ?? 12 + chunks.length;
  return [...utf8("glTF"), ...u32(options.version ?? 2), ...u32(length), ...chunks];
}
function glb1(json, bin, options = {}) {
  const content = utf8(JSON.stringify(json));
  const length = 20 + content.length + bin.length;
  return [...utf8("glTF"), ...u32(1), ...u32(options.length ?? length), ...u32(content.length), ...u32(options.format ?? 0), ...content, ...bin];
}

export default async function sceneGltfPipeline() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    s.dir = "gltf_pipeline";
    out.push(s);
    return s;
  };
  const samples = { gltf20: gltf20(), gltf10: gltf10(), gltf08: gltf08() };

  {
    const s = add("usesExtension");
    const fn = await pipeline("usesExtension");
    for (const [gltf, ext] of [[{}, "A"], [{ extensionsUsed: [] }, "A"], [{ extensionsUsed: ["A", "B"] }, "B"], [{ extensionsUsed: ["AB"] }, "A"], [{ extensionsUsed: "ABC" }, "B"], [{ extensionsUsed: null }, "A"]]) {
      s.add("usesExtension", [gltf, ext], (g, e) => fn(g, e));
    }
  }
  for (const name of ["addExtensionsUsed", "addExtensionsRequired", "removeExtensionsRequired", "removeExtensionsUsed"]) {
    const s = add(name);
    const fn = await pipeline(name);
    const states = [{}, { extensionsUsed: ["A"] }, { extensionsUsed: ["A", "B"], extensionsRequired: ["A"] }, { extensionsUsed: ["B"], extensionsRequired: [] }, { extensionsRequired: ["A", "A"] }, { extensionsUsed: null, extensionsRequired: null }];
    for (const state of states) for (const ext of ["A", "B"]) {
      s.add(name, [state, ext], (g, e) => {
        g = clone(g);
        fn(g, e);
        return g;
      });
    }
  }
  {
    const s = add("addToArray");
    const fn = await pipeline("addToArray");
    for (const [array, element, check] of [[[], 1, false], [[1, 2], 2, false], [[1, 2], 2, true], [["a"], "a", true], [[{ a: 1 }], { a: 1 }, true], [[null], null, true], [[1], "1", true], [[0], -0, true]]) {
      s.add("addToArray", [array, element, check], (a, e, c) => {
        a = clone(a);
        return [fn(a, e, c), a];
      });
    }
  }
  {
    const s = add("numberOfComponentsForType");
    const fn = await pipeline("numberOfComponentsForType");
    for (const t of ["SCALAR", "VEC2", "VEC3", "VEC4", "MAT2", "MAT3", "MAT4", "vec3", ""]) s.add("numberOfComponentsForType", [t], (t) => fn(t) ?? null);
  }
  {
    const s = add("getAccessorByteStride");
    const fn = await pipeline("getAccessorByteStride");
    const gltf = { bufferViews: [{}, { byteStride: 20 }, { byteStride: 0 }, { byteStride: -4 }] };
    for (const accessor of [{ componentType: 5126, type: "VEC3" }, { bufferView: 0, componentType: 5123, type: "SCALAR" }, { bufferView: 1, componentType: 5126, type: "VEC3" }, { bufferView: 2, componentType: 5121, type: "VEC4" }, { bufferView: 3, componentType: 5130, type: "MAT4" }, { componentType: 5126, type: "FOO" }, { componentType: 1, type: "VEC2" }]) {
      s.add("getAccessorByteStride", [gltf, accessor], (g, a) => fn(g, a));
    }
  }
  {
    const s = add("addBuffer");
    const fn = await pipeline("addBuffer");
    for (const [gltf, bytes] of [[{ buffers: [], bufferViews: [] }, [1, 2, 3]], [{ buffers: [{ byteLength: 1 }], bufferViews: [{ buffer: 0 }] }, []], [{}, [1]]]) {
      s.add("addBuffer", [gltf, bytes], (g, b) => {
        g = clone(g);
        const index = fn(g, Uint8Array.from(b));
        return [index, encode(g)];
      });
    }
  }
  for (const name of ["addPipelineExtras", "removePipelineExtras"]) {
    const s = add(name);
    const fn = await pipeline(name);
    const inputs = [
      {},
      { buffers: [{}, { extras: { a: 1 } }], images: [{ extras: { _pipeline: { x: 1 } } }], shaders: [{}], extras: { _pipeline: {} } },
      { extensionsUsed: ["KHR_techniques_webgl"], extensions: { KHR_techniques_webgl: { shaders: [{ extras: { _pipeline: {}, b: 2 } }] } }, shaders: [{}] },
      { buffers: { a: {}, b: { extras: { _pipeline: {} } } } },
      { buffers: [{ extras: 5 }] },
      { buffers: [null] },
    ];
    for (const input of inputs) {
      s.add(name, [input], (g) => {
        g = clone(g);
        fn(g);
        return g;
      });
    }
  }
  {
    const s = add("removeExtension");
    const fn = await pipeline("removeExtension");
    const inputs = [
      [{ extensionsUsed: ["A", "B"], extensionsRequired: ["A"], extensions: { A: { top: 1 }, B: {} }, meshes: [{ extensions: { A: {} }, primitives: [{ extensions: { A: 1, C: 2 } }] }], nodes: [{ extras: { extensions: { A: null } } }] }, "A"],
      [{ extensionsUsed: ["CESIUM_RTC"], extensions: { CESIUM_RTC: { center: [1, 2, 3] } }, techniques: { t: { uniforms: { u: { semantic: "CESIUM_RTC_MODELVIEW" }, v: { semantic: "MODELVIEW" } } } } }, "CESIUM_RTC"],
      [{ extensionsUsed: ["CESIUM_RTC", "KHR_techniques_webgl"], extensions: { KHR_techniques_webgl: { techniques: [{ uniforms: { u: { semantic: "CESIUM_RTC_MODELVIEW" } } }] } } }, "CESIUM_RTC"],
      [{ asset: {} }, "A"],
      [clone(samples.gltf20), "EXT_texture_webp"],
    ];
    for (const [gltf, ext] of inputs) {
      s.add("removeExtension", [gltf, ext], (g, e) => {
        g = decode(g);
        const removed = fn(g, e);
        return [removed ?? null, encode(g)];
      });
    }
  }
  {
    const s = add("ForEach");
    const ForEach = await pipeline("ForEach");
    const topLevel = ["accessor", "animation", "buffer", "bufferView", "camera", "image", "material", "mesh", "node", "program", "sampler", "scene", "shader", "skin", "technique", "texture"];
    const docs = { ...samples, techniques: { extensionsUsed: ["KHR_techniques_webgl"], extensions: { KHR_techniques_webgl: { programs: [{ a: 1 }], shaders: [{ b: 1 }, { c: 2 }], techniques: [{ uniforms: { u: { node: 1 } } }] } }, techniques: { legacy: {} } } };
    for (const [docName, doc] of Object.entries(docs)) {
      for (const name of topLevel) {
        s.add("topLevel", [name, doc], (n, g) => {
          const visits = [];
          ForEach[n](decode(g), (value, id) => {
            visits.push([encode(value), id]);
          });
          return visits;
        });
      }
      s.add("stopsEarly", [doc], (g) => {
        let calls = 0;
        const value = ForEach.accessor(decode(g), (accessor, id) => (++calls === 2 ? id : undefined));
        return [value ?? null, calls];
      });
      const nested = ["accessorContainingVertexAttributeData", "accessorContainingIndexData"];
      for (const n of nested) s.add("accessors", [n, doc], (n, g) => {
        const visits = [];
        ForEach[n](decode(g), (id) => {
          visits.push(id);
        });
        return visits;
      });
      for (const semantic of ["POSITION", "TEXCOORD", "JOINTS_0", "_"]) s.add("accessorWithSemantic", [semantic, doc], (sem, g) => {
        const visits = [];
        ForEach.accessorWithSemantic(decode(g), sem, (id) => {
          visits.push(id);
        });
        return visits;
      });
      s.add("nested", [doc], (g) => {
        g = decode(g);
        const out = [];
        ForEach.mesh(g, (mesh) => ForEach.meshPrimitive(mesh, (primitive, i) => {
          out.push(["primitive", i]);
          ForEach.meshPrimitiveAttribute(primitive, (a, sem) => {
            out.push(["attribute", a, sem]);
          });
          ForEach.meshPrimitiveTarget(primitive, (t, j) => {
            out.push(["target", j]);
            ForEach.meshPrimitiveTargetAttribute(t, (a, sem) => {
              out.push(["targetAttribute", a, sem]);
            });
          });
        }));
        ForEach.animation(g, (a) => {
          ForEach.animationChannel(a, (c, i) => {
            out.push(["channel", i]);
          });
          ForEach.animationSampler(a, (smp, i) => {
            out.push(["sampler", i]);
          });
        });
        ForEach.material(g, (m) => ForEach.materialValue(m, (v, name) => {
          out.push(["value", encode(v), name]);
        }));
        ForEach.skin(g, (skin) => ForEach.skinJoint(skin, (j) => {
          out.push(["joint", j]);
        }));
        ForEach.technique(g, (t) => {
          ForEach.techniqueAttribute(t, (v, n) => {
            out.push(["techniqueAttribute", v, n]);
          });
          ForEach.techniqueUniform(t, (v, n) => {
            out.push(["techniqueUniform", encode(v), n]);
          });
          ForEach.techniqueParameter(t, (v, n) => {
            out.push(["techniqueParameter", encode(v), n]);
          });
        });
        ForEach.scene(g, (scene) => ForEach.nodeInScene(g, scene, (node, id) => {
          out.push(["nodeInScene", id]);
        }));
        return out;
      });
    }
    s.add("objectLegacy", [{ b: 1, 2: 2, a: 3, 1: 4, "01": 5 }], (o) => {
      const visits = [];
      ForEach.objectLegacy(o, (v, id) => {
        visits.push([v, id]);
      });
      return visits;
    });
  }
  {
    const s = add("getComponentReader");
    const fn = await pipeline("getComponentReader");
    const bytes = Array.from({ length: 24 }, (_, i) => (i * 53 + 7) & 255);
    for (const type of [5120, 5121, 5122, 5123, 5124, 5125, 5126, 5130]) {
      for (const [offset, count, stride] of [[0, 4, 1], [1, 3, 2], [3, 2, 4], [0, 2, 8], [20, 1, 4], [22, 2, 2], [NaN, 2, 4]]) {
        s.add("read", [type, bytes, offset, count, stride], (t, b, o, c, st) => {
          const result = new Array(c);
          fn(t)(new DataView(Uint8Array.from(b).buffer), o, c, st, result);
          return result;
        });
      }
    }
  }
  const accessorDocs = [["gltf20", samples.gltf20, 14], ["gltf10", samples.gltf10, null]];
  for (const name of ["readAccessorPacked", "findAccessorMinMax"]) {
    const s = add(name);
    const fn = await pipeline(name);
    for (const [docName, doc] of accessorDocs) {
      const ids = docName === "gltf10" ? Object.keys(doc.accessors) : doc.accessors.map((_, i) => i);
      for (const id of ids) s.add(name, [doc, id], (g, id) => {
        g = decode(g);
        const accessor = g.accessors[id];
        return fn(g, accessor);
      });
    }
    const extra = [
      { componentType: 5126, count: 2, type: "FOO", bufferView: 0, byteOffset: 0 },
      { componentType: 5126, count: 2, type: "FOO", byteOffset: 0 },
      { componentType: 5126, count: 3, type: "VEC2", bufferView: 0 },
      { componentType: 5126, count: 100, type: "VEC4", bufferView: 0, byteOffset: 0 },
      { componentType: 5126, count: 1.5, type: "VEC2" },
    ];
    for (const accessor of extra) s.add(name, [samples.gltf20, accessor], (g, a) => fn(decode(g), a));
  }
  {
    const s = add("updateAccessorComponentTypes");
    const fn = await pipeline("updateAccessorComponentTypes");
    s.add("updateAccessorComponentTypes", [samples.gltf20], (g) => {
      g = decode(g);
      fn(g);
      return encode(g);
    });
  }
  {
    const s = add("forEachTextureInMaterial");
    const fn = await pipeline("forEachTextureInMaterial");
    const materials = [
      samples.gltf20.materials[1],
      { pbrMetallicRoughness: {}, extensions: { KHR_materials_pbrSpecularGlossiness: { diffuseTexture: { index: 0 }, specularGlossinessTexture: { index: 1 } }, KHR_materials_common: { values: { diffuse: { index: 2 }, ambient: [1, 1, 1, 1], emission: { index: 3 }, specular: { index: 4 } } } }, occlusionTexture: { index: 5 } },
      { values: { a: { index: 1 }, b: 2, c: { index: 3 } }, extensions: {} },
      { values: { a: { index: 1 } }, extensions: { KHR_techniques_webgl: { values: { x: { index: 7 }, y: [1] } } } },
      { pbrMetallicRoughness: { baseColorTexture: 5 }, emissiveTexture: null },
      {},
    ];
    for (const material of materials) {
      s.add("visits", [material], (m) => {
        const visits = [];
        fn(m, (index, info) => {
          visits.push([index ?? null, info]);
        });
        return visits;
      });
      s.add("stopsEarly", [material], (m) => fn(m, (index) => (index === 1 ? "found" : undefined)) ?? null);
    }
  }
  {
    const s = add("addDefaults");
    const fn = await pipeline("addDefaults");
    const inputs = [
      clone(samples.gltf20),
      { meshes: [{ primitives: [{ attributes: { POSITION: 0 } }, { attributes: {}, material: 0 }] }], accessors: [{ bufferView: 0, componentType: 5126, type: "VEC3" }], bufferViews: [{ buffer: 0 }], buffers: [{}], scenes: [{}], scene: null },
      {
        materials: [
          { extensions: { KHR_materials_common: { technique: "CONSTANT" } } },
          { extensions: { KHR_materials_common: { technique: "LAMBERT", values: { ambient: [1, 1, 1, 1], transparency: null } } } },
          { extensions: { KHR_materials_common: { technique: "PHONG", values: {}, doubleSided: true } } },
          { alphaMode: "BLEND", pbrMetallicRoughness: { baseColorTexture: { index: 0 } }, extensions: { KHR_materials_pbrSpecularGlossiness: { diffuseTexture: { index: 1 }, specularGlossinessTexture: { index: 2 } } } },
          { extensions: { KHR_techniques_webgl: { values: { a: { index: 0 }, b: [1, 2], c: { index: 1, texCoord: 2 } } } }, normalTexture: { index: 3 } },
        ],
        animations: [{ channels: [{ target: { node: 1, path: "translation" } }, { target: { node: 0, path: "weights" } }], samplers: [{}, { interpolation: "CUBICSPLINE" }] }],
        nodes: [{}, {}, { scale: [2, 2, 2] }, { matrix: [2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1] }],
        samplers: [{}, { wrapS: 33071 }],
      },
      { meshes: { m: { primitives: [{}] } } },
    ];
    for (const input of inputs) s.add("addDefaults", [input], (g) => {
      g = decode(g);
      fn(g);
      return encode(g);
    });
  }
  {
    const s = add("parseGlb");
    const fn = await pipeline("parseGlb");
    const json2 = { asset: { version: "2.0" }, buffers: [{ byteLength: 8 }], extensionsUsed: ["KHR_binary_glTF"] };
    const json1 = { asset: { version: "1.0" }, extensionsUsed: ["KHR_binary_glTF", "X"], buffers: { binary_glTF: { uri: "data:,", byteLength: 4 } } };
    const cases = [
      glb2(json2, [1, 2, 3, 4, 5, 6, 7, 8]),
      glb2(json2, null),
      glb2({ asset: { version: "2.0" } }, [1, 2]),
      glb2({ asset: { version: "2.0" }, buffers: [] }, [1, 2]),
      glb2(json2, [9, 8, 7], { extraChunk: true }),
      glb1(json1, [1, 2, 3, 4]),
      glb1({ asset: { version: "1.0" }, buffers: { KHR_binary_glTF: {} } }, [5, 6]),
      glb1({ asset: {} }, [5]),
      glb1(json1, [1, 2], { format: 1 }),
      glb2(json2, [1], { version: 3 }),
      [...utf8("gltf"), 2, 0, 0, 0],
      glb2(json2, [1, 2, 3, 4], { length: 200 }),
      [...utf8("glTF"), 2, 0, 0, 0, 24, 0, 0, 0, 4, 0, 0, 0, 0x4a, 0x53, 0x4f, 0x4e, ...utf8("{x}")],
      [...utf8("glTF"), 2, 0],
      [0xef, 0xbb],
    ];
    for (const bytes of cases) {
      for (const pad of [0, 5]) s.add("parseGlb", [{ $source: bytes, pad }], (g) => {
        const result = fn(decode(g));
        return result === undefined ? null : encode(result);
      });
    }
    // A header-only version 2 container: the five-word header read throws
    // unless the view's buffer extends past it.
    const headerOnly = [...utf8("glTF"), ...u32(2), ...u32(12)];
    for (const tail of [0, 3, 8, 20]) s.add("parseGlb", [{ $source: headerOnly, pad: 0, tail }], (g) => {
      const result = fn(decode(g));
      return result === undefined ? null : encode(result);
    });
  }
  {
    const s = add("moveTechniqueRenderStates");
    const fn = await pipeline("moveTechniqueRenderStates");
    for (const input of [samples.gltf10, { materials: [{ technique: 0 }] }, { techniques: [{ states: { enable: [3042] } }, { states: {} }, {}], materials: [{ technique: 0 }, { technique: 1 }, { technique: 2 }, {}] }, { techniques: { a: { states: { enable: [3042, 2884], functions: { blendEquationSeparate: [32778, 32774] } } } }, materials: { m: { technique: "a", extensions: { other: 1 } } }, extensions: { x: 1 } }]) {
      s.add("moveTechniqueRenderStates", [input], (g) => {
        g = decode(g);
        fn(g);
        return encode(g);
      });
    }
  }
  {
    const s = add("moveTechniquesToExtension");
    const fn = await pipeline("moveTechniquesToExtension");
    const shared = { techniques: { a: { name: "A", program: "p", parameters: { x: { type: 5126 } }, attributes: {}, uniforms: { u_x: "x" } }, b: { program: "p2", parameters: {}, attributes: {}, uniforms: {} } }, programs: { p: { name: "P", fragmentShader: "s", vertexShader: "s" }, p2: { fragmentShader: "missing", vertexShader: "missing2" } }, shaders: { s: { uri: "s.glsl" } }, materials: { m: { technique: "a", values: { x: 1, y: 2 } }, n: { technique: "b" }, o: { values: { z: 1 } } } };
    for (const input of [samples.gltf10, shared, { materials: [{ technique: 0, values: { a: 1 } }] }, { techniques: {}, materials: [{}] }]) {
      s.add("moveTechniquesToExtension", [input], (g) => {
        g = decode(g);
        fn(g);
        return encode(g);
      });
    }
  }
  {
    const s = add("removeUnusedElements");
    const fn = await pipeline("removeUnusedElements");
    const types = [null, ["mesh"], ["node"], ["material"], ["accessor"], ["bufferView"], ["buffer"], ["texture"], ["sampler"], ["image"], ["buffer", "mesh", "unknown"]];
    const metadata = clone(samples.gltf20);
    metadata.extensionsUsed.push("EXT_feature_metadata");
    metadata.extensions.EXT_feature_metadata = { featureTables: { t: { properties: { a: { bufferView: 14, arrayOffsetBufferView: 9 }, b: { stringOffsetBufferView: 3 } } } }, featureTextures: { f: { properties: { p: { texture: { index: 4 } } } } } };
    metadata.meshes[0].primitives[0].extensions = { EXT_feature_metadata: { featureIdTextures: [{ featureIds: { texture: { index: 3 } } }] } };
    for (const [docName, doc] of [["gltf20", samples.gltf20], ["metadata", metadata]]) {
      for (const t of types) s.add("removeUnusedElements", [doc, t], (g, t) => {
        g = decode(g);
        fn(g, t ?? undefined);
        return encode(g);
      });
    }
    s.add("removeUnusedElements", [{ nodes: [{ children: [1] }, { children: [2] }, {}], scenes: [{ nodes: [0] }] }, ["node"]], (g, t) => {
      g = clone(g);
      fn(g, t);
      return g;
    });
    s.add("removeUnusedElements", [{ meshes: [{ primitives: [{}] }], nodes: [{ mesh: 0 }, { mesh: 0, weights: [1] }], scenes: [{ nodes: [0, 1] }] }, null], (g, t) => {
      g = clone(g);
      fn(g, t ?? undefined);
      return g;
    });
  }
  {
    const s = add("updateVersion");
    const fn = await pipeline("updateVersion");
    const options = [null, { keepLegacyExtensions: true }, { targetVersion: "1.0" }, { baseColorTextureNames: ["u_diffuse"], baseColorFactorNames: ["u_emission"] }];
    for (const [docName, doc] of Object.entries(samples)) {
      for (const option of options) s.add("updateVersion", [doc, option], (g, o) => {
        g = decode(g);
        fn(g, o ?? undefined);
        return encode(g);
      });
    }
    const variants = [
      { asset: { version: "1.0.1" }, nodes: { a: {} } },
      { asset: { version: 2 } },
      { version: 1, asset: { version: "2.0" } },
      {},
      { asset: { version: "2.0" }, materials: [{ extensions: { KHR_materials_common: { technique: "BLINN", values: { diffuse: { index: 0 }, emission: { index: 1 } } } } }] },
      { asset: { version: "2.0" }, extensionsUsed: ["KHR_techniques_webgl"], materials: [{ extensions: { KHR_techniques_webgl: { values: { u_tex: { index: 2 }, u_diffuse_mat: [1, 0.5, 0.01, 0.5] } } } }], extensions: { KHR_techniques_webgl: {} } },
      { asset: { version: "0.8" } },
      { asset: { version: "0.85" }, meshes: { m: { primitives: [{ primitive: 0 }] } } },
    ];
    for (const doc of variants) s.add("updateVersion", [doc, null], (g) => {
      g = decode(g);
      fn(g);
      return encode(g);
    });
  }
  return out;
}
