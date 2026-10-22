// Scene 3D Metadata: schemas, classes, properties, enums and the
// MetadataEntity helpers, evaluated by the pinned CesiumJS engine.
//
// Metadata values are JSON with tags for what JSON cannot hold:
// `{ "$bigint": "<decimal>" }`, `{ "$Cartesian3": [x, y, z] }` (and the other
// Cartesian and Matrix classes, matrices column-major) and `{ "$null": true }`.
// `undefined` is null.
import * as C from "@cesium/engine";
import { suite, enc } from "./lib.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;
const MATH = ["Cartesian2", "Cartesian3", "Cartesian4", "Matrix2", "Matrix3", "Matrix4"];

export function val(v) {
  if (v === undefined) return null;
  if (v === null) return { $null: true };
  if (typeof v === "bigint") return { $bigint: v.toString() };
  for (const name of MATH) if (v instanceof C[name]) return { [`$${name}`]: Array.from(C[name].pack(v, [])).map(enc) };
  if (Array.isArray(v) || ArrayBuffer.isView(v)) return Array.from(v).map(val);
  if (typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, val(x)]));
  return enc(v);
}
export function unval(v) {
  if (v === null) return undefined;
  if (Array.isArray(v)) return v.map(unval);
  if (typeof v === "object") {
    if ("$null" in v) return null;
    if ("$bigint" in v) return BigInt(v.$bigint);
    for (const name of MATH) if (`$${name}` in v) return C[name].unpack(v[`$${name}`]);
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, unval(x)]));
  }
  if (v === "NaN") return NaN;
  if (v === "Infinity") return Infinity;
  if (v === "-Infinity") return -Infinity;
  return v;
}

// A schema with 3D Metadata properties of every shape.
export const schemaJson = {
  id: "schema",
  name: "Schema",
  description: "All the shapes",
  version: "1.0.0",
  extras: { a: 1 },
  enums: {
    species: { valueType: "UINT8", values: [{ name: "Oak", value: 0 }, { name: "Pine", value: 1, description: "evergreen" }, { name: "Birch", value: 2 }] },
    color: { values: [{ name: "Red", value: 0 }, { name: "Blue", value: 3 }, { name: "Again", value: 3 }] },
    bigEnum: { valueType: "INT64", values: [{ name: "Neg", value: -2 }, { name: "Big", value: 1000 }] },
  },
  classes: {
    tree: {
      name: "Tree",
      description: "A tree",
      properties: {
        height: { type: "SCALAR", componentType: "FLOAT32", semantic: "HEIGHT", min: 0, max: 100, offset: 1, scale: 2, required: true },
        count: { type: "SCALAR", componentType: "UINT8", normalized: true, noData: 255, default: 7 },
        signed: { type: "SCALAR", componentType: "INT16", normalized: true, offset: -1, scale: 0.5 },
        big: { type: "SCALAR", componentType: "INT64", noData: 5, semantic: "ID" },
        ubig: { type: "SCALAR", componentType: "UINT64", normalized: true },
        position: { type: "VEC3", componentType: "FLOAT64", default: [1, 2, 3], offset: [1, 1, 1] },
        uv: { type: "VEC2", componentType: "UINT16", normalized: true, array: true, count: 2 },
        matrix: { type: "MAT2", componentType: "FLOAT32", scale: [2, 2, 2, 2] },
        matrices: { type: "MAT3", componentType: "INT8", array: true },
        name: { type: "STRING", noData: "", default: "unnamed" },
        names: { type: "STRING", array: true, count: 3 },
        flag: { type: "BOOLEAN" },
        flags: { type: "BOOLEAN", array: true },
        species: { type: "ENUM", enumType: "species", noData: "Oak", default: "Pine" },
        speciesList: { type: "ENUM", enumType: "species", array: true, count: 2 },
        bigSpecies: { type: "ENUM", enumType: "bigEnum" },
        values: { type: "SCALAR", componentType: "FLOAT32", array: true, count: 3, offset: [1, 2, 3], scale: [1, 1, 2], noData: [0, 0, 0] },
        varValues: { type: "SCALAR", componentType: "UINT32", array: true, offset: [5] },
        vec4: { type: "VEC4", componentType: "UINT8", normalized: true },
        mat4: { type: "MAT4", componentType: "FLOAT32", array: true, count: 1 },
        noComponent: { type: "SCALAR" },
        optional: { type: "SCALAR", componentType: "INT32", required: false, extras: { e: 1 }, extensions: { X: {} } },
      },
    },
    legacy: {
      properties: {
        legacyFloat: { type: "FLOAT32", optional: true },
        legacyArray: { type: "ARRAY", componentType: "UINT16", componentCount: 4, normalized: true },
        legacyVarArray: { type: "ARRAY", componentType: "STRING" },
        legacyEnumArray: { type: "ARRAY", componentType: "ENUM", enumType: "color" },
        legacyBool: { type: "BOOLEAN", optional: false },
        legacyComponent: { type: "UINT8", componentType: "INT32" },
        unknownLegacy: { type: "STRING" },
        defaultedLegacy: { type: "FLOAT64", default: 3 },
      },
    },
    empty: {},
    semantics: { properties: { a: { type: "STRING", semantic: "NAME" }, b: { type: "STRING", semantic: "NAME" }, c: { type: "SCALAR", componentType: "UINT8", semantic: "ID" } } },
  },
};

const VALUES = [
  undefined, { $null: true }, 0, 1, -1, 0.5, 2, 255, 300, -129, 1e10, NaN, Infinity, "s", "Pine", "Oak", "Nope", true, false,
  { $bigint: "5" }, { $bigint: "-9223372036854775809" }, [1, 2, 3], [0, 0, 0], [1, 2], [[1, 2], [3, 4]], ["a", "b", "c"], [true, false],
  ["Pine", "Birch"], ["Pine", "Nope"], [], { $Cartesian2: [1, 2] }, { $Cartesian3: [1, 2, 3] }, { $Cartesian4: [0.5, 0.25, 1, 0] },
  { $Matrix2: [1, 2, 3, 4] }, { $Matrix3: [1, 0, 0, 0, 1, 0, 0, 0, 1] }, [{ $Cartesian2: [0.5, 1] }, { $Cartesian2: [0, 0.25] }],
  [{ $Matrix4: Array.from({ length: 16 }, (_, i) => i) }], [255, 0], [32767, -32768], [65535, 0, 1, 2],
];

function numericComponents(v) {
  if (Array.isArray(v)) return v.every(numericComponents);
  for (const name of MATH) {
    if (v instanceof C[name]) return Array.from(C[name].pack(v, [])).every((x) => x === undefined || typeof x === "number");
  }
  return v === undefined || typeof v === "number" || typeof v !== "object";
}

function propertyInfo(p) {
  return {
    id: p.id, name: p.name ?? null, description: p.description ?? null, type: p.type, enumType: p.enumType?.id ?? null,
    componentType: p.componentType ?? null, valueType: p.valueType ?? null, isArray: p.isArray, isVariableLengthArray: p.isVariableLengthArray,
    arrayLength: p.arrayLength ?? null, normalized: p.normalized, max: val(p.max), min: val(p.min), noData: val(p.noData), default: val(p.default),
    required: p.required, semantic: p.semantic ?? null, hasValueTransform: p.hasValueTransform, offset: val(p.offset), scale: val(p.scale),
    extras: p.extras ?? null, extensions: p.extensions ?? null, isLegacyExtension: p._isLegacyExtension ?? null,
  };
}

export default async function sceneMetadata() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  const MetadataSchema = await scene("MetadataSchema");
  const MetadataEnum = await scene("MetadataEnum");
  const MetadataEnumValue = await scene("MetadataEnumValue");
  const MetadataClass = await scene("MetadataClass");
  const MetadataClassProperty = await scene("MetadataClassProperty");
  const MetadataEntity = await scene("MetadataEntity");
  const getMetadataClassProperty = await scene("getMetadataClassProperty");
  const schema = MetadataSchema.fromJson(schemaJson);

  {
    const s = add("MetadataEnumValue");
    for (const json of [{ name: "A", value: 1 }, { name: "B", value: -2.5, description: "d", extras: { x: [1] }, extensions: { E: {} } }, { name: "C" }, { value: 1 }]) {
      s.add("fromJson", [json], (j) => {
        const v = MetadataEnumValue.fromJson(j);
        return [v.value, v.name, v.description ?? null, v.extras ?? null, v.extensions ?? null];
      });
    }
  }
  {
    const s = add("MetadataEnum");
    for (const [id, json] of Object.entries(schemaJson.enums).concat([["named", { name: "N", description: "D", valueType: "NOPE", values: [{ name: "x", value: 10 }, { name: "x", value: 11 }], extras: 1 }], ["noValues", {}]])) {
      s.add("fromJson", [id, json], (id, j) => {
        const e = MetadataEnum.fromJson({ id, enum: j });
        return {
          id: e.id, values: e.values.map((v) => [v.name, v.value]), namesByValue: e.namesByValue, valuesByName: e.valuesByName,
          valueType: e.valueType, name: e.name ?? null, description: e.description ?? null, extras: e.extras ?? null,
        };
      });
    }
  }
  {
    const s = add("MetadataClassProperty");
    for (const [classId, cls] of Object.entries(schemaJson.classes)) {
      for (const [id] of Object.entries(cls.properties ?? {})) {
        const p = schema.classes[classId].properties[id];
        const args = [classId, id];
        s.add("info", args, () => propertyInfo(p));
        for (const nested of [false, true]) s.add("expandConstant", [...args, nested], (c, i, n) => val(p.expandConstant(3, n)));
        s.add("bytesPerElement", args, () => p.bytesPerElement());
        for (const channels of [1, 2, 3, 4, NaN]) s.add("isGpuCompatible", [...args, channels], (c, i, ch) => p.isGpuCompatible(ch));
        s.add("getGlslType", args, () => p.getGlslType() ?? null);
        s.add("getGlslTypeWebGL1", args, () => p.getGlslTypeWebGL1() ?? null);
        for (const channels of ["r", "rg", "rgb", "rgba"]) s.add("unpackTextureInShader", [...args, channels], (c, i, ch) => {
          const lines = [];
          const name = p.unpackTextureInShader("texture(u_tex, v_uv)", ch, "metadata_x", lines);
          return [name, lines];
        });
        s.add("unpackTextureInShaderWebGL1", args, () => p.unpackTextureInShaderWebGL1("texture(u_tex, v_uv).r"));
        for (const v of VALUES) {
          s.add("validate", [...args, v], (c, i, x) => p.validate(unval(x)) ?? null);
          s.add("handleNoData", [...args, v], (c, i, x) => val(p.handleNoData(unval(x))));
          // The value methods take a defined value; with undefined they
          // throw a DeveloperError or return NaN or undefined by accident.
          if (v === undefined) continue;
          for (const f of ["normalize", "unnormalize", "applyValueTransform", "unapplyValueTransform"]) {
            s.add(f, [...args, v], (c, i, x) => val(p[f](unval(x))));
          }
          // Strings and booleans would land in Cartesian components, which
          // only numbers can be in the port.
          const textual = (x) => typeof x === "string" || typeof x === "boolean" || (Array.isArray(x) && x.some(textual));
          if (textual(v)) continue;
          for (const nested of [false, true]) {
            for (const f of ["unpackVectorAndMatrixTypes", "packVectorAndMatrixTypes"]) {
              // Skip results whose components are not numbers (an array read
              // as a component), which the port cannot hold.
              let r;
              try {
                r = p[f](unval(v), nested);
              } catch (e) {
                r = undefined;
              }
              if (!numericComponents(r)) continue;
              s.add(f, [...args, v, nested], (c, i, x, n) => val(p[f](unval(x), n)));
            }
          }
        }
      }
    }
    for (const json of [{ type: "SCALAR", componentType: "FLOAT32", array: true, count: 2.5 }, { type: "VEC2", componentType: "FLOAT32", array: true }, { type: "FOO" }, { type: "ENUM", enumType: "missing" }, { type: "SCALAR", componentType: "UINT8", normalized: 1 }, { type: "STRING", required: true, optional: false }]) {
      s.add("fromJson", [json], (j) => propertyInfo(MetadataClassProperty.fromJson({ id: "p", property: j, enums: schema.enums })));
    }
  }
  {
    const s = add("MetadataClass");
    for (const [id, json] of Object.entries(schemaJson.classes)) {
      s.add("fromJson", [id], (id) => {
        const c = schema.classes[id];
        return {
          id: c.id, name: c.name ?? null, description: c.description ?? null, properties: Object.keys(c.properties),
          propertiesBySemantic: Object.fromEntries(Object.entries(c.propertiesBySemantic).map(([k, p]) => [k, p.id])),
          extras: c.extras ?? null, extensions: c.extensions ?? null,
        };
      });
    }
    s.add("BATCH_TABLE_CLASS_NAME", [], () => MetadataClass.BATCH_TABLE_CLASS_NAME);
  }
  {
    const s = add("MetadataSchema");
    for (const json of [schemaJson, {}, { classes: { 2: {}, b: {}, 1: {} }, enums: null }, { id: "x", classes: { a: { properties: { p: { type: "ENUM", enumType: "e" } } } }, enums: { e: { values: [{ name: "v", value: 1 }] } } }]) {
      s.add("fromJson", [json], (j) => {
        const m = MetadataSchema.fromJson(j);
        return {
          id: m.id ?? null, name: m.name ?? null, description: m.description ?? null, version: m.version ?? null,
          classes: Object.keys(m.classes), enums: Object.keys(m.enums), extras: m.extras ?? null, extensions: m.extensions ?? null,
        };
      });
    }
  }
  {
    const s = add("getMetadataClassProperty");
    for (const [schemaId, className, propertyName] of [[null, "tree", "height"], ["schema", "tree", "height"], ["other", "tree", "height"], [null, "nope", "height"], [null, "tree", "nope"], [null, "empty", "x"], [null, "legacy", "legacyArray"]]) {
      s.add("get", [schemaId, className, propertyName], (sid, c, p) => getMetadataClassProperty(schema, sid ?? undefined, c, p)?.id ?? null);
    }
    s.add("get", ["noSchema", "tree", "height"], () => getMetadataClassProperty(undefined, undefined, "tree", "height") ?? null);
  }
  {
    const s = add("MetadataEntity");
    const entities = [
      ["tree", { height: 10, count: 255, signed: 100, big: 5, position: [1, 2, 3], uv: [1000, 2000, 65535, 0], matrix: [1, 2, 3, 4], name: "", names: ["a", "b", "c"], flag: true, species: "Oak", speciesList: ["Pine", "Birch"], values: [0, 0, 0], varValues: [1, 2], unknown: 1, optional: null }],
      ["tree", { count: 10, species: "Birch", values: [1, 2, 3], vec4: [255, 0, 128, 64], mat4: [Array.from({ length: 16 }, (_, i) => i)] }],
      ["legacy", { legacyFloat: 2.5, legacyArray: [65535, 0, 1, 2] }],
      ["semantics", { a: "first", b: "second", c: 3 }],
      ["empty", { a: 1 }],
    ];
    for (const [classId, properties] of entities) {
      const cls = schema.classes[classId];
      const ids = [...Object.keys(cls.properties ?? {}), "unknown", "missing"];
      s.add("getPropertyIds", [classId, properties], (c, p) => MetadataEntity.getPropertyIds(p, cls));
      for (const id of ids) {
        s.add("hasProperty", [classId, properties, id], (c, p, i) => MetadataEntity.hasProperty(i, p, cls));
        s.add("getProperty", [classId, properties, id], (c, p, i) => val(MetadataEntity.getProperty(i, structuredClone(p), cls)));
      }
      for (const semantic of ["HEIGHT", "ID", "NAME", "NONE"]) {
        s.add("hasPropertyBySemantic", [classId, properties, semantic], (c, p, sem) => MetadataEntity.hasPropertyBySemantic(sem, p, cls));
        s.add("getPropertyBySemantic", [classId, properties, semantic], (c, p, sem) => val(MetadataEntity.getPropertyBySemantic(sem, structuredClone(p), cls)));
      }
      const writes = [
        ["height", 21], ["count", 0.5], ["signed", -1], ["position", { $Cartesian3: [2, 3, 4] }], ["uv", [{ $Cartesian2: [0.5, 1] }, { $Cartesian2: [0, 0.25] }]],
        ["matrix", { $Matrix2: [2, 4, 6, 8] }], ["name", "new"], ["values", [2, 4, 6]], ["unknown", 9], ["missing", 1], ["species", "Pine"],
        ["vec4", { $Cartesian4: [1, 0, 0.5, 0.25] }], ["mat4", [{ $Matrix4: Array.from({ length: 16 }, () => 1) }]], ["legacyArray", [1, 0, 0.5, 0]], ["c", 4],
      ];
      for (const [id, value] of writes) {
        s.add("setProperty", [classId, properties, id, value], (c, p, i, v) => {
          const copy = structuredClone(p);
          const result = MetadataEntity.setProperty(i, unval(v), copy, cls);
          return [result, val(copy)];
        });
      }
      for (const [semantic, value] of [["HEIGHT", 5], ["ID", { $bigint: "6" }], ["NAME", "x"], ["NONE", 1]]) {
        s.add("setPropertyBySemantic", [classId, properties, semantic, value], (c, p, sem, v) => {
          const copy = structuredClone(p);
          const result = MetadataEntity.setPropertyBySemantic(sem, unval(v), copy, cls);
          return [result, val(copy)];
        });
      }
    }
  }
  return out;
}
