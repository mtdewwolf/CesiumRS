// Fixtures for the 3D Tiles binary content parsers: Cesium3DTileFeatureTable,
// B3dmParser, I3dmParser and PntsParser. Tiles are built byte by byte here,
// including both legacy b3dm headers, unaligned glbs, binary globals, Draco
// point clouds and every error path. Results are written in a fixed shape:
// typed arrays as { type, values }, views with their offset in the tile buffer.
import Cesium3DTileFeatureTable from '@cesium/engine/Source/Scene/Cesium3DTileFeatureTable.js';
import B3dmParser from '@cesium/engine/Source/Scene/B3dmParser.js';
import I3dmParser from '@cesium/engine/Source/Scene/I3dmParser.js';
import PntsParser from '@cesium/engine/Source/Scene/PntsParser.js';
import ComponentDatatype from '@cesium/engine/Source/Core/ComponentDatatype.js';
import {enc} from './lib.mjs';

const text = new TextEncoder();

const typed = (a) => (a === undefined || a === null ? null : ArrayBuffer.isView(a) ? {type: a.constructor.name, values: enc(Array.from(a))} : enc(a));

/** JSON padded with spaces to `pad` bytes (8 in the spec; 1 for unaligned tests). */
function jsonBytes(value, pad = 8, extra = 0) {
  if (value === null) return new Uint8Array(0);
  const raw = typeof value === 'string' ? text.encode(value) : text.encode(JSON.stringify(value));
  const length = Math.ceil((raw.length + extra) / pad) * pad;
  const out = new Uint8Array(length).fill(0x20);
  out.set(raw);
  return out;
}

function concat(parts) {
  const total = parts.reduce((s, p) => s + p.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

function u32s(values) {
  const out = new Uint8Array(values.length * 4);
  const view = new DataView(out.buffer);
  values.forEach((v, i) => view.setUint32(i * 4, v >>> 0, true));
  return out;
}

const fakeGlb = (n = 12) => {
  const out = new Uint8Array(n);
  out.set(text.encode('glTF').subarray(0, Math.min(4, n)));
  for (let i = 4; i < n; i++) out[i] = (i * 7) & 0xff;
  return out;
};

/** A tile: magic, version, byteLength, the header words, then the sections. */
function tile(magic, {version = 1, words, sections, byteLength, prefix = 0}) {
  const body = concat(sections);
  const headerLength = 12 + words.length * 4;
  const total = headerLength + body.length;
  const header = concat([text.encode(magic), u32s([version, byteLength ?? total, ...words])]);
  const bytes = concat([new Uint8Array(prefix).fill(0xee), header, body]);
  return {bytes: Array.from(bytes), byteOffset: prefix};
}

function view(v, arrayBuffer) {
  if (v === undefined) return null;
  const copied = v.buffer !== arrayBuffer;
  return {copied, byteOffset: copied ? null : v.byteOffset, bytes: Array.from(v)};
}

function capture(parser) {
  const warnings = [];
  const original = parser._deprecationWarning;
  parser._deprecationWarning = (id) => warnings.push(id);
  return {
    warnings,
    restore: () => {
      parser._deprecationWarning = original;
    },
  };
}

function b3dmCases() {
  const s = {module: 'B3dmParser', cases: []};
  const ft = {BATCH_LENGTH: 3, RTC_CENTER: [1, 2, 3]};
  const bt = {height: [1, 2, 3], id: {byteOffset: 0, componentType: 'UNSIGNED_SHORT', type: 'SCALAR'}};
  const btBin = new Uint8Array([1, 0, 2, 0, 3, 0, 0, 0]);
  const scenarios = {
    standard: tile('b3dm', {words: [jsonBytes(ft).length, 8, jsonBytes(bt).length, 8], sections: [jsonBytes(ft), new Uint8Array(8).fill(9), jsonBytes(bt), btBin, fakeGlb(16)]}),
    noBatchTable: tile('b3dm', {words: [jsonBytes(ft).length, 0, 0, 0], sections: [jsonBytes(ft), fakeGlb(12)]}),
    batchJsonOnly: tile('b3dm', {words: [jsonBytes(ft).length, 0, jsonBytes(bt).length, 0], sections: [jsonBytes(ft), jsonBytes(bt), fakeGlb(20)]}),
    noFeatureTable: tile('b3dm', {words: [0, 0, 0, 0], sections: [fakeGlb(8)]}),
    legacy1: tile('b3dm', {words: [7, jsonBytes(bt, 4).length], sections: [jsonBytes(bt, 4), fakeGlb(12)]}),
    legacy2: tile('b3dm', {words: [jsonBytes(bt, 4).length, 8, 5], sections: [jsonBytes(bt, 4), btBin, fakeGlb(12)]}),
    unaligned: tile('b3dm', {words: [jsonBytes(ft, 1).length, 0, 0, 0], sections: [jsonBytes(ft, 1), fakeGlb(10)]}),
    prefixed: tile('b3dm', {words: [jsonBytes(ft).length, 0, 0, 0], sections: [jsonBytes(ft), fakeGlb(16)], prefix: 12}),
    version2: tile('b3dm', {version: 2, words: [0, 0, 0, 0], sections: [fakeGlb(8)]}),
    emptyGlb: tile('b3dm', {words: [jsonBytes(ft).length, 0, 0, 0], sections: [jsonBytes(ft)]}),
    truncated: {bytes: Array.from(concat([text.encode('b3dm'), u32s([1, 40])])), byteOffset: 0},
    shortByteLength: tile('b3dm', {words: [jsonBytes(ft).length, 0, 0, 0], sections: [jsonBytes(ft), fakeGlb(8)], byteLength: 20}),
    shortByteLengthUnaligned: tile('b3dm', {words: [jsonBytes(ft, 1).length, 0, 0, 0], sections: [jsonBytes(ft, 1), fakeGlb(8)], byteLength: 20}),
    badJson: tile('b3dm', {words: [8, 0, 0, 0], sections: [jsonBytes('{"BATCH', 8), fakeGlb(8)]}),
    sectionPastEnd: tile('b3dm', {words: [jsonBytes(ft).length, 64, 0, 0], sections: [jsonBytes(ft), fakeGlb(8)]}),
    // byteLength stops 4 bytes before the end of the buffer.
    trailingBytes: tile('b3dm', {words: [jsonBytes(ft).length, 0, 0, 0], sections: [jsonBytes(ft), fakeGlb(12), new Uint8Array(4).fill(1)], byteLength: 28 + jsonBytes(ft).length + 12}),
  };
  for (const [name, {bytes, byteOffset}] of Object.entries(scenarios)) {
    const buffer = Uint8Array.from(bytes).buffer;
    const c = capture(B3dmParser);
    let r;
    try {
      const p = B3dmParser.parse(buffer, byteOffset);
      r = {batchLength: p.batchLength ?? null, featureTableJson: p.featureTableJson, featureTableBinary: view(p.featureTableBinary, buffer), batchTableJson: p.batchTableJson ?? null, batchTableBinary: p.batchTableBinary ? {copied: p.batchTableBinary.buffer !== buffer, bytes: Array.from(p.batchTableBinary)} : null, gltf: view(p.gltf, buffer), warnings: c.warnings};
      r = enc(r);
    } catch (e) {
      r = {throws: String(e.message), kind: e.name};
    } finally {
      c.restore();
    }
    s.cases.push({f: name, a: [bytes, byteOffset], r});
  }
  return s;
}

function i3dmCases() {
  const s = {module: 'I3dmParser', cases: []};
  const ft = {INSTANCES_LENGTH: 2, POSITION: {byteOffset: 0}};
  const ftBin = new Uint8Array(24).fill(3);
  const bt = {name: ['a', 'b']};
  const uri = text.encode('models/tree.glb');
  const scenarios = {
    embedded: tile('i3dm', {words: [jsonBytes(ft).length, 24, jsonBytes(bt).length, 8, 1], sections: [jsonBytes(ft), ftBin, jsonBytes(bt), new Uint8Array(8).fill(5), fakeGlb(12)]}),
    uri: tile('i3dm', {words: [jsonBytes(ft).length, 24, 0, 0, 0], sections: [jsonBytes(ft), ftBin, uri]}),
    batchJsonOnly: tile('i3dm', {words: [jsonBytes(ft).length, 24, jsonBytes(bt).length, 0, 1], sections: [jsonBytes(ft), ftBin, jsonBytes(bt), fakeGlb(8)]}),
    unaligned: tile('i3dm', {words: [jsonBytes(ft, 1).length, 0, 0, 0, 0], sections: [jsonBytes(ft, 1), uri]}),
    prefixed: tile('i3dm', {words: [jsonBytes(ft).length, 24, 0, 0, 1], sections: [jsonBytes(ft), ftBin, fakeGlb(8)], prefix: 8}),
    version0: tile('i3dm', {version: 0, words: [8, 0, 0, 0, 1], sections: [jsonBytes(ft), fakeGlb(8)]}),
    noFeatureTable: tile('i3dm', {words: [0, 0, 0, 0, 1], sections: [fakeGlb(8)]}),
    format2: tile('i3dm', {words: [jsonBytes(ft).length, 0, 0, 0, 2], sections: [jsonBytes(ft), fakeGlb(8)]}),
    emptyGlb: tile('i3dm', {words: [jsonBytes(ft).length, 24, 0, 0, 1], sections: [jsonBytes(ft), ftBin]}),
    truncated: {bytes: Array.from(concat([text.encode('i3dm'), u32s([1, 40, 16, 0])])), byteOffset: 0},
  };
  for (const [name, {bytes, byteOffset}] of Object.entries(scenarios)) {
    const buffer = Uint8Array.from(bytes).buffer;
    const c = capture(I3dmParser);
    let r;
    try {
      const p = I3dmParser.parse(buffer, byteOffset);
      r = enc({gltfFormat: p.gltfFormat, featureTableJson: p.featureTableJson, featureTableBinary: view(p.featureTableBinary, buffer), batchTableJson: p.batchTableJson ?? null, batchTableBinary: p.batchTableBinary ? {copied: p.batchTableBinary.buffer !== buffer, bytes: Array.from(p.batchTableBinary)} : null, gltf: view(p.gltf, buffer), warnings: c.warnings});
    } catch (e) {
      r = {throws: String(e.message), kind: e.name};
    } finally {
      c.restore();
    }
    s.cases.push({f: name, a: [bytes, byteOffset], r});
  }
  return s;
}

/** Binary body builder: appends typed data at aligned offsets and returns the JSON reference. */
function body() {
  const parts = [];
  let length = 0;
  return {
    add(array, align = 4) {
      const pad = (align - (length % align)) % align;
      if (pad) {
        parts.push(new Uint8Array(pad));
        length += pad;
      }
      const bytes = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
      const byteOffset = length;
      parts.push(Uint8Array.from(bytes));
      length += bytes.length;
      return byteOffset;
    },
    bytes() {
      const pad = (8 - (length % 8)) % 8;
      return concat([...parts, new Uint8Array(pad)]);
    },
  };
}

function pnts({ft, ftBin = new Uint8Array(0), bt = null, btBin = new Uint8Array(0), version = 1, ftPad = 8}) {
  const ftJson = jsonBytes(ft, ftPad);
  const btJson = bt === null ? new Uint8Array(0) : jsonBytes(bt);
  return tile('pnts', {version, words: [ftJson.length, ftBin.length, btJson.length, btBin.length], sections: [ftJson, ftBin, btJson, btBin]});
}

function pntsCases() {
  const s = {module: 'PntsParser', cases: []};
  const n = 4;
  const positions = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0.5]);
  const scenarios = {};
  {
    const b = body();
    const POSITION = {byteOffset: b.add(positions)};
    const RGB = {byteOffset: b.add(new Uint8Array([255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9]), 1)};
    const NORMAL = {byteOffset: b.add(new Float32Array([0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, -1]))};
    scenarios.floatRgbNormal = pnts({ft: {POINTS_LENGTH: n, POSITION, RGB, NORMAL}, ftBin: b.bytes()});
  }
  {
    const b = body();
    const POSITION_QUANTIZED = {byteOffset: b.add(new Uint16Array([0, 0, 0, 65535, 0, 0, 0, 65535, 0, 100, 200, 300]))};
    const RGBA = {byteOffset: b.add(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]))};
    const NORMAL_OCT16P = {byteOffset: b.add(new Uint8Array([0, 0, 255, 255, 128, 128, 7, 9]))};
    const BATCH_ID = {byteOffset: b.add(new Uint8Array([0, 1, 1, 0])), componentType: 'UNSIGNED_BYTE'};
    const btb = body();
    const temperature = {byteOffset: btb.add(new Float32Array([20, 21])), componentType: 'FLOAT', type: 'SCALAR'};
    scenarios.quantizedRgbaOct = pnts({
      ft: {POINTS_LENGTH: n, POSITION_QUANTIZED, QUANTIZED_VOLUME_SCALE: [10, 20, 30], QUANTIZED_VOLUME_OFFSET: [-5, -5, 0], RGBA, NORMAL_OCT16P, BATCH_ID, BATCH_LENGTH: 2},
      ftBin: b.bytes(),
      bt: {temperature, name: ['a', 'b']},
      btBin: btb.bytes(),
    });
  }
  {
    const b = body();
    const POSITION = {byteOffset: b.add(positions)};
    const RGB565 = {byteOffset: b.add(new Uint16Array([0xf800, 0x07e0, 0x001f, 0xffff]))};
    const BATCH_ID = {byteOffset: b.add(new Uint16Array([3, 2, 1, 0]))};
    scenarios.rgb565RtcBatch = pnts({ft: {POINTS_LENGTH: n, RTC_CENTER: [1000, 2000, -3000.5], POSITION, RGB565, BATCH_ID, BATCH_LENGTH: 4}, ftBin: b.bytes()});
  }
  for (const [name, rgba] of [['constantTranslucent', [255, 0, 0, 128]], ['constantOpaque', [10, 20, 30, 255]]]) {
    const b = body();
    scenarios[name] = pnts({ft: {POINTS_LENGTH: n, POSITION: {byteOffset: b.add(positions)}, CONSTANT_RGBA: rgba}, ftBin: b.bytes()});
  }
  {
    const b = body();
    const POSITION = {byteOffset: b.add(positions)};
    const CONSTANT_RGBA = {byteOffset: b.add(new Uint8Array([9, 8, 7, 6]))};
    const RTC_CENTER = {byteOffset: b.add(new Float32Array([1.5, 2.5, 3.5]))};
    const POINTS_LENGTH = {byteOffset: b.add(new Uint32Array([n]))};
    const BATCH_ID = {byteOffset: b.add(new Uint32Array([0, 0, 1, 1])), componentType: 'UNSIGNED_INT'};
    const BATCH_LENGTH = {byteOffset: b.add(new Uint32Array([2]))};
    scenarios.binaryGlobals = pnts({ft: {POINTS_LENGTH, POSITION, CONSTANT_RGBA, RTC_CENTER, BATCH_ID, BATCH_LENGTH}, ftBin: b.bytes()});
  }
  scenarios.jsonArrays = pnts({ft: {POINTS_LENGTH: 2, POSITION: [1, 2, 3, 4, 5, 6.5], RGB: [300, -1, 7.9, 1, 2, 3], NORMAL: 3, BATCH_ID: [1, 70000], BATCH_LENGTH: 1}});
  {
    const draco = new Uint8Array([68, 82, 65, 67, 79, 2, 2, 0, 0, 0, 0, 1, 2, 3, 4, 5]);
    const b = body();
    const off = b.add(draco, 1);
    const btb = body();
    const extra = {byteOffset: btb.add(new Float32Array([1, 2, 3, 4]))};
    const dracoFt = {POINTS_LENGTH: n, extensions: {'3DTILES_draco_point_compression': {properties: {POSITION: 0, RGB: 1, BATCH_ID: 3}, byteOffset: off, byteLength: draco.length}}, BATCH_LENGTH: 3};
    const dracoBt = {temperature: {byteOffset: 0, componentType: 'FLOAT', type: 'SCALAR'}, missing: {byteOffset: 8, componentType: 'FLOAT', type: 'SCALAR'}, names: ['x'], extensions: {'3DTILES_draco_point_compression': {properties: {temperature: 2}}}};
    scenarios.dracoWithoutBinary = pnts({ft: dracoFt, ftBin: b.bytes(), bt: dracoBt});
    scenarios.dracoWithBinary = pnts({ft: dracoFt, ftBin: b.bytes(), bt: dracoBt, btBin: btb.bytes()});
    scenarios.dracoPositionsOnly = pnts({ft: {POINTS_LENGTH: n, NORMAL: [0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1], RGBA: [1, 2, 3, 4], extensions: {'3DTILES_draco_point_compression': {properties: {POSITION: 0}, byteOffset: off + 2, byteLength: 100}}}, ftBin: b.bytes()});
    scenarios.dracoMissingLength = pnts({ft: {POINTS_LENGTH: n, extensions: {'3DTILES_draco_point_compression': {properties: {POSITION: 0}, byteOffset: 0}}}, ftBin: b.bytes()});
    scenarios.dracoColorsTranslucent = pnts({ft: {POINTS_LENGTH: n, extensions: {'3DTILES_draco_point_compression': {properties: {POSITION: 0, RGBA: 1}, byteOffset: -4, byteLength: 4}}}, ftBin: b.bytes()});
  }
  {
    const b = body();
    const POSITION = {byteOffset: b.add(positions)};
    const bin = b.bytes();
    scenarios.emptyBatchTable = pnts({ft: {POINTS_LENGTH: n, POSITION}, ftBin: bin, bt: {}});
    scenarios.batchTableNoBinary = pnts({ft: {POINTS_LENGTH: n, POSITION}, ftBin: bin, bt: {a: {byteOffset: 0, componentType: 'FLOAT', type: 'SCALAR'}, b: [1, 2, 3, 4], 3: {byteOffset: 4}, c: 5}});
    scenarios.batchTableNullProperty = pnts({ft: {POINTS_LENGTH: n, POSITION}, ftBin: bin, bt: {a: null}});
    scenarios.noPointsLength = pnts({ft: {POSITION}, ftBin: bin});
    scenarios.noPositions = pnts({ft: {POINTS_LENGTH: n, RGB: [1, 2, 3]}, ftBin: bin});
    scenarios.quantizedNoScale = pnts({ft: {POINTS_LENGTH: 1, POSITION_QUANTIZED: [1, 2, 3], QUANTIZED_VOLUME_OFFSET: [0, 0, 0]}});
    scenarios.quantizedNoOffset = pnts({ft: {POINTS_LENGTH: 1, POSITION_QUANTIZED: [1, 2, 3], QUANTIZED_VOLUME_SCALE: [1, 1, 1]}});
    scenarios.batchIdNoLength = pnts({ft: {POINTS_LENGTH: n, POSITION, BATCH_ID: [0, 0, 0, 0]}, ftBin: bin});
    scenarios.version2 = pnts({ft: {POINTS_LENGTH: n, POSITION}, ftBin: bin, version: 2});
    scenarios.misaligned = pnts({ft: {POINTS_LENGTH: 2, POSITION: {byteOffset: 2}}, ftBin: bin});
    scenarios.pastEnd = pnts({ft: {POINTS_LENGTH: 100, POSITION: {byteOffset: 0}}, ftBin: bin});
    scenarios.noFeatureTable = tile('pnts', {words: [0, 0, 0, 0], sections: []});
  }
  for (const [name, {bytes, byteOffset}] of Object.entries(scenarios)) {
    const buffer = Uint8Array.from(bytes).buffer;
    let r;
    try {
      const p = PntsParser.parse(buffer, byteOffset);
      const attribute = (a) => a === undefined ? null : {
        name: a.name, semantic: a.semantic, setIndex: a.setIndex ?? null, typedArray: typed(a.typedArray), constantColor: a.constantColor ? [a.constantColor.red, a.constantColor.green, a.constantColor.blue, a.constantColor.alpha] : null,
        componentDatatype: a.componentDatatype, type: a.type, normalized: a.normalized ?? null, isQuantized: a.isQuantized ?? null, isRGB565: a.isRGB565 ?? null, isTranslucent: a.isTranslucent ?? null,
        octEncoded: a.octEncoded ?? null, octEncodedZXY: a.octEncodedZXY ?? null, quantizedRange: a.quantizedRange ?? null, quantizedVolumeOffset: a.quantizedVolumeOffset ?? null, quantizedVolumeScale: a.quantizedVolumeScale ?? null,
        quantizedComponentDatatype: a.quantizedComponentDatatype ?? null, quantizedType: a.quantizedType ?? null,
      };
      r = enc({
        draco: p.draco ? {buffer: Array.from(p.draco.buffer), featureTableProperties: p.draco.featureTableProperties, batchTableProperties: p.draco.batchTableProperties ?? null, properties: p.draco.properties, dequantizeInShader: p.draco.dequantizeInShader} : null,
        hasPositions: p.hasPositions, hasColors: p.hasColors, hasNormals: p.hasNormals, hasBatchIds: p.hasBatchIds, isTranslucent: p.isTranslucent, hasConstantColor: p.hasConstantColor ?? null,
        rtcCenter: p.rtcCenter ?? null, pointsLength: typed(p.pointsLength),
        positions: attribute(p.positions), normals: attribute(p.normals), colors: attribute(p.colors), batchIds: attribute(p.batchIds),
        batchLength: typed(p.batchLength), batchTableJson: p.batchTableJson ?? null, batchTableBinary: p.batchTableBinary ? Array.from(p.batchTableBinary) : null,
      });
    } catch (e) {
      r = {throws: String(e.message), kind: e.name};
    }
    s.cases.push({f: name, a: [bytes, byteOffset], r});
  }
  return s;
}

function featureTableCases() {
  const s = {module: 'Cesium3DTileFeatureTable', cases: []};
  const b = body();
  b.add(new Uint8Array([0xaa, 0xbb, 0xcc]), 1);
  const floats = b.add(new Float32Array([1.5, -2, 3, 4, 5, 6]));
  const shorts = b.add(new Uint16Array([10, 20, 30, 40]));
  const global = b.add(new Uint32Array([77]));
  const bin = b.bytes();
  const json = {
    POINTS_LENGTH: 2, LIST: [1, 2.5, -3, 300], NUMBER: 3, OBJECT: {a: 1}, STRING: 'x',
    FLOATS: {byteOffset: floats}, SHORTS: {byteOffset: shorts, componentType: 'UNSIGNED_SHORT'}, GLOBAL: {byteOffset: global}, NULL: null,
    ODD: {byteOffset: floats + 2}, FAR: {byteOffset: 4096}, BYTES: {byteOffset: 1, componentType: 'UNSIGNED_BYTE'},
  };
  const T = ComponentDatatype;
  const sequences = [
    [['global', 'POINTS_LENGTH'], ['global', 'GLOBAL'], ['global', 'GLOBAL', T.FLOAT, 1], ['global', 'MISSING'], ['global', 'NULL'], ['global', 'LIST'], ['global', 'FLOATS', T.FLOAT, 3]],
    [['has', 'LIST'], ['has', 'NULL'], ['has', 'MISSING'], ['has', 'POINTS_LENGTH']],
    [['array', 'LIST', T.FLOAT, 1], ['array', 'LIST', T.UNSIGNED_BYTE, 1], ['array', 'NUMBER', T.SHORT, 1], ['array', 'OBJECT', T.FLOAT, 1], ['array', 'STRING', T.INT, 1], ['array', 'MISSING', T.FLOAT, 1]],
    [['array', 'FLOATS', T.FLOAT, 3], ['array', 'SHORTS', T.FLOAT, 2], ['array', 'BYTES', T.FLOAT, 3], ['array', 'FLOATS', T.UNSIGNED_BYTE, 1]],
    [['property', 'FLOATS', T.FLOAT, 3, 0], ['property', 'FLOATS', T.FLOAT, 3, 1], ['property', 'FLOATS', T.FLOAT, 3, 2], ['property', 'SHORTS', T.UNSIGNED_SHORT, 1, 3], ['property', 'SHORTS', T.UNSIGNED_SHORT, 1, 9], ['property', 'LIST', T.DOUBLE, 2, 1], ['property', 'MISSING', T.FLOAT, 1, 0]],
    [['array', 'ODD', T.FLOAT, 1]],
    [['array', 'FAR', T.FLOAT, 1]],
    [['global', 'ODD', T.FLOAT, 1]],
    [['featuresLength', 0.5], ['array', 'FLOATS', T.FLOAT, 3]],
  ];
  for (const ops of sequences) {
    let r;
    try {
      const table = new Cesium3DTileFeatureTable(json, new Uint8Array(Uint8Array.from(concat([new Uint8Array(8), bin])).buffer, 8, bin.length));
      table.featuresLength = 2;
      const out = [];
      for (const [op, semantic, type, length, id] of ops) {
        if (op === 'featuresLength') {
          table.featuresLength = semantic;
          out.push(null);
        } else if (op === 'global') out.push(typed(table.getGlobalProperty(semantic, type, length)));
        else if (op === 'has') out.push(table.hasProperty(semantic));
        else if (op === 'array') out.push(typed(table.getPropertyArray(semantic, type, length)));
        else out.push(enc(table.getProperty(semantic, type, length, id, new Array(length).fill(-1))));
      }
      r = out;
    } catch (e) {
      r = {throws: String(e.message), kind: e.name};
    }
    s.cases.push({f: 'sequence', a: [json, Array.from(bin), ops], r});
  }
  return s;
}

export default function tileFormats() {
  return [featureTableCases(), b3dmCases(), i3dmCases(), pntsCases()];
}
