// Scene structural metadata: property textures, StructuralMetadata,
// getMetadataProperty and the three parsers (EXT_structural_metadata,
// EXT_feature_metadata and 3D Tiles 1.0 batch tables), evaluated by the
// pinned CesiumJS engine.
//
// Textures are stand-ins named by glTF index. Texture.create is stubbed to
// return the packed property table source, so cases record exactly what
// would be uploaded; ContextLimits.maximumTextureSize is set per case.
// Warnings are recorded from console.warn and the parsers' test hooks.
import { suite } from "./lib.mjs";
import { val } from "./scene-metadata.mjs";
import { tableSchemaJson, encode, makeViews } from "./scene-metadata-tables.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;
const renderer = async (name) => (await import(`@cesium/engine/Source/Renderer/${name}.js`)).default;

function attempt(fn) {
  try {
    return { ok: fn() };
  } catch (e) {
    return { throws: e.name ?? "Error" };
  }
}

export const structuralSchemaJson = {
  id: "structural",
  enums: {
    kind: { valueType: "UINT8", values: [{ name: "A", value: 1 }, { name: "B", value: 7 }] },
  },
  classes: {
    building: {
      properties: {
        height: { type: "SCALAR", componentType: "FLOAT32", noData: -1 },
        floors: { type: "SCALAR", componentType: "UINT8", noData: 255, default: 1 },
        id16: { type: "SCALAR", componentType: "INT16" },
        kind: { type: "ENUM", enumType: "kind", noData: "B" },
        color: { type: "VEC3", componentType: "UINT8", noData: [1, 2, 3] },
        uv: { type: "VEC2", componentType: "UINT16", noData: [258, 772] },
        pair: { type: "SCALAR", componentType: "UINT8", array: true, count: 2, noData: [5, 6] },
        name: { type: "STRING" },
        big: { type: "SCALAR", componentType: "INT64" },
        f64: { type: "SCALAR", componentType: "FLOAT64" },
        tag: { type: "SCALAR", componentType: "UINT32" },
        flag: { type: "BOOLEAN" },
        vecs: { type: "VEC2", componentType: "UINT8", array: true, count: 2, noData: [[9, 8], [7, 6]] },
      },
    },
    tree: {
      properties: {
        age: { type: "SCALAR", componentType: "UINT16", noData: 0 },
        species: { type: "ENUM", enumType: "kind" },
      },
    },
    sensor: {
      properties: {
        temp: { type: "SCALAR", componentType: "FLOAT32", offset: 1, scale: 2 },
        rgb: { type: "VEC3", componentType: "UINT8", normalized: true },
        level: { type: "SCALAR", componentType: "UINT8", noData: 3 },
      },
    },
    labels: { properties: { name: { type: "STRING" }, big: { type: "SCALAR", componentType: "INT64" } } },
  },
};

const buildingRows = {
  height: [10.5, -1, 3.25], floors: [1, 255, 30], id16: [-5, 0, 300], kind: ["A", "B", "A"], color: [[1, 2, 3], [255, 0, 7], [9, 9, 9]],
  uv: [[1, 65535], [258, 772], [0, 0]], pair: [[1, 2], [5, 6], [0, 255]], name: ["a", "bb", ""], big: [{ $bigint: "-1" }, 0, 5],
  f64: [0.5, 1, 2], tag: [4294967295, 0, 7], flag: [true, false, true], vecs: [[[1, 2], [3, 4]], [[9, 8], [7, 6]], [[0, 0], [1, 1]]],
};

// Property JSON and buffer views for `rows` of `classId`, from buffer view `base`.
function columns(schema, classId, rows, base = 0, opts = {}) {
  const cls = schema.classes[classId];
  const properties = {};
  const views = {};
  let next = base;
  for (const [id, r] of Object.entries(rows)) {
    const e = encode(cls.properties[id], r, { base: next, legacy: opts.legacy, pad: opts.pad?.[id] ?? 0, mutate: opts.mutate?.[id] });
    properties[id] = e.property;
    Object.assign(views, e.views);
    next += 3;
  }
  return { properties, views };
}

const pick = (rows, ids) => Object.fromEntries(ids.map((id) => [id, rows[id]]));

// The reader's scale is a glTF normal texture scale; a metadata property's
// vector scale shares its key, and Rust reads only numbers.
const readerInfo = (r) => [r.index ?? null, r.texture?.name ?? null, r.texCoord, r.transform ? Array.from(r.transform) : null, typeof r.scale === "number" ? r.scale : null, r.channels ?? null];
const textureProperty = (p) => [readerInfo(p.textureReader), p.hasValueTransform, val(p.offset), val(p.scale), p.classProperty.id, p.extras ?? null, p.extensions ?? null];
const propertyTexture = (t) => [t.id ?? null, t.name ?? null, t.class.id, Object.fromEntries(Object.entries(t.properties).map(([k, p]) => [k, textureProperty(p)])), t.extras ?? null, t.extensions ?? null];
const attributeProperty = (p) => [p.attribute ?? null, p.hasValueTransform, val(p.offset), val(p.scale), p.classProperty.id];
const propertyAttribute = (a) => [a.id ?? null, a.name ?? null, a.class.id, Object.fromEntries(Object.entries(a.properties).map(([k, p]) => [k, attributeProperty(p)]))];
const propertyTable = (t) => {
  const rows = [];
  for (let i = 0; i < t.count; i++) {
    const ids = t.getPropertyIds(i);
    rows.push(Object.fromEntries(ids.map((id) => [id, attempt(() => val(t.getProperty(i, id)))])));
  }
  return {
    id: t.id ?? null, name: t.name ?? null, count: t.count, byteLength: t.byteLength, class: t.class?.id ?? null,
    properties: t.properties ? Object.keys(t.properties) : null, extras: t.extras ?? null, extensions: t.extensions ?? null,
    texture: t.texture ?? null, rows,
  };
};
const classInfo = (c) => Object.entries(c.properties).map(([id, p]) => [id, p.type, p.componentType ?? null, p.name ?? null]);
const summary = (m) => ({
  schema: Object.fromEntries(Object.entries(m.schema.classes).map(([id, c]) => [id, classInfo(c)])),
  propertyTableCount: m.propertyTableCount,
  propertyTablesByteLength: m.propertyTablesByteLength,
  propertyTables: m.propertyTables.map(propertyTable),
  propertyTextures: m.propertyTextures.map(propertyTexture),
  propertyAttributes: m.propertyAttributes.map(propertyAttribute),
  statistics: m.statistics ?? null, extras: m.extras ?? null, extensions: m.extensions ?? null,
});

const textures = (names) => Object.fromEntries(names.map((i) => [i, { name: `t${i}` }]));

export default async function sceneStructuralMetadata() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  const MetadataSchema = await scene("MetadataSchema");
  const PropertyTextureProperty = await scene("PropertyTextureProperty");
  const PropertyTexture = await scene("PropertyTexture");
  const PropertyAttribute = await scene("PropertyAttribute");
  const StructuralMetadata = await scene("StructuralMetadata");
  const getMetadataProperty = await scene("getMetadataProperty");
  const parseStructuralMetadata = await scene("parseStructuralMetadata");
  const parseFeatureMetadataLegacy = await scene("parseFeatureMetadataLegacy");
  const parseBatchTable = await scene("parseBatchTable");
  const Texture = await renderer("Texture");
  const ContextLimits = await renderer("ContextLimits");
  const PixelFormat = (await import("@cesium/engine/Source/Core/PixelFormat.js")).default;
  const PixelDatatype = await renderer("PixelDatatype");
  const TextureMinificationFilter = await renderer("TextureMinificationFilter");
  const TextureMagnificationFilter = await renderer("TextureMagnificationFilter");
  const TextureWrap = await renderer("TextureWrap");

  const tableSchema = MetadataSchema.fromJson(tableSchemaJson);
  const classT = tableSchema.classes.t;
  const schema = MetadataSchema.fromJson(structuralSchemaJson);

  // ---- PropertyTextureProperty ----
  {
    const s = add("PropertyTextureProperty");
    const cases = [
      ["f32", { index: 0 }],
      ["f64t", { index: 2, texCoord: 1, channels: [1] }],
      ["f64t", { index: 0, channels: [0, 1, 2, 3], offset: 5, scale: null }],
      ["vec3", { index: 2, channels: [2, 1, 0], offset: [1, 1, 1], scale: [2, 2, 2], extras: { x: 1 }, extensions: { E: 1 } }],
      ["vec2n", { index: 1, channels: [3, 4, -1, 0.5, "1", null, 2.9] }],
      ["u8n", { index: 0, scale: 2, extensions: { KHR_texture_transform: { offset: [0.5, 0.25], rotation: 0.5, scale: [2, 3], texCoord: 2 } } }],
      ["mat2", { index: 0, offset: [1, 2, 3, 4] }],
      ["f32", {}],
      ["f32", { index: 0, channels: [] }],
      ["f32", { index: 0, channels: "rg" }],
      ["str", { index: 0, offset: "x" }],
      ["fixed", { index: 2, channels: [0, 1, 2] }],
    ];
    for (const [id, json] of cases) {
      s.add("new", [id, json, [0, 2]], (i, j, names) =>
        textureProperty(new PropertyTextureProperty({ property: structuredClone(j), classProperty: classT.properties[i], textures: textures(names) })));
    }
  }

  // ---- PropertyTexture ----
  {
    const s = add("PropertyTexture");
    const cases = [
      [{ class: "t", properties: { f32: { index: 0 }, vec3: { index: 2, channels: [0, 1, 2] }, u8n: { index: 0, texCoord: 3 } }, extras: { a: 1 }, extensions: { E: {} } }, "pt", 3],
      [{ class: "t" }, null, null],
      [{ class: "t", properties: { missing: { index: 0 } } }, null, 0],
      [{ class: "t", properties: { f32: { index: 0, channels: 5 } } }, null, 0],
    ];
    for (const [json, name, id] of cases) {
      s.add("new", [json, name, id], (j, n, i) => {
        const t = new PropertyTexture({ propertyTexture: structuredClone(j), class: classT, textures: textures([0, 2]), name: n ?? undefined, id: i ?? undefined });
        return [...propertyTexture(t), t.getProperty("f32") ? "f32" : null, t.getProperty("nope") ?? null];
      });
    }
  }

  // Parse with stubbed texture creation and a context limit; returns the
  // summary and the warnings logged.
  const createTexture = Texture.create;
  const warn = console.warn;
  function withStubs(maximumTextureSize, fn) {
    const warnings = [];
    const previousSize = ContextLimits._maximumTextureSize;
    ContextLimits._maximumTextureSize = maximumTextureSize;
    Texture.create = (o) => ({
      width: o.source.width, height: o.source.height, bytes: Array.from(o.source.arrayBufferView), flipY: o.flipY,
      pixelFormat: o.pixelFormat === PixelFormat.RGBA, pixelDatatype: o.pixelDatatype === PixelDatatype.UNSIGNED_BYTE,
      nearest: o.sampler.minificationFilter === TextureMinificationFilter.NEAREST && o.sampler.magnificationFilter === TextureMagnificationFilter.NEAREST
        && o.sampler.wrapS === TextureWrap.CLAMP_TO_EDGE && o.sampler.wrapT === TextureWrap.CLAMP_TO_EDGE,
    });
    console.warn = (m) => warnings.push(String(m));
    try {
      return { result: fn(), warnings };
    } finally {
      Texture.create = createTexture;
      console.warn = warn;
      ContextLimits._maximumTextureSize = previousSize;
    }
  }

  // ---- parseStructuralMetadata ----
  const allBuilding = columns(schema, "building", buildingRows);
  const treeCols = columns(schema, "tree", { age: [1, 2], species: ["A", "B"] }, 100);
  const parseCases = [];
  {
    const views = { ...allBuilding.views, ...treeCols.views };
    parseCases.push({
      name: "all",
      extension: {
        propertyTables: [
          { name: "buildings", class: "building", count: 3, properties: allBuilding.properties, extras: { e: 1 }, extensions: { X: {} } },
          { class: "tree", count: 2, properties: treeCols.properties },
        ],
        propertyTextures: [
          { class: "sensor", name: "pt", properties: { temp: { index: 0, channels: [0] }, rgb: { index: 1, channels: [0, 1, 2], texCoord: 1 } } },
          { class: "tree", properties: { age: { index: 1, channels: [0, 1] } } },
        ],
        propertyAttributes: [{ class: "sensor", name: "pa", properties: { temp: { attribute: "_TEMP" }, level: { attribute: "_LEVEL", offset: 1 } } }],
        statistics: { classes: { building: { count: 3 } } }, extras: { x: 1 }, extensions: { Y: {} },
      },
      views, textures: [0, 1], max: 16384,
    });
  }
  {
    const c = columns(schema, "building", pick(buildingRows, ["kind", "pair", "tag", "vecs"]));
    parseCases.push({ name: "noData", extension: { propertyTables: [{ name: "defaults", class: "building", count: 3, properties: c.properties }] }, views: c.views, max: 16384 });
  }
  {
    const c = columns(schema, "tree", { age: [5, 6, 7] });
    parseCases.push({ name: "enumMissing", extension: { propertyTables: [{ class: "tree", count: 3, properties: c.properties }] }, views: c.views, max: 16384 });
  }
  {
    const c = columns(schema, "building", pick(buildingRows, ["kind", "tag", "vecs"]));
    parseCases.push({ name: "arrayMissing", extension: { propertyTables: [{ name: "noPair", class: "building", count: 3, properties: c.properties }] }, views: c.views, max: 16384 });
  }
  {
    const c = columns(schema, "building", pick(buildingRows, ["kind", "pair", "tag", "vecs"]));
    c.views[6].buffer.push(0, 0, 0, 0);
    parseCases.push({ name: "longView", extension: { propertyTables: [{ name: "long", class: "building", count: 3, properties: c.properties }] }, views: c.views, max: 16384 });
  }
  {
    const c = columns(schema, "building", pick(buildingRows, ["kind", "pair", "tag", "vecs"]));
    c.properties.tag.values = 99;
    parseCases.push({ name: "missingView", extension: { propertyTables: [{ name: "gone", class: "building", count: 3, properties: c.properties }] }, views: c.views, max: 16384 });
  }
  {
    const c = columns(schema, "sensor", { temp: [1, 2, 3], rgb: [[1, 2, 3], [4, 5, 6], [7, 8, 9]], level: [4, 5, 6] }, 0, { pad: { temp: 4, rgb: 1, level: 3 } });
    parseCases.push({ name: "tooLarge", extension: { propertyTables: [{ name: "wide", class: "sensor", count: 3, properties: c.properties }] }, views: c.views, max: 2 });
    // Three properties fit in a maximum of 3; three features do not fit in 2.
    parseCases.push({ name: "tooManyFeatures", extension: { propertyTables: [{ name: "long", class: "sensor", count: 3, properties: c.properties }] }, views: c.views, max: 3 });
    const two = columns(schema, "sensor", { temp: [1, 2], rgb: [[1, 2, 3], [4, 5, 6]], level: [4, 5] });
    parseCases.push({ name: "tooManyProperties", extension: { propertyTables: [{ name: "tall", class: "sensor", count: 2, properties: two.properties }] }, views: two.views, max: 2 });
    const four = columns(schema, "sensor", { temp: [1, 2, 3, 4], rgb: [[1, 2, 3], [4, 5, 6], [7, 8, 9], [0, 0, 0]], level: [4, 5, 6, 7] });
    parseCases.push({ name: "featuresOverLimit", extension: { propertyTables: [{ name: "four", class: "sensor", count: 4, properties: four.properties }] }, views: four.views, max: 3 });
    parseCases.push({ name: "padded", extension: { propertyTables: [{ name: "wide", class: "sensor", count: 3, properties: c.properties }] }, views: c.views, max: 16384 });
  }
  {
    const c = columns(schema, "labels", { name: ["x", "y"] });
    parseCases.push({
      name: "noTextures",
      extension: { propertyTables: [{ class: "labels", count: 2, properties: c.properties }, { name: "bare", class: "sensor", count: 4 }] },
      views: c.views, max: 16384,
    });
  }
  parseCases.push({ name: "empty", extension: {}, views: {}, max: 16384 });
  parseCases.push({ name: "missingClass", extension: { propertyTextures: [{ class: "nope", properties: {} }] }, views: {}, max: 16384 });
  parseCases.push({ name: "zeroCount", extension: { propertyTables: [{ class: "sensor", count: 0, properties: {} }] }, views: {}, max: 16384 });
  {
    const s = add("parseStructuralMetadata");
    for (const pc of parseCases) {
      s.add(pc.name, [structuralSchemaJson, pc.extension, pc.views, pc.textures ?? [], pc.max], (schemaJson, extension, views, names, max) =>
        withStubs(max, () => summary(parseStructuralMetadata({
          extension: structuredClone(extension), schema: MetadataSchema.fromJson(schemaJson), bufferViews: makeViews(views), textures: textures(names), context: {},
        }))));
    }
  }

  // ---- StructuralMetadata ----
  {
    const s = add("StructuralMetadata");
    const tables = parseCases[0];
    s.add("full", [structuralSchemaJson, tables.extension, tables.views], (schemaJson, extension, views) =>
      withStubs(16384, () => {
        const sch = MetadataSchema.fromJson(schemaJson);
        const parsed = parseStructuralMetadata({ extension: structuredClone(extension), schema: sch, bufferViews: makeViews(views), textures: textures([0, 1]), context: {} });
        const m = new StructuralMetadata({
          schema: sch, propertyTables: parsed.propertyTables,
          propertyTextures: [new PropertyTexture({ propertyTexture: { properties: { temp: { index: 1 } } }, class: sch.classes.sensor, textures: textures([1]), name: "only", id: 0 })],
          propertyAttributes: [new PropertyAttribute({ propertyAttribute: { properties: { level: { attribute: "_L" } } }, class: sch.classes.sensor, name: "attrs", id: 4 })],
          statistics: { s: 1 }, extras: { e: 2 }, extensions: { X: 3 },
        });
        return [summary(m), m.getPropertyTable(0)?.name ?? null, m.getPropertyTable(1)?.id ?? null, m.getPropertyTable(5) ?? null,
          m.getPropertyTexture(0)?.name ?? null, m.getPropertyTexture(1) ?? null, m.getPropertyAttribute(0)?.name ?? null, m.getPropertyAttribute(2) ?? null];
      }));
    s.add("bare", [structuralSchemaJson], (schemaJson) => {
      const m = new StructuralMetadata({ schema: MetadataSchema.fromJson(schemaJson) });
      return [summary(m), m.getPropertyTable(0) ?? null];
    });
  }

  // ---- getMetadataProperty ----
  {
    const s = add("getMetadataProperty");
    const extension = {
      propertyTextures: [
        { class: "sensor", properties: { temp: { index: 0, channels: [1] } } },
        { class: "sensor", properties: { rgb: { index: 1, channels: [0, 1, 2] }, temp: { index: 1, channels: [3] } } },
        { class: "tree", properties: { age: { index: 0 } } },
      ],
    };
    const queries = [["sensor", "temp"], ["sensor", "rgb"], ["tree", "age"], ["tree", "temp"], ["nope", "temp"], ["sensor", "level"]];
    s.add("find", [structuralSchemaJson, extension, queries], (schemaJson, ext, qs) => {
      const m = parseStructuralMetadata({ extension: ext, schema: MetadataSchema.fromJson(schemaJson), bufferViews: {}, textures: textures([0, 1]), context: {} });
      return qs.map(([c, p]) => {
        const property = getMetadataProperty(m, c, p);
        return property ? [property.classProperty.id, property.textureReader.channels, property.textureReader.index] : null;
      });
    });
    s.add("undefined", [], () => getMetadataProperty(undefined, "sensor", "temp") ?? null);
  }

  // ---- parseFeatureMetadataLegacy ----
  {
    const s = add("parseFeatureMetadataLegacy");
    const legacySchemaJson = {
      classes: {
        tree: { properties: { height: { type: "FLOAT32" }, count: { type: "UINT8" }, name: { type: "STRING" } } },
        ortho: { properties: { vegetation: { type: "UINT8", normalized: true }, rgb: { type: "ARRAY", componentType: "UINT8", componentCount: 3 } } },
      },
    };
    const legacy = MetadataSchema.fromJson(legacySchemaJson);
    const trees = columns(legacy, "tree", { height: [1.5, 2, 3], count: [1, 2, 3], name: ["oak", "elm", "ash"] }, 0, { legacy: true });
    const first = columns(legacy, "tree", { height: [9] }, 50, { legacy: true });
    const extension = {
      featureTables: {
        trees: { class: "tree", count: 3, properties: trees.properties },
        a_first: { class: "tree", count: 1, properties: first.properties, extras: { x: 1 } },
        B: { class: "tree", count: 1, extensions: { E: {} } },
      },
      featureTextures: {
        veg: {
          class: "ortho",
          properties: {
            vegetation: { channels: "r", texture: { index: 0, texCoord: 0, extras: { a: 1 } }, extras: { b: 2 } },
            rgb: { channels: "gbx", texture: { index: 1, texCoord: 1, extensions: { KHR_texture_transform: { offset: [0.5, 0.5] } } }, extensions: { Q: {} } },
          },
        },
        Alpha: { class: "ortho", properties: { vegetation: { channels: ["g", "ba", 5, null], texture: { index: 1 } } } },
        "10": { class: "ortho" },
      },
      statistics: { s: 1 }, extras: { e: 1 }, extensions: { X: {} },
    };
    const views = { ...trees.views, ...first.views };
    s.add("parse", [legacySchemaJson, extension, views], (schemaJson, ext, v) =>
      summary(parseFeatureMetadataLegacy({ extension: structuredClone(ext), schema: MetadataSchema.fromJson(schemaJson), bufferViews: makeViews(v), textures: textures([0, 1]) })));
    s.add("parse", [legacySchemaJson, {}, {}], (schemaJson, ext, v) =>
      summary(parseFeatureMetadataLegacy({ extension: ext, schema: MetadataSchema.fromJson(schemaJson), bufferViews: makeViews(v), textures: {} })));
    s.add("parse", [legacySchemaJson, { featureTextures: { t: { class: "ortho", properties: { vegetation: { texture: { index: 0 } } } } } }, {}], (schemaJson, ext, v) =>
      summary(parseFeatureMetadataLegacy({ extension: ext, schema: MetadataSchema.fromJson(schemaJson), bufferViews: makeViews(v), textures: {} })));
  }

  // ---- parseBatchTable ----
  {
    const s = add("parseBatchTable");
    const TYPED = { Int8Array, Uint8Array, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array };
    const bytes = (Type, values) => Array.from(new Uint8Array(new Type(values).buffer));
    // Body at byte 8 of the tile buffer.
    const body = [
      ...bytes(Float32Array, [1, 2, 3, 4, 5, 6, 7, 8, 9]), ...bytes(Uint16Array, [10, 20, 65535]), ...bytes(Int8Array, [-1, 2, 3, -4, 5, -128]),
      ...bytes(Float64Array, [0.1, 0.2, 0.3]),
    ];
    const tile = [...bytes(Uint32Array, [7, 8]), ...body];
    const binary = {
      pos: { byteOffset: 0, componentType: "FLOAT", type: "VEC3" },
      id: { byteOffset: 36, componentType: "UNSIGNED_SHORT", type: "SCALAR" },
      b: { byteOffset: 42, componentType: "BYTE", type: "VEC2" },
      d: { byteOffset: 48, componentType: "DOUBLE", type: "SCALAR" },
    };
    const hierarchy = {
      classes: [{ name: "Wall", length: 2, instances: { color: ["white", "red"] } }, { name: "Building", length: 1, instances: { name: ["unit"] } }],
      instancesLength: 3, classIds: [0, 0, 1], parentCounts: [1, 1, 0], parentIds: [2, 2],
    };
    const cases = [
      ["json", { name: ["a", "b", "c"], height: [1, 2, 3] }, 3, null, false],
      ["binary", { ...binary, extras: { e: 1 }, extensions: { X: {} } }, 3, 8, false],
      ["mixed", { ...binary, label: ["x", "y", "z"], HIERARCHY: hierarchy }, 3, 8, false],
      ["hierarchy", { extensions: { "3DTILES_batch_table_hierarchy": hierarchy } }, 3, null, false],
      ["noBody", { pos: binary.pos }, 3, null, false],
      ["misaligned", { pos: { byteOffset: 2, componentType: "FLOAT", type: "SCALAR" } }, 3, 8, false],
      ["outOfRange", { pos: { byteOffset: 60, componentType: "FLOAT", type: "VEC3" } }, 3, 8, false],
      ["noOffset", { first: { componentType: "UNSIGNED_INT", type: "SCALAR" } }, 3, 8, false],
      ["stringOffset", { s: { byteOffset: "4", componentType: "UNSIGNED_INT", type: "SCALAR" } }, 3, 0, false],
      // 1 + "2" is "12".
      ["stringOffsetConcat", { s: { byteOffset: "2", componentType: "UNSIGNED_BYTE", type: "SCALAR" } }, 3, 1, false],
      ["empty", {}, 2, null, false],
      ["attributes", { "Temperature ℃": binary.pos, gl_Pos: { byteOffset: 36, componentType: "UNSIGNED_SHORT", type: "SCALAR" }, "": binary.b, "123abc": binary.d, "a b": binary.id, "a-b": binary.id, u: { byteOffset: 0, componentType: "UNSIGNED_INT", type: "SCALAR" }, names: ["p", "q", "r"] }, 3, 8, true],
      ["draco", { drc: { componentType: "INT", type: "SCALAR" }, f: { componentType: "FLOAT", type: "VEC2" } }, 2, null, true, { drc: ["Int32Array", [1, -2]], f: ["Float32Array", [0.5, 1, 1.5, 2]] }],
      ["dracoMissing", { drc: { componentType: "INT", type: "SCALAR" }, raw: { byteOffset: 0, componentType: "FLOAT", type: "SCALAR" } }, 2, null, true, { drc: ["Int32Array", [1, 2]] }],
      ["emptyAttributes", {}, 2, null, true],
      ["attributeHierarchy", { HIERARCHY: hierarchy }, 3, null, true],
    ];
    const hookDeprecation = parseBatchTable._deprecationWarning;
    const hookOneTime = parseBatchTable._oneTimeWarning;
    for (const [name, batchTable, count, bodyOffset, asAttributes, decoded] of cases) {
      s.add(name, [batchTable, count, bodyOffset === null ? null : tile, bodyOffset, asAttributes, decoded ?? null], (bt, n, buffer, offset, attrs, typed) => {
        const warnings = [];
        parseBatchTable._deprecationWarning = (id, m) => warnings.push([id, m]);
        parseBatchTable._oneTimeWarning = (id, m) => warnings.push([id, m]);
        try {
          const json = structuredClone(bt);
          for (const [id, [type, values]] of Object.entries(typed ?? {})) json[id].typedArray = new TYPED[type](values);
          const output = [];
          const m = parseBatchTable({
            count: n, batchTable: json, binaryBody: buffer ? new Uint8Array(new Uint8Array(buffer).buffer, offset) : undefined,
            parseAsPropertyAttributes: attrs, customAttributeOutput: attrs ? output : undefined,
          });
          const attributes = output.map((a) => [a.name, a.count, a.type, a.componentDatatype, a.typedArray.constructor.name, Array.from(a.typedArray)]);
          return { metadata: summary(m), attributes, warnings };
        } finally {
          parseBatchTable._deprecationWarning = hookDeprecation;
          parseBatchTable._oneTimeWarning = hookOneTime;
        }
      });
    }
  }
  return out;
}
