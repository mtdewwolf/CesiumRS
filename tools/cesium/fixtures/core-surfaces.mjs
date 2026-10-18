// Fixtures for the filled surface geometries: ellipses and circles (and,
// as they are ported, rectangles and polygons): full vertex and index data,
// packing, rectangles and texture rotation points.

import * as Cesium from "@cesium/engine";
import { geometryEnc } from "./core-geometries.mjs";
import { generators, makeRng, suite } from "./lib.mjs";

const { ArcType, Cartesian2, Cartesian3, CircleGeometry, CoplanarPolygonGeometry, CoplanarPolygonOutlineGeometry, EllipseGeometry, Ellipsoid, GeometryOffsetAttribute, PolygonGeometry, PolygonHierarchy, PolygonOutlineGeometry, Rectangle, RectangleGeometry, RectangleOutlineGeometry, VertexFormat } = Cesium;

export const formats = {
  POSITION_ONLY: VertexFormat.POSITION_ONLY,
  POSITION_AND_NORMAL: VertexFormat.POSITION_AND_NORMAL,
  POSITION_NORMAL_AND_ST: VertexFormat.POSITION_NORMAL_AND_ST,
  POSITION_AND_ST: VertexFormat.POSITION_AND_ST,
  ALL: VertexFormat.ALL,
  TANGENT_ONLY: new VertexFormat({ position: true, tangent: true }),
  BITANGENT_ONLY: new VertexFormat({ position: true, bitangent: true }),
};
const formatNames = Object.keys(formats);
const offsets = [null, GeometryOffsetAttribute.NONE, GeometryOffsetAttribute.TOP, GeometryOffsetAttribute.ALL];
const ellipsoids = { WGS84: Ellipsoid.WGS84, MOON: Ellipsoid.MOON };
const packed = (Type, value) => Type.pack(value, new Array(Type.packedLength).fill(0));

/** Ellipse options as plain data: [center, major, minor, ellipsoid, height, extrudedHeight, rotation, stRotation, granularity, format, shadowVolume, offset]. */
function ellipseArgs(g, circle = false) {
  const lon = g.uniform(-180, 180);
  const lat = g.uniform(-80, 80);
  const name = g.rng() < 0.85 ? "WGS84" : "MOON";
  const center = Cartesian3.fromDegrees(lon, lat, g.uniform(-100, 100), ellipsoids[name]);
  const major = g.pick([1000, 20000, 60000, g.uniform(1000, 100000)]);
  const minor = circle ? major : major * g.uniform(0.3, 1);
  const height = g.rng() < 0.3 ? null : g.pick([0, 100, g.uniform(-500, 5000)]);
  const extruded = g.rng() < 0.5 ? null : g.pick([height ?? 0, 2000, g.uniform(-2000, 20000), (height ?? 0) + 0.005]);
  return [
    [center.x, center.y, center.z],
    major,
    minor,
    name,
    height,
    extruded,
    g.rng() < 0.5 ? 0 : g.uniform(-3, 3),
    g.rng() < 0.5 ? 0 : g.uniform(-3, 3),
    g.rng() < 0.3 ? null : g.uniform(1, 5) * (Math.PI / 180),
    g.pick(formatNames),
    g.rng() < 0.2,
    g.pick(offsets),
  ];
}

function ellipseOptions([c, major, minor, name, height, extruded, rotation, stRotation, granularity, format, shadowVolume, offset]) {
  return {
    center: new Cartesian3(...c),
    semiMajorAxis: major,
    semiMinorAxis: minor,
    ellipsoid: ellipsoids[name],
    height: height ?? undefined,
    extrudedHeight: extruded ?? undefined,
    rotation,
    stRotation,
    granularity: granularity ?? undefined,
    vertexFormat: formats[format],
    shadowVolume,
    offsetAttribute: offset ?? undefined,
  };
}

function ellipseSuite() {
  const rng = makeRng(1400);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("EllipseGeometry");
  for (let i = 0; i < 35; i++) {
    const args = ellipseArgs(g);
    s.add("ellipse", args, (...a) => {
      const e = new EllipseGeometry(ellipseOptions(a));
      const pack = packed(EllipseGeometry, e);
      const rect = e.rectangle;
      const points = e.textureCoordinateRotationPoints;
      // createGeometry moves the stored center to the surface, so it runs last.
      return [geometryEnc(EllipseGeometry.createGeometry(e)), pack, rect, points];
    });
  }
  for (let i = 0; i < 15; i++) {
    const args = ellipseArgs(g, true);
    s.add("circle", args, (...a) => {
      const o = ellipseOptions(a);
      const c = new CircleGeometry({ ...o, radius: o.semiMajorAxis });
      // createGeometry moves the stored center to the surface, so read the rest first.
      const pack = packed(CircleGeometry, c);
      const rect = c.rectangle;
      const points = c.textureCoordinateRotationPoints;
      return [geometryEnc(CircleGeometry.createGeometry(c)), pack, rect, points];
    });
  }
  // Packing round trips and shadow volumes.
  for (let i = 0; i < 6; i++) {
    const args = ellipseArgs(g);
    s.add("unpack", [packed(EllipseGeometry, new EllipseGeometry(ellipseOptions(args)))], (p) => packed(EllipseGeometry, EllipseGeometry.unpack(p)));
    s.add("circleUnpack", [packed(CircleGeometry, new CircleGeometry({ ...ellipseOptions(args), radius: args[1] }))], (p) => packed(CircleGeometry, CircleGeometry.unpack(p)));
    s.add("createShadowVolume", args, (...a) => {
      const e = EllipseGeometry.createShadowVolume(new EllipseGeometry(ellipseOptions(a)), () => -1000, () => 3000);
      return [packed(EllipseGeometry, e), geometryEnc(EllipseGeometry.createGeometry(e))];
    });
  }
  // Rectangles, including one wider than half the globe.
  for (const [lat, major] of [[0, 1e5], [89, 5e5], [-89.5, 8e5], [45, 3e6]]) {
    const center = Cartesian3.fromDegrees(30, lat);
    s.add("computeRectangle", [[center.x, center.y, center.z], major, major * 0.6, 0.3, null], (c, ma, mi, r, gr) =>
      EllipseGeometry.computeRectangle({ center: new Cartesian3(...c), semiMajorAxis: ma, semiMinorAxis: mi, rotation: r, granularity: gr ?? undefined }),
    );
  }
  // Degenerate axes give no geometry.
  s.add("ellipse", [[6378137, 0, 0], 0, 0, "WGS84", null, null, 0, 0, null, "ALL", false, null], (...a) => {
    const e = new EllipseGeometry(ellipseOptions(a));
    return [geometryEnc(EllipseGeometry.createGeometry(e)), packed(EllipseGeometry, e), e.rectangle, e.textureCoordinateRotationPoints];
  });
  return s;
}

const DEG = Math.PI / 180;

/** Rectangle options as plain data: [[w, s, e, n], ellipsoid, height, extrudedHeight, rotation, stRotation, granularity, format, shadowVolume, offset]. */
function rectangleArgs(g) {
  const kind = g.rng();
  let west = g.uniform(-180, 175);
  let south = g.uniform(-85, 80);
  let east = west + g.uniform(0.5, 12);
  let north = south + g.uniform(0.5, 8);
  if (east > 180) east -= 360; // crosses the antimeridian
  north = Math.min(north, 90);
  let r = [west * DEG, south * DEG, east * DEG, north * DEG];
  if (kind < 0.1) r = [west * DEG, 80 * DEG, (west + 10) * DEG, Math.PI / 2]; // north cap
  else if (kind < 0.2) r = [west * DEG, -Math.PI / 2, (west + 10) * DEG, -82 * DEG]; // south cap
  else if (kind < 0.25) r = [-Math.PI, -Math.PI / 2, Math.PI, Math.PI / 2]; // the globe
  const height = g.rng() < 0.3 ? null : g.pick([0, 100, g.uniform(-500, 5000)]);
  const extruded = g.rng() < 0.5 ? null : g.pick([height ?? 0, 2000, g.uniform(-2000, 20000), (height ?? 0) + 0.005]);
  const shadowVolume = g.rng() < 0.15;
  const format = shadowVolume ? g.pick(["POSITION_ONLY", "POSITION_AND_ST", "TANGENT_ONLY", "BITANGENT_ONLY", "ALL"]) : g.pick(formatNames);
  const globe = kind >= 0.2 && kind < 0.25;
  const capped = kind < 0.25;
  return [
    r,
    g.rng() < 0.85 ? "WGS84" : "MOON",
    height,
    extruded,
    capped || g.rng() < 0.5 ? 0 : g.uniform(-0.5, 0.5),
    g.rng() < 0.5 ? 0 : g.uniform(-3, 3),
    globe ? g.uniform(12, 20) * DEG : g.rng() < 0.3 ? null : g.uniform(1.5, 4) * DEG,
    format,
    shadowVolume,
    g.pick(offsets),
  ];
}

function rectangleOptions([r, name, height, extruded, rotation, stRotation, granularity, format, shadowVolume, offset]) {
  return {
    rectangle: new Rectangle(...r),
    ellipsoid: ellipsoids[name],
    height: height ?? undefined,
    extrudedHeight: extruded ?? undefined,
    rotation,
    stRotation,
    granularity: granularity ?? undefined,
    vertexFormat: formats[format],
    shadowVolume,
    offsetAttribute: offset ?? undefined,
  };
}

function rectangleSuite() {
  const rng = makeRng(1401);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("RectangleGeometry");
  for (let i = 0; i < 70; i++) {
    s.add("rectangle", rectangleArgs(g), (...a) => {
      const r = new RectangleGeometry(rectangleOptions(a));
      return [packed(RectangleGeometry, r), r.rectangle, r.textureCoordinateRotationPoints, geometryEnc(RectangleGeometry.createGeometry(r))];
    });
  }
  for (let i = 0; i < 40; i++) {
    s.add("outline", rectangleArgs(g), (...a) => {
      const r = new RectangleOutlineGeometry(rectangleOptions(a));
      return [packed(RectangleOutlineGeometry, r), geometryEnc(RectangleOutlineGeometry.createGeometry(r))];
    });
  }
  for (let i = 0; i < 6; i++) {
    const args = rectangleArgs(g);
    s.add("unpack", [packed(RectangleGeometry, new RectangleGeometry(rectangleOptions(args)))], (p) => packed(RectangleGeometry, RectangleGeometry.unpack(p)));
    s.add("outlineUnpack", [packed(RectangleOutlineGeometry, new RectangleOutlineGeometry(rectangleOptions(args)))], (p) => packed(RectangleOutlineGeometry, RectangleOutlineGeometry.unpack(p)));
    s.add("createShadowVolume", args, (...a) => {
      const r = RectangleGeometry.createShadowVolume(new RectangleGeometry(rectangleOptions(a)), () => -1000, () => 3000);
      return [packed(RectangleGeometry, r), geometryEnc(RectangleGeometry.createGeometry(r))];
    });
  }
  for (let i = 0; i < 10; i++) {
    const [r, , , , rotation, , granularity] = rectangleArgs(g);
    s.add("computeRectangle", [r, rotation || 0.3, granularity], (rr, rot, gr) =>
      RectangleGeometry.computeRectangle({ rectangle: new Rectangle(...rr), rotation: rot, granularity: gr ?? undefined }),
    );
  }
  // Degenerate rectangles give no geometry.
  for (const r of [[0.1, 0.2, 0.1, 0.3], [0.1, 0.2, 0.3, 0.2]]) {
    s.add("rectangle", [r, "WGS84", null, null, 0, 0, null, "ALL", false, null], (...a) => {
      const rg = new RectangleGeometry(rectangleOptions(a));
      return [packed(RectangleGeometry, rg), rg.rectangle, rg.textureCoordinateRotationPoints, geometryEnc(RectangleGeometry.createGeometry(rg))];
    });
  }
  return s;
}

/** A star ring in degrees around (lon, lat); counter-clockwise unless `cw`. */
function starRing(g, n, lon, lat, r, cw) {
  const angles = Array.from({ length: n }, () => g.uniform(0, 2 * Math.PI)).sort((a, b) => a - b);
  if (cw) angles.reverse();
  return angles.map((a) => {
    const k = g.uniform(0.6, 1) * r;
    return [lon + k * Math.cos(a), lat + k * Math.sin(a)];
  });
}

/** A hierarchy as plain data: { positions: [[x, y, z]], holes: [...] }. */
function hierarchyArgs(g, { large = false, polar = false, heights = false } = {}) {
  const lon = g.uniform(-175, 175);
  const lat = polar ? g.pick([84, -84]) : g.uniform(-60, 60);
  const r = large ? g.uniform(60, 80) : polar ? 4 : g.uniform(1, 6);
  const toC3 = ([x, y]) => {
    const c = Cartesian3.fromDegrees(x, Math.max(-89.9, Math.min(89.9, y)), heights ? g.uniform(0, 3000) : g.uniform(-10, 10));
    return [c.x, c.y, c.z];
  };
  const outer = starRing(g, 5 + Math.floor(g.rng() * 8), lon, large ? 0 : lat, r, g.rng() < 0.3);
  const holes = [];
  const nHoles = large || polar ? 0 : Math.floor(g.rng() * 3);
  for (let h = 0; h < nHoles; h++) {
    const a = g.uniform(0, 2 * Math.PI);
    holes.push({ positions: starRing(g, 3 + Math.floor(g.rng() * 4), lon + 0.25 * r * Math.cos(a), lat + 0.25 * r * Math.sin(a), 0.15 * r, g.rng() < 0.5).map(toC3), holes: [] });
  }
  return { positions: outer.map(toC3), holes };
}

const hierarchyOf = (h) => new PolygonHierarchy(h.positions.map((p) => new Cartesian3(...p)), (h.holes ?? []).map(hierarchyOf));
const texHierarchyOf = (h) => new PolygonHierarchy(h.positions.map((p) => new Cartesian2(...p)), (h.holes ?? []).map(texHierarchyOf));

/** Texture coordinates matching the hierarchy's shape: a scaled copy of each ring's x and y. */
function texArgs(h) {
  return { positions: h.positions.map((p, i) => [0.5 + 0.4 * Math.cos(i), 0.5 + 0.4 * Math.sin(i)]), holes: h.holes.map(texArgs) };
}

/** [hierarchy, ellipsoid, height, extrudedHeight, stRotation, granularity, format, perPositionHeight, closeTop, closeBottom, arcType, shadowVolume, offset, textureCoordinates]. */
function polygonArgs(g, kind) {
  const large = kind === "large";
  const polar = kind === "polar";
  const perPositionHeight = !large && g.rng() < 0.3;
  const h = hierarchyArgs(g, { large, polar, heights: perPositionHeight });
  // Cesium's debug build rejects a height together with per-position heights.
  const height = perPositionHeight || g.rng() < 0.3 ? null : g.pick([0, 100, g.uniform(-500, 5000)]);
  const extruded = g.rng() < 0.5 ? null : g.pick([2000, g.uniform(-2000, 20000), (height ?? 0) + 0.005]);
  const shadowVolume = !perPositionHeight && g.rng() < 0.12;
  const format = large || shadowVolume ? g.pick(["POSITION_ONLY", "POSITION_AND_NORMAL"]) : g.pick(formatNames);
  const textured = !large && g.rng() < 0.2;
  return [
    h,
    "WGS84",
    height,
    extruded,
    g.rng() < 0.6 ? 0 : g.uniform(-3, 3),
    large ? g.uniform(8, 12) * DEG : g.rng() < 0.3 ? null : g.uniform(1, 3) * DEG,
    format,
    perPositionHeight,
    g.rng() < 0.85,
    g.rng() < 0.85,
    g.rng() < 0.7 ? ArcType.GEODESIC : ArcType.RHUMB,
    shadowVolume,
    g.pick(offsets),
    textured ? texArgs(h) : null,
  ];
}

function polygonOptions([h, name, height, extruded, stRotation, granularity, format, perPositionHeight, closeTop, closeBottom, arcType, shadowVolume, offset, tex]) {
  return {
    polygonHierarchy: hierarchyOf(h),
    ellipsoid: ellipsoids[name],
    height: height ?? undefined,
    extrudedHeight: extruded ?? undefined,
    stRotation,
    granularity: granularity ?? undefined,
    vertexFormat: formats[format],
    perPositionHeight,
    closeTop,
    closeBottom,
    arcType,
    shadowVolume,
    offsetAttribute: offset ?? undefined,
    textureCoordinates: tex ? texHierarchyOf(tex) : undefined,
  };
}

const packedPolygon = (Type, value) => Type.pack(value, new Array(value.packedLength).fill(0));

function polygonSuite() {
  const rng = makeRng(1402);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("PolygonGeometry");
  const kinds = [...Array(60).fill("normal"), ...Array(5).fill("large"), ...Array(5).fill("polar")];
  for (const kind of kinds) {
    s.add("polygon", polygonArgs(g, kind), (...a) => {
      const p = new PolygonGeometry(polygonOptions(a));
      // createGeometry scales the hierarchy in place, so read the rest first.
      const pack = packedPolygon(PolygonGeometry, p);
      const rect = p.rectangle;
      const points = p.textureCoordinateRotationPoints;
      return [pack, rect, points, geometryEnc(PolygonGeometry.createGeometry(p))];
    });
  }
  for (let i = 0; i < 40; i++) {
    const a = polygonArgs(g, i < 34 ? "normal" : i < 37 ? "large" : "polar");
    s.add("outline", a, (...args) => {
      const o = polygonOptions(args);
      const p = new PolygonOutlineGeometry(o);
      const pack = packedPolygon(PolygonOutlineGeometry, p);
      return [pack, geometryEnc(PolygonOutlineGeometry.createGeometry(p))];
    });
  }
  for (let i = 0; i < 6; i++) {
    const a = polygonArgs(g, "normal");
    s.add("unpack", [packedPolygon(PolygonGeometry, new PolygonGeometry(polygonOptions(a)))], (arr) => {
      const p = PolygonGeometry.unpack(arr);
      return packedPolygon(PolygonGeometry, p);
    });
    s.add("outlineUnpack", [packedPolygon(PolygonOutlineGeometry, new PolygonOutlineGeometry(polygonOptions(a)))], (arr) =>
      packedPolygon(PolygonOutlineGeometry, PolygonOutlineGeometry.unpack(arr)),
    );
    s.add("createShadowVolume", a, (...args) => {
      const p = PolygonGeometry.createShadowVolume(new PolygonGeometry(polygonOptions(args)), () => -1000, () => 3000);
      return [packedPolygon(PolygonGeometry, p), geometryEnc(PolygonGeometry.createGeometry(p))];
    });
  }
  for (let i = 0; i < 12; i++) {
    const h = hierarchyArgs(g, { large: i % 4 === 0, polar: i % 4 === 1 });
    s.add("computeRectangleFromPositions", [h.positions, g.rng() < 0.5 ? ArcType.GEODESIC : ArcType.RHUMB], (ps, arc) =>
      PolygonGeometry.computeRectangleFromPositions(ps.map((p) => new Cartesian3(...p)), Ellipsoid.WGS84, arc),
    );
  }
  return s;
}

/** A planar hierarchy near the surface: rings in a random plane through a surface point, with a nested hole. */
function coplanarArgs(g, { degenerate = false } = {}) {
  const origin = Cartesian3.fromDegrees(g.uniform(-180, 180), g.uniform(-80, 80), g.uniform(0, 500));
  const u = Cartesian3.normalize(new Cartesian3(g.uniform(-1, 1), g.uniform(-1, 1), g.uniform(-1, 1)), new Cartesian3());
  let w = Cartesian3.normalize(new Cartesian3(g.uniform(-1, 1), g.uniform(-1, 1), g.uniform(-1, 1)), new Cartesian3());
  w = Cartesian3.normalize(Cartesian3.cross(u, w, w), w);
  const size = g.uniform(10, 500);
  const at = ([x, y]) => {
    const p = Cartesian3.add(origin, Cartesian3.add(Cartesian3.multiplyByScalar(u, x * size, new Cartesian3()), Cartesian3.multiplyByScalar(w, degenerate ? 0 : y * size, new Cartesian3()), new Cartesian3()), new Cartesian3());
    return [p.x, p.y, p.z];
  };
  const ring = (n, cx, cy, r, cw) => starRing(g, n, cx, cy, r, cw).map(at);
  const holes = [];
  if (!degenerate && g.rng() < 0.6) {
    holes.push({ positions: ring(4 + Math.floor(g.rng() * 3), 0.1, 0.1, 0.35, g.rng() < 0.5), holes: g.rng() < 0.5 ? [{ positions: ring(3, 0.1, 0.1, 0.1, false), holes: [] }] : [] });
  }
  return { positions: ring(5 + Math.floor(g.rng() * 8), 0, 0, 1, g.rng() < 0.4), holes };
}

function coplanarSuite() {
  const rng = makeRng(1403);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("CoplanarPolygonGeometry");
  const options = ([h, stRotation, format, tex]) => ({
    polygonHierarchy: hierarchyOf(h),
    stRotation,
    vertexFormat: formats[format],
    textureCoordinates: tex ? texHierarchyOf(tex) : undefined,
  });
  for (let i = 0; i < 45; i++) {
    const h = coplanarArgs(g, { degenerate: i >= 42 });
    const stRotation = g.rng() < 0.5 ? 0 : g.uniform(-3, 3);
    // Cesium's scratch tangent leaks between calls when only bitangents are asked for.
    const format = g.pick(stRotation !== 0 ? formatNames.filter((f) => f !== "BITANGENT_ONLY") : formatNames);
    const args = [h, stRotation, format, g.rng() < 0.2 ? texArgs(h) : null];
    s.add("coplanar", args, (...a) => {
      const p = new CoplanarPolygonGeometry(options(a));
      return [packedPolygon(CoplanarPolygonGeometry, p), geometryEnc(CoplanarPolygonGeometry.createGeometry(p))];
    });
    s.add("coplanarOutline", [h], (hh) => {
      const p = new CoplanarPolygonOutlineGeometry({ polygonHierarchy: hierarchyOf(hh) });
      return [packedPolygon(CoplanarPolygonOutlineGeometry, p), geometryEnc(CoplanarPolygonOutlineGeometry.createGeometry(p))];
    });
  }
  for (let i = 0; i < 4; i++) {
    const h = coplanarArgs(g);
    const p = new CoplanarPolygonGeometry(options([h, 0.5, "ALL", i % 2 ? texArgs(h) : null]));
    s.add("unpack", [packedPolygon(CoplanarPolygonGeometry, p)], (arr) => packedPolygon(CoplanarPolygonGeometry, CoplanarPolygonGeometry.unpack(arr)));
    const o = new CoplanarPolygonOutlineGeometry({ polygonHierarchy: hierarchyOf(h) });
    s.add("outlineUnpack", [packedPolygon(CoplanarPolygonOutlineGeometry, o)], (arr) =>
      packedPolygon(CoplanarPolygonOutlineGeometry, CoplanarPolygonOutlineGeometry.unpack(arr)),
    );
  }
  return s;
}

export default function coreSurfaces() {
  return [ellipseSuite(), rectangleSuite(), polygonSuite(), coplanarSuite()];
}
