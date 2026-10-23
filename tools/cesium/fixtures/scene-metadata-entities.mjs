// Scene 3D Metadata entities: tileset, tile, content, group and implicit
// subtree metadata, Cesium3DTilesetMetadata and the find*Metadata helpers,
// evaluated by the pinned CesiumJS engine against schemaJson of
// scene-metadata.mjs.
//
// Entity cases run a script of operations, each recorded as `{ ok: value }`
// or `{ throws: errorName }`.
import { suite } from "./lib.mjs";
import { val, unval, schemaJson } from "./scene-metadata.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;

function attempt(fn) {
  try {
    return { ok: fn() };
  } catch (e) {
    return { throws: e.name ?? "Error" };
  }
}

const treeProperties = { height: 10, count: 255, signed: 100, big: 5, position: [1, 2, 3], name: "", names: ["a", "b", "c"], species: "Oak", unknown: 1, optional: null };

export default async function sceneMetadataEntities() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  const MetadataSchema = await scene("MetadataSchema");
  const schema = MetadataSchema.fromJson(schemaJson);
  const ctors = {
    GroupMetadata: [await scene("GroupMetadata"), "group"],
    TilesetMetadata: [await scene("TilesetMetadata"), "tileset"],
    TileMetadata: [await scene("TileMetadata"), "tile"],
    ContentMetadata: [await scene("ContentMetadata"), "content"],
    ImplicitSubtreeMetadata: [await scene("ImplicitSubtreeMetadata"), "subtreeMetadata"],
  };
  const entityOps = () => {
    const ops = [["info"], ["ids"]];
    for (const p of ["height", "count", "position", "name", "species", "legacyFloat", "a", "unknown", "optional", "missing"]) ops.push(["has", p], ["get", p]);
    for (const sem of ["HEIGHT", "ID", "NAME", "NONE"]) ops.push(["hasSem", sem], ["getSem", sem]);
    const writes = [["height", 21], ["count", 0.5], ["position", { $Cartesian3: [2, 3, 4] }], ["name", "new"], ["missing", 1], ["a", "z"], ["legacyFloat", 1]];
    for (const [p, v] of writes) ops.push(["set", p, v], ["get", p]);
    for (const [sem, v] of [["HEIGHT", 5], ["NAME", "x"], ["NONE", 1]]) ops.push(["setSem", sem, v], ["getSem", sem]);
    ops.push(["ids"]);
    return ops;
  };
  const entities = [
    ["tree", { properties: treeProperties, extras: { e: 1 }, extensions: { X: {} } }],
    ["tree", { properties: { count: 10, species: "Birch" } }],
    ["legacy", { properties: { legacyFloat: 2.5 } }],
    ["semantics", { properties: { a: "first", b: "second", c: 3 } }],
    ["empty", { properties: {} }],
  ];
  for (const [module, [Ctor, key]] of Object.entries(ctors)) {
    const s = add(module);
    for (const [cls, json] of entities) {
      s.add("script", [cls, json, entityOps()], (c, j, script) => {
        const options = { [key]: structuredClone(j), class: schema.classes[c] };
        if (module === "GroupMetadata") options.id = "g";
        const e = new Ctor(options);
        return script.map((op) =>
          attempt(() => {
            switch (op[0]) {
              case "info": return [e.class.id, e.extras ?? null, e.extensions ?? null, module === "GroupMetadata" ? e.id : null];
              case "ids": return e.getPropertyIds();
              case "has": return e.hasProperty(op[1]);
              case "get": return val(e.getProperty(op[1]));
              case "hasSem": return e.hasPropertyBySemantic(op[1]);
              case "getSem": return val(e.getPropertyBySemantic(op[1]));
              case "set": return e.setProperty(op[1], unval(op[2]));
              case "setSem": return e.setPropertyBySemantic(op[1], unval(op[2]));
            }
          }),
        );
      });
    }
    // Without properties, the group and tileset start empty; a tile's or a
    // content's helpers fail their debug check.
    s.add("noProperties", ["tree", {}], (c, j) => {
      const e = new Ctor({ [key]: j, class: schema.classes[c] });
      return [e.getPropertyIds(), e.hasProperty("count"), val(e.getProperty("count"))];
    });
    s.add("noProperties", ["tree", 5], (c, j) => new Ctor({ [key]: j, class: schema.classes[c] }).class.id);
  }

  const Cesium3DTilesetMetadata = await scene("Cesium3DTilesetMetadata");
  const findGroupMetadata = await scene("findGroupMetadata");
  const findTileMetadata = await scene("findTileMetadata");
  const findContentMetadata = await scene("findContentMetadata");
  findTileMetadata._oneTimeWarning = () => {};
  findContentMetadata._oneTimeWarning = () => {};
  const describe = (e) => (e ? [e.class.id, e.getPropertyIds(), e.extras ?? null, e.extensions ?? null] : null);
  const metadataJsons = [
    {
      metadata: { class: "tree", properties: { count: 3, name: "tileset" } },
      groups: [{ class: "semantics", properties: { a: "g0" } }, { class: "tree", properties: { height: 2 }, extras: { x: 1 } }],
      statistics: { classes: { tree: { count: 1 } } }, extras: { e: 1 }, extensions: { X: {} },
    },
    { tileset: { class: "legacy", properties: { legacyFloat: 1 } }, groups: { b: { class: "tree" }, a: { class: "semantics", properties: { c: 1 } }, B: { class: "empty" } } },
    { metadata: null, tileset: { class: "tree" } },
    { groups: [] },
    {},
    { metadata: { class: "missing" } },
    { groups: [{ class: "tree" }, null] },
    { groups: [5] },
    { groups: "ab" },
    { groups: 7 },
    { metadata: 3 },
  ];
  {
    const s = add("Cesium3DTilesetMetadata");
    for (const json of metadataJsons) {
      s.add("new", [json], (j) => {
        const m = new Cesium3DTilesetMetadata({ metadataJson: j, schema });
        return [describe(m.tileset), m.groups.map((g) => [g.id ?? null, ...describe(g)]), m.groupIds, m.statistics ?? null, m.extras ?? null, m.extensions ?? null, m.schema.id];
      });
    }
  }
  {
    const s = add("findGroupMetadata");
    const headers = [{ group: 0 }, { group: 1 }, { group: 5 }, { group: 1.5 }, { group: -1 }, { group: "a" }, { group: "b" }, { group: "z" }, {}, { extensions: { "3DTILES_metadata": { group: "B" } } }, { extensions: { "3DTILES_metadata": { group: 0 } }, group: 1 }, { extensions: { "3DTILES_metadata": null }, group: 1 }];
    for (const json of [metadataJsons[0], metadataJsons[1], null]) {
      for (const header of headers) {
        s.add("find", [json, header], (j, h) => {
          const tileset = { metadataExtension: j ? new Cesium3DTilesetMetadata({ metadataJson: j, schema }) : undefined };
          const g = findGroupMetadata(tileset, h);
          return g ? [g.id ?? null, g.class.id, g.getPropertyIds()] : null;
        });
      }
    }
  }
  for (const [module, find] of [["findTileMetadata", findTileMetadata], ["findContentMetadata", findContentMetadata]]) {
    const s = add(module);
    const headers = [
      { metadata: { class: "tree", properties: { count: 1 }, extras: { a: 1 } } },
      { extensions: { "3DTILES_metadata": { class: "semantics", properties: { a: "x" }, extensions: { E: 1 } } }, metadata: { class: "tree" } },
      { metadata: {} }, { metadata: null }, {}, { metadata: { class: "missing" } }, { metadata: { class: null } }, { metadata: 5 },
    ];
    for (const withSchema of [true, false]) {
      for (const header of headers) {
        s.add("find", [withSchema, header], (w, h) => describe(find(w ? { schema } : {}, h)));
      }
    }
  }
  return out;
}
