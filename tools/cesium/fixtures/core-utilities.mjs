// Fixtures for Core's small utilities: array helpers, containers, typed-array
// strings, queries and URIs, pixel formats, and enums.

import * as Cesium from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const {
  arrayRemoveDuplicates,
  AssociativeArray,
  barycentricCoordinates,
  Cartesian2,
  Cartesian3,
  DoublyLinkedList,
  Ellipsoid,
  getAbsoluteUri,
  getBaseUri,
  getExtensionFromUri,
  getFilenameFromUri,
  getMagic,
  getStringFromTypedArray,
  ManagedArray,
  mergeSort,
  objectToQuery,
  PixelDatatype,
  PixelFormat,
  pointInsideTriangle,
  queryToObject,
  Queue,
  srgbToLinear,
  subdivideArray,
  VerticalExaggeration,
  WebGLConstants,
} = Cesium;

/** Object entries in enumeration order, as [key, value] pairs. */
const entries = (o) => Object.entries(o);

function arraySuite() {
  const g = generators(makeRng(800));
  const s = suite("ArrayUtilities");
  for (let k = 0; k < 30; k++) {
    const base = Array.from({ length: 1 + Math.floor(g.rng() * 8) }, () => g.c3(10));
    const values = [];
    for (const v of base) {
      values.push(v);
      if (g.rng() < 0.4) values.push(Cartesian3.add(v, new Cartesian3(g.pick([0, 1e-12, 1e-8]), 0, 0), new Cartesian3()));
    }
    if (g.rng() < 0.4) values.push(Cartesian3.clone(values[0]));
    const wrap = g.rng() < 0.5;
    s.add("arrayRemoveDuplicates", [values, wrap], (v, w) => {
      const removed = [];
      const result = arrayRemoveDuplicates(v, Cartesian3.equalsEpsilon, w, removed);
      return [result, removed];
    });
  }
  for (let k = 0; k < 30; k++) {
    const tri = [g.c3(10), g.c3(10), g.c3(10)];
    const p = g.pick([g.c3(10), tri[0], Cartesian3.lerp(tri[1], tri[2], 0.3, new Cartesian3())]);
    s.add("barycentricCoordinates", [p, ...tri], (p, a, b, c) => barycentricCoordinates(p, a, b, c) ?? null);
    s.add("pointInsideTriangle", [p, ...tri], (p, a, b, c) => pointInsideTriangle(p, a, b, c));
    const t2 = [g.c2(10), g.c2(10), g.c2(10)];
    const p2 = g.pick([g.c2(10), t2[1]]);
    s.add("barycentricCoordinates2D", [p2, ...t2], (p, a, b, c) => barycentricCoordinates(p, a, b, c) ?? null);
  }
  s.add("barycentricCoordinates", [new Cartesian3(1, 1, 0), new Cartesian3(0, 0, 0), new Cartesian3(1, 0, 0), new Cartesian3(2, 0, 0)], (p, a, b, c) => barycentricCoordinates(p, a, b, c) ?? null);
  for (const [n, parts] of [[0, 3], [5, 2], [7, 3], [10, 4], [3, 5], [9, 1]]) {
    s.add("subdivideArray", [Array.from({ length: n }, (_, i) => i), parts], (a, p) => subdivideArray(a, p));
  }
  for (let k = 0; k < 10; k++) {
    const arr = Array.from({ length: Math.floor(g.rng() * 30) }, (_, i) => [Math.floor(g.rng() * 5), i]);
    s.add("mergeSort", [arr], (a) => {
      const copy = a.map((x) => x.slice());
      mergeSort(copy, (x, y) => x[0] - y[0]);
      return copy;
    });
  }
  for (const v of [0, 0.01, 0.04045, 0.05, 0.5, 1, 2]) s.add("srgbToLinear", [v], srgbToLinear);
  return s;
}

function containerSuite() {
  const g = generators(makeRng(801));
  const s = suite("Containers");
  s.repeat("queue", 10, () => [Array.from({ length: 40 }, () => g.pick(["enqueue", "enqueue", "dequeue", "peek", "sort", "clear"]))], (ops) => {
    const q = new Queue();
    let n = 0;
    return ops.map((op) => {
      let r = null;
      if (op === "enqueue") q.enqueue((n++ * 7) % 11);
      else if (op === "dequeue") r = q.dequeue() ?? null;
      else if (op === "peek") r = q.peek() ?? null;
      else if (op === "sort") q.sort((a, b) => a - b);
      else q.clear();
      return [r, q.length];
    });
  });
  s.repeat("doublyLinkedList", 10, () => [Array.from({ length: 30 }, () => [g.pick(["add", "add", "remove", "splice"]), Math.floor(g.rng() * 100), Math.floor(g.rng() * 100)])], (ops) => {
    const list = new DoublyLinkedList();
    const nodes = [];
    const contents = () => {
      const out = [];
      for (let n = list.head; n; n = n.next) out.push(n.item);
      return out;
    };
    return ops.map(([op, a, b]) => {
      const live = nodes.filter((n) => n.live);
      if (op === "add") nodes.push(Object.assign(list.add(a), { live: true }));
      else if (op === "remove" && live.length) {
        const n = live[a % live.length];
        list.remove(n);
        n.live = false;
      } else if (op === "splice" && live.length) list.splice(live[a % live.length], live[b % live.length]);
      return [contents(), list.length];
    });
  });
  s.repeat("associativeArray", 10, () => [Array.from({ length: 30 }, () => [g.pick(["set", "set", "remove", "removeAll"]), g.pick(["a", "b", "c", "1", "2"]), Math.floor(g.rng() * 4)])], (ops) => {
    const a = new AssociativeArray();
    return ops.map(([op, k, v]) => {
      let r = null;
      if (op === "set") a.set(k, v);
      else if (op === "remove") r = a.remove(k);
      else a.removeAll();
      return [r, a.values.slice(), ["a", "b", "c", "1", "2"].map((key) => a.get(key) ?? null)];
    });
  });
  s.repeat("managedArray", 10, () => [Array.from({ length: 30 }, () => [g.pick(["push", "push", "pop", "set", "resize", "peek"]), Math.floor(g.rng() * 10)])], (ops) => {
    const m = new ManagedArray(2);
    return ops.map(([op, v]) => {
      let r = null;
      if (op === "push") m.push(v);
      else if (op === "pop") r = m.pop() ?? null;
      else if (op === "set") m.set(v, v * 10);
      else if (op === "resize") m.resize(v);
      else r = m.peek() ?? null;
      return [r, m.length, Array.from({ length: m.length }, (_, i) => m.get(i) ?? null)];
    });
  });
  return s;
}

function stringSuite() {
  const s = suite("Strings");
  const byteCases = [
    [72, 101, 108, 108, 111],
    [0xef, 0xbb, 0xbf, 65, 66],
    [0xe2, 0x82, 0xac, 32, 0xf0, 0x9f, 0x98, 0x80],
    [0xc3, 0x28, 65],
    [0xed, 0xa0, 0x80, 66],
    [0xf4, 0x90, 0x80, 0x80, 67],
    [0xe0, 0x80, 0x80],
    [103, 108, 84, 70, 2, 0, 0, 0],
    [0xff, 0xfe, 0x41],
    [],
  ];
  for (const b of byteCases) {
    s.add("getStringFromTypedArray", [b, 0, null], (b, o, l) => getStringFromTypedArray(new Uint8Array(b), o, l ?? undefined));
    if (b.length > 2) s.add("getStringFromTypedArray", [b, 1, 2], (b, o, l) => getStringFromTypedArray(new Uint8Array(b), o, l));
    s.add("getMagic", [b, 0], (b, o) => getMagic(new Uint8Array(b), o));
  }
  return s;
}

function querySuite() {
  const s = suite("QueryAndUri");
  const objects = [
    { a: "1", b: "two words", c: ["x", "y&z"] },
    { "2": "b", "1": "a", key: "v", "01": "z" },
    { "ü": "ö=&?#", empty: "" },
    {},
  ];
  for (const o of objects) s.add("objectToQuery", [entries(o)], (e) => objectToQuery(Object.fromEntries(e)));
  for (const q of ["a=1&b=2", "a=1&a=2&a=3", "x&y=&z=1;w=2", "q=hello+world&r=%C3%BC", "2=b&1=a&k=v", "", "a=1=2"]) {
    s.add("queryToObject", [q], (q) => entries(queryToObject(q)));
  }
  const uris = [
    "http://example.com/path/to/file.json?x=1#frag",
    "https://EXAMPLE.com:443/a/./b/../c/model.glb",
    "relative/path/image.png",
    "../up/one.b3dm?v=2",
    "/absolute/file.tar.gz",
    "file.txt",
    "http://example.com",
    "http://example.com/dir/",
    "data:application/json;base64,eyJ9",
    "http://example.com/a b/c%20d.png",
    "http://user:pw@host:8080/x/y.z",
    "//cdn.example.com/lib.js",
    "noextension",
    "http://example.com/path/.hidden",
    "./a/../b/./c",
  ];
  for (const u of uris) {
    s.add("getExtensionFromUri", [u], getExtensionFromUri);
    s.add("getFilenameFromUri", [u], getFilenameFromUri);
    s.add("getBaseUri", [u, false], getBaseUri);
    s.add("getBaseUri", [u, true], getBaseUri);
  }
  const bases = ["http://example.com/a/b/c.html", "https://host:8080/dir/", "http://example.com", "file:///C:/data/tileset.json"];
  const relatives = ["x.json", "../y.json", "/root.json", "?q=1", "#f", "", "sub/dir/../z.png", "http://other.org/abs", "//cdn.org/lib.js", "../../../../too/far", ".."];
  for (const b of bases) for (const r of relatives) s.add("getAbsoluteUri", [r, b], (r, b) => getAbsoluteUri(r, b));
  return s;
}

function pixelSuite() {
  const s = suite("PixelFormat");
  const formats = entries(PixelFormat).filter(([, v]) => typeof v === "number");
  const datatypes = entries(PixelDatatype).filter(([, v]) => typeof v === "number");
  s.add("formats", [], () => formats);
  s.add("datatypes", [], () => datatypes);
  for (const [name, f] of formats) {
    s.add("predicates", [name], () => [
      PixelFormat.componentsLength(f),
      PixelFormat.validate(f),
      PixelFormat.isColorFormat(f),
      PixelFormat.isDepthFormat(f),
      PixelFormat.isCompressedFormat(f),
      PixelFormat.isDXTFormat(f),
      PixelFormat.isPVRTCFormat(f),
      PixelFormat.isASTCFormat(f),
      PixelFormat.isETC1Format(f),
      PixelFormat.isETC2Format(f),
      PixelFormat.isBC7Format(f),
      [1, 3, 4, 7, 16, 33].map((w) => PixelFormat.compressedTextureSizeInBytes(f, w, w + 1)),
    ]);
    for (const [dname, d] of datatypes) {
      s.add("sizes", [name, dname], () => [
        PixelFormat.textureSizeInBytes(f, d, 5, 3),
        PixelFormat.texture3DSizeInBytes(f, d, 5, 3, 2),
        PixelFormat.alignmentInBytes(f, d, 5),
        PixelFormat.alignmentInBytes(f, d, 6),
        PixelFormat.toInternalFormat(f, d, { webgl2: true }),
        PixelFormat.toInternalFormat(f, d, { webgl2: false }),
      ]);
    }
  }
  for (const [dname, d] of datatypes) {
    s.add("datatype", [dname], () => [
      PixelDatatype.toWebGLConstant(d, { webgl2: true }),
      PixelDatatype.toWebGLConstant(d, { webgl2: false }),
      PixelDatatype.isPacked(d),
      PixelDatatype.sizeInBytes(d),
      PixelDatatype.getTypedArrayConstructor(d).name,
    ]);
  }
  s.add("flipY", [[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], "RGB", "UNSIGNED_BYTE", 2, 2], (v, f, d, w, h) =>
    Array.from(PixelFormat.flipY(new Uint8Array(v), PixelFormat[f], PixelDatatype[d], w, h)),
  );
  s.add("flipY", [[1.5, 2, 3, 4, 5, 6], "RG", "FLOAT", 1, 3], (v, f, d, w, h) =>
    Array.from(PixelFormat.flipY(new Float32Array(v), PixelFormat[f], PixelDatatype[d], w, h)),
  );
  return s;
}

function miscSuite() {
  const g = generators(makeRng(802));
  const s = suite("Misc");
  for (const c of [WebGLConstants.FLOAT, WebGLConstants.FLOAT_VEC3, WebGLConstants.FLOAT_MAT4, WebGLConstants.SAMPLER_2D, WebGLConstants.BOOL, WebGLConstants.INT]) {
    s.add("webGLConstantToGlslType", [c], (c) => Cesium.webGLConstantToGlslType?.(c) ?? null);
  }
  s.repeat("verticalExaggeration", 20, () => [g.surface(), g.pick([1, 2, 0.5, 3.7]), g.pick([0, 100, -50])], (p, sc, rel) => VerticalExaggeration.getPosition(p, Ellipsoid.WGS84, sc, rel));
  s.repeat("verticalExaggerationHeight", 10, () => [g.uniform(-100, 9000), g.uniform(0, 5), g.uniform(-100, 100)], (h, sc, r) => VerticalExaggeration.getHeight(h, sc, r));
  const enums = [
    "WindingOrder",
    "TrackingReferenceFrame",
    "ExtrapolationType",
    "InterpolationType",
    "HeightmapEncoding",
    "KeyboardEventModifier",
    "RequestState",
    "RequestType",
    "GeocodeType",
    "IonGeocodeProviderType",
    "ArticulationStageType",
    "ScreenSpaceEventType",
    "TileEdge",
    "TerrainQuantization",
  ];
  for (const e of enums) s.add("enum", [e], (e) => (Cesium[e] ? entries(Cesium[e]).filter(([, v]) => typeof v !== "function") : null));
  return s;
}

export default function coreUtilities() {
  return [arraySuite(), containerSuite(), stringSuite(), querySuite(), pixelSuite(), miscSuite()];
}
