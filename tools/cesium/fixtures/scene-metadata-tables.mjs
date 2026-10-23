// Scene 3D Metadata tables: binary and JSON property tables, the batch table
// hierarchy and property attributes, evaluated by the pinned CesiumJS engine.
//
// Each case builds one object and runs a script of operations on it; every
// operation records `{ ok: value }` or `{ throws: errorName }`, so state
// carries from one operation to the next. Values use the tags of
// scene-metadata.mjs. Buffer views are `{ buffer: [bytes], byteOffset }`.
import { suite } from "./lib.mjs";
import { val, unval } from "./scene-metadata.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;

const big = (s) => ({ $bigint: s });
const I64_MIN = big("-9223372036854775808");
const I64_MAX = big("9223372036854775807");
const U64_MAX = big("18446744073709551615");

export const tableSchemaJson = {
  id: "tables",
  enums: {
    species: { valueType: "UINT8", values: [{ name: "Oak", value: 0 }, { name: "Pine", value: 1 }, { name: "Birch", value: 2 }] },
    big: { valueType: "INT64", values: [{ name: "Neg", value: -2 }, { name: "Big", value: 1000 }] },
  },
  classes: {
    t: {
      properties: {
        f32: { type: "SCALAR", componentType: "FLOAT32", semantic: "HEIGHT" },
        f64t: { type: "SCALAR", componentType: "FLOAT64", offset: 1, scale: 2 },
        i8: { type: "SCALAR", componentType: "INT8", noData: -1, default: 42 },
        u8n: { type: "SCALAR", componentType: "UINT8", normalized: true, semantic: "N" },
        i16n: { type: "SCALAR", componentType: "INT16", normalized: true, offset: 1, scale: 2 },
        u16: { type: "SCALAR", componentType: "UINT16" },
        i32: { type: "SCALAR", componentType: "INT32" },
        u32: { type: "SCALAR", componentType: "UINT32" },
        i64: { type: "SCALAR", componentType: "INT64", semantic: "ID" },
        u64: { type: "SCALAR", componentType: "UINT64" },
        u64n: { type: "SCALAR", componentType: "UINT64", normalized: true },
        i64n: { type: "SCALAR", componentType: "INT64", normalized: true },
        vec3: { type: "VEC3", componentType: "FLOAT32", offset: [1, 2, 3] },
        vec2n: { type: "VEC2", componentType: "UINT8", normalized: true },
        mat2: { type: "MAT2", componentType: "FLOAT64" },
        fixed: { type: "SCALAR", componentType: "UINT16", array: true, count: 3, offset: [1, 2, 3] },
        fixedVec: { type: "VEC2", componentType: "FLOAT32", array: true, count: 2, offset: [[1, 1], [2, 2]], scale: [[2, 2], [1, 1]] },
        varArr: { type: "SCALAR", componentType: "INT32", array: true },
        varVec: { type: "VEC2", componentType: "UINT8", array: true },
        bool: { type: "BOOLEAN" },
        boolArr: { type: "BOOLEAN", array: true, count: 3 },
        boolVar: { type: "BOOLEAN", array: true },
        str: { type: "STRING", noData: "N/A", default: "none", semantic: "NAME" },
        strArr: { type: "STRING", array: true, count: 2 },
        strVar: { type: "STRING", array: true },
        enum: { type: "ENUM", enumType: "species" },
        enumArr: { type: "ENUM", enumType: "species", array: true, count: 2 },
        bigEnum: { type: "ENUM", enumType: "big" },
        enumDefault: { type: "ENUM", enumType: "species", noData: "Oak", default: "Pine" },
        vecDefault: { type: "VEC3", componentType: "FLOAT32", noData: [0, 0, 0], default: [9, 9, 9] },
        onlyDefault: { type: "SCALAR", componentType: "UINT8", default: 7 },
        onlyDefaultN: { type: "SCALAR", componentType: "UINT8", normalized: true, default: 255 },
      },
    },
    legacy: {
      properties: {
        legacyFloat: { type: "FLOAT32" },
        legacyArr: { type: "ARRAY", componentType: "UINT8", componentCount: 2 },
        legacyVar: { type: "ARRAY", componentType: "INT16" },
        legacyStr: { type: "STRING" },
        legacyStrVar: { type: "ARRAY", componentType: "STRING" },
      },
    },
  },
};

const TYPED = {
  INT8: Int8Array, UINT8: Uint8Array, INT16: Int16Array, UINT16: Uint16Array, INT32: Int32Array, UINT32: Uint32Array,
  INT64: BigInt64Array, UINT64: BigUint64Array, FLOAT32: Float32Array, FLOAT64: Float64Array,
};

function bytesOf(type, values) {
  const T = TYPED[type];
  const array = type.endsWith("64") && type !== "FLOAT64" ? T.from(values.map((v) => BigInt(v))) : T.from(values);
  return Array.from(new Uint8Array(array.buffer));
}

const view = (bytes, pad = 0) => ({ buffer: [...new Array(pad).fill(0), ...bytes], byteOffset: pad });

// The property JSON and buffer views of rows of a class property, with
// buffer view ids from `base`.
function encode(cp, rows, opts = {}) {
  const base = opts.base ?? 0;
  const legacy = opts.legacy ?? false;
  const elements = rows.map((r) => (cp.isArray ? r : [r]));
  const components = elements.flatMap((e) => e.flatMap((x) => (Array.isArray(x) ? x : [x])));
  const property = { ...(opts.extra ?? {}) };
  const views = {};
  let values;
  if (cp.type === "STRING") {
    const encoder = new TextEncoder();
    const parts = components.map((s) => Array.from(encoder.encode(s)));
    const offsets = [0];
    for (const p of parts) offsets.push(offsets[offsets.length - 1] + p.length);
    values = parts.flat();
    const type = opts.stringOffsetType ?? "UINT32";
    views[base + 2] = view(bytesOf(type, offsets));
    if (legacy) property.stringOffsetBufferView = base + 2;
    else property.stringOffsets = base + 2;
    if (opts.stringOffsetType) property[legacy ? "offsetType" : "stringOffsetType"] = type;
  } else if (cp.type === "BOOLEAN") {
    values = new Array(Math.ceil(components.length / 8)).fill(0);
    components.forEach((b, i) => {
      if (b) values[i >> 3] |= 1 << i % 8;
    });
  } else {
    const valueType = cp.valueType;
    const numbers = cp.enumType ? components.map((n) => cp.enumType.valuesByName[n] ?? n) : components;
    values = bytesOf(valueType, numbers.map((x) => (typeof x === "object" ? BigInt(x.$bigint) : x)));
  }
  views[base] = view(values, opts.pad ?? 0);
  if (legacy) property.bufferView = base;
  else property.values = base;
  if (cp.isVariableLengthArray) {
    const offsets = [0];
    for (const e of elements) offsets.push(offsets[offsets.length - 1] + e.length);
    const type = opts.arrayOffsetType ?? "UINT32";
    views[base + 1] = view(bytesOf(type, offsets));
    if (legacy) property.arrayOffsetBufferView = base + 1;
    else property.arrayOffsets = base + 1;
    if (opts.arrayOffsetType) property[legacy ? "offsetType" : "arrayOffsetType"] = type;
  }
  opts.mutate?.(property, views);
  return { property, views };
}

function makeViews(views) {
  const out = {};
  for (const [id, v] of Object.entries(views)) {
    const buffer = new Uint8Array(v.buffer).buffer;
    out[id] = new Uint8Array(buffer, v.byteOffset);
  }
  return out;
}

function attempt(fn) {
  try {
    return { ok: fn() };
  } catch (e) {
    return { throws: e.name ?? "Error" };
  }
}


export default async function sceneMetadataTables() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  const MetadataSchema = await scene("MetadataSchema");
  const MetadataTableProperty = await scene("MetadataTableProperty");
  const MetadataTable = await scene("MetadataTable");
  const JsonMetadataTable = await scene("JsonMetadataTable");
  const BatchTableHierarchy = await scene("BatchTableHierarchy");
  const PropertyTable = await scene("PropertyTable");
  const PropertyAttributeProperty = await scene("PropertyAttributeProperty");
  const PropertyAttribute = await scene("PropertyAttribute");
  const schema = MetadataSchema.fromJson(tableSchemaJson);
  const classT = schema.classes.t;

  // ---- MetadataTableProperty ----
  {
    const s = add("MetadataTableProperty");
    s.add("schema", [tableSchemaJson], () => Object.keys(schema.classes));
    const rows = {
      f32: [1.5, -2, 3.25], f64t: [0, 1, 2], i8: [-1, 5, -128], u8n: [0, 255, 128], i16n: [-32768, 0, 32767], u16: [0, 1, 65535],
      i32: [-5, 0, 2147483647], u32: [0, 4294967295, 7], i64: [I64_MIN, 0, big("9007199254740993")], u64: [U64_MAX, 0, 1],
      u64n: [U64_MAX, 0, big("9223372036854775808")], i64n: [I64_MIN, 0, I64_MAX], vec3: [[1, 2, 3], [4, 5, 6], [0, 0, 0]],
      vec2n: [[0, 255], [255, 0], [128, 128]], mat2: [[1, 2, 3, 4], [0, 0, 0, 0], [1, 0, 0, 1]], fixed: [[1, 2, 3], [4, 5, 6], [7, 8, 9]],
      fixedVec: [[[1, 2], [3, 4]], [[5, 6], [7, 8]], [[0, 0], [1, 1]]], varArr: [[1, 2], [], [3, 4, 5]], varVec: [[[1, 2], [3, 4]], [[5, 6]], []],
      bool: [true, false, true], boolArr: [[true, false, true], [false, false, false], [true, true, true]], boolVar: [[true], [false, true], []],
      str: ["a", "N/A", "ünï😀"], strArr: [["a", "b"], ["", ""], ["c", "d"]], strVar: [["x"], [], ["y", "z"]], enum: ["Oak", "Pine", "Birch"],
      enumArr: [["Oak", "Pine"], ["Birch", "Birch"], ["Pine", "Oak"]], bigEnum: ["Neg", "Big", "Neg"], enumDefault: ["Oak", "Pine", "Birch"],
      vecDefault: [[0, 0, 0], [1, 1, 1], [2, 2, 2]],
    };
    const writes = {
      f32: [[0, 2.5], [1, 1e40], [2, "x"]], f64t: [[0, 5]], i8: [[0, -1], [1, 127], [2, 200]], u8n: [[0, 0.5], [1, 1]], i16n: [[0, 0.25]],
      u16: [[0, 65535], [1, 1.5]], i32: [[2, -7]], u32: [[1, 3]], i64: [[0, I64_MAX], [1, 5], [2, 1.5]], u64: [[0, big("5")], [1, -1]],
      u64n: [[0, 1], [1, 0.5]], i64n: [[0, -1]], vec3: [[0, { $Cartesian3: [1, 1, 1] }], [1, [1, 2, 3]]], vec2n: [[0, { $Cartesian2: [0.5, 1] }]],
      mat2: [[0, { $Matrix2: [1, 2, 3, 4] }]], fixed: [[0, [4, 5, 6]], [1, [1, 2]]], fixedVec: [[0, [{ $Cartesian2: [1, 2] }, { $Cartesian2: [3, 4] }]]],
      varArr: [[0, [7, 8]], [1, [1, 2, 3, 4, 5]], [2, [9]]], varVec: [[0, [{ $Cartesian2: [9, 9] }, { $Cartesian2: [8, 8] }]]],
      bool: [[0, false], [1, true], [2, 1]], boolArr: [[0, [false, false, true]]], boolVar: [[0, [false]], [1, [true, true, true, true, true, true, true, true, true]]],
      str: [[0, "hello"], [1, "N/A"]], strArr: [[0, ["x", "y"]]], strVar: [[0, []]], enum: [[0, "Birch"], [1, "Nope"]], enumArr: [[0, ["Oak", "Oak"]]],
      bigEnum: [[0, "Big"]], enumDefault: [[0, "Oak"]], vecDefault: [[0, { $Cartesian3: [0, 0, 0] }], [1, { $Cartesian3: [3, 3, 3] }]],
    };
    const legacyRows = {
      legacyFloat: [0.5, 1, 2], legacyArr: [[1, 2], [3, 4], [5, 6]], legacyVar: [[-1], [2, 3], []], legacyStr: ["p", "q", "r"],
      legacyStrVar: [["a", "b"], [], ["c"]],
    };
    const cases = [];
    for (const [id, r] of Object.entries(rows)) cases.push({ cls: "t", id, rows: r, writes: writes[id] ?? [] });
    for (const [id, r] of Object.entries(legacyRows)) cases.push({ cls: "legacy", id, rows: r, writes: [[0, r[2]]], opts: { legacy: true } });
    cases.push(
      { cls: "t", id: "f32", rows: rows.f32, writes: [[0, 1]], opts: { extra: { offset: 10, scale: 3, extras: { a: 1 }, extensions: { E: {} } } } },
      { cls: "t", id: "vec3", rows: rows.vec3, writes: [[0, { $Cartesian3: [2, 2, 2] }]], opts: { extra: { offset: [[1], [2, 3]] } } },
      { cls: "t", id: "fixed", rows: rows.fixed, writes: [], opts: { extra: { scale: [2, 2, 2] } } },
      { cls: "t", id: "enum", rows: [0, 7, 2], writes: [[1, "Pine"]], raw: true },
      { cls: "t", id: "enumArr", rows: [[0, 9], [1, 2], [2, 2]], writes: [], raw: true },
      { cls: "t", id: "f32", rows: rows.f32, writes: [], opts: { pad: 1 } },
      { cls: "t", id: "f32", rows: rows.f32, writes: [], opts: { pad: 4 } },
      { cls: "t", id: "f32", rows: rows.f32, writes: [], opts: { mutate: (p, v) => (v[0].buffer = v[0].buffer.slice(0, 8)) } },
      { cls: "t", id: "f32", rows: rows.f32, writes: [], opts: { mutate: (p) => (p.values = 5) } },
      { cls: "t", id: "f32", rows: rows.f32, writes: [], opts: { mutate: (p, v) => (v[0].buffer = [...v[0].buffer, 0, 0, 128, 63]) } },
      { cls: "t", id: "varArr", rows: rows.varArr, writes: [[0, [1, 1]]], opts: { arrayOffsetType: "UINT8" } },
      { cls: "t", id: "varArr", rows: rows.varArr, writes: [], opts: { arrayOffsetType: "UINT16" } },
      { cls: "t", id: "varArr", rows: rows.varArr, writes: [], opts: { arrayOffsetType: "UINT64" } },
      { cls: "t", id: "varArr", rows: rows.varArr, writes: [], opts: { arrayOffsetType: "FLOAT32" } },
      { cls: "t", id: "varArr", rows: rows.varArr, writes: [], opts: { mutate: (p, v) => (v[1] = view(bytesOf("UINT32", [0, 2, 1, 3]))) } },
      { cls: "t", id: "varArr", rows: rows.varArr, writes: [], opts: { mutate: (p, v) => (v[1] = view(bytesOf("UINT32", [0, 2, 9, 5]))) } },
      { cls: "t", id: "str", rows: rows.str, writes: [[2, "zz"]], opts: { stringOffsetType: "UINT16" } },
      { cls: "t", id: "str", rows: rows.str, writes: [], opts: { stringOffsetType: "INT64" } },
      { cls: "t", id: "str", rows: rows.str, writes: [], opts: { mutate: (p, v) => (v[2] = view(bytesOf("UINT32", [0, 1, 4, 99]))) } },
      { cls: "t", id: "str", rows: rows.str, writes: [], opts: { mutate: (p, v) => (v[2] = view(bytesOf("UINT32", [0, 3, 1, 4]))) } },
      { cls: "t", id: "strVar", rows: rows.strVar, writes: [[1, ["new"]]], opts: { arrayOffsetType: "UINT16", stringOffsetType: "UINT8" } },
      { cls: "t", id: "boolVar", rows: rows.boolVar, writes: [[2, []]], opts: {} },
      { cls: "t", id: "i64", rows: rows.i64, writes: [], opts: { pad: 4 } },
      { cls: "t", id: "i64", rows: rows.i64, writes: [], opts: { pad: 8 } },
    );
    for (const c of cases) {
      const cp = schema.classes[c.cls].properties[c.id];
      let encoded;
      if (c.raw) {
        const flat = c.rows.flat();
        encoded = { property: { values: 0 }, views: { 0: view(bytesOf("UINT8", flat)) } };
      } else {
        encoded = encode(cp, c.rows.map(unval), c.opts);
      }
      const count = 3;
      const ops = [];
      for (let i = 0; i <= count; i++) ops.push(["get", i]);
      ops.push(["typedArray"], ["byteLength"], ["info"]);
      for (const [i, v] of c.writes) ops.push(["set", i, v], ["get", i]);
      ops.push(["set", count, c.writes[0]?.[1] ?? 0]);
      for (let i = 0; i < count; i++) ops.push(["get", i]);
      ops.push(["typedArray"]);
      s.add("script", [c.cls, c.id, count, encoded.property, encoded.views, ops], (cls, id, n, property, views, script) => {
        const p = new MetadataTableProperty({ count: n, property, classProperty: schema.classes[cls].properties[id], bufferViews: makeViews(views) });
        return script.map((op) =>
          attempt(() => {
            switch (op[0]) {
              case "get": return val(p.get(op[1]));
              case "set": return val(p.set(op[1], unval(op[2])));
              case "typedArray": return val(p.getTypedArray());
              case "byteLength": return p.byteLength;
              case "info": return [p.hasValueTransform, val(p.offset), val(p.scale), p.extras ?? null, p.extensions ?? null, p.classProperty.id];
            }
          }),
        );
      });
    }
    s.add("script", ["t", "f32", 0, { values: 0 }, { 0: view([]) }, []], (cls, id, n, property, views) =>
      new MetadataTableProperty({ count: n, property, classProperty: schema.classes[cls].properties[id], bufferViews: makeViews(views) }));
  }

  // ---- MetadataTable ----
  const tableColumns = (ids, rowsById) => {
    const properties = {};
    const views = {};
    ids.forEach((id, k) => {
      const e = encode(classT.properties[id], rowsById[id].map(unval), { base: k * 3 });
      properties[id] = e.property;
      Object.assign(views, e.views);
    });
    return { properties, views };
  };
  const tableRows = {
    f32: [1.5, -2, 3.25], i8: [-1, 5, 6], str: ["a", "N/A", "c"], vec3: [[1, 2, 3], [4, 5, 6], [0, 0, 0]], enum: ["Oak", "Pine", "Birch"],
    i64: [big("1"), big("2"), big("3")],
  };
  const buildTable = (spec) => new MetadataTable({ count: spec.count, properties: spec.properties, class: schema.classes[spec.cls], bufferViews: makeViews(spec.views) });
  const tableOp = (t, op) => {
    switch (op[0]) {
      case "has": return t.hasProperty(op[1]);
      case "hasSem": return t.hasPropertyBySemantic(op[1]);
      case "ids": return t.getPropertyIds();
      case "get": return val(t.getProperty(op[1], op[2]));
      case "set": return t.setProperty(op[1], op[2], unval(op[3]));
      case "getSem": return val(t.getPropertyBySemantic(op[1], op[2]));
      case "setSem": return t.setPropertyBySemantic(op[1], op[2], unval(op[3]));
      case "typed": return val(t.getPropertyTypedArray(op[1]));
      case "typedSem": return val(t.getPropertyTypedArrayBySemantic(op[1]));
      case "byteLength": return t.byteLength;
      case "count": return t.count;
      case "class": return t.class.id;
    }
  };
  const tableScript = () => {
    const ids = ["f32", "i8", "str", "vec3", "enum", "i64", "onlyDefault", "onlyDefaultN", "vecDefault", "u16", "missing"];
    const ops = [["ids"], ["byteLength"], ["count"], ["class"]];
    for (const id of ids) ops.push(["has", id], ["typed", id]);
    for (const sem of ["HEIGHT", "NAME", "ID", "N", "NONE"]) ops.push(["hasSem", sem], ["typedSem", sem], ["getSem", 1, sem]);
    for (let i = 0; i < 3; i++) for (const id of ids) ops.push(["get", i, id]);
    ops.push(["get", 3, "f32"], ["get", 3, "onlyDefault"]);
    ops.push(["set", 0, "f32", 9], ["set", 0, "onlyDefault", 1], ["set", 1, "str", "N/A"], ["set", 2, "vec3", { $Cartesian3: [1, 1, 1] }]);
    ops.push(["setSem", 0, "HEIGHT", 4], ["setSem", 0, "NAME", "named"], ["setSem", 0, "N", 1], ["setSem", 0, "NONE", 1], ["setSem", 0, "ID", big("7")]);
    for (let i = 0; i < 3; i++) for (const id of ids) ops.push(["get", i, id]);
    ops.push(["typed", "str"], ["typed", "f32"], ["byteLength"]);
    return ops;
  };
  {
    const s = add("MetadataTable");
    const specs = [
      { cls: "t", count: 3, ...tableColumns(["f32", "i8", "str", "vec3", "enum", "i64"], tableRows) },
      { cls: "t", count: 3, properties: undefined, views: {} },
      { cls: "t", count: 3, ...tableColumns(["f32"], tableRows), extraProperty: true },
      { cls: "t", count: 0, properties: {}, views: {} },
    ];
    for (const spec of specs) {
      if (spec.extraProperty) spec.properties.missing = { values: 0 };
      const ops = tableScript();
      s.add("script", [{ cls: spec.cls, count: spec.count, properties: spec.properties ?? null, views: spec.views }, ops], (sp, script) => {
        const t = buildTable({ ...sp, properties: sp.properties ?? undefined });
        return script.map((op) => attempt(() => tableOp(t, op)));
      });
    }
  }

  // ---- JsonMetadataTable ----
  const jsonOp = (t, op) => {
    switch (op[0]) {
      case "has": return t.hasProperty(op[1]);
      case "ids": return t.getPropertyIds();
      case "get": return val(t.getProperty(op[1], op[2]));
      case "set": return val(t.setProperty(op[1], op[2], unval(op[3])));
    }
  };
  {
    const s = add("JsonMetadataTable");
    const properties = { a: [1, 2, null], b: "str", c: { 0: 5, x: 1 }, d: null, e: 5, f: [[1, 2], { x: 1 }], g: [7], 3: [true, false, true], h: true };
    const names = ["a", "b", "c", "d", "e", "f", "g", "3", "h", "missing"];
    const ops = [["ids"]];
    for (const p of names) ops.push(["has", p]);
    for (let i = 0; i <= 3; i++) for (const p of names) ops.push(["get", i, p]);
    for (const p of names) ops.push(["set", 2, p, { $Cartesian3: [1, 2, 3] }], ["get", 2, p]);
    ops.push(["set", 1, "fresh", [1, 2]], ["get", 0, "fresh"], ["get", 1, "fresh"], ["set", 0, "a", { $null: true }], ["get", 0, "a"], ["has", "fresh"], ["ids"]);
    ops.push(["set", 3, "a", 1]);
    for (const [count, props] of [[3, properties], [1, {}], [0, {}], [2, [[1, 2]]]]) {
      s.add("script", [count, props, ops], (n, p, script) => {
        const t = new JsonMetadataTable({ count: n, properties: p });
        return script.map((op) => attempt(() => jsonOp(t, op)));
      });
    }
  }

  // ---- BatchTableHierarchy ----
  const hierarchyBase = () => ({
    classes: [
      { name: "Wall", length: 6, instances: { color: ["white", "red", "yellow", "gray", "brown", "black"] } },
      { name: "Building", length: 3, instances: { name: ["unit29", "unit20", "unit93"], address: ["100 Main St", "102 Main St", "104 Main St"] } },
      { name: "Owner", length: 3, instances: { type: ["city", "resident", "commercial"], id: [1120, 1250, 6445] } },
    ],
    instancesLength: 12,
    classIds: [0, 0, 0, 0, 0, 0, 1, 1, 1, 2, 2, 2],
    parentCounts: [1, 3, 2, 1, 1, 1, 1, 1, 1, 0, 0, 0],
    parentIds: [6, 6, 10, 11, 7, 11, 8, 8, 8, 9, 10, 11],
  });
  const binaryHierarchy = () => {
    const body = [];
    const put = (offset, bytes) => {
      while (body.length < offset) body.push(0);
      bytes.forEach((b, i) => (body[offset + i] = b));
    };
    const ext = hierarchyBase();
    put(0, bytesOf("UINT8", ext.classIds));
    put(12, bytesOf("UINT16", ext.parentCounts));
    put(36, bytesOf("UINT16", ext.parentIds));
    put(64, bytesOf("FLOAT32", [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5]));
    put(136, bytesOf("FLOAT64", [0.5, 1.5, 2.5]));
    put(160, bytesOf("FLOAT32", [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
    ext.classIds = { byteOffset: 0, componentType: "UNSIGNED_BYTE" };
    ext.parentCounts = { byteOffset: 12 };
    ext.parentIds = { byteOffset: 36, componentType: 5123 };
    ext.classes[0].instances.position = { byteOffset: 64, componentType: "FLOAT", type: "VEC3" };
    ext.classes[2].instances.score = { byteOffset: 136, componentType: "DOUBLE", type: "SCALAR" };
    ext.classes[1].instances.m = { byteOffset: 160, componentType: 5126, type: "MAT2" };
    return { ext, body: view(body, 8) };
  };
  const hierarchyCases = () => {
    const single = hierarchyBase();
    delete single.parentCounts;
    single.parentIds = [6, 6, 7, 8, 8, 8, 9, 10, 11, 9, 10, 11];
    const none = hierarchyBase();
    delete none.parentCounts;
    delete none.parentIds;
    const cycle = hierarchyBase();
    delete cycle.parentCounts;
    cycle.parentIds = [1, 0, 7, 8, 8, 8, 9, 10, 11, 9, 10, 11];
    const exceeds = hierarchyBase();
    exceeds.parentIds = [6, 6, 10, 11, 7, 11, 8, 8, 8, 9, 10, 40];
    const multiCycle = hierarchyBase();
    multiCycle.parentIds = [6, 6, 10, 11, 7, 11, 8, 8, 8, 9, 10, 1];
    multiCycle.parentCounts = [1, 3, 2, 1, 1, 1, 1, 1, 1, 0, 0, 1];
    const shortIds = hierarchyBase();
    shortIds.classIds = [0, 0, 0, 0, 0, 0, 1, 1, 1, 2, 2, 7];
    const nullColumn = hierarchyBase();
    nullColumn.classes[0].instances.bad = null;
    const noComponentType = binaryHierarchy();
    delete noComponentType.ext.classes[0].instances.position.componentType;
    const noType = binaryHierarchy();
    delete noType.ext.classes[0].instances.position.type;
    const unknownType = binaryHierarchy();
    unknownType.ext.classes[0].instances.position.type = "VEC9";
    const misaligned = binaryHierarchy();
    misaligned.ext.classes[2].instances.score.byteOffset = 137;
    const noClasses = hierarchyBase();
    delete noClasses.classes;
    const noClassIds = hierarchyBase();
    delete noClassIds.classIds;
    const nullLength = hierarchyBase();
    nullLength.instancesLength = -1;
    return [
      [hierarchyBase(), null], [single, null], [none, null], [binaryHierarchy().ext, binaryHierarchy().body], [binaryHierarchy().ext, null],
      [cycle, null], [exceeds, null], [multiCycle, null], [shortIds, null], [nullColumn, null], [noComponentType.ext, noComponentType.body],
      [noType.ext, noType.body], [unknownType.ext, unknownType.body], [misaligned.ext, misaligned.body], [noClasses, null], [noClassIds, null],
      [nullLength, null],
    ];
  };
  const hierarchyProps = ["color", "name", "address", "type", "id", "position", "score", "m", "missing"];
  const hierarchyOp = (h, op) => {
    switch (op[0]) {
      case "has": return h.hasProperty(op[1], op[2]);
      case "exists": return h.propertyExists(op[1]);
      case "ids": return h.getPropertyIds(op[1]);
      case "get": return val(h.getProperty(op[1], op[2]));
      case "set": return h.setProperty(op[1], op[2], unval(op[3]));
      case "isClass": return h.isClass(op[1], op[2]);
      case "className": return val(h.getClassName(op[1]));
      case "byteLength": return h.byteLength;
    }
  };
  const hierarchyScript = () => {
    const ops = [["byteLength"]];
    for (const p of hierarchyProps) ops.push(["exists", p]);
    for (let b = 0; b < 12; b++) {
      ops.push(["ids", b], ["className", b]);
      for (const p of hierarchyProps) ops.push(["has", b, p], ["get", b, p]);
      for (const n of ["Wall", "Building", "Owner", "None"]) ops.push(["isClass", b, n]);
    }
    ops.push(["get", 12, "color"], ["className", 12], ["has", 12, "color"]);
    const writes = [
      [0, "color", "blue"], [1, "name", "x"], [6, "name", "own"], [9, "id", 1], [0, "position", { $Cartesian3: [7, 8, 9] }], [9, "score", 4.25],
      [6, "m", { $Matrix2: [4, 3, 2, 1] }], [7, "m", 5], [0, "missing", 1], [9, "score", big("1")], [2, "position", { $null: true }],
    ];
    for (const [b, p, v] of writes) ops.push(["set", b, p, v], ["get", b, p]);
    ops.push(["get", 1, "name"], ["get", 0, "id"]);
    return ops;
  };
  {
    const s = add("BatchTableHierarchy");
    for (const [ext, body] of hierarchyCases()) {
      s.add("script", [ext, body, hierarchyScript()], (e, b, script) => {
        const h = new BatchTableHierarchy({ extension: structuredClone(e), binaryBody: b ? makeViews({ 0: b })[0] : undefined });
        return script.map((op) => attempt(() => hierarchyOp(h, op)));
      });
    }
  }

  // ---- PropertyTable ----
  {
    const s = add("PropertyTable");
    const table = { cls: "t", count: 3, ...tableColumns(["f32", "str", "i8"], tableRows) };
    const hierarchy = hierarchyBase();
    hierarchy.instancesLength = 3;
    hierarchy.classIds = [0, 1, 2];
    hierarchy.parentCounts = [1, 1, 0];
    hierarchy.parentIds = [1, 2];
    const specs = [
      { count: 3, name: "table", id: 2, extras: { e: 1 }, extensions: { X: {} }, table, json: { j: ["p", "q", "r"], f32: [9, 9, 9] }, hierarchy },
      { count: 3, table },
      { count: 3, json: { j: [1, null, 3] } },
      { count: 3, hierarchy },
      { count: 3 },
      { count: 0 },
    ];
    const props = ["f32", "str", "i8", "onlyDefault", "j", "color", "name", "type", "missing"];
    const ops = [["info"], ["byteLength"]];
    for (const p of props) ops.push(["exists", p], ["typed", p]);
    for (const sem of ["HEIGHT", "NAME", "NONE"]) ops.push(["existsSem", sem], ["hasSem", 0, sem], ["typedSem", sem], ["getSem", 1, sem]);
    for (let i = 0; i < 3; i++) {
      ops.push(["ids", i], ["exactName", i]);
      for (const p of props) ops.push(["has", i, p], ["get", i, p]);
      for (const n of ["Wall", "Building", "Owner"]) ops.push(["isClass", i, n], ["isExact", i, n]);
    }
    ops.push(["isClass", 3, "Wall"], ["exactName", 3]);
    const writes = [[0, "f32", 5], [1, "j", "new"], [2, "name", "hn"], [0, "created", 1], [1, "type", "t"], [2, "onlyDefault", 3], [0, "str", 4]];
    for (const [i, p, v] of writes) ops.push(["set", i, p, v], ["get", i, p]);
    ops.push(["setSem", 0, "NAME", "sem"], ["getSem", 0, "NAME"], ["setSem", 0, "NONE", 1], ["ids", 0], ["byteLength"]);
    for (const spec of specs) {
      s.add("script", [spec, ops], (sp, script) => {
        const t = new PropertyTable({
          count: sp.count, name: sp.name, id: sp.id, extras: sp.extras, extensions: sp.extensions,
          metadataTable: sp.table ? buildTable(sp.table) : undefined,
          jsonMetadataTable: sp.json ? new JsonMetadataTable({ count: sp.count, properties: sp.json }) : undefined,
          batchTableHierarchy: sp.hierarchy ? new BatchTableHierarchy({ extension: structuredClone(sp.hierarchy) }) : undefined,
        });
        return script.map((op) =>
          attempt(() => {
            switch (op[0]) {
              case "info": return [t.name ?? null, t.id ?? null, t.count, t.class?.id ?? null, t.properties ? Object.keys(t.properties) : null, t.extras ?? null, t.extensions ?? null];
              case "byteLength": return t.byteLength;
              case "has": return t.hasProperty(op[1], op[2]);
              case "hasSem": return t.hasPropertyBySemantic(op[1], op[2]);
              case "exists": return t.propertyExists(op[1]);
              case "existsSem": return t.propertyExistsBySemantic(op[1]);
              case "ids": return t.getPropertyIds(op[1]);
              case "get": return val(t.getProperty(op[1], op[2]));
              case "set": return val(t.setProperty(op[1], op[2], unval(op[3])));
              case "getSem": return val(t.getPropertyBySemantic(op[1], op[2]));
              case "setSem": return t.setPropertyBySemantic(op[1], op[2], unval(op[3]));
              case "typed": return val(t.getPropertyTypedArray(op[1]));
              case "typedSem": return val(t.getPropertyTypedArrayBySemantic(op[1]));
              case "isClass": return t.isClass(op[1], op[2]);
              case "isExact": return t.isExactClass(op[1], op[2]);
              case "exactName": return val(t.getExactClassName(op[1]));
            }
          }),
        );
      });
    }
  }

  // ---- PropertyAttributeProperty and PropertyAttribute ----
  const attributeInfo = (p) => [p.attribute ?? null, p.hasValueTransform, val(p.offset), val(p.scale), p.classProperty.id, p.extras ?? null, p.extensions ?? null];
  {
    // No array-of-vector property: Cesium unpacks it into Cartesians whose
    // components are arrays, which the spec rules out for attributes.
    const s = add("PropertyAttributeProperty");
    const cases = [
      ["f32", { attribute: "_A" }], ["f64t", { attribute: "_B" }], ["f64t", { attribute: "_B", offset: 5, scale: null }], ["vec3", { attribute: "_C" }],
      ["vec3", { attribute: "_C", scale: [2, 2, 2], extras: { x: 1 }, extensions: { E: 1 } }], ["mat2", { offset: [1, 2, 3, 4], scale: [1, 1, 1, 1] }],
      ["fixed", {}], ["vecDefault", { offset: [1, 2] }], ["vec2n", { offset: 3 }], ["str", { offset: "x" }],
      ["f32", null], ["f32", 5],
    ];
    for (const [id, json] of cases) {
      s.add("new", [id, json], (i, j) => attributeInfo(new PropertyAttributeProperty({ property: j, classProperty: classT.properties[i] })));
    }
  }
  {
    const s = add("PropertyAttribute");
    const cases = [
      [{ class: "t", properties: { f32: { attribute: "_A" }, vec3: { attribute: "_V", offset: [0, 0, 0] } }, extras: { a: 1 }, extensions: { E: {} } }, "pa", 4],
      [{ class: "t" }, null, null],
      [{ class: "t", properties: { missing: { attribute: "_M" } } }, null, 0],
      [{ class: "t", properties: { f32: null } }, null, 0],
    ];
    for (const [json, name, id] of cases) {
      s.add("new", [json, name, id], (j, n, i) => {
        const pa = new PropertyAttribute({ propertyAttribute: j, class: classT, name: n ?? undefined, id: i ?? undefined });
        const properties = Object.fromEntries(Object.entries(pa.properties).map(([k, p]) => [k, attributeInfo(p)]));
        return [pa.name ?? null, pa.id ?? null, pa.class.id, properties, pa.extras ?? null, pa.extensions ?? null, pa.getProperty("f32") ? "f32" : null, pa.getProperty("nope") ?? null];
      });
    }
  }
  // ---- ImplicitMetadataView ----
  {
    const s = add("ImplicitMetadataView");
    const ImplicitMetadataView = await scene("ImplicitMetadataView");
    const spec = { cls: "t", count: 3, ...tableColumns(["f32", "str", "i8"], tableRows) };
    const ops = [["info", 0], ["info", 2], ["ids", 0]];
    for (const k of [0, 1, 2]) {
      for (const id of ["f32", "str", "i8", "onlyDefault", "u16"]) ops.push(["has", k, id], ["get", k, id]);
      for (const sem of ["HEIGHT", "NAME", "NONE"]) ops.push(["hasSem", k, sem], ["getSem", k, sem]);
    }
    ops.push(["set", 0, "f32", 8], ["get", 0, "f32"], ["get", 2, "f32"], ["set", 2, "str", "two"], ["getSem", 2, "NAME"], ["get", 0, "str"]);
    ops.push(["setSem", 1, "HEIGHT", 4], ["get", 1, "f32"], ["setSem", 1, "NONE", 1], ["set", 1, "u16", 1], ["get", 3, "f32"]);
    for (const json of [{ extras: { a: 1 }, extensions: { E: {} } }, {}, null]) {
      s.add("script", [spec, json, ops], (sp, j, script) => {
        const table = buildTable(sp);
        const views = [0, 1, 2, 3].map((entityId) => new ImplicitMetadataView({ metadataTable: table, class: classT, entityId, propertyTableJson: j }));
        return script.map((op) =>
          attempt(() => {
            const v = views[op[1]];
            switch (op[0]) {
              case "info": return [v.class.id, v.extras ?? null, v.extensions ?? null];
              case "ids": return v.getPropertyIds();
              case "has": return v.hasProperty(op[2]);
              case "get": return val(v.getProperty(op[2]));
              case "hasSem": return v.hasPropertyBySemantic(op[2]);
              case "getSem": return val(v.getPropertyBySemantic(op[2]));
              case "set": return v.setProperty(op[2], unval(op[3]));
              case "setSem": return v.setPropertyBySemantic(op[2], unval(op[3]));
            }
          }),
        );
      });
    }
  }
  return out;
}
