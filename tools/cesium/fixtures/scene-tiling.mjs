// Scene 3D Tiles tiling: the tile bounding volumes (sphere, oriented box,
// region, S2 cell), bounding volume semantics, and implicit tiling
// (coordinates, availability bitstreams, the implicit tileset, subtrees and
// the subtree cache), evaluated by the pinned CesiumJS engine.
//
// Script cases record each operation as `{ ok: value }` or
// `{ throws: errorName }`. Debug volumes record each geometry instance with
// its geometry created, as the Primitive would create it.
import * as C from "@cesium/engine";
import { enc, generators, makeRng, suite } from "./lib.mjs";
import { geometryEnc } from "./core-geometries.mjs";
import { val } from "./scene-metadata.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;
const { Cartesian3, Cartographic, Color, Ellipsoid, GeographicProjection, Matrix3, Plane, Rectangle, S2Cell, WebMercatorProjection } = C;

function attempt(fn) {
  try {
    return { ok: fn() };
  } catch (e) {
    return { throws: e.name ?? "Error" };
  }
}

async function attemptAsync(fn) {
  try {
    return { ok: await fn() };
  } catch (e) {
    return { throws: e.name ?? "Error" };
  }
}

/** A debug Primitive's instances, with each geometry created. */
function debugVolume(primitive) {
  const instances = Array.isArray(primitive.geometryInstances) ? primitive.geometryInstances : [primitive.geometryInstances];
  return {
    instances: instances.map((instance) => {
      let g = instance.geometry;
      if (g && !(g instanceof C.Geometry)) g = g.constructor.createGeometry(g);
      return {
        geometry: geometryEnc(g),
        modelMatrix: instance.modelMatrix,
        id: instance.id,
        color: Array.from(instance.attributes.color.value),
      };
    }),
    translucent: primitive.appearance.translucent,
    flat: primitive.appearance.flat,
    asynchronous: primitive.asynchronous,
  };
}

const projections = { geographic: new GeographicProjection(), webMercator: new WebMercatorProjection() };
/** `[mode, positionWC, positionCartographic, projection]`. */
function frameState([mode, positionWC, positionCartographic, projection]) {
  return {
    mode,
    camera: { positionWC: Cartesian3.unpack(positionWC), positionCartographic: Cartographic.fromRadians(...positionCartographic) },
    mapProjection: projections[projection],
  };
}

/** A camera frame near `center`: 3D frames use the ellipsoid's cartographic. */
function frames(g, rng, center) {
  const out = [];
  for (let i = 0; i < 6; i++) {
    const offset = new Cartesian3(g.uniform(-2e5, 2e5), g.uniform(-2e5, 2e5), g.uniform(-2e5, 2e5));
    const scale = [0, 1e-3, 1, 10, 100][i % 5];
    const p = Cartesian3.add(center, Cartesian3.multiplyByScalar(offset, scale, new Cartesian3()), new Cartesian3());
    const c = Ellipsoid.WGS84.cartesianToCartographic(p) ?? new Cartographic();
    out.push([C.SceneMode.SCENE3D, Cartesian3.pack(p, []), [c.longitude, c.latitude, c.height], "geographic"]);
  }
  for (const mode of [C.SceneMode.SCENE2D, C.SceneMode.COLUMBUS_VIEW, C.SceneMode.MORPHING]) {
    const projection = rng() < 0.5 ? "geographic" : "webMercator";
    const p = new Cartesian3(g.uniform(-1e6, 1e7), g.uniform(-2e7, 2e7), g.uniform(-1e7, 1e7));
    out.push([mode, Cartesian3.pack(p, []), [g.uniform(-Math.PI, Math.PI), g.uniform(-1.5, 1.5), g.uniform(-1e3, 1e7)], projection]);
  }
  return out;
}

function planes(g, center) {
  const out = [];
  for (let i = 0; i < 4; i++) {
    const normal = g.unit3();
    const through = Cartesian3.add(center, new Cartesian3(g.uniform(-1e5, 1e5), g.uniform(-1e5, 1e5), g.uniform(-1e5, 1e5)), new Cartesian3());
    out.push(Plane.fromPointNormal(through, normal));
  }
  out.push(new Plane(Cartesian3.UNIT_X, 1e9), new Plane(Cartesian3.UNIT_X, -1e9));
  return out;
}

/** The encoded value now, before a later update mutates the object. */
const snap = (v) => enc(v);

/** Shared operations of a bounding volume. */
function volumeOps(volume, frameList, planeList, color) {
  return {
    boundingSphere: snap(volume.boundingSphere),
    distances: frameList.map((f) => volume.distanceToCamera(frameState(f))),
    intersections: planeList.map((p) => volume.intersectPlane(p)),
    debug: debugVolume(volume.createDebugVolume(color)),
  };
}

async function boundingVolumes(add) {
  const rng = makeRng(0x7111e);
  const g = generators(rng);
  const color = new Color(0.25, 0.5, 0.75, 1);

  const TileBoundingSphere = await scene("TileBoundingSphere");
  const s = add("TileBoundingSphere");
  const sphereInputs = [
    [[0, 0, 0], 0],
    [[1, 2, 3], 5],
    [[6378137, 0, 0], 1000],
  ];
  for (let i = 0; i < 12; i++) sphereInputs.push([Cartesian3.pack(g.surface(), []), g.uniform(0, 5e5)]);
  for (const [center, radius] of sphereInputs) {
    const c = Cartesian3.unpack(center);
    const frameList = frames(g, rng, c);
    const planeList = planes(g, c);
    s.add("sphere", [center, radius, frameList, planeList, color], () => {
      const v = new TileBoundingSphere(c, radius);
      const ops = { center: snap(v.center), radius: v.radius, ...volumeOps(v, frameList, planeList, color) };
      v.update(Cartesian3.add(c, Cartesian3.UNIT_Z, new Cartesian3()), 0);
      ops.updated = [v.center, v.radius];
      return ops;
    });
  }

  const TileOrientedBoundingBox = await scene("TileOrientedBoundingBox");
  const o = add("TileOrientedBoundingBox");
  const col = (m, i, v) => Matrix3.setColumn(m, i, v, m);
  const boxes = [];
  for (let i = 0; i < 10; i++) boxes.push([g.surface(), Matrix3.multiplyByScale(g.rotation3(), new Cartesian3(g.uniform(1, 1e4), g.uniform(1, 1e4), g.uniform(0, 1e4)), new Matrix3())]);
  // Every combination of zero half-axes, and an axis along x.
  for (let mask = 0; mask < 8; mask++) {
    const m = Matrix3.multiplyByScale(g.rotation3(), new Cartesian3(100, 200, 300), new Matrix3());
    for (let k = 0; k < 3; k++) if (mask & (1 << k)) col(m, k, Cartesian3.ZERO);
    boxes.push([g.surface(), m]);
  }
  const alongX = new Matrix3();
  col(alongX, 0, new Cartesian3(5, 0, 0));
  boxes.push([new Cartesian3(1, 2, 3), alongX]);
  const alongY = new Matrix3();
  col(alongY, 1, new Cartesian3(0, 7, 0));
  boxes.push([new Cartesian3(1, 2, 3), alongY]);
  for (const [center, halfAxes] of boxes) {
    const frameList = frames(g, rng, center);
    const planeList = planes(g, center);
    o.add("box", [center, halfAxes, frameList, planeList, color], () => {
      const v = new TileOrientedBoundingBox(center, Matrix3.clone(halfAxes));
      const ops = { boundingVolume: snap([v.boundingVolume.center, v.boundingVolume.halfAxes]), ...volumeOps(v, frameList, planeList, color) };
      v.update(Cartesian3.add(center, Cartesian3.UNIT_X, new Cartesian3()), new Matrix3());
      ops.updated = [v.boundingVolume.center, v.boundingVolume.halfAxes, v.boundingSphere];
      return ops;
    });
  }

  const TileBoundingRegion = await scene("TileBoundingRegion");
  const r = add("TileBoundingRegion");
  const rectangles = [
    Rectangle.fromDegrees(-10, -10, 10, 10),
    Rectangle.fromDegrees(10, 20, 30, 40),
    Rectangle.fromDegrees(10, -40, 30, -20),
    Rectangle.fromDegrees(170, -5, -170, 5),
    Rectangle.fromDegrees(-180, -90, 180, 90),
    Rectangle.fromDegrees(0, 0, 0, 0),
    Rectangle.fromDegrees(-75.1, 39.9, -75.0, 40.0),
  ];
  for (let i = 0; i < 8; i++) rectangles.push(g.rectangle());
  for (const rect of rectangles) {
    const minimumHeight = g.uniform(-500, 100);
    const maximumHeight = minimumHeight + g.uniform(0, 1e4);
    const center = Rectangle.center(rect);
    const c = Cartesian3.fromRadians(center.longitude, center.latitude);
    const frameList = frames(g, rng, c);
    const planeList = planes(g, c);
    r.add("region", [rect, minimumHeight, maximumHeight, frameList, planeList, color], () => {
      const v = new TileBoundingRegion({ rectangle: rect, minimumHeight, maximumHeight });
      return {
        rectangle: v.rectangle,
        heights: [v.minimumHeight, v.maximumHeight],
        corners: [v.southwestCornerCartesian, v.northeastCornerCartesian],
        normals: [v.westNormal, v.southNormal, v.eastNormal, v.northNormal],
        boundingVolume: [v.boundingVolume.center, v.boundingVolume.halfAxes],
        ...volumeOps(v, frameList, planeList, color),
      };
    });
  }
  const lazyFrames = frames(g, rng, Cartesian3.fromDegrees(20, 30));
  r.add("lazy", [rectangles[1], lazyFrames], (rect, frameList) => {
    const v = new TileBoundingRegion({ rectangle: rect, computeBoundingVolumes: false });
    return { boundingVolume: v.boundingVolume ?? null, boundingSphere: v.boundingSphere ?? null, heights: [v.minimumHeight, v.maximumHeight], distances: frameList.map((f) => v.distanceToCamera(frameState(f))) };
  });

  const TileBoundingS2Cell = await scene("TileBoundingS2Cell");
  const c2 = add("TileBoundingS2Cell");
  const tokens = ["1", "3", "5", "7", "9", "b", "04", "1c", "89c25", "89c259", "2ef59bd34", "6b"];
  for (let i = 0; i < 8; i++) {
    let cell = S2Cell.fromToken(["1", "3", "5", "7", "9", "b"][i % 6]);
    const depth = 2 + Math.floor(rng() * 14);
    for (let d = 0; d < depth; d++) cell = cell.getChild(Math.floor(rng() * 4));
    tokens.push(S2Cell.getTokenFromId(cell._cellId));
  }
  for (const token of tokens) {
    const minimumHeight = g.uniform(-1000, 1000);
    const maximumHeight = minimumHeight + g.uniform(0, 1e5);
    const center = S2Cell.fromToken(token).getCenter();
    // Points near the cell: outside each face, edge and corner.
    const frameList = frames(g, rng, center);
    const v0 = new TileBoundingS2Cell({ token, minimumHeight, maximumHeight });
    for (let i = 0; i < 8; i++) {
      const vertex = v0._vertices[i];
      const out = Cartesian3.subtract(vertex, v0.center, new Cartesian3());
      for (const k of [0.5, 1.01, 1.5, 3]) {
        const p = Cartesian3.add(v0.center, Cartesian3.multiplyByScalar(out, k, new Cartesian3()), new Cartesian3());
        frameList.push([C.SceneMode.SCENE3D, Cartesian3.pack(p, []), [0, 0, 0], "geographic"]);
      }
    }
    const planeList = planes(g, center);
    c2.add("s2", [token, minimumHeight, maximumHeight, frameList, planeList, color], () => {
      const v = new TileBoundingS2Cell({ token, minimumHeight, maximumHeight });
      return {
        center: v.center,
        heights: [v.minimumHeight, v.maximumHeight],
        planes: v._boundingPlanes,
        vertices: v._vertices,
        ...volumeOps(v, frameList, planeList, color),
      };
    });
  }
}

const quad = (level, x, y, subtreeLevels = 2) => ({ subdivisionScheme: "QUADTREE", subtreeLevels, level, x, y });
const oct = (level, x, y, z, subtreeLevels = 2) => ({ subdivisionScheme: "OCTREE", subtreeLevels, level, x, y, z });

async function coordinates(add) {
  const ImplicitTileCoordinates = await scene("ImplicitTileCoordinates");
  const make = (o) => new ImplicitTileCoordinates(o);
  const coord = (c) => (c === undefined ? null : { scheme: c.subdivisionScheme, subtreeLevels: c.subtreeLevels, level: c.level, x: c.x, y: c.y, z: c.z ?? null });
  const s = add("ImplicitTileCoordinates");
  const inputs = [
    quad(0, 0, 0),
    quad(1, 1, 0),
    quad(3, 5, 6, 3),
    quad(5, 31, 17, 4),
    quad(10, 1000, 23, 5),
    quad(15, 30000, 12345, 7),
    oct(0, 0, 0, 0),
    oct(2, 3, 1, 2),
    oct(4, 9, 15, 3, 3),
    oct(7, 100, 64, 127, 4),
    quad(-1, 0, 0),
    quad(2, -1, 0),
    quad(2, 4, 0),
    quad(2, 0, 4),
    oct(2, 0, 0, 4),
    oct(2, 0, 0, -1),
    quad(31, 5, 5),
  ];
  for (const o of inputs) {
    const offsets = [quad(1, 1, 1, o.subtreeLevels), quad(2, 3, 0, o.subtreeLevels), quad(0, 0, 0, o.subtreeLevels)].map((q) => (o.subdivisionScheme === "OCTREE" ? { ...q, subdivisionScheme: "OCTREE", z: q.level ? 1 : 0 } : q));
    s.add("coordinates", [o, offsets], () => ({ result: attempt(() => {
      const c = make(o);
      const others = offsets.map(make);
      return {
        coord: coord(c),
        childIndex: c.childIndex,
        mortonIndex: c.mortonIndex,
        tileIndex: c.tileIndex,
        descendants: others.map((d) => attempt(() => coord(c.getDescendantCoordinates(d)))),
        ancestors: [-1, 0, 1, 2, 3, 40].map((n) => attempt(() => coord(c.getAncestorCoordinates(n)))),
        offsets: others.map((d) => {
          const desc = attempt(() => c.getDescendantCoordinates(d));
          return desc.ok ? attempt(() => coord(c.getOffsetCoordinates(desc.ok))) : desc;
        }),
        offsetSelf: attempt(() => coord(c.getOffsetCoordinates(c))),
        offsetNotDescendant: attempt(() => coord(c.getOffsetCoordinates(make(quad(0, 0, 0, o.subtreeLevels))))),
        children: [-1, 0, 1, 2, 3, 4, 7, 8].map((i) => attempt(() => coord(c.getChildCoordinates(i)))),
        subtree: attempt(() => coord(c.getSubtreeCoordinates())),
        parentSubtree: attempt(() => coord(c.getParentSubtreeCoordinates())),
        isAncestor: others.map((d) => attempt(() => c.isAncestor(d))),
        isAncestorOfChild: attempt(() => c.isAncestor(c.getChildCoordinates(1))),
        isEqual: [c, ...others].map((d) => c.isEqual(d)),
        isRoot: c.isImplicitTilesetRoot(),
        isSubtreeRoot: c.isSubtreeRoot(),
        isBottom: c.isBottomOfSubtree(),
        template: c.getTemplateValues(),
      };
    }) }));
  }
  s.add("mismatch", [quad(1, 0, 0, 2), quad(1, 0, 0, 3), oct(1, 0, 0, 0, 2)], (a, b, o) => {
    const [ca, cb, co] = [make(a), make(b), make(o)];
    return [attempt(() => ca.isAncestor(cb)), attempt(() => ca.isAncestor(co)), attempt(() => ca.getDescendantCoordinates(cb)), ca.isEqual(cb), ca.isEqual(co)];
  });
  for (const [scheme, levels, level, morton] of [["QUADTREE", 2, 0, 0], ["QUADTREE", 3, 2, 13], ["QUADTREE", 4, 8, 65535], ["OCTREE", 2, 3, 300], ["OCTREE", 3, 1, 7], ["QUADTREE", 2, 1, 4]]) {
    s.add("fromMortonIndex", [scheme, levels, level, morton], () => ({ result: attempt(() => coord(ImplicitTileCoordinates.fromMortonIndex(scheme, levels, level, morton))) }));
  }
  for (const [scheme, index] of [["QUADTREE", 0], ["QUADTREE", 1], ["QUADTREE", 4], ["QUADTREE", 5], ["QUADTREE", 20], ["QUADTREE", 21], ["QUADTREE", 5000], ["OCTREE", 0], ["OCTREE", 8], ["OCTREE", 9], ["OCTREE", 72], ["OCTREE", 73], ["OCTREE", 4000]]) {
    s.add("fromTileIndex", [scheme, index], () => ({ result: attempt(() => coord(ImplicitTileCoordinates.fromTileIndex(scheme, 3, index))) }));
  }
}

async function bitstreams(add) {
  const ImplicitAvailabilityBitstream = await scene("ImplicitAvailabilityBitstream");
  const s = add("ImplicitAvailabilityBitstream");
  const cases = [
    { lengthBits: 5, constant: true },
    { lengthBits: 5, constant: false, availableCount: 2 },
    { lengthBits: 10, bitstream: [0b10110101, 0b10] },
    { lengthBits: 10, bitstream: [0b10110101, 0b11111110], computeAvailableCountEnabled: true },
    { lengthBits: 10, bitstream: [0xff, 0xff], availableCount: 3, computeAvailableCountEnabled: true },
    { lengthBits: 16, bitstream: [0x0f, 0xf0], computeAvailableCountEnabled: true },
    { lengthBits: 9, bitstream: [1] },
    { lengthBits: 8, bitstream: [1, 2] },
    { lengthBits: 0, bitstream: [] },
  ];
  for (const o of cases) {
    s.add("bitstream", [o], () => ({
      result: attempt(() => {
        const b = new ImplicitAvailabilityBitstream({ ...o, bitstream: o.bitstream && new Uint8Array(o.bitstream) });
        const bits = [];
        for (let i = -1; i <= o.lengthBits; i++) bits.push(attempt(() => b.getBit(i)));
        return { lengthBits: b.lengthBits, availableCount: b.availableCount ?? null, bits };
      }),
    }));
  }
}

/** A binary subtree: 24-byte header, JSON padded to 8, binary padded to 8. */
function subtreeFile(json, binary) {
  const pad = (bytes, fill) => {
    const out = [...bytes];
    while (out.length % 8) out.push(fill);
    return out;
  };
  const jsonBytes = pad(Array.from(new TextEncoder().encode(JSON.stringify(json))), 0x20);
  const bin = pad(binary, 0);
  const header = new Uint8Array(24);
  const view = new DataView(header.buffer);
  view.setUint32(0, 0x74627573, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, jsonBytes.length, true);
  view.setUint32(16, bin.length, true);
  return [...header, ...jsonBytes, ...bin];
}

const tilingSchema = {
  classes: {
    tile: {
      properties: {
        height: { type: "SCALAR", componentType: "FLOAT32", semantic: "TILE_MAXIMUM_HEIGHT" },
        name: { type: "STRING" },
      },
    },
    content: { properties: { color: { type: "VEC3", componentType: "UINT8" } } },
    subtree: { properties: { author: { type: "STRING" }, year: { type: "SCALAR", componentType: "UINT16" } } },
  },
};

const tileJsonBase = {
  boundingVolume: { region: [-1, -1, 1, 1, 0, 100] },
  geometricError: 500,
  refine: "REPLACE",
  content: { uri: "content/{level}/{x}/{y}.glb" },
  implicitTiling: { subdivisionScheme: "QUADTREE", subtreeLevels: 2, availableLevels: 4, subtrees: { uri: "subtrees/{level}/{x}/{y}.subtree" } },
};

async function implicitTilesets(add) {
  const ImplicitTileset = await scene("ImplicitTileset");
  const MetadataSchema = await scene("MetadataSchema");
  const s = add("ImplicitTileset");
  const base = new C.Resource({ url: "https://example.com/tileset/tileset.json" });
  const variants = [
    tileJsonBase,
    { ...tileJsonBase, implicitTiling: undefined, extensions: { "3DTILES_implicit_tiling": { subdivisionScheme: "OCTREE", subtreeLevels: 3, maximumLevel: 5, subtrees: { uri: "s/{level}.{x}.{y}.{z}.subtree" } }, other: { a: 1 } }, boundingVolume: { box: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1] } },
    { ...tileJsonBase, content: undefined, contents: [{ uri: "a/{level}.b3dm", group: 0 }, { uri: "b/{level}.pnts" }] },
    { ...tileJsonBase, content: undefined, extensions: { "3DTILES_multiple_contents": { contents: [{ uri: "m/{x}.glb" }] }, "3DTILES_implicit_tiling": undefined } },
    { ...tileJsonBase, content: undefined, extensions: { "3DTILES_multiple_contents": { content: [{ uri: "legacy/{x}.glb" }] } } },
    { ...tileJsonBase, content: undefined },
    { ...tileJsonBase, boundingVolume: { extensions: { "3DTILES_bounding_volume_S2": { token: "1", minimumHeight: 0, maximumHeight: 10 } } } },
    { ...tileJsonBase, boundingVolume: { extensions: { "3DTILES_bounding_volume_cylinder": { minRadius: 0, maxRadius: 1, height: 2 } } } },
    { ...tileJsonBase, boundingVolume: { sphere: [0, 0, 0, 1] } },
    { ...tileJsonBase, implicitTiling: { ...tileJsonBase.implicitTiling, availableLevels: undefined } },
  ];
  for (const json of variants) {
    const clean = JSON.parse(JSON.stringify(json));
    s.add("tileset", [clean], () => ({
      result: attempt(() => {
        const t = new ImplicitTileset(base, clean, MetadataSchema.fromJson(tilingSchema));
        return {
          geometricError: t.geometricError ?? null,
          boundingVolume: t.boundingVolume,
          refine: t.refine ?? null,
          subtreeUri: t.subtreeUriTemplate.url,
          contentUris: t.contentUriTemplates.map((r) => r.url),
          contentHeaders: t.contentHeaders,
          contentCount: t.contentCount,
          tileHeader: t.tileHeader,
          subdivisionScheme: t.subdivisionScheme,
          branchingFactor: t.branchingFactor,
          subtreeLevels: t.subtreeLevels,
          availableLevels: t.availableLevels,
        };
      }),
    }));
  }
}

// Subtree inputs: `{ json | file, external: { uri: [bytes] }, tileset, coordinates }`.
function subtreeInputs() {
  const bits = (list, length) => {
    const bytes = new Array(Math.ceil(length / 8)).fill(0);
    list.forEach((on, i) => {
      if (on) bytes[i >> 3] |= 1 << i % 8;
    });
    return bytes;
  };
  // Quadtree, 2 levels: 5 tiles, 16 child subtrees.
  const tileBits = bits([1, 1, 0, 1, 1], 5);
  const contentBits = bits([1, 0, 0, 1, 1], 5);
  const childBits = bits([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1], 16);
  const f32 = (values) => Array.from(new Uint8Array(new Float32Array(values).buffer));
  const u16 = (values) => Array.from(new Uint8Array(new Uint16Array(values).buffer));
  const u32 = (values) => Array.from(new Uint8Array(new Uint32Array(values).buffer));
  const names = ["root", "c0", "c2", "c3"];
  const nameBytes = Array.from(new TextEncoder().encode(names.join("")));
  let nameOffsets = [0];
  for (const n of names) nameOffsets.push(nameOffsets.at(-1) + n.length);
  // Binary layout, every view 8-aligned.
  const views = [];
  const binary = [];
  const push = (bytes) => {
    while (binary.length % 8) binary.push(0);
    views.push({ buffer: 0, byteOffset: binary.length, byteLength: bytes.length });
    binary.push(...bytes);
    return views.length - 1;
  };
  const vTile = push(tileBits);
  const vContent = push(contentBits);
  const vChild = push(childBits);
  const vHeight = push(f32([10, 20, 30, 40]));
  const vNames = push(nameBytes);
  const vNameOffsets = push(u32(nameOffsets));
  const vColor = push([255, 0, 0, 0, 255, 0, 0, 0, 255]);
  const json = {
    buffers: [{ byteLength: binary.length }],
    bufferViews: views,
    propertyTables: [
      { class: "tile", count: 4, properties: { height: { values: vHeight }, name: { values: vNames, stringOffsets: vNameOffsets } } },
      { class: "content", count: 3, properties: { color: { values: vColor } } },
    ],
    tileAvailability: { bitstream: vTile, availableCount: 4 },
    contentAvailability: [{ bitstream: vContent, availableCount: 3 }],
    childSubtreeAvailability: { bitstream: vChild },
    tileMetadata: 0,
    contentMetadata: [1],
    subtreeMetadata: { class: "subtree", properties: { author: "cesium", year: 2026 } },
  };
  const external = { buffers: [{ uri: "external.bin", byteLength: binary.length }], bufferViews: views.map((v) => ({ ...v })) };
  const yearsView = { buffer: 1, byteOffset: 0, byteLength: 8 };
  return [
    { name: "binary", file: subtreeFile(json, binary), coordinates: quad(0, 0, 0) },
    { name: "external", json: { ...json, ...external }, externalBuffers: { "https://example.com/tileset/subtrees/external.bin": binary }, coordinates: quad(2, 1, 2) },
    {
      name: "constants",
      json: { tileAvailability: { constant: 1 }, childSubtreeAvailability: { constant: 0 } },
      coordinates: quad(0, 0, 0),
    },
    {
      name: "legacy",
      json: {
        buffers: [{ byteLength: binary.length }],
        bufferViews: views,
        tileAvailability: { bufferView: vTile },
        contentAvailability: { bufferView: vContent },
        childSubtreeAvailability: { constant: 1 },
        extensions: { "3DTILES_metadata": { class: "tile", properties: { height: { bufferView: vHeight } } } },
      },
      binary,
      coordinates: quad(0, 0, 0),
    },
    {
      name: "multiple",
      json: {
        buffers: [{ byteLength: binary.length }, { uri: "years.bin" }],
        bufferViews: [...views, yearsView],
        tileAvailability: { constant: 1 },
        childSubtreeAvailability: { constant: 0 },
        extensions: { "3DTILES_multiple_contents": { contentAvailability: [{ bitstream: vContent }, { constant: 1 }] } },
      },
      binary,
      externalBuffers: { "https://example.com/tileset/subtrees/years.bin": u16([1, 2, 3, 4]) },
      coordinates: quad(0, 0, 0),
    },
    { name: "badLength", json: { buffers: [{ byteLength: 8 }], bufferViews: [{ buffer: 0, byteOffset: 0, byteLength: 2 }], tileAvailability: { bitstream: 0 }, childSubtreeAvailability: { constant: 0 } }, binary: [1, 2, 0, 0, 0, 0, 0, 0], coordinates: quad(0, 0, 0) },
    { name: "missingBuffer", json: { buffers: [{ byteLength: 8 }], bufferViews: [{ buffer: 0, byteOffset: 0, byteLength: 1 }], tileAvailability: { bitstream: 0 }, childSubtreeAvailability: { constant: 0 } }, coordinates: quad(0, 0, 0) },
    { name: "badJson", file: [...subtreeFile({}, []).slice(0, 24), 0x7b, 0x7b, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20], coordinates: quad(0, 0, 0) },
    { name: "short", file: [0x73, 0x75, 0x62, 0x74, 1, 0, 0, 0, 8], coordinates: quad(0, 0, 0) },
    { name: "octree", json: { tileAvailability: { constant: 1 }, childSubtreeAvailability: { constant: 1 }, contentAvailability: { constant: 0 } }, coordinates: oct(0, 0, 0, 0), octree: true },
  ];
}

async function subtrees(add) {
  const ImplicitSubtree = await scene("ImplicitSubtree");
  const ImplicitTileset = await scene("ImplicitTileset");
  const ImplicitTileCoordinates = await scene("ImplicitTileCoordinates");
  const MetadataSchema = await scene("MetadataSchema");
  const s = add("ImplicitSubtree");
  const base = new C.Resource({ url: "https://example.com/tileset/tileset.json" });
  const subtreeResource = new C.Resource({ url: "https://example.com/tileset/subtrees/0.0.0.subtree" });

  // External buffers come from the fixture instead of the network.
  let external = {};
  const originalFetch = C.Resource.prototype.fetchArrayBuffer;
  C.Resource.prototype.fetchArrayBuffer = function () {
    const bytes = external[this.url];
    return bytes ? Promise.resolve(new Uint8Array(bytes).buffer) : Promise.reject(new C.RuntimeError(`no fixture for ${this.url}`));
  };

  for (const input of subtreeInputs()) {
    const tileJson = input.octree ? { ...tileJsonBase, implicitTiling: { ...tileJsonBase.implicitTiling, subdivisionScheme: "OCTREE" } } : tileJsonBase;
    const tileset = new ImplicitTileset(base, JSON.parse(JSON.stringify(tileJson)), MetadataSchema.fromJson(tilingSchema));
    external = input.externalBuffers ?? {};
    // A subtree with internal buffer views is a binary file.
    const file = input.binary ? subtreeFile(input.json, input.binary) : input.file;
    const json = file ? undefined : input.json;
    const coordinates = new ImplicitTileCoordinates(input.coordinates);
    const result = await attemptAsync(async () => {
      const subtree = await ImplicitSubtree.fromSubtreeJson(subtreeResource, json && JSON.parse(JSON.stringify(json)), file && new Uint8Array(file), tileset, coordinates);
      return describeSubtree(subtree, coordinates, ImplicitTileCoordinates, input.octree);
    });
    const args = { name: input.name, json: json ?? null, file: file ?? null, externalBuffers: input.externalBuffers ?? {}, coordinates: input.coordinates, octree: !!input.octree, tileJson, schema: tilingSchema };
    s.add("subtree", [args], () => ({ result }));
  }
  C.Resource.prototype.fetchArrayBuffer = originalFetch;
}

function describeSubtree(subtree, root, ImplicitTileCoordinates, octree) {
  const levels = 2;
  const tiles = [];
  for (let level = 0; level <= levels; level++) {
    const n = 1 << level;
    for (let x = 0; x < n; x++) {
      for (let y = 0; y < n; y++) {
        for (let z = 0; z < (octree ? n : 1); z++) {
          const local = new ImplicitTileCoordinates(octree ? oct(level, x, y, z) : quad(level, x, y));
          const c = root.getDescendantCoordinates(local);
          const view = (v) => (v ? Object.fromEntries(v.getPropertyIds().map((id) => [id, val(v.getProperty(id))])) : null);
          tiles.push({
            coordinates: [c.level, c.x, c.y, c.z ?? null],
            tileIndex: attempt(() => subtree.getTileIndex(c)),
            tile: attempt(() => subtree.tileIsAvailableAtCoordinates(c)),
            content: attempt(() => subtree.contentIsAvailableAtCoordinates(c)),
            content1: attempt(() => subtree.contentIsAvailableAtCoordinates(c, 1)),
            childIndex: attempt(() => subtree.getChildSubtreeIndex(c)),
            child: attempt(() => subtree.childSubtreeIsAvailableAtCoordinates(c)),
            tileView: attempt(() => view(subtree.getTileMetadataView(c))),
            contentView: attempt(() => view(subtree.getContentMetadataView(c, 0))),
          });
        }
      }
    }
  }
  const metadata = subtree.metadata;
  return {
    ready: subtree.ready,
    metadata: metadata ? Object.fromEntries(metadata.getPropertyIds().map((id) => [id, val(metadata.getProperty(id))])) : null,
    tileTableCount: subtree.tileMetadataTable?.count ?? null,
    tilePropertyTableJson: subtree.tilePropertyTableJson ?? null,
    contentTableCounts: subtree.contentMetadataTables.map((t) => t.count),
    tileAvailableCount: subtree._tileAvailability.availableCount ?? null,
    contentAvailableCounts: subtree._contentAvailabilityBitstreams.map((b) => b.availableCount ?? null),
    childAvailableCount: subtree._childSubtreeAvailability.availableCount ?? null,
    tileJumpBuffer: subtree._tileJumpBuffer ? Array.from(subtree._tileJumpBuffer) : null,
    contentJumpBuffers: subtree._contentJumpBuffers.map((b) => Array.from(b)),
    levelOffsets: [0, 1, 2, 3].map((l) => subtree.getLevelOffset(l)),
    parentMorton: [0, 3, 4, 17, 63].map((m) => subtree.getParentMortonIndex(m)),
    indexBits: [-1, 0, 4, 5].map((i) => [attempt(() => subtree.tileIsAvailableAtIndex(i)), attempt(() => subtree.childSubtreeIsAvailableAtIndex(i)), attempt(() => subtree.contentIsAvailableAtIndex(i)), attempt(() => subtree.contentIsAvailableAtIndex(0, i))]),
    tiles,
  };
}

async function subtreeCache(add) {
  const ImplicitSubtreeCache = await scene("ImplicitSubtreeCache");
  const ImplicitTileCoordinates = await scene("ImplicitTileCoordinates");
  const s = add("ImplicitSubtreeCache");
  const fake = (o) => ({ implicitCoordinates: new ImplicitTileCoordinates(o) });
  const scripts = [
    [0, [quad(0, 0, 0), quad(2, 1, 1), quad(2, 3, 0), quad(4, 5, 5), quad(2, 0, 3)]],
    [3, [quad(0, 0, 0), quad(2, 1, 1), quad(2, 3, 0), quad(4, 5, 5), quad(2, 0, 3), quad(4, 13, 2), quad(2, 2, 2)]],
    [2, [quad(0, 0, 0), quad(2, 0, 0), quad(4, 0, 0), quad(6, 1, 1)]],
    [1, [quad(0, 0, 0), quad(2, 1, 1), quad(4, 6, 6)]],
    [0, [quad(0, 0, 0), quad(4, 0, 0)]],
    [2, [oct(0, 0, 0, 0), oct(2, 1, 2, 3), oct(2, 3, 3, 3), oct(4, 7, 11, 13)]],
  ];
  for (const [maximumSubtreeCount, adds] of scripts) {
    s.add("cache", [maximumSubtreeCount, adds], () => {
      const cache = new ImplicitSubtreeCache({ maximumSubtreeCount });
      const coord = (c) => [c.level, c.x, c.y, c.z ?? null];
      return adds.map((o) => {
        const r = attempt(() => cache.addSubtree(fake(o)));
        return {
          add: r.throws ? r : { ok: null },
          queue: cache._queue.internalArray.slice(0, cache._queue.length).map((n) => [...coord(n.subtree.implicitCoordinates), n.stamp]),
          found: adds.map((q) => cache.find(new ImplicitTileCoordinates(q)) !== undefined),
        };
      });
    });
  }
}

async function semantics(add) {
  const BoundingVolumeSemantics = await scene("BoundingVolumeSemantics");
  const TileMetadata = await scene("TileMetadata");
  const MetadataSchema = await scene("MetadataSchema");
  const schemaJson = {
    classes: {
      box: { properties: { b: { type: "SCALAR", componentType: "FLOAT64", array: true, count: 12, semantic: "TILE_BOUNDING_BOX" }, min: { type: "SCALAR", componentType: "FLOAT32", semantic: "TILE_MINIMUM_HEIGHT" }, max: { type: "SCALAR", componentType: "FLOAT64", semantic: "TILE_MAXIMUM_HEIGHT" } } },
      region: { properties: { r: { type: "SCALAR", componentType: "FLOAT64", array: true, count: 6, semantic: "CONTENT_BOUNDING_REGION" }, b: { type: "SCALAR", componentType: "FLOAT64", array: true, count: 12, semantic: "TILE_BOUNDING_BOX" } } },
      sphere: { properties: { s: { type: "VEC4", componentType: "FLOAT64", semantic: "TILE_BOUNDING_SPHERE" }, h: { type: "SCALAR", componentType: "FLOAT64", semantic: "CONTENT_MAXIMUM_HEIGHT" } } },
      none: { properties: { x: { type: "SCALAR", componentType: "FLOAT64" } } },
    },
  };
  const schema = MetadataSchema.fromJson(schemaJson);
  const s = add("BoundingVolumeSemantics");
  const tiles = [
    ["box", { b: [0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3], min: 1.5, max: 30 }],
    ["box", { min: 2 }],
    ["region", { r: [-1, -1, 1, 1, 0, 50], b: [1, 2, 3, 1, 0, 0, 0, 1, 0, 0, 0, 1] }],
    ["region", { r: [-1, -1, 1, 1, 0, 50] }],
    ["sphere", { s: [1, 2, 3, 4], h: 9 }],
    ["none", { x: 1 }],
  ];
  for (const [cls, properties] of tiles) {
    s.add("semantics", [schemaJson, cls, properties], () => {
      const metadata = new TileMetadata({ tile: { class: cls, properties }, class: schema.classes[cls] });
      const out = {};
      for (const prefix of ["TILE", "CONTENT"]) {
        const all = BoundingVolumeSemantics.parseAllBoundingVolumeSemantics(prefix, metadata);
        out[prefix] = { boundingVolume: all.boundingVolume ? Object.fromEntries(Object.entries(all.boundingVolume).map(([k, v]) => [k, val(v)])) : null, minimumHeight: val(all.minimumHeight), maximumHeight: val(all.maximumHeight) };
      }
      return out;
    });
  }
}

export default async function sceneTiling() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  await boundingVolumes(add);
  await coordinates(add);
  await bitstreams(add);
  await implicitTilesets(add);
  await subtrees(add);
  await subtreeCache(add);
  await semantics(add);
  return out;
}
