// Fixtures for Hermite and Catmull-Rom splines, the tridiagonal solver,
// easing, heaps, polynomial approximations, 2D intersections, Tipsify,
// wireframe indices, datatypes, Stereographic, and tiling schemes.

import {
  Cartesian2,
  Cartesian3,
  Cartographic,
  CatmullRomSpline,
  ComponentDatatype,
  DoubleEndedPriorityQueue,
  EasingFunction,
  Ellipsoid,
  GeographicTilingScheme,
  Heap,
  HermitePolynomialApproximation,
  HermiteSpline,
  IndexDatatype,
  Intersections2D,
  LagrangePolynomialApproximation,
  LinearApproximation,
  LinearSpline,
  PrimitiveType,
  Quaternion,
  Rectangle,
  Stereographic,
  Tipsify,
  TridiagonalSystemSolver,
  WebMercatorTilingScheme,
  WireframeIndexGenerator,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const N = 30;

function times(g, n) {
  const t = [g.uniform(-10, 10)];
  for (let i = 1; i < n; i++) t.push(t.at(-1) + g.uniform(0.1, 5));
  return t;
}

function queries(g, t) {
  const q = [...t];
  for (let i = 0; i < 6; i++) q.push(g.uniform(t[0], t.at(-1)));
  return q;
}

function splineSuites() {
  const g = generators(makeRng(600));
  const hermite = suite("HermiteSpline");
  const catmull = suite("CatmullRomSpline");
  const tri = suite("TridiagonalSystemSolver");
  const evalAll = (spline, q) => q.map((x) => spline.evaluate(x));

  for (let k = 0; k < N; k++) {
    const n = 2 + (k % 6);
    const t = times(g, n);
    const q = queries(g, t);
    const kind = ["scalar", "cartesian", "quaternion"][k % 3];
    const point = { scalar: () => g.num(100), cartesian: () => g.c3(1000), quaternion: () => g.quat() }[kind];
    const pts = t.map(point);
    const ins = t.slice(1).map(point);
    const outs = t.slice(1).map(point);
    hermite.add(`new:${kind}`, [t, pts, ins, outs, q], (t, p, i, o, q) => evalAll(new HermiteSpline({ times: t, points: p, inTangents: i, outTangents: o }), q));
    const tangents = t.map(point);
    hermite.add(`createC1:${kind}`, [t, pts, tangents, q], (t, p, tg, q) => evalAll(HermiteSpline.createC1({ times: t, points: p, tangents: tg }), q));
  }
  for (let k = 0; k < N; k++) {
    const n = 2 + (k % 7);
    const t = times(g, n);
    const q = queries(g, t);
    const pts = t.map(() => g.c3(1000));
    const enc = (s) => [s instanceof LinearSpline ? "linear" : "hermite", s.inTangents ?? null, s.outTangents ?? null, evalAll(s, q)];
    hermite.add("createNaturalCubic", [t, pts, q], (t, p, q) => enc(HermiteSpline.createNaturalCubic({ times: t, points: p })));
    const first = g.c3(100);
    const last = g.c3(100);
    hermite.add("createClampedCubic", [t, pts, first, last, q], (t, p, f, l, q) =>
      enc(HermiteSpline.createClampedCubic({ times: t, points: p, firstTangent: f, lastTangent: l })),
    );

    const withTangents = k % 2 === 0;
    const ft = withTangents ? g.c3(100) : undefined;
    const lt = withTangents ? g.c3(100) : undefined;
    // Two-point splines extrapolate, so query outside the times too.
    const cq = n === 2 ? [...q, t[0] - 1, t[1] + 2] : q;
    catmull.add("evaluate", [t, pts, ft ?? null, lt ?? null, cq], (t, p, f, l, q) => {
      const s = new CatmullRomSpline({ times: t, points: p, firstTangent: f ?? undefined, lastTangent: l ?? undefined });
      return [s.firstTangent ?? null, s.lastTangent ?? null, evalAll(s, q)];
    });
  }
  for (let k = 0; k < N; k++) {
    const n = 2 + (k % 6);
    const diag = Array.from({ length: n }, () => g.uniform(2, 6));
    const lower = Array.from({ length: n - 1 }, () => g.uniform(-1, 1));
    const upper = Array.from({ length: n - 1 }, () => g.uniform(-1, 1));
    const right = Array.from({ length: n }, () => g.c3(100));
    tri.add("solve", [lower, diag, upper, right], (l, d, u, r) => TridiagonalSystemSolver.solve(l, d, u, r));
  }
  return [hermite, catmull, tri];
}

function easingSuite() {
  const s = suite("EasingFunction");
  const amounts = [0, 1, 0.5, 0.25, 0.75, 0.1, 0.9, 1 / 2.75, 2 / 2.75, 2.5 / 2.75, 0.3636, 0.95, -0.2, 1.3];
  for (const [name, f] of Object.entries(EasingFunction)) {
    s.add("evaluate", [name, amounts], (n, a) => a.map((x) => EasingFunction[n](x)));
  }
  return s;
}

function heapSuites() {
  const g = generators(makeRng(601));
  const heap = suite("Heap");
  const depq = suite("DoubleEndedPriorityQueue");
  const ops = () =>
    Array.from({ length: 30 }, () => {
      const r = g.rng();
      if (r < 0.55) return ["insert", Math.round(g.uniform(-50, 50))];
      if (r < 0.7) return ["pop", 0];
      if (r < 0.78) return ["popMax", 0];
      if (r < 0.88) return ["max", g.pick([3, 5, 8, 12, -1])];
      return ["mutate", Math.round(g.uniform(-50, 50))];
    });
  heap.repeat("ops", N, () => [ops()], (o) => {
    const h = new Heap({ comparator: (a, b) => a - b });
    const out = [];
    for (const [op, v] of o) {
      if (op === "insert") out.push(h.insert(v) ?? null);
      else if (op === "pop") out.push(h.pop() ?? null);
      else if (op === "popMax") out.push(h.length > 2 ? h.pop(2) ?? null : null);
      // Cesium's setter only accepts numbers.
      else if (op === "max") {
        if (v >= 0) h.maximumLength = v;
      }
      else if (h.length > 0) {
        h.internalArray[h.length - 1] = v;
        h.resort();
      }
      out.push(h.internalArray.slice(0, h.length));
    }
    return out;
  });
  depq.repeat("ops", N, () => [ops(), g.pick([-1, 4, 7, 0])], (o, max) => {
    const q = new DoubleEndedPriorityQueue({ comparator: (a, b) => a - b, maximumLength: max < 0 ? undefined : max });
    const out = [];
    for (const [op, v] of o) {
      if (op === "insert") out.push(q.insert(v) ?? null);
      else if (op === "pop") out.push(q.removeMinimum() ?? null);
      else if (op === "popMax") out.push(q.removeMaximum() ?? null);
      else if (op === "max") q.maximumLength = v < 0 ? undefined : v;
      else if (q.length > 0) {
        q.internalArray[q.length - 1] = v;
        q.resort();
      }
      out.push([q.internalArray.slice(0, q.length), q.getMinimum() ?? null, q.getMaximum() ?? null]);
    }
    return out;
  });
  return [heap, depq];
}

function approximationSuite() {
  const g = generators(makeRng(602));
  const s = suite("PolynomialApproximation");
  for (let k = 0; k < N; k++) {
    const n = 2 + (k % 6);
    const stride = 1 + (k % 3);
    const xs = times(g, n);
    const ys = Array.from({ length: n * stride }, () => g.uniform(-100, 100));
    const x = g.uniform(xs[0], xs.at(-1));
    s.add("lagrange", [x, xs, ys, stride], (x, xs, ys, st) => LagrangePolynomialApproximation.interpolateOrderZero(x, xs, ys, st));
    s.add("hermiteOrderZero", [x, xs, ys, stride], (x, xs, ys, st) => HermitePolynomialApproximation.interpolateOrderZero(x, xs, ys, st));
    if (n === 2) s.add("linear", [x, xs, ys, stride], (x, xs, ys, st) => LinearApproximation.interpolateOrderZero(x, xs, ys, st));
    const inputOrder = k % 3;
    const outputOrder = (k >> 1) % 3;
    const yd = Array.from({ length: n * stride * (inputOrder + 1) }, () => g.uniform(-100, 100));
    s.add("hermite", [x, xs, yd, stride, inputOrder, outputOrder], (x, xs, ys, st, i, o) => HermitePolynomialApproximation.interpolate(x, xs, ys, st, i, o));
  }
  // Values on a low-degree polynomial make whole difference rows zero.
  s.add("hermiteOrderZero", [0.5, [0, 1, 2, 3], [1, 1, 1, 1], 1], (x, xs, ys, st) => HermitePolynomialApproximation.interpolateOrderZero(x, xs, ys, st));
  s.add("hermite", [0.5, [0, 1, 2], [2, 0, 2, 0, 2, 0], 1, 1, 2], (x, xs, ys, st, i, o) => HermitePolynomialApproximation.interpolate(x, xs, ys, st, i, o));
  for (const d of [0, 1, 2, 5, 9]) {
    s.add("requiredLagrange", [d], (d) => LagrangePolynomialApproximation.getRequiredDataPoints(d));
    for (const i of [0, 1, 2]) s.add("requiredHermite", [d, i], (d, i) => HermitePolynomialApproximation.getRequiredDataPoints(d, i));
  }
  return s;
}

function intersections2DSuite() {
  const g = generators(makeRng(603));
  const s = suite("Intersections2D");
  const u = () => g.pick([0.5, g.uniform(0, 1), g.uniform(0, 1), 0, 1]);
  s.repeat("clipTriangleAtAxisAlignedThreshold", N * 3, () => [g.pick([0.5, 0.25]), g.pick([true, false]), u(), u(), u()], (t, k, a, b, c) =>
    Intersections2D.clipTriangleAtAxisAlignedThreshold(t, k, a, b, c),
  );
  s.repeat("computeBarycentricCoordinates", N, () => Array.from({ length: 8 }, () => g.uniform(-10, 10)), (...a) => Intersections2D.computeBarycentricCoordinates(...a));
  s.repeat("computeLineSegmentLineSegmentIntersection", N * 2, () => Array.from({ length: 8 }, () => g.uniform(-10, 10)), (...a) =>
    Intersections2D.computeLineSegmentLineSegmentIntersection(...a) ?? null,
  );
  s.add("computeLineSegmentLineSegmentIntersection", [0, 0, 1, 1, 0, 1, 1, 2], (...a) => Intersections2D.computeLineSegmentLineSegmentIntersection(...a) ?? null);
  return s;
}

function meshSuites() {
  const g = generators(makeRng(604));
  const tipsify = suite("Tipsify");
  const wire = suite("WireframeIndexGenerator");
  const grid = (w, h) => {
    const idx = [];
    for (let y = 0; y < h; y++)
      for (let x = 0; x < w; x++) {
        const a = y * (w + 1) + x;
        idx.push(a, a + 1, a + w + 1, a + 1, a + w + 2, a + w + 1);
      }
    return idx;
  };
  for (let k = 0; k < 12; k++) {
    let idx = grid(2 + (k % 5), 2 + ((k * 3) % 4));
    // Shuffle triangles so reordering has work to do.
    const tris = [];
    for (let i = 0; i < idx.length; i += 3) tris.push(idx.slice(i, i + 3));
    for (let i = tris.length - 1; i > 0; i--) {
      const j = Math.floor(g.rng() * (i + 1));
      [tris[i], tris[j]] = [tris[j], tris[i]];
    }
    idx = tris.flat();
    const cache = g.pick([3, 6, 24]);
    const max = g.rng() < 0.5 ? Math.max(...idx) : null;
    tipsify.add("tipsify", [idx, max, cache], (i, m, c) => Tipsify.tipsify({ indices: i, maximumIndex: m ?? undefined, cacheSize: c }));
    tipsify.add("calculateACMR", [idx, max, cache], (i, m, c) => Tipsify.calculateACMR({ indices: i, maximumIndex: m ?? undefined, cacheSize: c }));
  }
  const types = { TRIANGLES: PrimitiveType.TRIANGLES, TRIANGLE_STRIP: PrimitiveType.TRIANGLE_STRIP, TRIANGLE_FAN: PrimitiveType.TRIANGLE_FAN, LINES: PrimitiveType.LINES };
  for (const [name, type] of Object.entries(types)) {
    for (const n of [3, 4, 6, 7, 70000]) {
      wire.add("createWireframeIndices", [name, n, null], (t, n) => {
        const r = WireframeIndexGenerator.createWireframeIndices(types[t], n);
        return r ? [r.constructor.name, r.length, Array.from(r.slice(0, 40))] : null;
      });
      const idx = Array.from({ length: n === 70000 ? 9 : n }, () => Math.floor(g.uniform(0, n)));
      wire.add("createWireframeIndices", [name, n, idx], (t, n, i) => {
        const r = WireframeIndexGenerator.createWireframeIndices(types[t], n, i);
        return r ? [r.constructor.name, r.length, Array.from(r)] : null;
      });
      wire.add("getWireframeIndicesCount", [name, n], (t, n) => WireframeIndexGenerator.getWireframeIndicesCount(types[t], n));
    }
  }
  return [tipsify, wire];
}

function datatypeSuite() {
  const s = suite("Datatypes");
  const values = [0, 1, -1, 1.5, -1.5, 127, 128, 255, 256, -129, 32767, 32768, 65535, 65536, -32769, 2147483647, 2147483648, 4294967295, 4294967296, -2147483649, 1e10, -1e10, 0.1, 3.4e38, 1e39, NaN, Infinity, -Infinity, -0];
  for (const name of ["BYTE", "UNSIGNED_BYTE", "SHORT", "UNSIGNED_SHORT", "INT", "UNSIGNED_INT", "FLOAT", "DOUBLE"]) {
    s.add("createTypedArray", [name, values], (n, v) => {
      const d = ComponentDatatype.fromName(n);
      const a = ComponentDatatype.createTypedArray(d, v);
      return [d, ComponentDatatype.getSizeInBytes(d), a.constructor.name, Array.from(a)];
    });
    s.add("createArrayBufferView", [name, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 200, 255, 0, 128], 4, null], (n, bytes, offset, length) => {
      const d = ComponentDatatype.fromName(n);
      const size = ComponentDatatype.getSizeInBytes(d);
      // Cesium needs an aligned offset; use a multiple of the element size.
      const off = Math.floor(offset / size) * size;
      const a = ComponentDatatype.createArrayBufferView(d, new Uint8Array(bytes).buffer, off, length ?? undefined);
      return [off, Array.from(a)];
    });
  }
  for (const n of [10, 65535, 65536, 100000]) {
    s.add("indexCreateTypedArray", [n, [0, 1, 65535, 65536, 70000]], (n, v) => {
      const a = IndexDatatype.createTypedArray(n, v);
      return [a.constructor.name, Array.from(a)];
    });
  }
  return s;
}

function projectionSuites() {
  const g = generators(makeRng(605));
  const stereo = suite("Stereographic");
  stereo.repeat("fromCartesian", N * 2, () => [g.pick([g.surface(), g.c3(1e7), g.c3(1)])], (c) => {
    const st = Stereographic.fromCartesian(c);
    return [st.position, st.conformalLatitude, st.longitude, st.getLatitude(Ellipsoid.WGS84), st.tangentPlane.plane.normal.z];
  });

  const tiling = suite("TilingScheme");
  const schemes = {
    geographic: () => new GeographicTilingScheme(),
    geographicCustom: () => new GeographicTilingScheme({ rectangle: Rectangle.fromDegrees(170, -40, -150, 20), numberOfLevelZeroTilesX: 3, numberOfLevelZeroTilesY: 2 }),
    webMercator: () => new WebMercatorTilingScheme(),
    webMercatorCustom: () =>
      new WebMercatorTilingScheme({
        numberOfLevelZeroTilesX: 2,
        rectangleSouthwestInMeters: new Cartesian2(-5e6, -3e6),
        rectangleNortheastInMeters: new Cartesian2(7e6, 4e6),
      }),
  };
  for (const name of Object.keys(schemes)) {
    tiling.add("rectangle", [name], (n) => {
      const s = schemes[n]();
      return [s.rectangle, s.getNumberOfXTilesAtLevel(3), s.getNumberOfYTilesAtLevel(3)];
    });
    tiling.repeat("tileXYToRectangle", 12, () => {
      const level = Math.floor(g.uniform(0, 12));
      return [name, Math.floor(g.uniform(0, 2 << level)), Math.floor(g.uniform(0, 1 << level)), level];
    }, (n, x, y, l) => {
      const s = schemes[n]();
      return [s.tileXYToRectangle(x, y, l), s.tileXYToNativeRectangle(x, y, l)];
    });
    tiling.repeat("positionToTileXY", 15, () => [name, g.lonLat(), Math.floor(g.uniform(0, 15))], (n, p, l) => schemes[n]().positionToTileXY(p, l) ?? null);
    tiling.repeat("rectangleToNativeRectangle", 6, () => [name, g.rectangle()], (n, r) => schemes[n]().rectangleToNativeRectangle(r));
  }
  return [stereo, tiling];
}

export default function coreAlgorithms() {
  return [...splineSuites(), easingSuite(), ...heapSuites(), approximationSuite(), intersections2DSuite(), ...meshSuites(), datatypeSuite(), ...projectionSuites()];
}
