// Fixtures for earcut (the triangulator CesiumJS bundles) and PolygonPipeline:
// triangle order on simple, holed, degenerate and large (z-order hashed)
// polygons, and subdivision on the ellipsoid.

import * as Cesium from "@cesium/engine";
import earcut from "earcut";
import { generators, makeRng, suite } from "./lib.mjs";

const { Cartesian2, Cartesian3, Ellipsoid, PolygonPipeline } = Cesium;

/** A star-shaped ring around (cx, cy): sorted random angles, random radii. */
function star(g, n, cx, cy, r0, r1, ccw = true) {
  const angles = Array.from({ length: n }, () => g.uniform(0, 2 * Math.PI)).sort((a, b) => a - b);
  if (!ccw) angles.reverse();
  return angles.flatMap((a) => {
    const r = g.uniform(r0, r1);
    return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
  });
}

/** An outer star with `holes` small stars inside it; returns [data, holeIndices]. */
function holed(g, n, holes, { snap = false } = {}) {
  let data = star(g, n, 0, 0, 60, 100);
  const indices = [];
  for (let h = 0; h < holes; h++) {
    indices.push(data.length / 2);
    const a = g.uniform(0, 2 * Math.PI);
    const d = g.uniform(0, 35);
    data = data.concat(star(g, g.pick([3, 4, 5, 8, 12]), d * Math.cos(a), d * Math.sin(a), 2, 12, g.rng() < 0.5));
  }
  if (snap) data = data.map(Math.round);
  return [data, indices];
}

function earcutSuite() {
  const rng = makeRng(1200);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("Earcut");
  const add = (data, holes, dim) => s.add("earcut", [data, holes, dim], (d, h, k) => earcut(d, h, k));

  // Documented examples and hand-made degenerate rings.
  add([10, 0, 0, 50, 60, 60, 70, 10], [], 2);
  add([0, 0, 100, 0, 100, 100, 0, 100, 20, 20, 80, 20, 80, 80, 20, 80], [4], 2);
  add([10, 0, 1, 0, 50, 2, 60, 60, 3, 70, 10, 4], [], 3);
  add([0, 0, 1, 1], [], 2);
  add([0, 0, 1, 1, 2, 2], [], 2);
  add([0, 0, 1, 0, 2, 0, 2, 1, 2, 2, 1, 2, 0, 2, 0, 1], [], 2);
  add([0, 0, 0, 0, 4, 0, 4, 0, 4, 4, 0, 4, 0, 4], [], 2);
  add([0, 0, 4, 0, 4, 4, 0, 4, 2, 2], [4], 2); // Steiner point
  add([0, 0, 4, 0, 4, 4, 0, 4, 1, 1, 3, 3], [4, 5], 2); // two Steiner points
  add([0, 0, 10, 0, 10, 10, 0, 10, 0, 0, 5, 2, 5, 5], [4], 2); // hole touching an outer vertex
  add([0, 0, 10, 0, 10, 10, 0, 10, 5, 5, 8, 2, 8, 8, 2, 5, 2, 2, 5, 5], [4, 7], 2); // holes meeting at a vertex
  add([0, 0, 10, 10, 10, 0, 0, 10], [], 2); // self-intersecting bow tie
  add([0, 0, 10, 0, 10, 10, 5, 0, 0, 10], [], 2);
  add([0, 0, 10, 0, 10, 10, 0, 10, 2, 5, 5, 5, 5, 8], [4], 2); // hole on the ray's horizontal

  // Random star polygons, small and z-order hashed, with and without holes.
  for (let i = 0; i < 40; i++) add(star(g, 3 + Math.floor(rng() * 30), 0, 0, 10, 100), [], 2);
  for (let i = 0; i < 40; i++) add(...holed(g, 4 + Math.floor(rng() * 30), 1 + Math.floor(rng() * 4)), 2);
  for (let i = 0; i < 12; i++) add(star(g, 81 + Math.floor(rng() * 300), 0, 0, 20, 100), [], 2);
  for (let i = 0; i < 12; i++) add(...holed(g, 60 + Math.floor(rng() * 200), 1 + Math.floor(rng() * 10)), 2);
  // Integer-snapped rings hit the equality and collinearity paths.
  for (let i = 0; i < 20; i++) add(...holed(g, 5 + Math.floor(rng() * 100), Math.floor(rng() * 6), { snap: true }), 2);
  // Random points: self-intersecting rings exercise the cure and split stages.
  for (let i = 0; i < 25; i++) add(Array.from({ length: 2 * (4 + Math.floor(rng() * 40)) }, () => Math.round(g.uniform(0, 50))), [], 2);
  for (let i = 0; i < 8; i++) add(Array.from({ length: 2 * (90 + Math.floor(rng() * 100)) }, () => g.uniform(-50, 50)), [], 2);
  // Clockwise input and 3D data.
  for (let i = 0; i < 6; i++) add(star(g, 5 + Math.floor(rng() * 20), 3, -2, 5, 30, false), [], 2);
  for (let i = 0; i < 6; i++) {
    const flat = star(g, 4 + Math.floor(rng() * 100), 0, 0, 10, 50);
    const data = [];
    for (let k = 0; k < flat.length; k += 2) data.push(flat[k], flat[k + 1], g.uniform(-5, 5));
    add(data, [], 3);
  }
  return s;
}

/** A subdivided geometry: position values, st values, indices and their datatypes. */
const subEnc = (geom) => [
  geom.attributes.position.componentDatatype,
  Array.from(geom.attributes.position.values),
  geom.attributes.st ? [geom.attributes.st.componentDatatype, Array.from(geom.attributes.st.values)] : null,
  Array.from(geom.indices),
];

function pipelineSuite() {
  const rng = makeRng(1201);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("PolygonPipeline");
  const ellipsoids = { WGS84: Ellipsoid.WGS84, UNIT_SPHERE: Ellipsoid.UNIT_SPHERE, MOON: Ellipsoid.MOON };

  for (let i = 0; i < 30; i++) {
    const ring = star(g, 3 + Math.floor(rng() * 20), g.uniform(-5, 5), g.uniform(-5, 5), 1, 10, rng() < 0.5);
    const pts = [];
    for (let k = 0; k < ring.length; k += 2) pts.push([ring[k], ring[k + 1]]);
    s.add("computeArea2D", [pts], (p) => PolygonPipeline.computeArea2D(p.map(([x, y]) => new Cartesian2(x, y))));
    s.add("computeWindingOrder2D", [pts], (p) => PolygonPipeline.computeWindingOrder2D(p.map(([x, y]) => new Cartesian2(x, y))));
  }
  for (let i = 0; i < 20; i++) {
    const [data, holes] = holed(g, 4 + Math.floor(rng() * 40), Math.floor(rng() * 3));
    const pts = [];
    for (let k = 0; k < data.length; k += 2) pts.push([data[k], data[k + 1]]);
    s.add("triangulate", [pts, holes], (p, h) => PolygonPipeline.triangulate(p.map(([x, y]) => new Cartesian2(x, y)), h));
  }

  // Polygons a few degrees across on the ellipsoid, triangulated in lon/lat.
  for (let i = 0; i < 40; i++) {
    const name = i < 30 ? "WGS84" : rng() < 0.5 ? "UNIT_SPHERE" : "MOON";
    const ellipsoid = ellipsoids[name];
    const lon = g.uniform(-170, 170);
    const lat = g.uniform(-70, 70);
    const ring = star(g, 3 + Math.floor(rng() * 8), lon, lat, 0.5, g.uniform(1, 8));
    const heights = rng() < 0.5;
    const degrees = [];
    for (let k = 0; k < ring.length; k += 2) degrees.push([ring[k], ring[k + 1], heights ? g.uniform(0, 5000) : 0]);
    const indices = earcut(ring, [], 2);
    const st = rng() < 0.6 ? degrees.map(([x, y]) => [(x - lon) / 20 + 0.5, (y - lat) / 20 + 0.5]) : null;
    const granularity = rng() < 0.2 ? null : g.uniform(0.3, 3) * (Math.PI / 180);
    const args = [name, degrees, indices, st, granularity];
    for (const f of ["computeSubdivision", "computeRhumbLineSubdivision"]) {
      s.add(f, args, (e, d, idx, t, gr) =>
        subEnc(
          PolygonPipeline[f](
            ellipsoids[e],
            d.map(([x, y, h]) => Cartesian3.fromDegrees(x, y, h, ellipsoids[e])),
            idx,
            t ? t.map(([x, y]) => new Cartesian2(x, y)) : undefined,
            gr ?? undefined,
          ),
        ),
      );
    }
  }

  for (let i = 0; i < 25; i++) {
    const name = i < 20 ? "WGS84" : "UNIT_SPHERE";
    const scale = name === "WGS84" ? 6.4e6 : 1.1;
    const positions = Array.from({ length: 3 * (1 + Math.floor(rng() * 6)) }, () => g.uniform(-scale, scale));
    const height = rng() < 0.2 ? null : g.pick([0, 100, -50, g.uniform(-1e4, 1e5)]);
    const toSurface = g.pick([null, true, false]);
    s.add("scaleToGeodeticHeight", [positions, height, name, toSurface], (p, h, e, t) =>
      PolygonPipeline.scaleToGeodeticHeight(p.slice(), h ?? undefined, ellipsoids[e], t ?? undefined),
    );
  }
  return s;
}

export default function corePolygon() {
  return [earcutSuite(), pipelineSuite()];
}
