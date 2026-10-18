// Fixtures for the filled surface geometries: ellipses and circles (and,
// as they are ported, rectangles and polygons): full vertex and index data,
// packing, rectangles and texture rotation points.

import * as Cesium from "@cesium/engine";
import { geometryEnc } from "./core-geometries.mjs";
import { generators, makeRng, suite } from "./lib.mjs";

const { Cartesian3, CircleGeometry, EllipseGeometry, Ellipsoid, GeometryOffsetAttribute, Rectangle, RectangleGeometry, RectangleOutlineGeometry, VertexFormat } = Cesium;

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

export default function coreSurfaces() {
  return [ellipseSuite(), rectangleSuite()];
}
