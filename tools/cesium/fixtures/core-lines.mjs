// Fixtures for the line and volume geometries: PolylinePipeline, polylines,
// walls, corridors and polyline volumes. Full vertex and index data.

import * as Cesium from "@cesium/engine";
import { geometryEnc } from "./core-geometries.mjs";
import { generators, makeRng, suite } from "./lib.mjs";

const { ArcType, Cartesian2, Cartesian3, Color, CornerType, Ellipsoid, Matrix4, PolylineGeometry, PolylineVolumeGeometry, PolylineVolumeOutlineGeometry, PolylinePipeline, Quaternion, SimplePolylineGeometry, VertexFormat, WallGeometry, WallOutlineGeometry } = Cesium;

export const formats = {
  POSITION_ONLY: VertexFormat.POSITION_ONLY,
  POSITION_AND_NORMAL: VertexFormat.POSITION_AND_NORMAL,
  POSITION_NORMAL_AND_ST: VertexFormat.POSITION_NORMAL_AND_ST,
  POSITION_AND_ST: VertexFormat.POSITION_AND_ST,
  ALL: VertexFormat.ALL,
  TANGENT_ONLY: new VertexFormat({ position: true, tangent: true }),
  BITANGENT_ONLY: new VertexFormat({ position: true, bitangent: true }),
};
export const formatNames = Object.keys(formats);
export const ellipsoids = { WGS84: Ellipsoid.WGS84, MOON: Ellipsoid.MOON };
const DEG = Math.PI / 180;
const c3 = (p) => new Cartesian3(...p);
const packed = (Type, value) => Type.pack(value, new Array(value.packedLength ?? Type.packedLength).fill(0));

/** A random walk of positions in degrees, as Cartesian3 data, with optional repeats and an antimeridian crossing. */
export function pathArgs(g, { n = 2 + Math.floor(g.rng() * 6), step = 3, heights = true, repeat = false, name = "WGS84", cross = false } = {}) {
  let lon = cross ? 176 : g.uniform(-170, 170);
  let lat = g.uniform(-60, 60);
  const out = [];
  for (let i = 0; i < n; i++) {
    const c = Cartesian3.fromDegrees(lon, lat, heights ? g.uniform(0, 5000) : 0, ellipsoids[name]);
    out.push([c.x, c.y, c.z]);
    if (repeat && g.rng() < 0.25) out.push([c.x, c.y, c.z]);
    lon += cross ? g.uniform(1, 4) : g.uniform(-step, step);
    if (lon > 180) lon -= 360;
    lat = Math.max(-80, Math.min(80, lat + g.uniform(-step, step)));
  }
  return out;
}

const colorArgs = (g, n) => Array.from({ length: n }, () => [g.rng(), g.rng(), g.rng(), g.pick([1, 0.5, g.rng()])]);

function pipelineSuite() {
  const rng = makeRng(1500);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("PolylinePipeline");
  for (let i = 0; i < 25; i++) {
    const name = i < 20 ? "WGS84" : "MOON";
    const path = pathArgs(g, { name, repeat: i % 5 === 0 });
    const perPosition = g.rng() < 0.5;
    const height = perPosition ? path.map(() => g.uniform(0, 1000)) : g.pick([0, 500]);
    const granularity = g.rng() < 0.3 ? null : g.uniform(0.5, 3) * DEG;
    for (const f of ["generateArc", "generateRhumbArc"]) {
      s.add(f, [path, name, height, granularity], (ps, e, h, gr) =>
        PolylinePipeline[f]({ positions: ps.map(c3), ellipsoid: ellipsoids[e], height: h, granularity: gr ?? undefined }),
      );
    }
    s.add("extractHeights", [path, name], (ps, e) => PolylinePipeline.extractHeights(ps.map(c3), ellipsoids[e]));
  }
  for (let i = 0; i < 4; i++) {
    const path = pathArgs(g, { n: 1 });
    s.add("generateArc", [path, "WGS84", g.pick([0, 250]), null], (ps, e, h) => PolylinePipeline.generateArc({ positions: ps.map(c3), ellipsoid: ellipsoids[e], height: h }));
  }
  s.add("generateArcMinDistance", [pathArgs(g, { n: 4 }), 20000], (ps, d) => PolylinePipeline.generateArc({ positions: ps.map(c3), minDistance: d }));
  for (let i = 0; i < 10; i++) {
    const path = pathArgs(g, { cross: i < 7, n: 3 + Math.floor(g.rng() * 5) });
    const m = i % 3 === 2 ? Array.from(Matrix4.toArray(Matrix4.fromTranslationQuaternionRotationScale(g.c3(1e3), Quaternion.fromAxisAngle(Cartesian3.UNIT_Z, g.uniform(-0.3, 0.3)), new Cartesian3(1, 1, 1)))) : null;
    s.add("wrapLongitude", [path, m], (ps, mm) => {
      const r = PolylinePipeline.wrapLongitude(ps.map(c3), mm ? Matrix4.fromArray(mm) : undefined);
      return [r.positions, r.lengths];
    });
  }
  return s;
}

function polylineSuite() {
  const rng = makeRng(1501);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("PolylineGeometry");
  const args = (simple) => {
    const name = g.rng() < 0.85 ? "WGS84" : "MOON";
    const path = pathArgs(g, { name, repeat: g.rng() < 0.4, step: simple ? 4 : 2.5 });
    const perVertex = g.rng() < 0.5;
    const colors = g.rng() < 0.5 ? colorArgs(g, perVertex ? path.length : path.length - 1) : null;
    return [path, colors, perVertex, g.pick([ArcType.GEODESIC, ArcType.RHUMB, ArcType.NONE]), g.rng() < 0.3 ? null : g.uniform(1, 4) * DEG, name, g.pick(formatNames), g.pick([1, 5, 0.5])];
  };
  const options = ([path, colors, perVertex, arcType, granularity, name, format, width]) => ({
    positions: path.map(c3),
    colors: colors ? colors.map((c) => new Color(...c)) : undefined,
    colorsPerVertex: perVertex,
    arcType,
    granularity: granularity ?? undefined,
    ellipsoid: ellipsoids[name],
    vertexFormat: formats[format],
    width,
  });
  for (let i = 0; i < 45; i++) {
    s.add("polyline", args(false), (...a) => {
      const p = new PolylineGeometry(options(a));
      return [packed(PolylineGeometry, p), geometryEnc(PolylineGeometry.createGeometry(p))];
    });
  }
  for (let i = 0; i < 45; i++) {
    s.add("simplePolyline", args(true), (...a) => {
      const p = new SimplePolylineGeometry(options(a));
      return [packed(SimplePolylineGeometry, p), geometryEnc(SimplePolylineGeometry.createGeometry(p))];
    });
  }
  for (let i = 0; i < 4; i++) {
    const a = args(false);
    s.add("unpack", [packed(PolylineGeometry, new PolylineGeometry(options(a)))], (arr) => packed(PolylineGeometry, PolylineGeometry.unpack(arr)));
    s.add("simpleUnpack", [packed(SimplePolylineGeometry, new SimplePolylineGeometry(options(a)))], (arr) =>
      packed(SimplePolylineGeometry, SimplePolylineGeometry.unpack(arr)),
    );
  }
  // Degenerate: one distinct position, or no width.
  const one = pathArgs(g, { n: 1 });
  s.add("polyline", [[one[0], one[0]], null, false, ArcType.GEODESIC, null, "WGS84", "ALL", 1], (...a) => {
    const p = new PolylineGeometry(options(a));
    return [packed(PolylineGeometry, p), geometryEnc(PolylineGeometry.createGeometry(p))];
  });
  return s;
}

function wallSuite() {
  const rng = makeRng(1502);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("WallGeometry");
  // Cesium's scratch tangent leaks between calls when only bitangents are asked for.
  const wallFormats = formatNames.filter((f) => f !== "BITANGENT_ONLY");
  const args = () => {
    const name = g.rng() < 0.85 ? "WGS84" : "MOON";
    const path = pathArgs(g, { name, repeat: g.rng() < 0.3, heights: g.rng() < 0.5, step: 2 });
    const max = g.rng() < 0.75 ? path.map(() => g.uniform(1000, 9000)) : null;
    const min = g.rng() < 0.6 ? path.map(() => g.uniform(-500, 800)) : null;
    return [path, max, min, g.pick(wallFormats), g.rng() < 0.3 ? null : g.uniform(0.5, 3) * DEG, name];
  };
  const options = ([path, max, min, format, granularity, name]) => ({
    positions: path.map(c3),
    maximumHeights: max ?? undefined,
    minimumHeights: min ?? undefined,
    vertexFormat: formats[format],
    granularity: granularity ?? undefined,
    ellipsoid: ellipsoids[name],
  });
  for (let i = 0; i < 40; i++) {
    const a = args();
    s.add("wall", a, (...x) => {
      const w = new WallGeometry(options(x));
      return [packed(WallGeometry, w), geometryEnc(WallGeometry.createGeometry(w))];
    });
    s.add("wallOutline", a, (...x) => {
      const w = new WallOutlineGeometry(options(x));
      return [packed(WallOutlineGeometry, w), geometryEnc(WallOutlineGeometry.createGeometry(w))];
    });
  }
  for (let i = 0; i < 6; i++) {
    const path = pathArgs(g, { heights: false });
    const min = g.pick([null, -100, 300]);
    const max = g.pick([null, 2500]);
    s.add("fromConstantHeights", [path, min, max, g.pick(wallFormats)], (ps, mn, mx, f) => {
      const o = { positions: ps.map(c3), minimumHeight: mn ?? undefined, maximumHeight: mx ?? undefined, vertexFormat: formats[f] };
      const w = WallGeometry.fromConstantHeights(o);
      const wo = WallOutlineGeometry.fromConstantHeights(o);
      return [packed(WallGeometry, w), geometryEnc(WallGeometry.createGeometry(w)), packed(WallOutlineGeometry, wo), geometryEnc(WallOutlineGeometry.createGeometry(wo))];
    });
    const a = args();
    s.add("unpack", [packed(WallGeometry, new WallGeometry(options(a)))], (arr) => packed(WallGeometry, WallGeometry.unpack(arr)));
    s.add("outlineUnpack", [packed(WallOutlineGeometry, new WallOutlineGeometry(options(a)))], (arr) => packed(WallOutlineGeometry, WallOutlineGeometry.unpack(arr)));
  }
  return s;
}

/** A closed 2D shape in metres: a regular polygon, a star or a box, optionally clockwise or with a repeated point. */
function shapeArgs(g) {
  const kind = g.pick(["polygon", "star", "box"]);
  const r = g.uniform(500, 20000);
  let pts;
  if (kind === "box") {
    const w = r * g.uniform(0.3, 1);
    pts = [[-w, -r], [w, -r], [w, r], [-w, r]];
  } else {
    const n = 3 + Math.floor(g.rng() * 6);
    const count = kind === "star" ? n * 2 : n;
    pts = Array.from({ length: count }, (_, i) => {
      const a = (i / count) * 2 * Math.PI;
      const rr = kind === "star" && i % 2 ? r * 0.4 : r;
      return [rr * Math.cos(a), rr * Math.sin(a)];
    });
  }
  if (g.rng() < 0.3) pts.reverse();
  if (g.rng() < 0.2) pts.splice(1, 0, pts[1].slice());
  if (g.rng() < 0.2) pts.push(pts[0].slice());
  return pts;
}

function volumeSuite() {
  const rng = makeRng(1503);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("PolylineVolumeGeometry");
  const args = () => {
    const name = g.rng() < 0.85 ? "WGS84" : "MOON";
    const path = pathArgs(g, { name, n: 2 + Math.floor(g.rng() * 4), repeat: g.rng() < 0.3, step: 1.5 });
    return [path, shapeArgs(g), g.pick([CornerType.ROUNDED, CornerType.MITERED, CornerType.BEVELED]), g.pick(formatNames), g.rng() < 0.4 ? null : g.uniform(1, 4) * DEG, name];
  };
  const options = ([path, shape, cornerType, format, granularity, name]) => ({
    polylinePositions: path.map(c3),
    shapePositions: shape.map((p) => new Cartesian2(...p)),
    cornerType,
    vertexFormat: formats[format],
    granularity: granularity ?? undefined,
    ellipsoid: ellipsoids[name],
  });
  // Pack first: createGeometry scales the input positions onto the surface in place.
  for (let i = 0; i < 40; i++) {
    const a = args();
    s.add("volume", a, (...x) => {
      const v = new PolylineVolumeGeometry(options(x));
      const p = packed(PolylineVolumeGeometry, v);
      return [p, geometryEnc(PolylineVolumeGeometry.createGeometry(v))];
    });
    s.add("volumeOutline", a, (...x) => {
      const v = new PolylineVolumeOutlineGeometry(options(x));
      const p = packed(PolylineVolumeOutlineGeometry, v);
      return [p, geometryEnc(PolylineVolumeOutlineGeometry.createGeometry(v))];
    });
  }
  // Degenerate: one distinct position, or a two-point shape.
  const one = pathArgs(g, { n: 1 });
  for (const a of [
    [[one[0], one[0]], shapeArgs(g), CornerType.ROUNDED, "ALL", null, "WGS84"],
    [pathArgs(g, { n: 3 }), [[0, 0], [100, 0], [0, 0]], CornerType.ROUNDED, "ALL", null, "WGS84"],
  ]) {
    s.add("volume", a, (...x) => {
      const v = new PolylineVolumeGeometry(options(x));
      return [packed(PolylineVolumeGeometry, v), geometryEnc(PolylineVolumeGeometry.createGeometry(v))];
    });
  }
  for (let i = 0; i < 4; i++) {
    const a = args();
    s.add("unpack", [packed(PolylineVolumeGeometry, new PolylineVolumeGeometry(options(a)))], (arr) => packed(PolylineVolumeGeometry, PolylineVolumeGeometry.unpack(arr)));
    s.add("outlineUnpack", [packed(PolylineVolumeOutlineGeometry, new PolylineVolumeOutlineGeometry(options(a)))], (arr) =>
      packed(PolylineVolumeOutlineGeometry, PolylineVolumeOutlineGeometry.unpack(arr)),
    );
  }
  return s;
}

export default function coreLines() {
  return [pipelineSuite(), polylineSuite(), wallSuite(), volumeSuite()];
}
