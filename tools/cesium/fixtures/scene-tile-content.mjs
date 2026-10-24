// preprocess3DTileContent, TileReplacementQueue and Cesium3DContentGroup,
// evaluated by the pinned CesiumJS engine.
//
// TileReplacementQueue cases replay random scripts of renders, frame starts
// and trims on stand-in tiles, recording the queue after every step.
import { makeRng, suite } from "./lib.mjs";
import { val } from "./scene-metadata.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;

const utf8 = (s) => Array.from(new TextEncoder().encode(s));
const withMagic = (magic, rest = [1, 2, 3, 4]) => [...utf8(magic), ...rest];

export default async function sceneTileContent() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  const preprocess3DTileContent = await scene("preprocess3DTileContent");
  const TileReplacementQueue = await scene("TileReplacementQueue");
  const Cesium3DContentGroup = await scene("Cesium3DContentGroup");
  const GroupMetadata = await scene("GroupMetadata");
  const MetadataSchema = await scene("MetadataSchema");

  // ---- preprocess3DTileContent ----
  {
    const s = add("preprocess3DTileContent");
    const inputs = [
      ...["b3dm", "i3dm", "cmpt", "pnts", "vctr", "geom", "subt", "voxl", "glTF"].map((m) => withMagic(m)),
      withMagic("glb "), withMagic("gltf"),
      utf8(JSON.stringify({ asset: { version: "1.1" }, root: { geometricError: 1 } })),
      utf8(JSON.stringify({ asset: { version: "2.0" }, meshes: [] })),
      utf8(JSON.stringify({ root: null, asset: {} })),
      utf8(JSON.stringify({ tileAvailability: { constant: 1 } })),
      utf8(JSON.stringify({ type: "FeatureCollection", features: [] })),
      utf8(JSON.stringify({ voxelTable: {} })),
      utf8(JSON.stringify({ type: null, voxelTable: { a: 1 } })),
      utf8(JSON.stringify({ name: "ünï", asset: { k: [1, 2.5, -3e-7] } })),
      [0xef, 0xbb, 0xbf, ...utf8('  {"root": {}}  ')],
      utf8("{}"), utf8("[1]"), utf8("5"), utf8('"root"'), utf8("{"), utf8("null"), [], utf8("b3d"),
      [...utf8("pnts"), 0xff, 0xfe],
    ];
    for (const bytes of inputs) {
      s.add("preprocess", [bytes], (b) => {
        const r = preprocess3DTileContent(new Uint8Array(b).buffer);
        return { contentType: r.contentType, binaryPayload: r.binaryPayload ? r.binaryPayload.byteLength : null, jsonPayload: r.jsonPayload ?? null };
      });
    }
  }

  // ---- TileReplacementQueue ----
  {
    const s = add("TileReplacementQueue");
    const rng = makeRng(0x3d711e5);
    const scripts = [];
    // A fixed script through the edge cases, then random ones.
    scripts.push({
      tiles: 4, ineligible: [2],
      ops: [["trim", 0], ["render", 0], ["render", 0], ["start"], ["render", 0], ["render", 1], ["render", 2], ["render", 1], ["start"],
        ["trim", 1], ["render", 3], ["trim", 0], ["start"], ["trim", 0], ["render", 2], ["start"], ["render", 2], ["trim", 0]],
    });
    for (let k = 0; k < 8; k++) {
      const tiles = 3 + Math.floor(rng() * 8);
      const ineligible = [];
      for (let i = 0; i < tiles; i++) if (rng() < 0.25) ineligible.push(i);
      const ops = [];
      for (let i = 0; i < 60; i++) {
        const r = rng();
        if (r < 0.6) ops.push(["render", Math.floor(rng() * tiles)]);
        else if (r < 0.8) ops.push(["start"]);
        else ops.push(["trim", Math.floor(rng() * tiles)]);
      }
      scripts.push({ tiles, ineligible, ops });
    }
    for (const script of scripts) {
      s.add("script", [script.tiles, script.ineligible, script.ops], (n, ineligible, ops) => {
        const freed = [];
        const tiles = Array.from({ length: n }, (_, id) => ({
          id, eligibleForUnloading: !ineligible.includes(id), freeResources() { freed.push(id); },
        }));
        const q = new TileReplacementQueue();
        const ids = (t) => t?.id ?? null;
        return ops.map((op) => {
          freed.length = 0;
          if (op[0] === "render") q.markTileRendered(tiles[op[1]]);
          else if (op[0] === "start") q.markStartOfRenderFrame();
          else q.trimTiles(op[1]);
          const forward = [];
          for (let t = q.head; t; t = t.replacementNext) forward.push(t.id);
          const backward = [];
          for (let t = q.tail; t; t = t.replacementPrevious) backward.push(t.id);
          return [forward, backward, q.count, ids(q._lastBeforeStartOfFrame), [...freed]];
        });
      });
    }
  }

  // ---- Cesium3DContentGroup ----
  {
    const s = add("Cesium3DContentGroup");
    const schemaJson = { classes: { layer: { properties: { name: { type: "STRING" }, priority: { type: "SCALAR", componentType: "UINT8", default: 3 } } } } };
    const cases = [[{ class: "layer", properties: { name: "roads" }, extras: { e: 1 } }, "g"], [{ class: "layer" }, null]];
    for (const [group, id] of cases) {
      s.add("new", [schemaJson, group, id], (sj, g, i) => {
        const schema = MetadataSchema.fromJson(sj);
        const metadata = new GroupMetadata({ id: i ?? undefined, group: g, class: schema.classes.layer });
        const content = new Cesium3DContentGroup({ metadata });
        const m = content.metadata;
        return [m === metadata, m.id ?? null, m.class.id, m.getPropertyIds(), val(m.getProperty("name")), val(m.getProperty("priority")), m.extras ?? null];
      });
    }
  }
  return out;
}
