// Fixtures for VertexFormat and the box, plane, ellipsoid, sphere, cylinder,
// and frustum geometries: the full vertex and index data, and packing.

import {
  BoxGeometry,
  BoxOutlineGeometry,
  Cartesian3,
  CylinderGeometry,
  CylinderOutlineGeometry,
  EllipsoidGeometry,
  EllipsoidOutlineGeometry,
  FrustumGeometry,
  FrustumOutlineGeometry,
  GeometryOffsetAttribute,
  OrthographicFrustum,
  PerspectiveFrustum,
  PlaneGeometry,
  PlaneOutlineGeometry,
  SphereGeometry,
  SphereOutlineGeometry,
  VertexFormat,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

/** A geometry as plain data; typed arrays keep their element type's name. */
export function geometryEnc(g) {
  if (!g) return null;
  const attributes = {};
  for (const [name, a] of Object.entries(g.attributes)) {
    if (a) attributes[name] = [a.componentDatatype, a.componentsPerAttribute, a.normalize, a.values.constructor.name, Array.from(a.values)];
  }
  return {
    attributes,
    indices: g.indices ? [g.indices.constructor.name, Array.from(g.indices)] : null,
    primitiveType: g.primitiveType,
    boundingSphere: g.boundingSphere ?? null,
    geometryType: g.geometryType,
    offsetAttribute: g.offsetAttribute ?? null,
  };
}

const formats = {
  POSITION_ONLY: VertexFormat.POSITION_ONLY,
  POSITION_AND_NORMAL: VertexFormat.POSITION_AND_NORMAL,
  POSITION_NORMAL_AND_ST: VertexFormat.POSITION_NORMAL_AND_ST,
  POSITION_AND_ST: VertexFormat.POSITION_AND_ST,
  ALL: VertexFormat.ALL,
  NONE: new VertexFormat(),
  TANGENT_ONLY: new VertexFormat({ position: true, tangent: true }),
  BITANGENT_ONLY: new VertexFormat({ position: true, bitangent: true }),
};
const formatNames = Object.keys(formats);
const offsets = [null, GeometryOffsetAttribute.NONE, GeometryOffsetAttribute.ALL];
const packed = (Type, value) => Type.pack(value, new Array(value.packedLength ?? Type.packedLength).fill(0));

function boxPlaneSuite() {
  const g = generators(makeRng(700));
  const s = suite("BoxPlaneGeometry");
  for (let k = 0; k < 16; k++) {
    const min = g.c3(50);
    const max = Cartesian3.add(min, new Cartesian3(g.uniform(0.1, 30), g.uniform(0.1, 30), g.uniform(0.1, 30)), new Cartesian3());
    const format = formatNames[k % formatNames.length];
    const offset = offsets[k % 3];
    s.add("box", [min, max, format, offset], (mn, mx, f, o) => {
      const box = new BoxGeometry({ minimum: mn, maximum: mx, vertexFormat: formats[f], offsetAttribute: o ?? undefined });
      return [geometryEnc(BoxGeometry.createGeometry(box)), packed(BoxGeometry, box)];
    });
    s.add("boxOutline", [min, max, offset], (mn, mx, o) => {
      const box = new BoxOutlineGeometry({ minimum: mn, maximum: mx, offsetAttribute: o ?? undefined });
      return [geometryEnc(BoxOutlineGeometry.createGeometry(box)), packed(BoxOutlineGeometry, box)];
    });
  }
  s.add("boxFromDimensions", [[2, 4, 6], "ALL"], (d, f) => geometryEnc(BoxGeometry.createGeometry(BoxGeometry.fromDimensions({ dimensions: Cartesian3.fromArray(d), vertexFormat: formats[f] }))));
  s.add("box", [new Cartesian3(1, 1, 1), new Cartesian3(1, 1, 1), "ALL", null], (mn, mx, f) => {
    const box = new BoxGeometry({ minimum: mn, maximum: mx, vertexFormat: formats[f] });
    return [geometryEnc(BoxGeometry.createGeometry(box)), packed(BoxGeometry, box)];
  });
  for (const f of formatNames) {
    s.add("plane", [f], (f) => {
      const p = new PlaneGeometry({ vertexFormat: formats[f] });
      return [geometryEnc(PlaneGeometry.createGeometry(p)), packed(PlaneGeometry, p)];
    });
  }
  s.add("planeOutline", [], () => geometryEnc(PlaneOutlineGeometry.createGeometry(new PlaneOutlineGeometry())));
  return s;
}

function ellipsoidSuite() {
  const g = generators(makeRng(701));
  const s = suite("EllipsoidGeometry");
  const cases = [];
  for (let k = 0; k < 14; k++) {
    const radii = new Cartesian3(g.uniform(1, 100), g.uniform(1, 100), g.uniform(1, 100));
    const hollow = k % 3 !== 0;
    const inner = hollow ? Cartesian3.multiplyByScalar(radii, g.uniform(0.3, 0.9), new Cartesian3()) : null;
    const partial = k % 2 === 1;
    cases.push([
      radii,
      inner,
      partial ? g.uniform(0, 1) : 0,
      partial ? g.uniform(3, 6.28) : 2 * Math.PI,
      partial ? g.uniform(0, 0.8) : 0,
      partial ? g.uniform(2, Math.PI) : Math.PI,
      3 + Math.floor(g.rng() * 12),
      3 + Math.floor(g.rng() * 12),
      formatNames[k % formatNames.length],
      offsets[k % 3],
    ]);
  }
  cases.push([new Cartesian3(1, 1, 1), null, 0, 2 * Math.PI, 0, Math.PI, 20, 20, "ALL", null]);
  cases.push([new Cartesian3(0, 1, 1), null, 0, 2 * Math.PI, 0, Math.PI, 8, 8, "ALL", null]);
  for (const [radii, inner, minClock, maxClock, minCone, maxCone, stacks, slices, format, offset] of cases) {
    s.add("ellipsoid", [radii, inner, minClock, maxClock, minCone, maxCone, stacks, slices, format, offset], (r, i, a, b, c, d, st, sl, f, o) => {
      const e = new EllipsoidGeometry({
        radii: r,
        innerRadii: i ?? undefined,
        minimumClock: a,
        maximumClock: b,
        minimumCone: c,
        maximumCone: d,
        stackPartitions: st,
        slicePartitions: sl,
        vertexFormat: formats[f],
        offsetAttribute: o ?? undefined,
      });
      return [geometryEnc(EllipsoidGeometry.createGeometry(e)), packed(EllipsoidGeometry, e)];
    });
    s.add("ellipsoidOutline", [radii, inner, minClock, maxClock, minCone, maxCone, stacks, slices, 5 + stacks, offset], (r, i, a, b, c, d, st, sl, sub, o) => {
      const e = new EllipsoidOutlineGeometry({
        radii: r,
        innerRadii: i ?? undefined,
        minimumClock: a,
        maximumClock: b,
        minimumCone: c,
        maximumCone: d,
        stackPartitions: st,
        slicePartitions: sl,
        subdivisions: sub,
        offsetAttribute: o ?? undefined,
      });
      return [geometryEnc(EllipsoidOutlineGeometry.createGeometry(e)), packed(EllipsoidOutlineGeometry, e)];
    });
  }
  for (const [radius, stacks, slices, format] of [[3, 8, 9, "ALL"], [10, 16, 4, "POSITION_ONLY"], [0.5, 3, 3, "POSITION_NORMAL_AND_ST"]]) {
    s.add("sphere", [radius, stacks, slices, format], (r, st, sl, f) => {
      const sp = new SphereGeometry({ radius: r, stackPartitions: st, slicePartitions: sl, vertexFormat: formats[f] });
      return [geometryEnc(SphereGeometry.createGeometry(sp)), packed(SphereGeometry, sp)];
    });
    s.add("sphereOutline", [radius, stacks, slices, 2 * stacks], (r, st, sl, sub) => {
      const sp = new SphereOutlineGeometry({ radius: r, stackPartitions: st, slicePartitions: sl, subdivisions: sub });
      return [geometryEnc(SphereOutlineGeometry.createGeometry(sp)), packed(SphereOutlineGeometry, sp)];
    });
  }
  return s;
}

function cylinderSuite() {
  const g = generators(makeRng(702));
  const s = suite("CylinderGeometry");
  for (let k = 0; k < 16; k++) {
    const length = g.pick([g.uniform(0.5, 50), g.uniform(0.5, 50), 0]);
    const top = g.pick([g.uniform(0, 20), 0, g.uniform(0, 20)]);
    const bottom = g.pick([g.uniform(0.1, 20), g.uniform(0, 20), 0]);
    const slices = 3 + Math.floor(g.rng() * 30);
    const format = formatNames[k % formatNames.length];
    const offset = offsets[k % 3];
    s.add("cylinder", [length, top, bottom, slices, format, offset], (l, t, b, sl, f, o) => {
      const c = new CylinderGeometry({ length: l, topRadius: t, bottomRadius: b, slices: sl, vertexFormat: formats[f], offsetAttribute: o ?? undefined });
      return [geometryEnc(CylinderGeometry.createGeometry(c)), packed(CylinderGeometry, c)];
    });
    const lines = g.pick([16, 0, 4, 7, 100]);
    s.add("cylinderOutline", [length, top, bottom, slices, lines, offset], (l, t, b, sl, n, o) => {
      const c = new CylinderOutlineGeometry({ length: l, topRadius: t, bottomRadius: b, slices: sl, numberOfVerticalLines: n, offsetAttribute: o ?? undefined });
      return [geometryEnc(CylinderOutlineGeometry.createGeometry(c)), packed(CylinderOutlineGeometry, c)];
    });
  }
  return s;
}

function frustumSuite() {
  const g = generators(makeRng(703));
  const s = suite("FrustumGeometry");
  for (let k = 0; k < 16; k++) {
    const perspective = k % 2 === 0;
    const params = perspective
      ? [g.uniform(0.3, 2), g.uniform(0.5, 2), g.uniform(0.1, 5), g.uniform(10, 200), g.pick([0, g.uniform(-0.2, 0.2)]), g.pick([0, g.uniform(-0.2, 0.2)])]
      : [g.uniform(1, 50), g.uniform(0.5, 2), g.uniform(0.1, 5), g.uniform(10, 200)];
    const origin = g.c3(100);
    const orientation = g.quat();
    const drawNear = k % 4 < 2;
    const make = (p, persp) => {
      if (persp) {
        const f = new PerspectiveFrustum({ fov: p[0], aspectRatio: p[1], near: p[2], far: p[3], xOffset: p[4], yOffset: p[5] });
        return f;
      }
      return new OrthographicFrustum({ width: p[0], aspectRatio: p[1], near: p[2], far: p[3] });
    };
    s.add("frustum", [perspective, params, origin, orientation, drawNear], (persp, p, o, q, near) => {
      const fg = new FrustumGeometry({ frustum: make(p, persp), origin: o, orientation: q, vertexFormat: VertexFormat.POSITION_ONLY, _drawNearPlane: near });
      return [geometryEnc(FrustumGeometry.createGeometry(fg)), FrustumGeometry.pack(fg, new Array(fg.packedLength).fill(0))];
    });
    s.add("frustumOutline", [perspective, params, origin, orientation, drawNear], (persp, p, o, q, near) => {
      const fg = new FrustumOutlineGeometry({ frustum: make(p, persp), origin: o, orientation: q, _drawNearPlane: near });
      return [geometryEnc(FrustumOutlineGeometry.createGeometry(fg)), FrustumOutlineGeometry.pack(fg, new Array(fg.packedLength).fill(0))];
    });
  }
  return s;
}

function vertexFormatSuite() {
  const s = suite("VertexFormat");
  for (const f of formatNames) s.add("pack", [f], (f) => VertexFormat.pack(formats[f], new Array(6).fill(0)));
  return s;
}

export default function coreGeometries() {
  return [boxPlaneSuite(), ellipsoidSuite(), cylinderSuite(), frustumSuite(), vertexFormatSuite()];
}
